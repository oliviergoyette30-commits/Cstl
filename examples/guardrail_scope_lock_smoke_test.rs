/// examples/guardrail_scope_lock_smoke_test.rs -- verification live que
/// GUARDRAIL_REPORT et SCOPE_LOCK (ajoutes 2026-09-08) produisent bien un
/// effet observable sur la reponse d'un vrai CstlNativeServer, pas seulement
/// en isolation dans server::parser/server::validator.
///
/// Contexte (voir CSTL_SPEC_v5_0.md, nouvelle section) : les 4 blocs
/// GUARDRAIL_REPORT / SCOPE_LOCK / EXECUTION_TRACE / ERROR_SIGNAL ont ete
/// valides EMPIRIQUEMENT EN CONVERSATION (Claude+Gemini+ChatGPT, pas du code)
/// le 22 mai 2026, sans jamais etre portes en Rust. Ce smoke test couvre les
/// deux qui avaient une fonction deja testee en conversation a l'epoque :
/// GUARDRAIL_REPORT (ChatGPT V2) et SCOPE_LOCK (Gemini V2).
///
/// LIMITE HONNETE, repetee ici volontairement (voir aussi les commentaires de
/// server/handler.rs et server/validator.rs) : ce test verifie seulement que
/// le SERVEUR relaie fidelement un GUARDRAIL_REPORT et confirme/detecte une
/// derive de SCOPE_LOCK dans les RELATION structurees d'UN payload. Il ne
/// verifie PAS -- et ne peut structurellement PAS verifier, faute d'un vrai
/// LLM tiers connecte en boucle -- qu'un LLM receveur reel produirait ou
/// respecterait ces blocs dans une reponse en langage naturel.
///
/// Scenarios verifies :
/// 1. GUARDRAIL_REPORT [status=BLOCKED, reason=...] -> relaye fidelement
///    (GUARDRAIL_REPORT_RELAYED) dans la reponse serveur.
/// 2. GUARDRAIL_REPORT sans 'status' -> rejet E311 (payload invalide).
/// 3. SCOPE_LOCK [mode=STRICT, allowed_ids=...] -> confirme explicitement
///    (SCOPE_LOCK_ACK) dans la reponse, meme quand aucune derive n'existe.
/// 4. SCOPE_LOCK STRICT + RELATION hors allowed_ids -> SEMANTIC_WARNING W607
///    (avertissement seul, payload quand meme "processed").
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
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
    server.agent_registry = Arc::new(Mutex::new(registry));
    server
}

fn payload(sender: &str, body: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=guardrail_scope_lock_smoke, sender={sender}, receiver=server]\n\
         {body}\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15211;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: GUARDRAIL_REPORT bien forme -> relaye fidelement ----
    println!("[1/4] GUARDRAIL_REPORT [status=BLOCKED, reason=...] relaye...");
    let body1 = "GUARDRAIL_REPORT [status=BLOCKED, reason=policy_violation, blocked_operator=PERFORM]";
    let resp1 = send(port, &payload("agent_1", body1)).await;
    let status1 = extract_field(&resp1, "META", "status");
    let relayed_status = extract_field(&resp1, "GUARDRAIL_REPORT_RELAYED", "status");
    let relayed_reason = extract_field(&resp1, "GUARDRAIL_REPORT_RELAYED", "reason");
    println!("      status={:?} relayed_status={:?} relayed_reason={:?}", status1, relayed_status, relayed_reason);
    if status1.as_deref() != Some("processed") {
        failures.push("scenario 1: un GUARDRAIL_REPORT bien forme ne doit pas faire rejeter le payload".to_string());
    }
    if relayed_status.as_deref() != Some("BLOCKED") || relayed_reason.as_deref() != Some("policy_violation") {
        failures.push("scenario 1: GUARDRAIL_REPORT_RELAYED doit reprendre fidelement status/reason".to_string());
    }

    // ---- Scenario 2: GUARDRAIL_REPORT sans 'status' -> rejet E311 ----
    println!("[2/4] GUARDRAIL_REPORT sans 'status' (format invalide)...");
    let body2 = "GUARDRAIL_REPORT [reason=policy_violation]";
    let resp2 = send(port, &payload("agent_2", body2)).await;
    let status2 = extract_field(&resp2, "META", "status");
    let has_e311 = resp2.contains("E311");
    println!("      status={:?} E311_present={}", status2, has_e311);
    if status2.as_deref() != Some("error") || !has_e311 {
        failures.push("scenario 2: GUARDRAIL_REPORT sans 'status' doit etre rejete avec E311".to_string());
    }

    // ---- Scenario 3: SCOPE_LOCK STRICT sans derive -> SCOPE_LOCK_ACK ----
    println!("[3/4] SCOPE_LOCK [mode=STRICT] confirme dans la reponse...");
    let body3 = "DEFINE patient AS human [id=e001]\n\
                 SCOPE_LOCK [mode=STRICT, allowed_ids=\"e001\"]\n\
                 RELATION [type=EQUALS, subject=e001, object=e001]";
    let resp3 = send(port, &payload("agent_3", body3)).await;
    let status3 = extract_field(&resp3, "META", "status");
    let ack_mode = extract_field(&resp3, "SCOPE_LOCK_ACK", "mode");
    let has_w607_3 = resp3.contains("W607");
    println!("      status={:?} ack_mode={:?} W607_present={}", status3, ack_mode, has_w607_3);
    if status3.as_deref() != Some("processed") {
        failures.push("scenario 3: un SCOPE_LOCK bien forme sans derive ne doit pas etre rejete".to_string());
    }
    if ack_mode.as_deref() != Some("STRICT") {
        failures.push("scenario 3: SCOPE_LOCK_ACK doit confirmer mode=STRICT".to_string());
    }
    if has_w607_3 {
        failures.push("scenario 3: aucune RELATION hors scope -> pas de W607 attendu".to_string());
    }

    // ---- Scenario 4: SCOPE_LOCK STRICT + RELATION hors scope -> W607 ----
    println!("[4/4] SCOPE_LOCK STRICT + RELATION hors allowed_ids -> W607...");
    let body4 = "SCOPE_LOCK [mode=STRICT, allowed_ids=\"e001\"]\n\
                 RELATION [type=EQUALS, subject=e001, object=e999]";
    let resp4 = send(port, &payload("agent_4", body4)).await;
    let status4 = extract_field(&resp4, "META", "status");
    let has_w607_4 = resp4.contains("W607") && resp4.contains("e999");
    println!("      status={:?} W607_present={}", status4, has_w607_4);
    if status4.as_deref() != Some("processed") {
        failures.push("scenario 4: une derive de scope ne doit etre qu'un avertissement, pas un rejet".to_string());
    }
    if !has_w607_4 {
        failures.push("scenario 4: RELATION.object=e999 hors allowed_ids doit produire un SEMANTIC_WARNING W607".to_string());
    }

    println!();
    if failures.is_empty() {
        println!("✅ GUARDRAIL_REPORT (relai + E311) et SCOPE_LOCK (ACK + W607) sont conformes de bout en bout sur le vrai serveur TCP.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
