/// examples/key_rotation_smoke_test.rs -- verification live de la preuve de
/// rotation de cle sur `purpose=agent_register` (Couche 7, 2026-09-04, item
/// #1 de la liste des choses a faire), sur une vraie connexion TCP en
/// process (meme convention que signing_registration_smoke_test.rs).
///
/// Trouvaille corrigee: AVANT ce fix, un re-enregistrement avec une NOUVELLE
/// cle publique passait sur simple auto-signature -- celle-ci prouve
/// seulement "je possede cette nouvelle cle", jamais "je suis le meme agent
/// que celui deja enregistre sous ce nom". N'importe qui connaissant juste
/// le NOM d'un agent deja enregistre pouvait donc lui voler son identite en
/// soumettant son propre `agent_register`, sans jamais prouver posseder
/// l'ANCIENNE cle privee.
///
/// Scenarios verifies:
/// 1. Premier enregistrement d'un nom (aucune cle existante) -> accepte sans
///    rotation_signature (bootstrap inchange, pas de regression).
/// 2. Meme nom, MEME cle publique (juste rafraichir capabilities/trust_score)
///    -> accepte sans rotation_signature (pas une rotation).
/// 3. Meme nom, NOUVELLE cle publique, SANS rotation_signature -> rejete
///    (agent_register_rejected/rotation_proof_required) -- la cle en
///    registre reste l'ANCIENNE.
/// 4. Meme nom, NOUVELLE cle, rotation_signature signee avec une MAUVAISE
///    cle (l'attaquant ne possede pas la vraie ancienne cle privee) ->
///    rejete (rotation_proof_invalid) -- la cle en registre reste
///    l'ANCIENNE.
/// 5. Meme nom, NOUVELLE cle, rotation_signature correctement signee avec
///    la VRAIE ancienne cle privee -> accepte, la cle en registre passe a
///    la nouvelle (verifie indirectement: un payload ordinaire signe avec
///    la NOUVELLE cle est desormais accepte, un payload signe avec
///    l'ANCIENNE cle est desormais rejete).
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
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("Olivier"));
    server
}

/// Construit et signe un `agent_register` pour `name`/`pubkey_hex`, avec en
/// plus (optionnel) une `rotation_signature` deja calculee par l'appelant.
/// La signature "normale" (auto-signature avec `signing_key`, prouve la
/// possession de la NOUVELLE cle) est toujours calculee ici.
fn agent_register_payload(
    name: &str,
    signing_key: &SigningKey,
    pubkey_hex: &str,
    rotation_signature_hex: Option<&str>,
) -> String {
    let rotation_field = rotation_signature_hex
        .map(|s| format!(", rotation_signature={}", s))
        .unwrap_or_default();
    let unsigned = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=communication{rotation_field}]\n\
         ---END---\n"
    );
    let parsed = parse_payload(&unsigned).expect("brouillon agent_register doit parser");
    // signing_bytes exclut "signature" ET "rotation_signature" -- le champ
    // rotation_field ci-dessus, deja present dans `parsed`, ne perturbe donc
    // pas le message signe par `signing_key`.
    let sig = signing_key.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=communication{rotation_field}, signature={sig_hex}]\n\
         ---END---\n"
    )
}

fn born_in_payload(sender: &str, subject: &str, city: &str, pubkey_hex: &str, sig_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=key_rotation_smoke, sender={sender}, receiver=server, signature={sig_hex}]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

