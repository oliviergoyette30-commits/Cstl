/// examples/strict_deontic_history_smoke_test.rs -- verification live du
/// rejet dur OPT-IN de l'audit deontique cross-history (handler.rs,
/// STEP 3c-quinquies, 2026-10-30).
///
/// Contexte: avant ce fix, une contradiction deontique qui s'etale sur
/// PLUSIEURS payloads distincts (ex: un agent etablit MUST_NOT PERFORM X
/// dans un payload, puis MUST PERFORM X dans un payload suivant) n'etait
/// JAMAIS rejetee -- seulement signalee de facon informative via le bloc
/// DEONTIC_AUDIT [consistent=false, ...], par design assume (un desaccord
/// historique entre agents/moments n'est pas forcement une erreur de
/// protocole). Cette question est restee ouverte toute la soirÃ©e du
/// 2026-10-29 : certains domaines (finance, securite) ont besoin de
/// l'inverse.
///
/// Plutot que de changer le comportement par defaut (casserait la semantique
/// documentee et testee depuis le 2026-09-04, cf.
/// deontic_audit_smoke_test.rs scenario 3), le client opte in explicitement
/// via INTENT_PAYLOAD[strict_deontic_history=true].
///
/// Ce test verifie les DEUX sens, en live contre un vrai serveur TCP:
///   1. SANS le flag (comportement par defaut, inchange) -- la contradiction
///      cross-history est acceptee, seulement signalee (regression directe
///      du design existant, doit rester vrai).
///   2. AVEC le flag -- la MEME contradiction cross-history est rejetee
///      avec purpose=validation_error et un code E107H par violation.
///   3. AVEC le flag, mais AUCUNE contradiction -- le payload passe
///      normalement (pas de faux positif introduit par le flag lui-meme).
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

fn deontic_payload(sender: &str, subject: &str, object: &str, modality: &str, strict: bool) -> String {
    let strict_field = if strict { ", strict_deontic_history=true" } else { "" };
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=StrictSmokeTest, produced_by=StrictSmokeTest]\n\
         INTENT_PAYLOAD [purpose=strict_deontic_smoke, sender={sender}, receiver=server{strict_field}]\n\
         RELATION [type=PERFORM, subject={subject}, object={object}, modality={modality}]\n\
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

    // ---- Etape A: etablir MUST_NOT dans un premier payload ----
    println!("[A] Payload 1: MUST_NOT PERFORM sur (agent_z, wipe_prod)...");
    let resp_a = send(port, &deontic_payload("agent_a", "agent_z", "wipe_prod", "MUST_NOT", false)).await;
    let status_a = extract_field(&resp_a, "META", "status");
    if status_a.as_deref() != Some("processed") {
        failures.push(format!("etape A: le premier payload doit passer normalement, reponse: {}", resp_a));
    }

    // ---- Scenario 1: contradiction cross-history SANS le flag (defaut inchange) ----
    println!("[1/3] Payload 2 SANS strict_deontic_history: MUST sur le meme (subject,object) -- doit PASSER (comportement par defaut inchange)...");
    let resp1 = send(port, &deontic_payload("agent_b1", "agent_z", "wipe_prod", "MUST", false)).await;
    let status1 = extract_field(&resp1, "META", "status");
    let audit1 = extract_field(&resp1, "DEONTIC_AUDIT", "consistent");
    println!("      status={:?} DEONTIC_AUDIT.consistent={:?}", status1, audit1);
    if status1.as_deref() != Some("processed") {
        failures.push(format!("scenario 1: comportement par defaut (sans flag) ne doit JAMAIS rejeter, reponse: {}", resp1));
    }
    if audit1.as_deref() != Some("false") {
        failures.push(format!("scenario 1: DEONTIC_AUDIT doit signaler la contradiction meme sans rejet, reponse: {}", resp1));
    }

    // ---- Scenario 2: la MEME contradiction, AVEC le flag -- doit etre rejetee ----
    println!("[2/3] Payload 3 AVEC strict_deontic_history=true: MUST sur le meme (subject,object) -- doit etre REJETE...");
    let resp2 = send(port, &deontic_payload("agent_b2", "agent_z", "wipe_prod", "MUST", true)).await;
    let purpose2 = extract_field(&resp2, "INTENT_PAYLOAD", "purpose");
    let has_e107h = resp2.contains("E107H");
    println!("      purpose={:?} E107H_present={}", purpose2, has_e107h);
    if purpose2.as_deref() != Some("validation_error") || !has_e107h {
        failures.push(format!("scenario 2: avec le flag, la contradiction cross-history doit etre rejetee avec E107H, reponse: {}", resp2));
    }

    // ---- Scenario 3: AVEC le flag, mais aucune contradiction -- doit passer ----
    println!("[3/3] Payload 4 AVEC strict_deontic_history=true, objet DIFFERENT (aucun conflit) -- doit PASSER...");
    let resp3 = send(port, &deontic_payload("agent_c", "agent_z", "rotate_keys", "MUST", true)).await;
    let status3 = extract_field(&resp3, "META", "status");
    println!("      status={:?}", status3);
    if status3.as_deref() != Some("processed") {
        failures.push(format!("scenario 3: le flag seul (sans contradiction reelle) ne doit jamais causer un rejet, reponse: {}", resp3));
    }

    println!();
    if failures.is_empty() {
        println!("Tous les scenarios strict_deontic_history sont conformes (defaut inchange + rejet dur opt-in fonctionnel).");
    } else {
        println!("{} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
