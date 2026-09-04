/// examples/extended_semantic_smoke_test.rs -- verification live que les 11
/// checks de `semantic.rs::SemanticValidator::check_additional_diagnostics`
/// (E108/E109/E701/W502/W503/R9/R10/W602/W603/W604/W605), branches le
/// 2026-09-04 (item #2 de la liste des choses a faire), sont bel et bien
/// appeles sur un vrai payload envoye au serveur -- pas seulement testes en
/// isolation dans semantic.rs.
///
/// Avant ce fix: ces 11 checks etaient testes depuis des mois dans
/// semantic.rs mais JAMAIS appeles par le serveur reel -- seuls
/// check_operator_whitelist et check_axiom_d l'etaient. Cette trouvaille a
/// ete faite en supprimant le systeme Block/AST mort (voir ast.rs):
/// contrairement a ce qui a ete retire, ces checks operent sur `Relation`
/// (toujours peuplee sur le chemin reel), pas sur `Block` (jamais peuplee).
///
/// Scenarios verifies (avertissement seul -- aucun de ces payloads n'est
/// rejete, ValidationResult.valid reste inchange):
/// 1. AMP + INH sur la meme paire (sujet, objet) -> SEMANTIC_WARNING E109.
/// 2. MAINTAIN avec tau=p -> SEMANTIC_WARNING W502.
/// 3. Payload "propre" (aucun operateur/attribut suspect) -> aucun
///    SEMANTIC_WARNING de ce nouveau bloc.
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

fn payload(sender: &str, relation_line: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=extended_semantic_smoke, sender={sender}, receiver=server]\n\
         {relation_line}\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15200;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: AMP + INH sur la meme paire -> E109 ----
    println!("[1/3] AMP + INH sur la meme paire (sujet, objet)...");
    let two_relations = "RELATION [type=AMP, subject=drug_A, object=treatment_response]\nRELATION [type=INH, subject=drug_A, object=treatment_response]";
    let resp1 = send(port, &payload("agent_1", two_relations)).await;
    let status1 = extract_field(&resp1, "META", "status");
    let has_e109 = resp1.contains("E109");
    println!("      status={:?} E109_present={}", status1, has_e109);
    if status1.as_deref() != Some("processed") {
        failures.push("scenario 1: AMP+INH ne doit PAS etre rejete (avertissement seul)".to_string());
    }
    if !has_e109 {
        failures.push("scenario 1: AMP+INH sur la meme paire doit produire un SEMANTIC_WARNING E109".to_string());
    }

    // ---- Scenario 2: MAINTAIN avec tau=p -> W502 ----
    println!("[2/3] MAINTAIN avec tau=p...");
    let maintain_rel = "RELATION [type=MAINTAIN, subject=physician, object=audit_trace, tau=p]";
    let resp2 = send(port, &payload("agent_2", maintain_rel)).await;
    let status2 = extract_field(&resp2, "META", "status");
    let has_w502 = resp2.contains("W502");
    println!("      status={:?} W502_present={}", status2, has_w502);
    if status2.as_deref() != Some("processed") {
        failures.push("scenario 2: MAINTAIN tau=p ne doit PAS etre rejete (avertissement seul)".to_string());
    }
    if !has_w502 {
        failures.push("scenario 2: MAINTAIN avec tau=p doit produire un SEMANTIC_WARNING W502".to_string());
    }

    // ---- Scenario 3: payload propre -> aucun de ces warnings ----
    println!("[3/3] Payload propre (aucun operateur/attribut suspect)...");
    let clean_rel = "RELATION [type=born_in, subject=extended_semantic_subject_3, object=Vienne]";
    let resp3 = send(port, &payload("agent_3", clean_rel)).await;
    let status3 = extract_field(&resp3, "META", "status");
    let has_any_new_warning = ["E108", "E109", "E701", "W502", "W503", "W602", "W603", "W604", "W605"]
        .iter()
        .any(|code| resp3.contains(code));
    println!("      status={:?} any_new_warning_present={}", status3, has_any_new_warning);
    if status3.as_deref() != Some("processed") {
        failures.push("scenario 3: un payload propre doit etre traite normalement".to_string());
    }
    if has_any_new_warning {
        failures.push("scenario 3: un payload propre ne doit declencher AUCUN des 11 nouveaux checks".to_string());
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios de diagnostics semantiques etendus sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