fn sign_ordinary_payload(sk: &SigningKey, sender: &str, subject: &str, city: &str, pubkey_hex: &str) -> String {
    let unsigned = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=key_rotation_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    );
    let parsed = parse_payload(&unsigned).expect("brouillon doit parser");
    let sig = sk.sign(&signing_bytes(&parsed));
    born_in_payload(sender, subject, city, pubkey_hex, &hex::encode(sig.to_bytes()))
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
    let mut csprng = OsRng;
    let agent_name = "rotating_agent";

    let old_key = SigningKey::generate(&mut csprng);
    let old_pubkey_hex = hex::encode(old_key.verifying_key().to_bytes());
    let new_key = SigningKey::generate(&mut csprng);
    let new_pubkey_hex = hex::encode(new_key.verifying_key().to_bytes());
    let impostor_key = SigningKey::generate(&mut csprng);

    // ---- Scenario 1: premier enregistrement (aucune cle existante) ----
    println!("[1/5] Premier enregistrement (aucune cle existante, aucune rotation requise)...");
    let resp1 = send(port, &agent_register_payload(agent_name, &old_key, &old_pubkey_hex, None)).await;
    let purpose1 = extract_field(&resp1, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose1);
    if purpose1.as_deref() != Some("agent_register_ack") {
        failures.push("scenario 1: premier enregistrement doit etre accepte sans rotation_signature".to_string());
    }

    // ---- Scenario 2: meme nom, MEME cle -> pas une rotation ----
    println!("[2/5] Re-enregistrement avec la MEME cle (rafraichir capabilities)...");
    let resp2 = send(port, &agent_register_payload(agent_name, &old_key, &old_pubkey_hex, None)).await;
    let purpose2 = extract_field(&resp2, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose2);
    if purpose2.as_deref() != Some("agent_register_ack") {
        failures.push("scenario 2: re-enregistrement avec la meme cle ne doit pas exiger de rotation_signature".to_string());
    }

    // ---- Scenario 3: NOUVELLE cle, SANS rotation_signature -> rejete ----
    println!("[3/5] Re-enregistrement avec une NOUVELLE cle, SANS rotation_signature...");
    let resp3 = send(port, &agent_register_payload(agent_name, &new_key, &new_pubkey_hex, None)).await;
    let purpose3 = extract_field(&resp3, "INTENT_PAYLOAD", "purpose");
    let reason3 = extract_field(&resp3, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose3, reason3);
    if purpose3.as_deref() != Some("agent_register_rejected") || reason3.as_deref() != Some("rotation_proof_required") {
        failures.push("scenario 3: rotation sans rotation_signature doit etre rejetee (rotation_proof_required)".to_string());
    }

    // ---- Scenario 4: NOUVELLE cle, rotation_signature signee par un IMPOSTEUR ----
    println!("[4/5] Re-enregistrement avec une NOUVELLE cle, rotation_signature d'un IMPOSTEUR...");
    let unsigned_for_rotation = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={new_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication]\n\
         ---END---\n"
    );
    let parsed_for_rotation = parse_payload(&unsigned_for_rotation).expect("brouillon doit parser");
    let fake_rotation_sig = impostor_key.sign(&signing_bytes(&parsed_for_rotation));
    let fake_rotation_sig_hex = hex::encode(fake_rotation_sig.to_bytes());
    let resp4 = send(port, &agent_register_payload(agent_name, &new_key, &new_pubkey_hex, Some(&fake_rotation_sig_hex))).await;
    let purpose4 = extract_field(&resp4, "INTENT_PAYLOAD", "purpose");
    let reason4 = extract_field(&resp4, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose4, reason4);
    if purpose4.as_deref() != Some("agent_register_rejected") || reason4.as_deref() != Some("rotation_proof_invalid") {
        failures.push("scenario 4: rotation_signature d'un imposteur doit etre rejetee (rotation_proof_invalid)".to_string());
    }

    // Verifie qu'apres les scenarios 3 et 4 (tous deux rejetes), l'ANCIENNE
    // cle est toujours celle en registre: un payload ordinaire signe avec
    // old_key doit encore passer.
    println!("      (verification: la cle en registre est toujours l'ANCIENNE apres les rejets 3 et 4)");
    let resp_check = send(port, &sign_ordinary_payload(&old_key, agent_name, "key_rotation_subject_check", "Lisbonne", &old_pubkey_hex)).await;
    let status_check = extract_field(&resp_check, "META", "status");
    if status_check.as_deref() != Some("processed") {
        failures.push("apres rejet: un payload signe avec l'ANCIENNE cle doit encore etre accepte (la rotation n'a pas eu lieu)".to_string());
    }

    // ---- Scenario 5: NOUVELLE cle, VRAIE rotation_signature -> accepte ----
    println!("[5/5] Re-enregistrement avec une NOUVELLE cle, VRAIE rotation_signature (signee par l'ancienne cle privee)...");
    let real_rotation_sig = old_key.sign(&signing_bytes(&parsed_for_rotation));
    let real_rotation_sig_hex = hex::encode(real_rotation_sig.to_bytes());
    let resp5 = send(port, &agent_register_payload(agent_name, &new_key, &new_pubkey_hex, Some(&real_rotation_sig_hex))).await;
    let purpose5 = extract_field(&resp5, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose5);
    if purpose5.as_deref() != Some("agent_register_ack") {
        failures.push("scenario 5: rotation avec une VRAIE rotation_signature doit etre acceptee".to_string());
    }

    // Verifie que la cle en registre est maintenant la NOUVELLE: un payload
    // signe avec new_key passe, un payload signe avec old_key (desormais
    // perimee) est rejete.
    println!("      (verification: la cle en registre est maintenant la NOUVELLE)");
    let resp_new = send(port, &sign_ordinary_payload(&new_key, agent_name, "key_rotation_subject_new", "Oslo", &new_pubkey_hex)).await;
    let status_new = extract_field(&resp_new, "META", "status");
    if status_new.as_deref() != Some("processed") {
        failures.push("apres rotation: un payload signe avec la NOUVELLE cle doit etre accepte".to_string());
    }
    let resp_old = send(port, &sign_ordinary_payload(&old_key, agent_name, "key_rotation_subject_old", "Helsinki", &old_pubkey_hex)).await;
    let purpose_old = extract_field(&resp_old, "INTENT_PAYLOAD", "purpose");
    let reason_old = extract_field(&resp_old, "INTENT_PAYLOAD", "reason");
    println!("      (post-rotation, ancienne cle) purpose={:?} reason={:?}", purpose_old, reason_old);
    if purpose_old.as_deref() != Some("signature_rejected") || reason_old.as_deref() != Some("public_key_mismatch") {
        failures.push("apres rotation: un payload signe avec l'ANCIENNE cle (desormais perimee) doit etre rejete".to_string());
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios de rotation de cle sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
