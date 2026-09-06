/// examples/fipa_negotiation_smoke_test.rs -- verification EN DIRECT (vrai
/// TCP, vrai serveur, pas un test unitaire isole) du mecanisme de
/// negociation FIPA minimal ajoute le 2026-09-06 (voir le commentaire de
/// `semantic::negotiation_status_for` et le bloc `NEGOTIATION` dans
/// `server/handler.rs`).
///
/// Contexte: avant ce mecanisme, le bloc `PERFORMATIVE` (voir
/// `examples/fipa_performative_smoke_test.rs`) etait "dormant" -- le serveur
/// RECONNAISSAIT un performatif recu mais ne reagissait jamais differemment
/// selon son type. Un `PROPOSE` et un `REFUSE` recevaient exactement le meme
/// traitement de fond, seule l'annotation changeait -- aucune boucle
/// PROPOSE -> REFUSE n'etait fermee. Ce test verifie en conditions reelles
/// que:
///
/// 1. Un `PROPOSE` recoit un vrai hash `AUDIT [hash=...]` (calcule par le
///    serveur, pas invente cote client).
/// 2. Un `REFUSE` qui reference ce hash via `INTENT_PAYLOAD.in_reply_to=...`
///    (nouveau champ de wire format, additif) recoit en retour un bloc
///    `NEGOTIATION [status=refused, original_proposal=<meme hash>,
///    original_purpose=PROPOSE, counter_eligible=true]` -- preuve que le
///    serveur a reellement retrouve la proposition originale dans
///    `adn_store`/`audit_trail`, pas juste annote le REFUSE isolement.
/// 3. Un `REFUSE` SANS `in_reply_to` recoit `counter_eligible=false,
///    reason=missing_in_reply_to` -- pas de faux positif.
/// 4. Un `REFUSE` avec un `in_reply_to` qui ne correspond a AUCUN hash connu
///    recoit `counter_eligible=false, reason=original_not_found`.
/// 5. Un `ACCEPT_PROPOSAL` qui reference le meme `PROPOSE` recoit
///    `status=accepted` (et jamais `counter_eligible=true`, l'acceptation ne
///    se contre-propose pas).
/// 6. Regression: un `PROPOSE` (qui n'est ni un refus ni une acceptation)
///    ne recoit JAMAIS de bloc `NEGOTIATION`, meme s'il porte lui-meme un
///    `in_reply_to` (portee volontairement limitee a REFUSE/REJECT_PROPOSAL/
///    ACCEPT_PROPOSAL, voir `negotiation_status_for`).
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

