/// examples/fipa_performative_smoke_test.rs -- verification EN DIRECT (vrai
/// TCP, vrai serveur, pas un test unitaire isole) de l'ajout FIPA-ACL du
/// 2026-09-06 (voir le commentaire de `semantic::FIPA_PERFORMATIVES`).
///
/// Contexte: `INTENT_PAYLOAD [purpose=...]` existe depuis le debut du projet
/// comme champ d'ENVELOPPE (le type d'acte de communication), separe des
/// `RELATION [type=...]` qui portent le CONTENU semantique -- mais `purpose`
/// n'a jamais recu de vocabulaire structure pour la communication ordinaire
/// entre agents (seuls des purposes de CONTROLE de protocole comme
/// `agent_register` recoivent un traitement special). Ce test verifie que:
///
/// 1. Un payload avec `purpose=PROPOSE` (performatif FIPA reconnu) recoit en
///    reponse un bloc `PERFORMATIVE [type=PROPOSE, recognized=true,
///    relations_attached=N]` -- preuve que le serveur distingue reellement
///    ce vocabulaire, pas juste qu'il l'accepte comme texte libre.
/// 2. Un payload avec un `purpose` arbitraire non-FIPA (ex. "smoke_test")
///    NE recoit PAS ce bloc -- regression directe: l'ajout est purement
///    additif, le comportement pour tout le trafic existant est inchange.
/// 3. La reconnaissance est insensible a la casse (`purpose=propose` en
///    minuscules est aussi reconnu) -- coherent avec `is_fipa_performative`.
/// 4. Le champ `relations_attached` reflete le vrai compte de RELATION du
///    payload envoye, pas une valeur figee.
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

fn payload(purpose: &str, relations: &[&str]) -> String {
    let relation_lines: String = relations
        .iter()
        .map(|r| format!("RELATION [type=STATE, subject=price, object={r}]\n"))
        .collect();
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose={purpose}, sender=agent_1, receiver=server]\n\
         {relation_lines}\
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
    server.agent_registry = Arc::new(Mutex::new(registry));
    server
}

#[tokio::main]
async fn main() {
    let port: u16 = 15190;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: purpose=PROPOSE (performatif FIPA reconnu) ----
    println!("[1/4] purpose=PROPOSE avec 2 relations -> bloc PERFORMATIVE attendu...");
    let resp1 = send(port, &payload("PROPOSE", &["flight_is_direct", "price_is_optimal"])).await;
    match extract_field(&resp1, "PERFORMATIVE", "type") {
        Some(t) if t == "PROPOSE" => println!("    OK: PERFORMATIVE [type=PROPOSE] present"),
        other => failures.push(format!("1) attendu PERFORMATIVE[type=PROPOSE], obtenu {other:?} -- reponse: {resp1}")),
    }
    match extract_field(&resp1, "PERFORMATIVE", "relations_attached") {
        Some(n) if n == "2" => println!("    OK: relations_attached=2 (compte reel)"),
        other => failures.push(format!("1) attendu relations_attached=2, obtenu {other:?}")),
    }

    // ---- Scenario 2: purpose arbitraire non-FIPA -- regression ----
    println!("[2/4] purpose=smoke_test_arbitraire -> AUCUN bloc PERFORMATIVE attendu (regression)...");
    let resp2 = send(port, &payload("smoke_test_arbitraire", &["x"])).await;
    if resp2.contains("PERFORMATIVE") {
        failures.push(format!("2) bloc PERFORMATIVE present a tort pour un purpose non-FIPA -- reponse: {resp2}"));
    } else {
        println!("    OK: aucun bloc PERFORMATIVE (comportement inchange pour le trafic existant)");
    }

    // ---- Scenario 3: insensibilite a la casse ----
    println!("[3/4] purpose=refuse (minuscules) -> reconnu quand meme...");
    let resp3 = send(port, &payload("refuse", &[])).await;
    match extract_field(&resp3, "PERFORMATIVE", "type") {
        Some(t) if t == "REFUSE" => println!("    OK: reconnu et normalise en majuscules (REFUSE)"),
        other => failures.push(format!("3) attendu PERFORMATIVE[type=REFUSE], obtenu {other:?} -- reponse: {resp3}")),
    }
    match extract_field(&resp3, "PERFORMATIVE", "relations_attached") {
        Some(n) if n == "0" => println!("    OK: relations_attached=0 (aucune RELATION jointe, compte exact)"),
        other => failures.push(format!("3) attendu relations_attached=0, obtenu {other:?}")),
    }

    // ---- Scenario 4: un purpose de controle existant n'est pas confondu ----
    println!("[4/4] purpose=agent_register (controle de protocole) -> jamais traite comme performatif FIPA...");
    // Volontairement sans public_key/name -- on veut seulement verifier qu'AUCUN
    // bloc PERFORMATIVE n'apparait, peu importe le sort de l'enregistrement.
    let resp4 = send(port, &payload("agent_register", &[])).await;
    if resp4.contains("PERFORMATIVE") {
        failures.push(format!("4) bloc PERFORMATIVE present a tort pour purpose=agent_register -- reponse: {resp4}"));
    } else {
        println!("    OK: agent_register reste un purpose de controle, jamais confondu avec un performatif");
    }

    if failures.is_empty() {
        println!("\n✅ TOUS LES SCENARIOS FIPA PASSENT ({} verifications)", 4);
    } else {
        println!("\n❌ {} ECHEC(S):", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
}
