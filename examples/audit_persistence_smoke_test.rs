/// examples/audit_persistence_smoke_test.rs -- verification live de la
/// persistance de la chaine d'audit (Couche 5/8, 2026-09-04).
///
/// Trouvaille corrigee par ce travail: `HashChain` (seq + parent_hash) etait
/// PUREMENT en memoire -- `CstlNativeServer::new()` la reconstruisait vide a
/// CHAQUE demarrage, alors que `adn_store.rs` persistait deja les payloads
/// (avec leur propre `parent_hash`) sur disque via `cstl_adn.db`.
/// `AuditStore` (src/server/audit_store.rs) implementait deja la
/// persistance necessaire mais n'etait appele NULLE PART en dehors de son
/// propre test unitaire -- code mort depuis sa creation. Un redemarrage
/// reel du serveur cassait donc silencieusement la continuite de la chaine
/// de hachage, alors meme que le README/ARCHITECTURE.md revendiquent une
/// "provenance immuable en chaine de hachage" (Couche 8).
///
/// Ce smoke-test simule un redemarrage complet SANS avoir besoin de tuer un
/// vrai processus: `CstlNativeServer::with_data_path(port, chemin)` fait
/// EXACTEMENT ce qu'un vrai demarrage ferait (ouvre audit_store au meme
/// chemin, charge la chaine persistee via `load_chain()`) -- construire une
/// DEUXIEME instance pointee sur le MEME fichier reel (pas ":memory:", qui
/// ne partage jamais d'etat entre deux Connection) est donc un test fidele
/// de ce qui se passerait a un vrai redemarrage.
///
/// Scenarios verifies:
/// 1. server_a (chemin reel neuf) traite 2 payloads -> seq=0 (parent=root),
///    seq=1 (parent=hash de seq=0).
/// 2. server_b, NOUVELLE instance pointee sur le MEME fichier, traite un
///    3e payload -> doit continuer a seq=2 avec parent_hash = hash de seq=1
///    (PAS repartir a seq=0/parent=root comme avant ce fix).
/// 3. Un payload de contenu IDENTIQUE renvoye a server_b ne fait pas planter
///    la persistance (INSERT OR IGNORE, cf. audit_store.rs) -- meme hash,
///    nouvelle entree en memoire (HashChain ne deduplique pas), mais
///    audit_store.save() doit rester silencieux (pas d'erreur remontee au
///    client).
use cstl_parser::agent_discovery::AgentCard;
use cstl_parser::server::CstlNativeServer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Enregistre un agent "communication" legacy (public_key: None) sur
/// l'instance donnee -- necessaire pour que STEP 4 (routage) trouve une
/// destination, independamment de l'objet de ce smoke-test (persistance de
/// la chaine, pas signature/registre).
async fn register_router(server: &CstlNativeServer, name: &str) {
    let mut reg = server.agent_registry.lock().await;
    reg.register(AgentCard {
        name: name.to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
}

async fn send(port: u16, payload: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    stream.write_all(payload.as_bytes()).await.expect("send");
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

fn relation_payload(sender: &str, subject: &str, city: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=audit_persistence_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let tmp_path = std::env::temp_dir().join(format!(
        "cstl_audit_persistence_smoke_{}.db",
        std::process::id()
    ));
    let tmp_path_str = tmp_path.to_str().unwrap().to_string();
    let _ = std::fs::remove_file(&tmp_path_str); // run precedent eventuel

    let mut failures = Vec::new();

    // ---- "Session 1" (server_a) ----
    let port_a: u16 = 15170;
    let server_a = CstlNativeServer::with_data_path(port_a, &tmp_path_str);
    register_router(&server_a, "smoke_router_a").await;
    tokio::spawn(async move {
        server_a.start().await.expect("server_a start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    println!("[1/3] Session 1: 2 payloads sur un fichier neuf...");
    let resp1 = send(port_a, &relation_payload("agent_1", "audit_persist_subject_1", "Lisbonne")).await;
    let seq1 = extract_field(&resp1, "AUDIT", "seq");
    let hash1 = extract_field(&resp1, "AUDIT", "hash").expect("hash seq0");
    let parent1 = extract_field(&resp1, "AUDIT", "parent_hash");
    println!("      seq={:?} parent_hash={:?} hash={}", seq1, parent1, hash1);
    if seq1.as_deref() != Some("0") || parent1.as_deref() != Some("root") {
        failures.push("scenario 1a: premier payload attendu seq=0, parent_hash=root".to_string());
    }

    let resp2 = send(port_a, &relation_payload("agent_1", "audit_persist_subject_2", "Vienne")).await;
    let seq2 = extract_field(&resp2, "AUDIT", "seq");
    let hash2 = extract_field(&resp2, "AUDIT", "hash").expect("hash seq1");
    let parent2 = extract_field(&resp2, "AUDIT", "parent_hash");
    println!("      seq={:?} parent_hash={:?} hash={}", seq2, parent2, hash2);
    if seq2.as_deref() != Some("1") || parent2.as_deref() != Some(hash1.as_str()) {
        failures.push("scenario 1b: deuxieme payload attendu seq=1, parent_hash=hash du premier".to_string());
    }

    // ---- "Redemarrage" : server_b, NOUVELLE instance, MEME fichier ----
    println!("[2/3] \"Redemarrage\" (nouvelle instance, meme fichier)...");
    let port_b: u16 = 15171;
    let server_b = CstlNativeServer::with_data_path(port_b, &tmp_path_str);
    register_router(&server_b, "smoke_router_b").await;
    tokio::spawn(async move {
        server_b.start().await.expect("server_b start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let resp3 = send(port_b, &relation_payload("agent_2", "audit_persist_subject_3", "Prague")).await;
    let seq3 = extract_field(&resp3, "AUDIT", "seq");
    let hash3 = extract_field(&resp3, "AUDIT", "hash").expect("hash seq2");
    let parent3 = extract_field(&resp3, "AUDIT", "parent_hash");
    println!("      seq={:?} parent_hash={:?} hash={}", seq3, parent3, hash3);
    if seq3.as_deref() != Some("2") {
        failures.push(format!(
            "scenario 2: apres 'redemarrage', seq attendu=2 (continuite), obtenu={:?} -- \
             la chaine a ete reinitialisee au lieu d'etre rechargee depuis le disque",
            seq3
        ));
    }
    if parent3.as_deref() != Some(hash2.as_str()) {
        failures.push(format!(
            "scenario 2: apres 'redemarrage', parent_hash attendu={} (dernier hash de la session 1), obtenu={:?}",
            hash2, parent3
        ));
    }

    // ---- Renvoi d'un payload de contenu identique (dedup a la persistance) ----
    println!("[3/3] Renvoi d'un payload de contenu identique (ne doit pas planter)...");
    let resp4 = send(port_b, &relation_payload("agent_2", "audit_persist_subject_3", "Prague")).await;
    let status4 = extract_field(&resp4, "META", "status");
    let hash4 = extract_field(&resp4, "AUDIT", "hash");
    println!("      status={:?} hash={:?} (identique attendu a hash3={})", status4, hash4, hash3);
    if status4.as_deref() != Some("processed") {
        failures.push("scenario 3: un payload de contenu identique renvoye doit rester traite normalement (pas d'erreur de contrainte UNIQUE remontee au client)".to_string());
    }
    if hash4.as_deref() != Some(hash3.as_str()) {
        failures.push("scenario 3: meme contenu doit produire le meme hash (canonical_hash est deterministe sur le contenu)".to_string());
    }

    let _ = std::fs::remove_file(&tmp_path_str);

    println!();
    if failures.is_empty() {
        println!("✅ La chaine d'audit survit correctement a un redemarrage (persistance reelle verifiee en direct).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
