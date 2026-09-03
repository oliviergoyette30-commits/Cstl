/// examples/governance_smoke_test.rs -- verification live de la Couche 2
/// (gouvernance/resilience: circuit breaker + quorum 2/3, observation seule).
///
/// Demarre deux instances de CstlNativeServer EN PROCESS (memes conventions
/// que examples/kb_verify_smoke_test.rs), chacune avec son propre AdnStore
/// en memoire (":memory:") pour ne jamais toucher a un vrai cstl_adn.db, et
/// sans TELEGRAM_BOT_TOKEN/OBSIDIAN_VAULT_PATH dans l'environnement -- donc
/// telegram/obsidian restent None, aucun appel reseau reel.
///
/// Scenarios verifies:
/// 1. Un payload normal et coherent -> GOVERNANCE [circuit=closed, ...],
///    status=processed inchange (aucune regression sur le trafic normal).
/// 2. Une meme incoherence repetee -> GOVERNANCE [circuit=open,
///    breaker_trips=3, ...] apparait, ET le payload reste status=processed
///    avec un AUDIT normal (preuve qu'il n'est jamais rejete).
/// 3. Council a 2 membres: membre 1 vote commit -> quorum=1/2,
///    committed=false ; membre 2 distinct vote -> quorum=2/2, committed=true.
/// 4. Council a 1 membre (config par defaut, celle d'aujourd'hui): un seul
///    commit -> committed=true immediatement, comportement inchange.
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::adn_store::AdnStore;
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::CstlNativeServer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

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

fn born_in_payload(sender: &str, subject: &str, city: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=governance_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

fn council_decision_payload(sender: &str, target_hash: &str, decision: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=council_decision, sender={sender}, receiver=server, target_hash={target_hash}, decision={decision}]\n\
         ---END---\n"
    )
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

fn make_test_server(port: u16, council: RestrictedCouncil) -> CstlNativeServer {
    let mut server = CstlNativeServer::new(port);
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "smoke_agent".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
    });
    server.agent_registry = Arc::new(registry);
    server.adn_store = Arc::new(Mutex::new(AdnStore::open(":memory:").expect("in-memory adn_store")));
    server.restricted_council = Arc::new(council);
    server
}

