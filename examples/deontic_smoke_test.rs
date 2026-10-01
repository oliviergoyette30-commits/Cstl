/// examples/deontic_smoke_test.rs -- verification live du cablage de
/// `deontic_orchestration.rs` (Couche 9, 2026-10-01) sur le pipeline reel
/// d'arbitrage (`arbitrage.rs`) et d'enregistrement d'agents
/// (`purpose=agent_register`), via `/deontic/executions` (REST).
///
/// Contexte: `deontic_orchestration.rs` existait depuis des mois (700
/// lignes, 15 tests d'integration reels dans
/// `tests/deontic_orchestration_integration_test.rs`) mais n'etait JAMAIS
/// construit ni appele nulle part -- le README affirmait pourtant "v5.1
/// COMPLETE" avec un `src/server/arbitration_api.rs` qui s'est revele etre
/// un fichier VIDE (0 octet). Son module jumeau, `deontic_state_machine.rs`
/// (543 lignes, 15 tests unitaires reels), s'est revele DUPLIQUER
/// `arbitrage.rs` deja en production (persistance SQLite, signatures
/// Ed25519, deja cable dans handler.rs) -- en memoire seulement, sans
/// signature, sans hash chain malgre l'affirmation README. Decision
/// utilisateur (2026-10-01): cabler l'ORCHESTRATEUR sur le systeme
/// `arbitrage.rs` deja reel, laisser `deontic_state_machine.rs` superseded.
///
/// Limite honnete de ce cablage, prouvee ci-dessous plutot qu'affirmee:
/// `emit_event` est appele APRES que l'action sous-jacente a deja ete
/// commise (ecriture registre, persistance SQLite du ruling) -- c'est une
/// couche d'observation/audit, PAS une garde preventive. Les handlers
/// MUST/MUST_NOT de `deontic_orchestration.rs` eux-memes ne font que logger
/// (`eprintln!`), ils ne bloquent rien.
///
/// Scenarios verifies:
/// 1. `/deontic/executions` avant tout trafic -- 3 regles par defaut
///    enregistrees au demarrage (`start()`), 0 execution encore.
/// 2. `purpose=agent_register` reel (vraie paire de cles Ed25519, vraie
///    signature, meme pattern que signing_registration_smoke_test.rs) sur
///    le port TCP -> `/deontic/executions` reflete une execution reelle
///    (modality=Must, event_type implicite via l'action de la regle
///    "log_agent_registration", result=Success).
/// 3. Un vrai cas d'arbitrage ouvert + arbitre assigne + ruling soumis
///    (`purpose=arbitrage_channel` sur le meme port TCP, systeme
///    `arbitrage.rs` deja en production) -> `/deontic/executions` reflete
///    une DEUXIEME execution (modality=Must, action="log_ruling_applied").
///
/// Non verifie ici (documente honnetement, pas simule): la regle
/// `governance_breach` (MAY) existe et est cablee dans handler.rs (emise
/// quand `GovernanceState.circuit_open` ou `.drift_flagged`), mais
/// declencher un vrai circuit breaker exige un historique d'incoherences
/// construit sur plusieurs payloads -- hors de la portee de ce smoke test
/// ponctuel. Couverte par lecture de code + les tests existants de
/// `governance.rs`, pas par un trafic live ici.
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::audit::signing_bytes;
use cstl_parser::server::parser::parse_payload;
use cstl_parser::server::CstlNativeServer;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
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

