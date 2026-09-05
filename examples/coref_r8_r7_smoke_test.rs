/// examples/coref_r8_r7_smoke_test.rs -- verification live que R8 (coref_with
/// valide contre les DEFINE du meme payload) et R7 (DEFINE mal forme -> dropped
/// plus warning) sont bel et bien appliques sur un vrai payload envoye a un
/// vrai serveur CstlNativeServer, pas seulement testes en isolation dans les
/// modules server::parser et server::validator.
///
/// Contexte (voir CSTL_SPEC_v5_0.md §19) : R8 avait ete completement retire le
/// 2026-09-04 -- son unique implementation dependait de `ast::Block`, un arbre
/// que ni le tokenizer ni le parser reel n'ont jamais construit hors tests.
/// Reconstruit le 2026-09-05 sur le vrai `CstlPayload` (HashMap plat) : il a
/// d'abord fallu apprendre a `server::parser::parse_payload` a reconnaitre les
/// blocs DEFINE (spec §9), qui n'existaient meme pas cote serveur avant ca.
///
/// Scenarios verifies (avertissement seul -- aucun payload rejete) :
/// 1. coref_with vers un DEFINE existant du meme payload -> pas de warning R8.
/// 2. coref_with vers un id jamais DEFINE -> SEMANTIC_WARNING R8.
/// 3. DEFINE avec en-tete malforme (pas de "AS") -> avertissement R7 remonte
///    au client, bloc absent des DEFINE retenus (verifie indirectement: le
///    coref_with qui le visait devient lui-meme orphelin -> R8 en plus du R7).
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
         INTENT_PAYLOAD [purpose=coref_r8_r7_smoke, sender={sender}, receiver=server]\n\
         {body}\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15210;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: coref_with valide (DEFINE present) -> pas de R8 ----
    println!("[1/3] coref_with vers un DEFINE existant du meme payload...");
    let body1 = "DEFINE patient AS human [id=e001]\n\
                 RELATION [type=EQUALS, subject=e002, object=e001, coref_with=e001]";
    let resp1 = send(port, &payload("agent_1", body1)).await;
    let status1 = extract_field(&resp1, "META", "status");
    let has_r8 = resp1.contains("R8:");
    println!("      status={:?} R8_present={}", status1, has_r8);
    if status1.as_deref() != Some("processed") {
        failures.push("scenario 1: un coref_with valide ne doit pas etre rejete".to_string());
    }
    if has_r8 {
        failures.push("scenario 1: coref_with vers un DEFINE existant ne doit PAS produire R8".to_string());
    }

    // ---- Scenario 2: coref_with orphelin -> SEMANTIC_WARNING R8 ----
    println!("[2/3] coref_with vers un id jamais DEFINE...");
    let body2 = "DEFINE patient AS human [id=e001]\n\
                 RELATION [type=EQUALS, subject=e003, object=e999, coref_with=e999]";
    let resp2 = send(port, &payload("agent_2", body2)).await;
    let status2 = extract_field(&resp2, "META", "status");
    let has_r8_2 = resp2.contains("R8:") && resp2.contains("e999");
    println!("      status={:?} R8_present={}", status2, has_r8_2);
    if status2.as_deref() != Some("processed") {
        failures.push("scenario 2: coref_with orphelin ne doit PAS etre rejete (avertissement seul)".to_string());
    }
    if !has_r8_2 {
        failures.push("scenario 2: coref_with=e999 (jamais DEFINE) doit produire un SEMANTIC_WARNING R8".to_string());
    }

    // ---- Scenario 3: DEFINE mal forme (pas de "AS") -> R7 remonte au client,
    // et le coref_with qui le visait devient orphelin (double avertissement) ----
    println!("[3/3] DEFINE avec en-tete malforme (sans AS)...");
    let body3 = "DEFINE patient human [id=e001]\n\
                 RELATION [type=EQUALS, subject=e004, object=e001, coref_with=e001]";
    let resp3 = send(port, &payload("agent_3", body3)).await;
    let status3 = extract_field(&resp3, "META", "status");
    let has_r7 = resp3.contains("R7:");
    let has_r8_3 = resp3.contains("R8:") && resp3.contains("e001");
    println!("      status={:?} R7_present={} R8_present={}", status3, has_r7, has_r8_3);
    if status3.as_deref() != Some("processed") {
        failures.push("scenario 3: un DEFINE mal forme ne doit PAS faire rejeter tout le payload".to_string());
    }
    if !has_r7 {
        failures.push("scenario 3: DEFINE sans 'AS' doit produire un avertissement R7 remonte au client".to_string());
    }
    if !has_r8_3 {
        failures.push("scenario 3: le DEFINE malforme etant droppe, coref_with=e001 devient orphelin -> R8 attendu aussi".to_string());
    }

    println!();
    if failures.is_empty() {
        println!("✅ R7 (DEFINE malforme) et R8 (coref_with) sont conformes de bout en bout sur le vrai serveur TCP.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
