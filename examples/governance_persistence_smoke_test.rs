/// examples/governance_persistence_smoke_test.rs -- verification live de la
/// persistance de la Couche 2 (gouvernance/resilience: circuit breaker +
/// drift d'operateur, src/governance.rs), ajoutee le 2026-09-05.
///
/// Trouvaille corrigee par ce travail: `GovernanceTracker` (breaker_trips,
/// drift_ratio, par sender) etait PUREMENT en memoire
/// (`Arc<Mutex<GovernanceTracker>>` cote serveur) -- `CstlNativeServer::new`/
/// `with_data_path` le reconstruisaient TOUJOURS vide (`with_defaults()`),
/// meme quand la meme base SQLite avait deja de l'historique de breaker/
/// drift sur disque. Un redemarrage reel remettait donc silencieusement le
/// circuit breaker a "closed" et le ratio de drift a 0, meme pour un sender
/// dont le circuit venait tout juste de s'ouvrir.
///
/// Meme patron que `examples/audit_persistence_smoke_test.rs`:
/// `CstlNativeServer::with_data_path(port, chemin)` fait exactement ce
/// qu'un vrai demarrage ferait (ouvre `adn_store` au meme chemin reel --
/// PAS ":memory:" --, charge l'etat de gouvernance persiste). Construire
/// une DEUXIEME instance pointee sur le MEME fichier reel simule fidelement
/// un redemarrage complet du processus, sans avoir besoin de tuer/relancer
/// un vrai processus OS pour observer le meme effet: aucune Connection ni
/// etat en memoire n'est partage entre les deux instances, seul le fichier
/// SQLite sur disque l'est.
///
/// Scenarios verifies:
/// 1. server_a (fichier reel neuf) recoit 3 incoherences consecutives pour
///    le meme sender -> breaker_trips=3, circuit=open.
/// 2. server_a recoit aussi 3 SEMANTIC_WARNING pour un DEUXIEME sender ->
///    drift_ratio=1.0, drift_flagged=true.
/// 3. server_a est "arrete" (sort de portee), server_b -- NOUVELLE instance,
///    MEME fichier -- demarre : un SEUL nouveau payload (sans nouvelle
///    incoherence/warning) doit deja montrer breaker_trips=3/circuit=open
///    pour le premier sender ET drift_ratio=1.0/drift_flagged=true pour le
///    second -- preuve que l'etat a survecu au redemarrage, pas remis a
///    zero.
/// 4. Un sender jamais vu ni sur server_a ni server_b reste a l'etat neutre
///    sur server_b (pas de contamination croisee entre senders via la
///    persistance).
use cstl_parser::agent_discovery::AgentCard;
use cstl_parser::server::CstlNativeServer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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

