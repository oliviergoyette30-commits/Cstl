/// examples/epistemic_antonym_smoke_test.rs -- verification live du fix
/// E703 (2026-09-23), meme patron que examples/deontic_audit_smoke_test.rs.
///
/// Trouvaille (cstl_comprehension_test.py, item edge_001, "Alice does not
/// believe Bob left early"): avant ce fix, OFFICIAL_OPERATORS n'avait aucun
/// antonyme pour BELIEVES/KNOWS/ASSUMES, un encodeur LLM a improvise
/// "DISBELIEVES" (passe la whitelist E101, avertissement seul), mais rien
/// ne reliait ce mot a BELIEVES -- une contradiction directe passait
/// inapercue meme quand les deux relations etaient explicitement presentes,
/// dans le meme payload OU a travers l'historique.
///
/// Scenarios verifies en direct contre le vrai serveur TCP compile:
/// 1. DISBELIEVES seul -- traite normalement, plus de warning E101 (mot
///    maintenant officiel).
/// 2. BELIEVES et DISBELIEVES sur le MEME (subject, object), MEME payload
///    -- SEMANTIC_WARNING E703 present (check intra-payload, semantic.rs).
/// 3. BELIEVES etabli par un PREMIER payload, DISBELIEVES sur le MEME
///    (subject, object) dans un DEUXIEME payload distinct -- SEMANTIC_
///    WARNING E703 present (check historique, execution_lab.rs) -- le cas
///    qu'un simple check intra-payload ne peut PAS voir.
/// 4. BELIEVES et DISBELIEVES sur des OBJETS differents -- aucun warning
///    E703 (pas de faux positif).
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::server::CstlNativeServer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;

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

fn epistemic_payload(sender: &str, subject: &str, object: &str, operators: &[&str]) -> String {
    let relations: String = operators.iter().map(|op| {
        format!("RELATION [type={op}, subject={subject}, object={object}]\n")
    }).collect();
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=epistemic_antonym_smoke, sender={sender}, receiver=server]\n\
         {relations}\
         ---END---\n"
    )
}

fn make_test_server(port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server
}

#[tokio::main]
async fn main() {
    let port: u16 = 15181;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: DISBELIEVES seul -- doit passer sans warning E101 ----
    println!("[1/4] DISBELIEVES seul (doit passer la whitelist, plus de E101)...");
    let resp1 = send(port, &epistemic_payload("agent_1", "alice", "bob_left_early", &["DISBELIEVES"])).await;
    let has_e101 = resp1.contains("E101");
    println!("      E101_present={}", has_e101);
    if has_e101 {
        failures.push(format!("scenario 1: DISBELIEVES doit etre reconnu (plus de E101), reponse: {}", resp1));
    }

    // ---- Scenario 2: BELIEVES + DISBELIEVES, meme (subject,object), MEME payload ----
    println!("[2/4] BELIEVES et DISBELIEVES sur le meme (subject,object), meme payload (doit avoir E703)...");
    let resp2 = send(port, &epistemic_payload("agent_2", "alice", "bob_left_early", &["BELIEVES", "DISBELIEVES"])).await;
    let has_e703_2 = resp2.contains("E703");
    println!("      E703_present={}", has_e703_2);
    if !has_e703_2 {
        failures.push(format!("scenario 2: contradiction intra-payload doit produire E703, reponse: {}", resp2));
    }

    // ---- Scenario 3: BELIEVES (payload A) puis DISBELIEVES (payload B distinct) ----
    println!("[3/4] BELIEVES etabli par un payload, DISBELIEVES sur le meme (subject,object) dans un AUTRE payload...");
    let resp3a = send(port, &epistemic_payload("agent_3a", "carla", "project_failed", &["BELIEVES"])).await;
    let has_e703_3a = resp3a.contains("E703");
    if has_e703_3a {
        failures.push(format!("scenario 3a: aucune contradiction attendue sur le premier payload, reponse: {}", resp3a));
    }
    let resp3b = send(port, &epistemic_payload("agent_3b", "carla", "project_failed", &["DISBELIEVES"])).await;
    let has_e703_3b = resp3b.contains("E703");
    println!("      E703_present_sur_2e_payload={}", has_e703_3b);
    if !has_e703_3b {
        failures.push(format!("scenario 3b: contradiction HISTORIQUE doit produire E703, reponse: {}", resp3b));
    }

    // ---- Scenario 4: BELIEVES et DISBELIEVES sur des objets DIFFERENTS ----
    println!("[4/4] BELIEVES et DISBELIEVES sur des objets differents (aucun E703 attendu)...");
    let resp4a = send(port, &epistemic_payload("agent_4", "dan", "fact_x", &["BELIEVES"])).await;
    let resp4b = send(port, &epistemic_payload("agent_4", "dan", "fact_y", &["DISBELIEVES"])).await;
    let has_e703_4 = resp4a.contains("E703") || resp4b.contains("E703");
    println!("      E703_present={}", has_e703_4);
    if has_e703_4 {
        failures.push(format!("scenario 4: objets differents ne doivent PAS produire E703, reponses: {} / {}", resp4a, resp4b));
    }

    println!("\n{}", "=".repeat(70));
    if failures.is_empty() {
        println!("TOUS LES SCENARIOS PASSENT (4/4) -- fix E703 verifie en direct contre le vrai serveur TCP.");
    } else {
        println!("ECHECS ({}/4):", failures.len());
        for f in &failures {
            println!("  - {}", f);
        }
        std::process::exit(1);
    }
}