#[tokio::main]
async fn main() {
    // --- Serveur A: config par defaut (1 membre, "Olivier") -- scenarios 1, 2, 4.
    let port_a: u16 = 15150;
    let server_a = make_test_server(port_a, RestrictedCouncil::single_member("Olivier"));
    tokio::spawn(async move {
        server_a.start().await.expect("server_a start");
    });

    // --- Serveur B: council a 2 membres -- scenario 3 (quorum 2/3 -> quorum_size=2).
    let port_b: u16 = 15151;
    let server_b = make_test_server(
        port_b,
        RestrictedCouncil::new(vec!["alice_h".to_string(), "bob_h".to_string()]),
    );
    tokio::spawn(async move {
        server_b.start().await.expect("server_b start");
    });

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: payload normal et coherent ----
    println!("[1/4] Payload normal, coherent...");
    let resp1 = send(port_a, &born_in_payload("agent_gov_1", "governance_smoke_subject_1", "Varsovie")).await;
    let status1 = extract_field(&resp1, "META", "status");
    let circuit1 = extract_field(&resp1, "GOVERNANCE", "circuit");
    println!("      status={:?} circuit={:?}", status1, circuit1);
    if status1.as_deref() != Some("processed") || circuit1.as_deref() != Some("closed") {
        failures.push("scenario 1: attendu status=processed, circuit=closed");
    }

    // ---- Scenario 2: incoherence repetee -> breaker ouvert, jamais rejete ----
    println!("[2/4] Incoherence repetee (meme sujet, villes differentes)...");
    let subject2 = "governance_smoke_subject_2";
    let _ = send(port_a, &born_in_payload("agent_gov_2", subject2, "Varsovie")).await; // baseline
    let _ = send(port_a, &born_in_payload("agent_gov_2", subject2, "Paris")).await; // trip 1
    let _ = send(port_a, &born_in_payload("agent_gov_2", subject2, "Berlin")).await; // trip 2
    let resp2 = send(port_a, &born_in_payload("agent_gov_2", subject2, "Londres")).await; // trip 3 -> open
    let status2 = extract_field(&resp2, "META", "status");
    let circuit2 = extract_field(&resp2, "GOVERNANCE", "circuit");
    let trips2 = extract_field(&resp2, "GOVERNANCE", "breaker_trips");
    let has_audit2 = resp2.contains("AUDIT [hash=");
    println!("      status={:?} circuit={:?} breaker_trips={:?} audit_present={}", status2, circuit2, trips2, has_audit2);
    if status2.as_deref() != Some("processed") {
        failures.push("scenario 2: le payload a ete rejete -- la couche 2 ne doit JAMAIS bloquer");
    }
    if circuit2.as_deref() != Some("open") || trips2.as_deref() != Some("3") {
        failures.push("scenario 2: circuit breaker attendu ouvert avec 3 trips");
    }
    if !has_audit2 {
        failures.push("scenario 2: bloc AUDIT absent -- le payload aurait du etre traite normalement");
    }

    // ---- Scenario 3: quorum 2/3 sur serveur B ----
    println!("[3/4] Quorum 2/3 (council a 2 membres)...");
    let resp_seed = send(port_b, &born_in_payload("agent_gov_3", "governance_smoke_subject_3", "Rome")).await;
    let target_hash = extract_field(&resp_seed, "AUDIT", "hash").expect("hash pour scenario 3");
    let vote1 = send(port_b, &council_decision_payload("alice_h", &target_hash, "commit")).await;
    let purpose1 = extract_field(&vote1, "INTENT_PAYLOAD", "purpose");
    let quorum1 = extract_field(&vote1, "INTENT_PAYLOAD", "quorum");
    let committed1 = extract_field(&vote1, "INTENT_PAYLOAD", "committed");
    println!("      vote 1 (alice_h): purpose={:?} quorum={:?} committed={:?}", purpose1, quorum1, committed1);
    if purpose1.as_deref() != Some("council_decision_recorded") || quorum1.as_deref() != Some("1/2") || committed1.as_deref() != Some("false") {
        failures.push("scenario 3: premier vote aurait du etre 'recorded', quorum=1/2, committed=false");
    }
    let vote2 = send(port_b, &council_decision_payload("bob_h", &target_hash, "commit")).await;
    let purpose2 = extract_field(&vote2, "INTENT_PAYLOAD", "purpose");
    let quorum2 = extract_field(&vote2, "INTENT_PAYLOAD", "quorum");
    let committed2 = extract_field(&vote2, "INTENT_PAYLOAD", "committed");
    println!("      vote 2 (bob_h):   purpose={:?} quorum={:?} committed={:?}", purpose2, quorum2, committed2);
    if purpose2.as_deref() != Some("council_decision_applied") || quorum2.as_deref() != Some("2/2") || committed2.as_deref() != Some("true") {
        failures.push("scenario 3: deuxieme vote distinct aurait du atteindre le quorum et committer");
    }

    // ---- Scenario 4: config a 1 membre (aujourd'hui) -- aucune regression ----
    println!("[4/4] Config a 1 membre (comportement d'avant ce changement)...");
    let resp_seed4 = send(port_a, &born_in_payload("agent_gov_4", "governance_smoke_subject_4", "Tokyo")).await;
    let target_hash4 = extract_field(&resp_seed4, "AUDIT", "hash").expect("hash pour scenario 4");
    let vote4 = send(port_a, &council_decision_payload("Olivier", &target_hash4, "commit")).await;
    let purpose4 = extract_field(&vote4, "INTENT_PAYLOAD", "purpose");
    let committed4 = extract_field(&vote4, "INTENT_PAYLOAD", "committed");
    println!("      purpose={:?} committed={:?}", purpose4, committed4);
    if purpose4.as_deref() != Some("council_decision_applied") || committed4.as_deref() != Some("true") {
        failures.push("scenario 4: un seul commit en config a 1 membre doit committer immediatement (regression)");
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios de gouvernance sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
