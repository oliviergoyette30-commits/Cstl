/// examples/tls_smoke_test.rs -- verification live d'un VRAI handshake TLS
/// 1.3 mutuellement authentifie, serveur reel, client reel, sur une vraie
/// connexion TCP (meme convention que les autres smoke-tests de cette
/// session). C'est la preuve que `server/tls.rs` n'est plus un stub: aucun
/// des scenarios ci-dessous ne peut reussir par accident si le handshake
/// TLS, le parsing X.509 ou la verification de chaine est factice ou
/// absent -- un stub comme l'ancien (qui acceptait n'importe quel octet
/// non vide comme "certificat valide") echouerait IMMEDIATEMENT des le
/// scenario 1, qui exige un vrai `rustls::ClientConfig` cote client.
///
/// Scenarios verifies:
/// 1. Client TLS correctement configure (cert client signe par la bonne CA,
///    verifie le certificat serveur contre la meme CA) -> handshake reussi,
///    paylaod CSTL envoye/recu en clair A L'INTERIEUR du tunnel TLS, flux
///    normal (status=processed).
/// 2. Client SANS certificat alors que require_mutual_auth=true -> le
///    handshake TLS lui-meme echoue (rejet au niveau transport, avant tout
///    octet applicatif) -- pas une erreur applicative.
/// 3. Client avec un certificat signe par une CA DIFFERENTE (non fiable) ->
///    handshake refuse par le serveur (verification de chaine reelle, pas
///    juste "non vide").
/// 4. Client qui parle TCP brut (pas TLS du tout) vers un port TLS -> la
///    tentative de handshake echoue cote client (le serveur n'envoie jamais
///    de reponse CSTL en clair).
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::tls::{generate_test_pki, TlsConfig, TlsServer};
use cstl_parser::server::CstlNativeServer;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_rustls::TlsConnector;

fn parse_cert_chain(pem: &[u8]) -> Vec<CertificateDer<'static>> {
    let mut reader = std::io::Cursor::new(pem);
    rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>().expect("PEM cert valide")
}
fn parse_private_key(pem: &[u8]) -> PrivateKeyDer<'static> {
    let mut reader = std::io::Cursor::new(pem);
    rustls_pemfile::private_key(&mut reader).expect("parsing cle").expect("cle presente")
}

fn make_test_server(port: u16, tls: TlsServer) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    // Agent bootstrap "communication" legacy (public_key: None) -- necessaire
    // pour que STEP 4 (routage) trouve une destination, independamment du
    // TLS: meme patron que les autres smoke-tests (ex.
    // signing_registration_smoke_test.rs::make_test_server).
    registry.register(AgentCard {
        name: "tls_smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("olivier"));
    server.tls = Some(Arc::new(tls));
    server
}

fn root_store_with(ca_pem: &[u8]) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    for cert in parse_cert_chain(ca_pem) {
        roots.add(cert).expect("CA valide");
    }
    roots
}

const CSTL_PING: &str = "#!CSTL v5.0.0 MODE=A\nMETA [encoder=TlsSmokeTest, produced_by=TlsSmokeTest]\nINTENT_PAYLOAD [purpose=tls_smoke, sender=tls_client, receiver=server]\n---END---\n";

