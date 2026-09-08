/// examples/execution_trace_error_signal_smoke_test.rs -- verification live que
/// EXECUTION_TRACE et ERROR_SIGNAL (ajoutes 2026-09-08) produisent bien un
/// effet observable sur la reponse d'un vrai CstlNativeServer, pas seulement
/// en isolation dans server::parser/server::validator/server::handler.
///
/// Contexte (voir CSTL_SPEC_v5_0.md §16.6) : les 4 blocs GUARDRAIL_REPORT /
/// SCOPE_LOCK / EXECUTION_TRACE / ERROR_SIGNAL ont ete valides EMPIRIQUEMENT
/// EN CONVERSATION (Claude+Gemini+ChatGPT, pas du code) le 22 mai 2026. Les
/// deux premiers ont ete portes le 2026-09-08 (voir
/// examples/guardrail_scope_lock_smoke_test.rs) -- ce test-ci couvre les deux
/// derniers, qui etaient encore purement theoriques (jamais testes, meme en
/// conversation, au 22 mai 2026) : aucune grammaire EBNF n'existait pour eux
/// avant cet ajout, elle a ete concue de zero pour ce commit.
///
/// LIMITE HONNETE, repetee ici volontairement (voir aussi les commentaires de
/// server/handler.rs et CSTL_SPEC_v5_0.md §16.6) :
/// - EXECUTION_TRACE documente ce qui a REELLEMENT tourne SUR CE SERVEUR
///   POUR CE payload -- il ne garantit rien sur ce qu'un LLM tiers fait de la
///   reponse ensuite.
/// - ERROR_SIGNAL ne couvre ICI que la detection de violation deontique
///   ([NOT]/MUST_NOT, Axiome D) a travers l'historique persiste. La
///   divergence de `sigma=` (autre moitie de la FONCTION documentee le
///   22 mai 2026) N'EST PAS implementee -- aucune identite de relation ne
///   survit au-dela d'un payload dans ce depot (voir CSTL_SPEC_v5_0.md §16.6
///   pour le detail complet des trois raisons verifiees, pas supposees).
///
/// Scenarios verifies :
/// 1. Payload normal (une RELATION factuelle simple, sans modality) ->
///    EXECUTION_TRACE [verdict=PASS, semantic_validation=PASS,
///    consistency_check=PASS, deontic_audit=PASS] present dans la reponse.
/// 2. Deux payloads successifs qui etablissent un MUST puis un MUST_NOT sur
///    le meme (subject, object) -- Couche 8, deja teste isolement dans
///    examples/deontic_audit_smoke_test.rs -- font apparaitre un
///    ERROR_SIGNAL [role=REPORT, signal_type=DEONTIC_VIOLATION,
///    status=DETECTED, ...] sur le DEUXIEME payload, avec le detail exact de
///    la violation (subject/object/required_by/forbidden_by).
/// 3. Cas negatif SANS requete client : un payload propre (aucune violation)
///    ne produit AUCUN bloc ERROR_SIGNAL -- silence, pas de bruit sur le
///    trafic normal (meme convention que DEONTIC_AUDIT).
/// 4. Cas negatif AVEC requete client explicite (`ERROR_SIGNAL
///    [role=REQUEST]`) sur un payload propre : ERROR_SIGNAL [role=REPORT,
///    signal_type=NONE, status=CLEAN] apparait -- confirmation explicite
///    plutot que silence ambigu.
/// 5. `ERROR_SIGNAL [role=REPORT]` envoye par un CLIENT (usurpant la forme
///    serveur) est rejete avec E316 (payload invalide).
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

