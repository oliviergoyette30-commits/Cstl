/// examples/temporal_cycle_smoke_test.rs -- verification live que la detection
/// de cycle temporel (code E702, ExecutionLab, 2026-09-05) fonctionne sur un
/// vrai `CstlNativeServer`, pas seulement en isolation dans
/// `execution_lab.rs`.
///
/// Rappel de la difference avec E701 (semantic.rs::check_temporal_pair_
/// consistency, deja branche): E701 detecte une incoherence PAIRWISE -- A
/// BEFORE B et A AFTER B declares pour la MEME paire, dans le MEME payload.
/// E702 detecte un CYCLE qui s'etend sur PLUSIEURS paires distinctes (A
/// BEFORE B, B BEFORE C, C BEFORE A), potentiellement reparties entre
/// plusieurs payloads via l'historique de l'ADN store -- exactement comme le
/// cycle part_of/located_in deja existant, mais sur le graphe temporel
/// normalise BEFORE/AFTER.
///
/// Scenarios verifies:
/// 1. Cycle temporel a 3 noeuds dans UN SEUL payload -> SEMANTIC_WARNING E702,
///    payload quand meme traite (status=processed, jamais rejete).
/// 2. Cycle ferme A TRAVERS deux payloads separes (historique + nouveau) --
///    le premier payload etablit A BEFORE B, le second ferme le cycle avec
///    B BEFORE A -> E702 detecte au second payload seulement.
/// 3. Chaine BEFORE/AFTER non-cyclique (avec un AFTER, verifie la
///    normalisation d'inverse) -> AUCUN E702 (pas de faux positif).
/// 4. Payload propre (aucune relation temporelle) -> AUCUN E702.
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

fn payload(sender: &str, purpose: &str, relation_lines: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose={purpose}, sender={sender}, receiver=server]\n\
         {relation_lines}\n\
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

    // ---- Scenario 1: cycle temporel a 3 noeuds dans un seul payload ----
    println!("[1/4] Cycle temporel a 3 noeuds (A BEFORE B, B BEFORE C, C BEFORE A) dans un seul payload...");
    let cyclic_relations = "RELATION [type=BEFORE, subject=EventA, object=EventB]\n\
                             RELATION [type=BEFORE, subject=EventB, object=EventC]\n\
                             RELATION [type=BEFORE, subject=EventC, object=EventA]";
    let resp1 = send(port, &payload("agent_1", "temporal_cycle_smoke", cyclic_relations)).await;
    let status1 = extract_field(&resp1, "META", "status");
    let has_e702 = resp1.contains("E702");
    println!("      status={:?} E702_present={}", status1, has_e702);
    println!("      reponse brute:\n{}", resp1);
    if status1.as_deref() != Some("processed") {
        failures.push("scenario 1: un cycle temporel ne doit PAS etre rejete (avertissement seul)".to_string());
    }
    if !has_e702 {
        failures.push("scenario 1: un cycle A->B->C->A doit produire un SEMANTIC_WARNING E702".to_string());
    }

    // ---- Scenario 2: cycle ferme a travers deux payloads (historique) ----
    println!("[2/4] Cycle temporel ferme a travers deux payloads separes...");
    let first_half = "RELATION [type=BEFORE, subject=Hist1, object=Hist2]";
    let resp2a = send(port, &payload("agent_2", "temporal_cycle_smoke", first_half)).await;
    let has_e702_first = resp2a.contains("E702");
    println!("      1er payload (Hist1 BEFORE Hist2): E702_present={} (attendu: false)", has_e702_first);
    if has_e702_first {
        failures.push("scenario 2: le premier payload seul ne forme aucun cycle, E702 ne devrait pas apparaitre".to_string());
    }
    let second_half = "RELATION [type=BEFORE, subject=Hist2, object=Hist1]";
    let resp2b = send(port, &payload("agent_2", "temporal_cycle_smoke", second_half)).await;
    let status2b = extract_field(&resp2b, "META", "status");
    let has_e702_second = resp2b.contains("E702");
    println!("      2e payload (Hist2 BEFORE Hist1, ferme le cycle): status={:?} E702_present={}", status2b, has_e702_second);
    if status2b.as_deref() != Some("processed") {
        failures.push("scenario 2: le payload qui ferme le cycle ne doit PAS etre rejete non plus".to_string());
    }
    if !has_e702_second {
        failures.push("scenario 2: le cycle temporel ferme via l'historique de l'ADN store doit produire E702".to_string());
    }

    // ---- Scenario 3: chaine BEFORE/AFTER non-cyclique -- pas de faux positif ----
    println!("[3/4] Chaine BEFORE/AFTER non-cyclique (verifie la normalisation d'inverse)...");
    let non_cyclic = "RELATION [type=BEFORE, subject=ChainA, object=ChainB]\n\
                       RELATION [type=AFTER, subject=ChainC, object=ChainB]\n\
                       RELATION [type=BEFORE, subject=ChainC, object=ChainD]";
    let resp3 = send(port, &payload("agent_3", "temporal_cycle_smoke", non_cyclic)).await;
    let status3 = extract_field(&resp3, "META", "status");
    let has_e702_3 = resp3.contains("E702");
    println!("      status={:?} E702_present={} (attendu: false)", status3, has_e702_3);
    if status3.as_deref() != Some("processed") {
        failures.push("scenario 3: une chaine non-cyclique doit etre traitee normalement".to_string());
    }
    if has_e702_3 {
        failures.push("scenario 3: une chaine BEFORE/AFTER non-cyclique ne doit PAS declencher E702 (faux positif)".to_string());
    }

    // ---- Scenario 4: payload propre (aucune relation temporelle) ----
    println!("[4/4] Payload propre (aucune relation BEFORE/AFTER)...");
    let clean_rel = "RELATION [type=born_in, subject=temporal_cycle_smoke_subject_4, object=Vienne]";
    let resp4 = send(port, &payload("agent_4", "temporal_cycle_smoke", clean_rel)).await;
    let status4 = extract_field(&resp4, "META", "status");
    let has_e702_4 = resp4.contains("E702");
    println!("      status={:?} E702_present={} (attendu: false)", status4, has_e702_4);
    if status4.as_deref() != Some("processed") {
        failures.push("scenario 4: un payload propre doit etre traite normalement".to_string());
    }
    if has_e702_4 {
        failures.push("scenario 4: un payload sans relation temporelle ne doit jamais declencher E702".to_string());
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios de cycle temporel (E702) sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