/// Deux relations contradictoires sur le meme sujet -- ce qui declenche
/// EventReason::Inconsistency via ExecutionLab (Couche 3b), exactement
/// comme dans governance_smoke_test.rs.
fn born_in_payload(sender: &str, subject: &str, city: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=governance_persistence_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

/// Un operateur SDL uppercase inconnu -- declenche EventReason::SemanticWarning
/// via `validator::check_sdl_operator_whitelist` (purement consultatif, ne
/// bloque jamais le payload).
fn semantic_warning_payload(sender: &str, subject: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=governance_persistence_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=UNKNOWN_OPERATOR_XYZ, subject={subject}, object=target]\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let tmp_path = std::env::temp_dir().join(format!(
        "cstl_governance_persistence_smoke_{}.db",
        std::process::id()
    ));
    let tmp_path_str = tmp_path.to_str().unwrap().to_string();
    let _ = std::fs::remove_file(&tmp_path_str); // run precedent eventuel

    let mut failures = Vec::new();

    // ---- "Session 1" (server_a) ----
    let port_a: u16 = 15180;
    let server_a = CstlNativeServer::with_data_path(port_a, &tmp_path_str);
    register_router(&server_a, "smoke_router_a").await;
    tokio::spawn(async move {
        server_a.start().await.expect("server_a start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    println!("[1/4] Session 1: 3 incoherences consecutives pour breaker_sender...");
    let subject = "gov_persist_subject_breaker";
    let _ = send(port_a, &born_in_payload("breaker_sender", subject, "Varsovie")).await; // baseline
    let _ = send(port_a, &born_in_payload("breaker_sender", subject, "Paris")).await; // trip 1
    let _ = send(port_a, &born_in_payload("breaker_sender", subject, "Berlin")).await; // trip 2
    let resp1 = send(port_a, &born_in_payload("breaker_sender", subject, "Londres")).await; // trip 3 -> open
    let circuit1 = extract_field(&resp1, "GOVERNANCE", "circuit");
    let trips1 = extract_field(&resp1, "GOVERNANCE", "breaker_trips");
    println!("      circuit={:?} breaker_trips={:?}", circuit1, trips1);
    if circuit1.as_deref() != Some("open") || trips1.as_deref() != Some("3") {
        failures.push("scenario 1: circuit breaker attendu ouvert avec 3 trips avant redemarrage".to_string());
    }

    println!("[2/4] Session 1: 5 SEMANTIC_WARNING pour drift_sender (drift_min_samples=5)...");
    for i in 0..4 {
        let _ = send(port_a, &semantic_warning_payload("drift_sender", &format!("drift_subject_{i}"))).await;
    }
    let resp2 = send(port_a, &semantic_warning_payload("drift_sender", "drift_subject_4")).await;
    let drift_ratio2 = extract_field(&resp2, "GOVERNANCE", "drift_ratio");
    let drift_flagged2 = extract_field(&resp2, "GOVERNANCE", "drift_flagged");
    println!("      drift_ratio={:?} drift_flagged={:?}", drift_ratio2, drift_flagged2);
    if drift_ratio2.as_deref() != Some("1.00") || drift_flagged2.as_deref() != Some("true") {
        failures.push("scenario 2: drift attendu ratio=1.00, flagged=true avant redemarrage".to_string());
    }

    // ---- "Redemarrage" : server_b, NOUVELLE instance, MEME fichier ----
    println!("[3/4] \"Redemarrage\" (nouvelle instance, meme fichier) -- l'etat doit survivre...");
    let port_b: u16 = 15181;
    let server_b = CstlNativeServer::with_data_path(port_b, &tmp_path_str);
    register_router(&server_b, "smoke_router_b").await;
    tokio::spawn(async move {
        server_b.start().await.expect("server_b start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Un seul payload NORMAL (sans nouvelle incoherence) pour breaker_sender
    // sur server_b -- si l'etat n'avait PAS survecu, ce serait breaker_trips=0
    // et circuit=closed (un seul payload propre ne peut pas ouvrir le circuit).
    let resp3 = send(port_b, &born_in_payload("breaker_sender", "gov_persist_subject_breaker_after_restart", "Rome")).await;
    let circuit3 = extract_field(&resp3, "GOVERNANCE", "circuit");
    let trips3 = extract_field(&resp3, "GOVERNANCE", "breaker_trips");
    println!("      breaker_sender apres redemarrage: circuit={:?} breaker_trips={:?}", circuit3, trips3);
    if circuit3.as_deref() != Some("open") || trips3.as_deref() != Some("3") {
        failures.push(format!(
            "scenario 3a: apres 'redemarrage', circuit=open/breaker_trips=3 attendus (survie), obtenu circuit={:?} breaker_trips={:?} -- \
             l'etat de gouvernance a ete reinitialise au lieu d'etre recharge depuis le disque",
            circuit3, trips3
        ));
    }

    // Meme verification pour le drift, avec un payload SANS SEMANTIC_WARNING
    // (born_in normal) -- si l'etat avait survecu, le ratio doit baisser
    // legerement (6 echantillons, 5 avec warning) mais rester flagged,
    // PAS repartir de 0/false.
    let resp4 = send(port_b, &born_in_payload("drift_sender", "gov_persist_subject_drift_after_restart", "Tokyo")).await;
    let drift_ratio4 = extract_field(&resp4, "GOVERNANCE", "drift_ratio");
    let drift_flagged4 = extract_field(&resp4, "GOVERNANCE", "drift_flagged");
    println!("      drift_sender apres redemarrage: drift_ratio={:?} drift_flagged={:?}", drift_ratio4, drift_flagged4);
    if drift_flagged4.as_deref() != Some("true") {
        failures.push(format!(
            "scenario 3b: apres 'redemarrage', drift_flagged=true attendu (survie, 5 warnings persistes + 1 nouveau payload propre), obtenu drift_ratio={:?} drift_flagged={:?}",
            drift_ratio4, drift_flagged4
        ));
    }

    // ---- Isolation: un sender jamais vu doit rester neutre ----
    println!("[4/4] Sender jamais vu (aucune contamination croisee via la persistance)...");
    let resp5 = send(port_b, &born_in_payload("never_seen_sender", "gov_persist_subject_fresh", "Vienne")).await;
    let circuit5 = extract_field(&resp5, "GOVERNANCE", "circuit");
    let trips5 = extract_field(&resp5, "GOVERNANCE", "breaker_trips");
    println!("      circuit={:?} breaker_trips={:?}", circuit5, trips5);
    if circuit5.as_deref() != Some("closed") || trips5.as_deref() != Some("0") {
        failures.push("scenario 4: un sender jamais vu doit demarrer a l'etat neutre (circuit=closed, breaker_trips=0)".to_string());
    }

    let _ = std::fs::remove_file(&tmp_path_str);

    println!();
    if failures.is_empty() {
        println!("✅ L'etat de gouvernance (breaker + drift) survit correctement a un redemarrage (persistance reelle verifiee en direct).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