fn make_test_server(tcp_port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(tcp_port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "deontic_smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("olivier"));
    server
}

fn arbitrage_channel_payload(action: &str, case_id: Option<&str>, extra: &str) -> String {
    let case_field = case_id.map(|c| format!(", case_id={c}")).unwrap_or_default();
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=DeonticSmokeTest, produced_by=DeonticSmokeTest]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=deontic_smoke_opener, receiver=server, action={action}{case_field}{extra}]\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let tcp_port: u16 = 15200;
    let rest_port: u16 = 15201;

    let server = make_test_server(tcp_port);
    let adn_store_for_rest = server.adn_store.clone();
    let chain_for_rest = server.chain.clone();
    let deontic_for_rest = server.deontic.clone();

    tokio::spawn(async move {
        server.start().await.expect("TCP server start");
    });
    tokio::spawn(async move {
        cstl_parser::server::rest_api::start_rest_api(adn_store_for_rest, chain_for_rest, deontic_for_rest, "127.0.0.1", rest_port)
            .await
            .expect("REST API start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();
    let http = reqwest::Client::new();
    let deontic_url = format!("http://127.0.0.1:{}/deontic/executions", rest_port);

    // ---- Scenario 1: 3 regles par defaut, 0 execution avant tout trafic ----
    println!("[1/3] /deontic/executions avant tout trafic -- regles enregistrees, aucune execution...");
    match http.get(&deontic_url).send().await {
        Ok(resp) => {
            let body: serde_json::Value = resp.json().await.expect("JSON valide");
            let rules_count = body["rules_count"].as_u64();
            let execution_count = body["execution_count"].as_u64();
            println!("      rules_count={:?} execution_count={:?}", rules_count, execution_count);
            if rules_count != Some(3) {
                failures.push(format!("scenario 1: 3 regles par defaut attendues (agent_register/arbitration_ruling/governance_breach), recu {:?}", rules_count));
            }
            if execution_count != Some(0) {
                failures.push(format!("scenario 1: 0 execution attendue avant tout trafic, recu {:?}", execution_count));
            }
        }
        Err(e) => failures.push(format!("scenario 1: requete HTTP echouee: {e}")),
    }

    // ---- Scenario 2: agent_register reel (vraie signature Ed25519) ----
    println!("[2/3] purpose=agent_register reel (Ed25519) -> execution deontique Must/log_agent_registration...");
    let signing_key = SigningKey::generate(&mut OsRng);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let agent_name = "deontic_smoke_agent";
    let draft = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=DeonticSmokeTest, produced_by=DeonticSmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication]\n\
         ---END---\n"
    );
    let parsed_draft = parse_payload(&draft).expect("brouillon agent_register doit parser");
    let sig = signing_key.sign(&signing_bytes(&parsed_draft));
    let sig_hex = hex::encode(sig.to_bytes());
    let register_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=DeonticSmokeTest, produced_by=DeonticSmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication, signature={sig_hex}]\n\
         ---END---\n"
    );
    let resp_register = send(tcp_port, &register_payload).await;
    let purpose_register = extract_field(&resp_register, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose_register);
    if purpose_register.as_deref() != Some("agent_register_ack") {
        failures.push(format!("scenario 2: agent_register aurait du reussir (agent_register_ack), recu purpose={:?}", purpose_register));
    }

    match http.get(&deontic_url).send().await {
        Ok(resp) => {
            let body: serde_json::Value = resp.json().await.expect("JSON valide");
            let execution_count = body["execution_count"].as_u64();
            let executions = body["executions"].as_array().cloned().unwrap_or_default();
            let found = executions.iter().any(|e| {
                e["action"] == "log_agent_registration"
                    && e["modality"] == "Must"
                    && e["result"] == "Success"
            });
            println!("      execution_count={:?} agent_register_execution_found={}", execution_count, found);
            if !found {
                failures.push("scenario 2: /deontic/executions doit contenir une execution Must/log_agent_registration/Success apres l'agent_register reel".to_string());
            }
        }
        Err(e) => failures.push(format!("scenario 2: requete HTTP echouee: {e}")),
    }

    // ---- Scenario 3: vrai cas d'arbitrage -> ruling signe -> execution deontique ----
    println!("[3/3] Cas d'arbitrage reel (arbitre enregistre + ouverture + assignation + ruling SIGNE) -> execution deontique Must/log_ruling_applied...");

    // Un arbitre doit exister AVANT assign_arbiters (select_arbiters_round_robin
    // tire dans le registre reel des arbitres actifs, voir arbitrage.rs) --
    // meme pattern self-signe que agent_register.
    let arbiter_key = SigningKey::generate(&mut OsRng);
    let arbiter_pubkey_hex = hex::encode(arbiter_key.verifying_key().to_bytes());
    let arbiter_id = "deontic_smoke_arbiter";
    let arbiter_draft = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=DeonticSmokeTest, produced_by=DeonticSmokeTest, public_key={arbiter_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=deontic_smoke_opener, receiver=server, arbiter_id={arbiter_id}, authority_level=senior, stake_amount=100]\n\
         ---END---\n"
    );
    let arbiter_parsed = parse_payload(&arbiter_draft).expect("brouillon arbiter_register doit parser");
    let arbiter_sig = arbiter_key.sign(&signing_bytes(&arbiter_parsed));
    let arbiter_sig_hex = hex::encode(arbiter_sig.to_bytes());
    let arbiter_register_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=DeonticSmokeTest, produced_by=DeonticSmokeTest, public_key={arbiter_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=deontic_smoke_opener, receiver=server, arbiter_id={arbiter_id}, authority_level=senior, stake_amount=100, signature={arbiter_sig_hex}]\n\
         ---END---\n"
    );
    let resp_arbiter = send(tcp_port, &arbiter_register_payload).await;
    let purpose_arbiter = extract_field(&resp_arbiter, "INTENT_PAYLOAD", "purpose");
    println!("      arbiter_register purpose={:?}", purpose_arbiter);
    if purpose_arbiter.as_deref() != Some("arbiter_register_ack") {
        failures.push(format!("scenario 3: arbiter_register aurait du reussir (arbiter_register_ack), recu purpose={:?}", purpose_arbiter));
    }

    let resp_open = send(tcp_port, &arbitrage_channel_payload(
        "open_case", None, ", escalation_source=deontic_smoke_test, contradiction_type=logical_break, description=Test+deontic+wiring",
    )).await;
    let case_id = extract_field(&resp_open, "INTENT_PAYLOAD", "case_id");
    println!("      case_id={:?}", case_id);

    if let Some(cid) = &case_id {
        let resp_assign = send(tcp_port, &arbitrage_channel_payload("assign_arbiters", Some(cid), ", arbiter_count=1")).await;
        let assigned = extract_field(&resp_assign, "INTENT_PAYLOAD", "assigned_arbiters");
        println!("      assigned_arbiters={:?}", assigned);
        if assigned.as_deref() != Some(arbiter_id) {
            failures.push(format!("scenario 3: l'unique arbitre enregistre ({arbiter_id}) aurait du etre assigne, recu {:?}", assigned));
        }

        // ruling_id choisi par le CLIENT (voir le fix dans handler.rs): il doit
        // le connaitre AVANT de signer, le serveur ne peut pas lui en generer
        // un a l'avance. Signature Ed25519 BRUTE (pas signing_bytes/CstlPayload
        // -- verify_ruling_signatures verifie sur
        // "ruling_id||decision||justification" tel quel, voir arbitrage.rs).
        // Valeurs SANS espace: le parser CSTL ne decode pas "+"/"%20" --
        // ce qui est signe doit correspondre OCTET PAR OCTET a ce que
        // `payload.intent.get(..)` retournera une fois reparse cote serveur.
        let ruling_id = "ruling_deontic_smoke_1";
        let decision = "accept_assertion_A";
        let justification = "deontic_smoke_test_justification";
        let ruling_signing_payload = format!("{ruling_id}||{decision}||{justification}");
        let ruling_sig = arbiter_key.sign(ruling_signing_payload.as_bytes());
        let ruling_sig_hex = hex::encode(ruling_sig.to_bytes());

        let submit_ruling_payload = format!(
            "#!CSTL v5.0.0 MODE=A\n\
             META [encoder=DeonticSmokeTest, produced_by=DeonticSmokeTest, signature={ruling_sig_hex}]\n\
             INTENT_PAYLOAD [purpose=arbitrage_channel, sender=deontic_smoke_opener, receiver=server, action=submit_ruling, case_id={cid}, ruling_id={ruling_id}, arbiter_id={arbiter_id}, decision={decision}, justification={justification}]\n\
             ---END---\n"
        );
        let resp_ruling = send(tcp_port, &submit_ruling_payload).await;
        let purpose_ruling = extract_field(&resp_ruling, "INTENT_PAYLOAD", "purpose");
        println!("      purpose={:?}", purpose_ruling);
        if purpose_ruling.as_deref() != Some("ruling_submitted") {
            failures.push(format!("scenario 3: submit_ruling (signe reellement) aurait du reussir (ruling_submitted), recu purpose={:?}", purpose_ruling));
        }
    } else {
        failures.push("scenario 3: open_case aurait du retourner un case_id".to_string());
    }

    match http.get(&deontic_url).send().await {
        Ok(resp) => {
            let body: serde_json::Value = resp.json().await.expect("JSON valide");
            let execution_count = body["execution_count"].as_u64();
            let executions = body["executions"].as_array().cloned().unwrap_or_default();
            let found = executions.iter().any(|e| {
                e["action"] == "log_ruling_applied"
                    && e["modality"] == "Must"
                    && e["result"] == "Success"
            });
            println!("      execution_count={:?} ruling_execution_found={}", execution_count, found);
            if execution_count != Some(2) {
                failures.push(format!("scenario 3: 2 executions totales attendues (agent_register + ruling), recu {:?}", execution_count));
            }
            if !found {
                failures.push("scenario 3: /deontic/executions doit contenir une execution Must/log_ruling_applied/Success apres le ruling reel".to_string());
            }
        }
        Err(e) => failures.push(format!("scenario 3: requete HTTP echouee: {e}")),
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios deontic sont conformes (agent_register + arbitrage ruling reels, orchestrateur cable en observation/audit sur arbitrage.rs).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