fn count_blocks(response: &str, block: &str) -> usize {
    response.lines().filter(|l| l.starts_with(block)).count()
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

fn payload(sender: &str, purpose: &str, body: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose={purpose}, sender={sender}, receiver=server]\n\
         {body}\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15212;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: payload normal -> EXECUTION_TRACE genere, verdict=PASS ----
    println!("[1/5] Payload normal -> EXECUTION_TRACE [verdict=PASS, ...]...");
    let body1 = "RELATION [type=EQUALS, subject=alice, object=alice]";
    let resp1 = send(port, &payload("agent_1", "smoke_execution_trace", body1)).await;
    let status1 = extract_field(&resp1, "META", "status");
    let verdict1 = extract_field(&resp1, "EXECUTION_TRACE", "verdict");
    let semantic1 = extract_field(&resp1, "EXECUTION_TRACE", "semantic_validation");
    let consistency1 = extract_field(&resp1, "EXECUTION_TRACE", "consistency_check");
    let deontic1 = extract_field(&resp1, "EXECUTION_TRACE", "deontic_audit");
    let scope1 = extract_field(&resp1, "EXECUTION_TRACE", "scope");
    println!(
        "      status={:?} verdict={:?} semantic={:?} consistency={:?} deontic={:?} scope={:?}",
        status1, verdict1, semantic1, consistency1, deontic1, scope1
    );
    if status1.as_deref() != Some("processed") {
        failures.push("scenario 1: un payload normal ne doit pas etre rejete".to_string());
    }
    if verdict1.as_deref() != Some("PASS")
        || semantic1.as_deref() != Some("PASS")
        || consistency1.as_deref() != Some("PASS")
        || deontic1.as_deref() != Some("PASS")
    {
        failures.push(format!(
            "scenario 1: EXECUTION_TRACE doit etre tout PASS sur un payload propre, recu: {}",
            resp1
        ));
    }
    if scope1.as_deref() != Some("SERVER_LOCAL_THIS_PAYLOAD") {
        failures.push("scenario 1: EXECUTION_TRACE.scope doit documenter la portee honnete SERVER_LOCAL_THIS_PAYLOAD".to_string());
    }

    // ---- Scenario 2: MUST puis MUST_NOT sur le meme (subject,object) -> ERROR_SIGNAL ----
    println!("[2/5] MUST puis MUST_NOT historique -> ERROR_SIGNAL [signal_type=DEONTIC_VIOLATION]...");
    let body2a = "RELATION [type=PERFORM, subject=agentX, object=actionY, modality=MUST]";
    let resp2a = send(port, &payload("agent_2a", "smoke_deontic_1", body2a)).await;
    let status2a = extract_field(&resp2a, "META", "status");
    if status2a.as_deref() != Some("processed") {
        failures.push(format!("scenario 2 (setup): le premier payload MUST doit etre accepte, recu: {}", resp2a));
    }

    let body2b = "RELATION [type=PERFORM, subject=agentX, object=actionY, modality=MUST_NOT]";
    let resp2b = send(port, &payload("agent_2b", "smoke_deontic_2", body2b)).await;
    let status2b = extract_field(&resp2b, "META", "status");
    let error_signal_type = extract_field(&resp2b, "ERROR_SIGNAL", "signal_type");
    let error_signal_status = extract_field(&resp2b, "ERROR_SIGNAL", "status");
    let error_signal_subject = extract_field(&resp2b, "ERROR_SIGNAL", "subject");
    let error_signal_object = extract_field(&resp2b, "ERROR_SIGNAL", "object");
    println!(
        "      status={:?} signal_type={:?} status_field={:?} subject={:?} object={:?}",
        status2b, error_signal_type, error_signal_status, error_signal_subject, error_signal_object
    );
    if status2b.as_deref() != Some("processed") {
        failures.push("scenario 2: une contradiction deontique historique reste INFORMATIVE, jamais un rejet".to_string());
    }
    if error_signal_type.as_deref() != Some("DEONTIC_VIOLATION") || error_signal_status.as_deref() != Some("DETECTED") {
        failures.push(format!(
            "scenario 2: ERROR_SIGNAL [signal_type=DEONTIC_VIOLATION, status=DETECTED] attendu, reponse: {}",
            resp2b
        ));
    }
    if error_signal_subject.as_deref() != Some("agentX") || error_signal_object.as_deref() != Some("actionY") {
        failures.push("scenario 2: ERROR_SIGNAL doit porter le subject/object exacts de la violation".to_string());
    }

    // ---- Scenario 3: payload propre SANS requete -> aucun ERROR_SIGNAL ----
    println!("[3/5] Payload propre, sans requete -> silence (aucun ERROR_SIGNAL)...");
    let body3 = "RELATION [type=EQUALS, subject=bob, object=bob]";
    let resp3 = send(port, &payload("agent_3", "smoke_clean_silent", body3)).await;
    let has_error_signal3 = resp3.contains("ERROR_SIGNAL");
    println!("      ERROR_SIGNAL_present={}", has_error_signal3);
    if has_error_signal3 {
        failures.push(format!("scenario 3: aucune violation, aucune requete -> pas d'ERROR_SIGNAL attendu, reponse: {}", resp3));
    }

    // ---- Scenario 4: payload propre AVEC requete explicite -> status=CLEAN ----
    println!("[4/5] Payload propre + ERROR_SIGNAL [role=REQUEST] -> status=CLEAN...");
    let body4 = "RELATION [type=EQUALS, subject=carol, object=carol]\nERROR_SIGNAL [role=REQUEST]";
    let resp4 = send(port, &payload("agent_4", "smoke_clean_requested", body4)).await;
    let status4 = extract_field(&resp4, "META", "status");
    let signal_type4 = extract_field(&resp4, "ERROR_SIGNAL", "signal_type");
    let signal_status4 = extract_field(&resp4, "ERROR_SIGNAL", "status");
    println!("      status={:?} signal_type={:?} signal_status={:?}", status4, signal_type4, signal_status4);
    if status4.as_deref() != Some("processed") {
        failures.push("scenario 4: une requete ERROR_SIGNAL bien formee ne doit pas faire rejeter le payload".to_string());
    }
    if signal_type4.as_deref() != Some("NONE") || signal_status4.as_deref() != Some("CLEAN") {
        failures.push(format!(
            "scenario 4: ERROR_SIGNAL [signal_type=NONE, status=CLEAN] attendu sur requete explicite + payload propre, reponse: {}",
            resp4
        ));
    }
    if count_blocks(&resp4, "ERROR_SIGNAL") != 1 {
        failures.push("scenario 4: exactement un ERROR_SIGNAL attendu (pas de doublon avec l'auto-emission)".to_string());
    }

    // ---- Scenario 5: ERROR_SIGNAL [role=REPORT] envoye par un client -> E316 ----
    println!("[5/5] ERROR_SIGNAL [role=REPORT] usurpe par un client -> rejet E316...");
    let body5 = "ERROR_SIGNAL [role=REPORT]";
    let resp5 = send(port, &payload("agent_5", "smoke_role_spoof", body5)).await;
    let status5 = extract_field(&resp5, "META", "status");
    let has_e316 = resp5.contains("E316");
    println!("      status={:?} E316_present={}", status5, has_e316);
    if status5.as_deref() != Some("error") || !has_e316 {
        failures.push(format!("scenario 5: role=REPORT venant d'un client doit etre rejete avec E316, reponse: {}", resp5));
    }

    println!();
    if failures.is_empty() {
        println!("✅ EXECUTION_TRACE (verdict/semantic_validation/consistency_check/deontic_audit/scope) et ERROR_SIGNAL (detection deontique DETECTED/CLEAN + rejet E316 de la forme REPORT usurpee) sont conformes de bout en bout sur le vrai serveur TCP.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