async fn read_response(stream: &mut (impl tokio::io::AsyncRead + Unpin)) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.expect("read");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(9).any(|w| w == b"---END---") {
            break;
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn extract_field(response: &str, block: &str, key: &str) -> Option<String> {
    for line in response.lines() {
        if line.starts_with(block) {
            let inner = line.split_once('[')?.1.trim_end_matches(']').trim_end_matches("]\n");
            for part in inner.split(',') {
                let part = part.trim();
                if let Some((k, v)) = part.split_once('=') {
                    if k.trim() == key {
                        return Some(v.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let port: u16 = 15180;

    // Deux chaines de confiance DISTINCTES -- la deuxieme (`other_pki`) sert
    // uniquement au scenario 3 (certificat signe par une CA que le serveur
    // ne connait pas).
    let pki = generate_test_pki(vec!["localhost".to_string(), "127.0.0.1".to_string()], "tls-smoke-client")
        .expect("generation de la PKI principale doit reussir");
    let other_pki = generate_test_pki(vec!["localhost".to_string()], "untrusted-client")
        .expect("generation de la PKI secondaire doit reussir");

    let tls_config = TlsConfig {
        cert_chain_pem: pki.server_cert_pem.clone(),
        private_key_pem: pki.server_key_pem.clone(),
        client_ca_pem: Some(pki.ca_cert_pem.clone()),
        require_mutual_auth: true,
    };
    let tls_server = TlsServer::new(tls_config).expect("configuration TLS serveur doit reussir avec des certs reels");

    let server = make_test_server(port, tls_server);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();
    let server_name = ServerName::try_from("localhost").expect("nom de serveur valide");

    // ---- Scenario 1: handshake mutuel complet, correctement configure ----
    println!("[1/4] Handshake TLS 1.3 mutuel avec un client correctement configure...");
    {
        let roots = root_store_with(&pki.ca_cert_pem);
        let client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(parse_cert_chain(&pki.client_cert_pem), parse_private_key(&pki.client_key_pem))
            .expect("configuration client avec cert valide doit reussir");
        let connector = TlsConnector::from(Arc::new(client_config));

        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(tcp) => match connector.connect(server_name.clone(), tcp).await {
                Ok(mut tls_stream) => {
                    tls_stream.write_all(CSTL_PING.as_bytes()).await.expect("ecriture TLS");
                    let resp = read_response(&mut tls_stream).await;
                    let status = extract_field(&resp, "META", "status");
                    println!("      handshake reussi, status={:?}", status);
                    if status.as_deref() != Some("processed") {
                        failures.push("scenario 1: un payload CSTL valide a travers un tunnel TLS correctement authentifie doit etre traite normalement".to_string());
                    }
                }
                Err(e) => failures.push(format!("scenario 1: le handshake TLS aurait du reussir avec un client correctement configure: {e}")),
            },
            Err(e) => failures.push(format!("scenario 1: connexion TCP initiale echouee: {e}")),
        }
    }

    // ---- Scenario 2: client sans certificat, mutual auth exigee ----
    println!("[2/4] Client SANS certificat (mutual auth exigee -> handshake doit echouer)...");
    {
        let roots = root_store_with(&pki.ca_cert_pem);
        let client_config = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("connexion TCP");
        // Note (trouvaille live): en TLS 1.3, `connect()` cote CLIENT peut
        // retourner Ok des que le client a envoye son propre Finished --
        // AVANT que le serveur ait fini de valider (ou rejeter) le
        // certificat client. Le rejet arrive alors comme une alerte TLS
        // fatale lue au PROCHAIN read()/write(), pas comme un Err de
        // connect() lui-meme. Le test verifie donc l'echange applicatif
        // complet, pas seulement connect().
        match connector.connect(server_name.clone(), tcp).await {
            Ok(mut tls_stream) => {
                let io_result = async {
                    tls_stream.write_all(CSTL_PING.as_bytes()).await?;
                    let mut buf = [0u8; 256];
                    tls_stream.read(&mut buf).await
                }.await;
                match io_result {
                    Ok(0) | Err(_) => println!("      rejet detecte a l'echange applicatif (connexion fermee/alerte TLS), comme attendu"),
                    Ok(n) => failures.push(format!("scenario 2: le serveur ne doit jamais accepter d'echange applicatif sans certificat client -- {} octets recus", n)),
                }
            }
            Err(e) => println!("      handshake refuse directement comme attendu: {e}"),
        }
    }

    // ---- Scenario 3: certificat client signe par une CA NON fiable ----
    println!("[3/4] Client avec un certificat signe par une CA differente (non fiable)...");
    {
        let roots = root_store_with(&pki.ca_cert_pem); // le client fait confiance au VRAI serveur...
        let client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            // ...mais presente un certificat signe par une AUTRE CA, que le
            // SERVEUR ne connait pas (client_ca_pem du serveur = pki.ca_cert_pem, pas other_pki).
            .with_client_auth_cert(parse_cert_chain(&other_pki.client_cert_pem), parse_private_key(&other_pki.client_key_pem))
            .expect("configuration client doit construire (la validation cote serveur vient au handshake)");
        let connector = TlsConnector::from(Arc::new(client_config));
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("connexion TCP");
        match connector.connect(server_name.clone(), tcp).await {
            Ok(mut tls_stream) => {
                let io_result = async {
                    tls_stream.write_all(CSTL_PING.as_bytes()).await?;
                    let mut buf = [0u8; 256];
                    tls_stream.read(&mut buf).await
                }.await;
                match io_result {
                    Ok(0) | Err(_) => println!("      rejet detecte a l'echange applicatif (connexion fermee/alerte TLS), comme attendu"),
                    Ok(n) => failures.push(format!("scenario 3: le serveur ne doit jamais accepter un certificat signe par une CA inconnue -- {} octets recus", n)),
                }
            }
            Err(e) => println!("      handshake refuse directement comme attendu: {e}"),
        }
    }

    // ---- Scenario 4: client TCP brut (pas de TLS du tout) ----
    println!("[4/4] Client TCP brut (sans TLS) contre un port TLS...");
    {
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("connexion TCP");
        tcp.write_all(CSTL_PING.as_bytes()).await.expect("ecriture TCP brute");
        // Le serveur attend un ClientHello TLS, pas du texte CSTL en clair --
        // soit la connexion se ferme sans reponse, soit read() renvoie 0.
        // Timeout court: si le serveur repondait en clair (ce qu'il ne doit
        // JAMAIS faire sur un port TLS), ce test le detecterait comme un
        // echec plutot que de bloquer indefiniment.
        let mut buf = [0u8; 256];
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), tcp.read(&mut buf)).await;
        match result {
            Ok(Ok(0)) => println!("      connexion fermee par le serveur comme attendu (handshake TLS jamais complete)"),
            Ok(Ok(n)) => {
                // Le serveur peut repondre par une alerte TLS BINAIRE (ex. un
                // enregistrement "unexpected_message"/"decode_error" de 7
                // octets: 1 type + 2 version + 2 longueur + 2 alerte) avant
                // de couper -- ce n'est PAS une reponse CSTL en clair.
                // L'echec reel a detecter: un texte CSTL lisible
                // ("#!CSTL"/"META") renvoye a un client qui n'a jamais
                // complete de handshake TLS.
                let looks_like_plain_cstl = buf[..n].starts_with(b"#!CSTL") || buf[..n].windows(4).any(|w| w == b"META");
                if looks_like_plain_cstl {
                    failures.push(format!("scenario 4: le serveur a renvoye du texte CSTL en clair sur un port TLS -- {} octets: {:?}", n, String::from_utf8_lossy(&buf[..n])));
                } else {
                    println!("      {} octets recus mais non-CSTL (alerte TLS binaire attendue a ce point), comme attendu: {:02x?}", n, &buf[..n]);
                }
            }
            Ok(Err(e)) => println!("      connexion rompue comme attendu: {e}"),
            Err(_) => failures.push("scenario 4: le serveur aurait du fermer la connexion rapidement plutot que d'attendre un ClientHello qui ne viendra jamais".to_string()),
        }
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios TLS 1.3 mutuel sont conformes (vrai handshake, vraie verification de chaine X.509).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
