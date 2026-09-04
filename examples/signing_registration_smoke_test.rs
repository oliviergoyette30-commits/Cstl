/// examples/signing_registration_smoke_test.rs -- verification live de la
/// signature Ed25519 (feature A) et de l'enregistrement dynamique d'agents
/// (feature B, purpose=agent_register), sur une vraie connexion TCP en
/// process (meme convention que governance_smoke_test.rs / kb_verify_smoke_test.rs).
///
/// Scenarios verifies:
/// 1. Payload NON signe d'un expediteur NON enregistre -> toujours accepte
///    (regression: le legacy alice/bob-like doit continuer a marcher).
/// 2. agent_register avec une vraie paire de cles Ed25519 + auto-signature
///    valide -> agent_register_ack, l'agent apparait ensuite dans le registre.
/// 3. agent_register SANS signature -> agent_register_rejected,
///    reason=self_signature_required.
/// 4. Payload NON signe d'un expediteur MAINTENANT enregistre (avec
///    public_key) -> signature_rejected, reason=missing_signature_for_registered_agent.
/// 5. Meme payload, correctement signe -> flux normal (status=processed).
/// 6. Signature corrompue (un caractere hex change) -> signature_rejected,
///    reason=signature_invalid.
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
    // Voir le commentaire equivalent dans governance_smoke_test.rs:
    // with_data_path evite un open+load reel sur "cstl_adn.db" avant que
    // .adn_store soit ecrase (Couche 5, audit_store/chain seedes des le
    // demarrage depuis le 2026-09-04).
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    // Agent bootstrap "communication" legacy (public_key: None) -- necessaire
    // pour que STEP 4 (routage) trouve une destination, independamment de
    // l'expediteur/signature du message lui-meme (route() choisit parmi TOUT
    // le registre, pas seulement l'expediteur en cours).
    registry.register(AgentCard {
        name: "smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(Mutex::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("Olivier"));
    server
}

fn born_in_payload(sender: &str, subject: &str, city: &str, extra_meta: &str, extra_intent: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest{extra_meta}]\n\
         INTENT_PAYLOAD [purpose=signing_smoke, sender={sender}, receiver=server{extra_intent}]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

/// Calcule signing_bytes() sur le MEME texte de payload que celui qui sera
/// parse et envoye -- reproduit exactement ce qu'un vrai client ferait
/// (parser son propre brouillon, signer, injecter la signature).
fn sign_payload(sk: &SigningKey, sender: &str, subject: &str, city: &str, pubkey_hex: &str) -> (String, String) {
    let unsigned_extra_meta = format!(", public_key={}", pubkey_hex);
    let unsigned = born_in_payload(sender, subject, city, &unsigned_extra_meta, "");
    let parsed = parse_payload(&unsigned).expect("le brouillon non signe doit parser");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    let signed = born_in_payload(sender, subject, city, &unsigned_extra_meta, &format!(", signature={}", sig_hex));
    (signed, sig_hex)
}

fn agent_register_payload(name: &str, pubkey_hex: &str, sig_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=communication, signature={sig_hex}]\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15160;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: payload non signe, expediteur non enregistre (regression) ----
    println!("[1/6] Payload non signe, expediteur inconnu (regression legacy)...");
    let resp1 = send(port, &born_in_payload("legacy_agent", "signing_smoke_subject_1", "Varsovie", "", "")).await;
    let status1 = extract_field(&resp1, "META", "status");
    println!("      status={:?}", status1);
    if status1.as_deref() != Some("processed") {
        failures.push("scenario 1: un payload non signe d'un expediteur non enregistre doit toujours passer");
    }

    // Prepare une vraie paire de cles Ed25519.
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let agent_name = "signed_agent";

    // ---- Scenario 3: agent_register SANS signature -> rejete ----
    println!("[3/6] agent_register sans signature (avant le scenario 2 volontairement: prouve qu'AUCUN enregistrement n'a lieu sans signature)...");
    let unsigned_register = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication]\n\
         ---END---\n"
    );
    // Note: public_key present sans signature = SignatureCheck::Invalid("incomplete_signature_fields"),
    // PAS NotPresent (qui exigerait l'ABSENCE des deux champs -- impossible ici
    // puisque le bloc agent_register exige deja public_key avant meme de
    // regarder sig_check) -- donc la branche emise ici est
    // agent_register_rejected/reason=signature_invalid, pas self_signature_required.
    let resp3 = send(port, &unsigned_register).await;
    let purpose3 = extract_field(&resp3, "INTENT_PAYLOAD", "purpose");
    let reason3 = extract_field(&resp3, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose3, reason3);
    if purpose3.as_deref() != Some("agent_register_rejected") || reason3.as_deref() != Some("signature_invalid") {
        failures.push("scenario 3: agent_register sans signature doit etre rejete (agent_register_rejected/signature_invalid)");
    }

    // ---- Scenario 2: agent_register avec auto-signature valide -> ack ----
    println!("[2/6] agent_register avec auto-signature Ed25519 valide...");
    let register_unsigned_parsed = parse_payload(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication]\n\
         ---END---\n"
    )).expect("brouillon agent_register doit parser");
    let register_sig = signing_key.sign(&signing_bytes(&register_unsigned_parsed));
    let register_sig_hex = hex::encode(register_sig.to_bytes());
    let resp2 = send(port, &agent_register_payload(agent_name, &pubkey_hex, &register_sig_hex)).await;
    let purpose2 = extract_field(&resp2, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose2);
    if purpose2.as_deref() != Some("agent_register_ack") {
        failures.push("scenario 2: agent_register avec signature valide doit etre accepte (agent_register_ack)");
    }

    // ---- Scenario 4: payload non signe d'un expediteur MAINTENANT enregistre -> rejete ----
    println!("[4/6] Payload non signe du meme expediteur, maintenant enregistre avec public_key...");
    let resp4 = send(port, &born_in_payload(agent_name, "signing_smoke_subject_4", "Berlin", "", "")).await;
    let purpose4 = extract_field(&resp4, "INTENT_PAYLOAD", "purpose");
    let reason4 = extract_field(&resp4, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose4, reason4);
    if purpose4.as_deref() != Some("signature_rejected") || reason4.as_deref() != Some("missing_signature_for_registered_agent") {
        failures.push("scenario 4: payload non signe d'un expediteur enregistre avec public_key doit etre rejete");
    }

    // ---- Scenario 5: meme payload, correctement signe -> flux normal ----
    println!("[5/6] Meme payload, correctement signe...");
    let (signed5, _sig5) = sign_payload(&signing_key, agent_name, "signing_smoke_subject_5", "Rome", &pubkey_hex);
    let resp5 = send(port, &signed5).await;
    let status5 = extract_field(&resp5, "META", "status");
    let has_audit5 = resp5.contains("AUDIT [hash=");
    println!("      status={:?} audit_present={}", status5, has_audit5);
    if status5.as_deref() != Some("processed") || !has_audit5 {
        failures.push("scenario 5: payload correctement signe doit etre traite normalement");
    }

    // ---- Scenario 6: signature corrompue -> rejete ----
    println!("[6/6] Signature corrompue (un caractere modifie)...");
    let (mut signed6, sig6) = sign_payload(&signing_key, agent_name, "signing_smoke_subject_6", "Madrid", &pubkey_hex);
    let corrupted_char = if sig6.starts_with('a') { 'b' } else { 'a' };
    let corrupted_sig = format!("{}{}", corrupted_char, &sig6[1..]);
    signed6 = signed6.replace(&sig6, &corrupted_sig);
    let resp6 = send(port, &signed6).await;
    let purpose6 = extract_field(&resp6, "INTENT_PAYLOAD", "purpose");
    let reason6 = extract_field(&resp6, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose6, reason6);
    if purpose6.as_deref() != Some("signature_rejected") || reason6.as_deref() != Some("signature_invalid") {
        failures.push("scenario 6: signature corrompue doit etre rejetee (signature_invalid)");
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios signature/enregistrement sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}