#[tokio::main]
async fn main() {
    let port: u16 = 15191;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Etape 1: un vrai PROPOSE, on capture le hash reel de la reponse ----
    println!("[1/6] PROPOSE (offre de prix) -> capture du hash AUDIT reel...");
    let propose_payload =
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=PROPOSE, sender=agent_buyer, receiver=server]\n\
         RELATION [type=STATE, subject=price, object=100_usd]\n\
         ---END---\n";
    let resp_propose = send(port, propose_payload).await;
    let original_hash = match extract_field(&resp_propose, "AUDIT", "hash") {
        Some(h) => {
            println!("    OK: hash capture = {}", h);
            h
        }
        None => {
            failures.push(format!("1) pas de hash AUDIT dans la reponse au PROPOSE -- reponse: {resp_propose}"));
            String::new()
        }
    };

    // ---- Etape 2: REFUSE qui reference ce PROPOSE via in_reply_to ----
    println!("[2/6] REFUSE avec in_reply_to=<hash du PROPOSE> -> NEGOTIATION attendu...");
    let refuse_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=REFUSE, sender=agent_seller, receiver=server, in_reply_to={original_hash}]\n\
         ---END---\n"
    );
    let resp_refuse = send(port, &refuse_payload).await;
    match extract_field(&resp_refuse, "NEGOTIATION", "status") {
        Some(s) if s == "refused" => println!("    OK: NEGOTIATION[status=refused]"),
        other => failures.push(format!("2) attendu status=refused, obtenu {other:?} -- reponse: {resp_refuse}")),
    }
    match extract_field(&resp_refuse, "NEGOTIATION", "original_proposal") {
        Some(h) if h == original_hash => println!("    OK: original_proposal pointe vers le vrai hash du PROPOSE"),
        other => failures.push(format!(
            "2) attendu original_proposal={original_hash}, obtenu {other:?} -- reponse: {resp_refuse}"
        )),
    }
    match extract_field(&resp_refuse, "NEGOTIATION", "original_purpose") {
        Some(p) if p == "PROPOSE" => println!("    OK: original_purpose=PROPOSE retrouve dans audit_trail"),
        other => failures.push(format!("2) attendu original_purpose=PROPOSE, obtenu {other:?}")),
    }
    match extract_field(&resp_refuse, "NEGOTIATION", "counter_eligible") {
        Some(v) if v == "true" => println!("    OK: counter_eligible=true (une contre-proposition a du sens ici)"),
        other => failures.push(format!("2) attendu counter_eligible=true, obtenu {other:?}")),
    }

    // ---- Etape 3: REFUSE sans in_reply_to ----
    println!("[3/6] REFUSE sans in_reply_to -> counter_eligible=false, reason=missing_in_reply_to...");
    let refuse_no_ref =
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=REFUSE, sender=agent_seller, receiver=server]\n\
         ---END---\n";
    let resp3 = send(port, refuse_no_ref).await;
    match extract_field(&resp3, "NEGOTIATION", "reason") {
        Some(r) if r == "missing_in_reply_to" => println!("    OK: reason=missing_in_reply_to"),
        other => failures.push(format!("3) attendu reason=missing_in_reply_to, obtenu {other:?} -- reponse: {resp3}")),
    }
    match extract_field(&resp3, "NEGOTIATION", "counter_eligible") {
        Some(v) if v == "false" => println!("    OK: counter_eligible=false"),
        other => failures.push(format!("3) attendu counter_eligible=false, obtenu {other:?}")),
    }

    // ---- Etape 4: REFUSE avec in_reply_to inconnu ----
    println!("[4/6] REFUSE avec in_reply_to inconnu -> reason=original_not_found...");
    let refuse_unknown =
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=REFUSE, sender=agent_seller, receiver=server, in_reply_to=sha256:0000000000000000000000000000000000000000000000000000000000000000]\n\
         ---END---\n";
    let resp4 = send(port, refuse_unknown).await;
    match extract_field(&resp4, "NEGOTIATION", "reason") {
        Some(r) if r == "original_not_found" => println!("    OK: reason=original_not_found"),
        other => failures.push(format!("4) attendu reason=original_not_found, obtenu {other:?} -- reponse: {resp4}")),
    }

    // ---- Etape 5: ACCEPT_PROPOSAL qui reference le meme PROPOSE ----
    println!("[5/6] ACCEPT_PROPOSAL avec in_reply_to=<hash du PROPOSE> -> status=accepted, jamais counter_eligible=true...");
    let accept_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=ACCEPT_PROPOSAL, sender=agent_seller, receiver=server, in_reply_to={original_hash}]\n\
         ---END---\n"
    );
    let resp5 = send(port, &accept_payload).await;
    match extract_field(&resp5, "NEGOTIATION", "status") {
        Some(s) if s == "accepted" => println!("    OK: NEGOTIATION[status=accepted]"),
        other => failures.push(format!("5) attendu status=accepted, obtenu {other:?} -- reponse: {resp5}")),
    }
    match extract_field(&resp5, "NEGOTIATION", "counter_eligible") {
        Some(v) if v == "false" => println!("    OK: counter_eligible=false (un accord ne se contre-propose pas)"),
        other => failures.push(format!("5) attendu counter_eligible=false, obtenu {other:?}")),
    }

    // ---- Etape 6: un PROPOSE ne recoit jamais de bloc NEGOTIATION ----
    println!("[6/6] PROPOSE (meme avec in_reply_to) -> AUCUN bloc NEGOTIATION attendu (regression)...");
    let propose_with_ref = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=PROPOSE, sender=agent_buyer, receiver=server, in_reply_to={original_hash}]\n\
         ---END---\n"
    );
    let resp6 = send(port, &propose_with_ref).await;
    if resp6.contains("NEGOTIATION") {
        failures.push(format!("6) bloc NEGOTIATION present a tort pour un PROPOSE -- reponse: {resp6}"));
    } else {
        println!("    OK: aucun bloc NEGOTIATION pour PROPOSE (PROPOSE/CFP n'ouvrent jamais ce bloc)");
    }

    if failures.is_empty() {
        println!("\n✅ TOUS LES SCENARIOS DE NEGOCIATION FIPA PASSENT (6 verifications)");
    } else {
        println!("\n❌ {} ECHEC(S):", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
}
