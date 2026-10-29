/// examples/http_bridge_for_ai.rs -- pont HTTP minimal pour qu'une IA SANS
/// aucune execution de code (un chat web, genre ChatGPT ou un Claude sans
/// shell) puisse essayer CSTL pour vrai, via un humain qui relaie.
///
/// Pourquoi ce fichier existe (2026-10-29): AGENTS.md Option 2 montre deja
/// comment parler au serveur en TCP brut via `/dev/tcp` de bash -- mais ca
/// suppose que la personne qui relaie sait ce qu'est un socket, utilise
/// bash, et copie un bloc de 6 lignes sans faire d'erreur. Une IA qui n'a
/// AUCUN acces shell (le cas reel qui a motive ce fichier: une IA qui a
/// elle-meme admis "je ne clique pas sur Entree dans ton terminal") depend
/// entierement de la simplicite de ce qu'elle peut demander a l'humain de
/// faire. `curl` est installe partout, fonctionne pareil sur Linux/Mac/
/// Windows (WSL ou natif), et une seule ligne suffit.
///
/// Flux complet:
///   1. Quelqu'un avec le repo lance UNE FOIS:
///        cargo run --bin cstl_parser &
///        cargo run --example http_bridge_for_ai &
///   2. L'IA sans shell compose elle-meme le texte du payload CSTL (a partir
///      de CSTL_SPEC_v5_0.md ou des exemples de AGENTS.md) et le donne a
///      l'humain, par exemple:
///        "Colle ca dans un fichier payload.cstl, puis lance:
///         curl -s --data-binary @payload.cstl http://localhost:8089/try"
///   3. L'humain colle la vraie reponse du serveur dans le chat de l'IA.
///   4. L'IA peut iterer -- composer un autre payload, demander un autre
///      curl -- sans jamais executer une seule commande elle-meme.
///
/// Ce pont ne fait AUCUNE interpretation du payload: il copie les octets
/// recus (le corps HTTP) tels quels vers le vrai serveur CSTL en TCP, et
/// renvoie tels quels les octets recus en retour. C'est un simple relais
/// bidirectionnel, pas un deuxieme parseur -- la seule logique CSTL reelle
/// reste dans src/server/, exactement comme pour n'importe quel autre
/// client TCP.
///
/// Verifie en live le 2026-10-29: demarrage du vrai CstlNativeServer sur un
/// port ephemere, demarrage de ce pont en face, puis un vrai
/// `curl --data-binary @payload.cstl http://127.0.0.1:<port>/try` depuis un
/// processus externe (pas depuis ce binaire) -- la reponse HTTP contient la
/// vraie reponse CSTL signee du serveur, capturee telle quelle.
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

/// Lit une requete HTTP brute depuis `stream` et retourne le corps (le
/// payload CSTL envoye par l'humain via curl). On ne parse que le strict
/// necessaire: la ligne vide qui separe les en-tetes du corps, et
/// Content-Length pour savoir combien d'octets de corps lire -- tout le
/// reste de la requete (methode, chemin, en-tetes) est ignore volontairement
/// puisque ce pont n'a qu'un seul travail.
fn read_http_body(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_double_crlf(&raw) {
            header_end = pos;
            let headers = String::from_utf8_lossy(&raw[..header_end]);
            let content_length: usize = headers
                .lines()
                .find_map(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().to_string())
                })
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let body_start = header_end + 4;
            while raw.len() < body_start + content_length {
                let n2 = stream.read(&mut chunk)?;
                if n2 == 0 {
                    break;
                }
                raw.extend_from_slice(&chunk[..n2]);
            }
            return Ok(raw[body_start..raw.len().min(body_start + content_length)].to_vec());
        }
    }
    Ok(Vec::new())
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Envoie le payload au vrai serveur CSTL et retourne sa reponse brute,
/// exactement comme le ferait n'importe quel client TCP (meme logique de
/// lecture-jusqu'a-terminateur que dans les autres examples de ce repo).
fn forward_to_cstl_server(cstl_addr: &str, payload: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(cstl_addr)?;
    stream.write_all(payload)?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(9).any(|w| w == b"---END---") {
            break;
        }
    }
    Ok(buf)
}

fn write_http_response(stream: &mut TcpStream, status_line: &str, body: &[u8]) {
    let header = format!(
        "{status_line}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn main() -> std::io::Result<()> {
    let bridge_port: u16 = env::var("CSTL_BRIDGE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8089);
    let cstl_addr = env::var("CSTL_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:5050".to_string());

    let listener = TcpListener::bind(("0.0.0.0", bridge_port))?;
    println!(
        "[http_bridge_for_ai] ecoute sur http://0.0.0.0:{bridge_port}/ -- relaie vers le vrai serveur CSTL sur {cstl_addr}"
    );
    println!(
        "[http_bridge_for_ai] exemple: curl -s --data-binary @payload.cstl http://127.0.0.1:{bridge_port}/try"
    );

    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(s) => s,
            Err(_) => continue,
        };
        let cstl_addr = cstl_addr.clone();
        std::thread::spawn(move || {
            let body = match read_http_body(&mut stream) {
                Ok(b) => b,
                Err(e) => {
                    write_http_response(
                        &mut stream,
                        "HTTP/1.1 400 Bad Request",
                        format!("lecture de la requete echouee: {e}").as_bytes(),
                    );
                    return;
                }
            };
            if body.is_empty() {
                write_http_response(
                    &mut stream,
                    "HTTP/1.1 400 Bad Request",
                    b"corps vide -- envoie un payload CSTL complet (#!CSTL ... ---END---) dans le corps de la requete POST",
                );
                return;
            }
            match forward_to_cstl_server(&cstl_addr, &body) {
                Ok(response) => write_http_response(&mut stream, "HTTP/1.1 200 OK", &response),
                Err(e) => write_http_response(
                    &mut stream,
                    "HTTP/1.1 502 Bad Gateway",
                    format!(
                        "impossible de joindre le serveur CSTL a {cstl_addr}: {e} -- est-il bien lance? (cargo run --bin cstl_parser)"
                    )
                    .as_bytes(),
                ),
            }
        });
    }
    Ok(())
}
