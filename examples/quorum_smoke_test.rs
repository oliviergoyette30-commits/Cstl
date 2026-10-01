/// examples/quorum_smoke_test.rs -- verification live de quorum.rs cable
/// dans le pipeline live (Layer 2, consensus BFT multi-agent,
/// purpose=quorum_propose/quorum_vote/quorum_circuit_breaker, 2026-10-01).
/// Meme convention que signing_registration_smoke_test.rs: vraie connexion
/// TCP en process.
///
/// Scenarios verifies:
/// 1. quorum_propose d'un expediteur NON enregistre -> quorum_rejected.
/// 2. Enregistrement de 3 agents (alice/bob/carol) avec vraies cles Ed25519.
/// 3. quorum_propose par alice -> quorum_proposal_created, threshold=2
///    (compute_threshold(3)).
/// 4. quorum_vote (alice, yea) sur UNE NOUVELLE CONNEXION TCP -> pas encore
///    de consensus. POINT STRUCTUREL CLE: contrairement a CASTLE (dictionnaire
///    scope a la connexion), l'etat quorum doit survivre a travers des
///    connexions SEPAREES (chaque agent vote depuis son propre processus/
///    connexion dans la vraie vie) -- c'est pourquoi quorum persiste via
///    adn_store (SQLite) plutot qu'une struct en memoire liee au socket.
/// 5. quorum_vote (bob, yea) sur une TROISIEME connexion -> consensus_reached=true,
///    final_decision=yea.
/// 6. Vote tardif (carol) sur une proposition deja decidee -> rejete.
/// 7. Usurpation: un attaquant signe avec SA PROPRE cle en se faisant passer
///    pour alice -> public_key_mismatch.
/// 8. quorum_circuit_breaker par un agent NON membre du RestrictedCouncil ->
///    not_authorized.
/// 9. quorum_circuit_breaker par le membre legitime du RestrictedCouncil ->
///    quorum_circuit_breaker_activated, puis un vote ulterieur est refuse
///    (circuit_breaker_active).
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

/// Chaque appel ouvre une connexion TCP FRAICHE -- volontairement, pour
/// prouver que l'etat quorum survit a travers des connexions separees (voir
/// scenario 4/5 plus haut), contrairement a CASTLE qui exige keep_alive sur
/// UNE connexion.
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
    let registry = AgentRegistry::new();
    server.agent_registry = Arc::new(RwLock::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("olivier"));
    server
}

fn register_payload(name: &str, pubkey_hex: &str, sig_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=communication, signature={sig_hex}]\n\
         ---END---\n"
    )
}

/// Construit et signe un payload CSTL pour `purpose`, avec les champs
/// INTENT_PAYLOAD supplementaires donnes dans `extra`.
fn build_signed(sender: &str, pubkey_hex: &str, sk: &SigningKey, purpose: &str, extra: &str) -> String {
    let unsigned = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose={purpose}, sender={sender}, receiver=server{extra}]\n\
         ---END---\n"
    );
    let parsed = parse_payload(&unsigned).expect("brouillon doit parser");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose={purpose}, sender={sender}, receiver=server{extra}, signature={sig_hex}]\n\
         ---END---\n"
    )
}

async fn register_agent(port: u16, name: &str) -> (SigningKey, String) {
    let mut csprng = OsRng;
    let sk = SigningKey::generate(&mut csprng);
    let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
    let register_unsigned = parse_payload(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=communication]\n\
         ---END---\n"
    )).expect("brouillon agent_register doit parser");
    let sig = sk.sign(&signing_bytes(&register_unsigned));
    let sig_hex = hex::encode(sig.to_bytes());
    let resp = send(port, &register_payload(name, &pubkey_hex, &sig_hex)).await;
    let purpose = extract_field(&resp, "INTENT_PAYLOAD", "purpose");
    assert_eq!(purpose.as_deref(), Some("agent_register_ack"), "l'enregistrement de '{}' doit reussir: {}", name, resp);
    (sk, pubkey_hex)
}

#[tokio::main]
async fn main() {
    let port: u16 = 15170;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: quorum_propose d'un expediteur non enregistre ----
    println!("[1/9] quorum_propose d'un expediteur inconnu...");
    let resp1 = send(port, &format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=SmokeTest, produced_by=SmokeTest]\nINTENT_PAYLOAD [purpose=quorum_propose, sender=ghost, receiver=server, proposal_id=smoke_1]\n---END---\n"
    )).await;
    let purpose1 = extract_field(&resp1, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose1);
    if purpose1.as_deref() != Some("quorum_rejected") {
        failures.push("scenario 1: quorum_propose d'un expediteur non enregistre doit etre rejete");
    }

    // ---- Scenario 2: enregistre 3 agents ----
    println!("[2/9] Enregistrement de alice/bob/carol...");
    let (alice_sk, alice_pub) = register_agent(port, "alice").await;
    let (bob_sk, bob_pub) = register_agent(port, "bob").await;
    let (carol_sk, carol_pub) = register_agent(port, "carol").await;
    println!("      3 agents enregistres");

    // ---- Scenario 3: quorum_propose par alice ----
    println!("[3/9] quorum_propose par alice (threshold attendu = compute_threshold(3) = 2)...");
    let propose = build_signed("alice", &alice_pub, &alice_sk, "quorum_propose", ", proposal_id=smoke_main, description=live_smoke_test");
    let resp3 = send(port, &propose).await;
    let purpose3 = extract_field(&resp3, "INTENT_PAYLOAD", "purpose");
    let threshold3 = extract_field(&resp3, "INTENT_PAYLOAD", "threshold");
    println!("      purpose={:?} threshold={:?}", purpose3, threshold3);
    if purpose3.as_deref() != Some("quorum_proposal_created") || threshold3.as_deref() != Some("2") {
        failures.push("scenario 3: quorum_propose doit creer la proposition avec threshold=2");
    }

    // ---- Scenario 4: vote d'alice sur une NOUVELLE connexion ----
    println!("[4/9] quorum_vote (alice, yea) -- NOUVELLE connexion TCP (preuve de persistance inter-connexion)...");
    let vote_alice = build_signed("alice", &alice_pub, &alice_sk, "quorum_vote", ", proposal_id=smoke_main, decision=yea");
    let resp4 = send(port, &vote_alice).await;
    let purpose4 = extract_field(&resp4, "INTENT_PAYLOAD", "purpose");
    let consensus4 = extract_field(&resp4, "INTENT_PAYLOAD", "consensus_reached");
    println!("      purpose={:?} consensus_reached={:?}", purpose4, consensus4);
    if purpose4.as_deref() != Some("quorum_vote_recorded") || consensus4.as_deref() != Some("false") {
        failures.push("scenario 4: premier vote (1/2) ne doit pas atteindre consensus");
    }

    // ---- Scenario 5: vote de bob sur une TROISIEME connexion -> consensus ----
    println!("[5/9] quorum_vote (bob, yea) -- encore une NOUVELLE connexion -> consensus attendu...");
    let vote_bob = build_signed("bob", &bob_pub, &bob_sk, "quorum_vote", ", proposal_id=smoke_main, decision=yea");
    let resp5 = send(port, &vote_bob).await;
    let purpose5 = extract_field(&resp5, "INTENT_PAYLOAD", "purpose");
    let consensus5 = extract_field(&resp5, "INTENT_PAYLOAD", "consensus_reached");
    let final5 = extract_field(&resp5, "INTENT_PAYLOAD", "final_decision");
    println!("      purpose={:?} consensus_reached={:?} final_decision={:?}", purpose5, consensus5, final5);
    if purpose5.as_deref() != Some("quorum_vote_recorded") || consensus5.as_deref() != Some("true") || final5.as_deref() != Some("yea") {
        failures.push("scenario 5: deuxieme vote (2/2) doit atteindre consensus=yea -- etat quorum doit avoir survecu a travers 3 connexions TCP distinctes");
    }

    // ---- Scenario 6: vote tardif de carol -> rejete ----
    println!("[6/9] quorum_vote (carol) sur une proposition deja decidee...");
    let vote_carol = build_signed("carol", &carol_pub, &carol_sk, "quorum_vote", ", proposal_id=smoke_main, decision=nay");
    let resp6 = send(port, &vote_carol).await;
    let purpose6 = extract_field(&resp6, "INTENT_PAYLOAD", "purpose");
    let reason6 = extract_field(&resp6, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose6, reason6);
    if purpose6.as_deref() != Some("quorum_rejected") || reason6.as_deref() != Some("proposal_already_decided") {
        failures.push("scenario 6: un vote apres decision finale doit etre refuse (proposal_already_decided)");
    }

    // ---- Scenario 7: usurpation (nouvelle proposition pour isoler le cas) ----
    println!("[7/9] Usurpation: attaquant signe avec SA PROPRE cle en se faisant passer pour alice...");
    let propose2 = build_signed("alice", &alice_pub, &alice_sk, "quorum_propose", ", proposal_id=smoke_impersonation");
    send(port, &propose2).await;
    let mut csprng = OsRng;
    let attacker_sk = SigningKey::generate(&mut csprng);
    let attacker_pub = hex::encode(attacker_sk.verifying_key().to_bytes());
    let forged_vote = build_signed("alice", &attacker_pub, &attacker_sk, "quorum_vote", ", proposal_id=smoke_impersonation, decision=yea");
    let resp7 = send(port, &forged_vote).await;
    let purpose7 = extract_field(&resp7, "INTENT_PAYLOAD", "purpose");
    let reason7 = extract_field(&resp7, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose7, reason7);
    // Trouvaille live (pas anticipee en ecrivant quorum_wire.rs): pour un
    // sender DEJA enregistre avec une public_key, STEP 2a de handler.rs
    // (garde globale, la meme qui protege TOUT le trafic signe) intercepte
    // deja ce cas AVANT meme d'atteindre handle_quorum_vote -- reponse
    // purpose=signature_rejected/reason=public_key_mismatch, pas
    // purpose=quorum_rejected. La garde dediee dans
    // quorum_wire::verify_registered_voter (meme logique, memes raisons)
    // reste utile en defense en profondeur et pour un sender legacy sans
    // garde globale equivalente, mais n'est jamais celle qui tranche ici.
    if !(reason7.as_deref() == Some("public_key_mismatch")
        && (purpose7.as_deref() == Some("signature_rejected") || purpose7.as_deref() == Some("quorum_rejected")))
    {
        failures.push("scenario 7: usurpation (cle embarquee != cle enregistree pour sender=alice) doit etre detectee");
    }

    // ---- Scenario 8: circuit_breaker par un agent non autorise ----
    println!("[8/9] quorum_circuit_breaker par alice (non membre du RestrictedCouncil)...");
    let cb_unauthorized = build_signed("alice", &alice_pub, &alice_sk, "quorum_circuit_breaker", ", proposal_id=smoke_impersonation, reason=suspected_stall");
    let resp8 = send(port, &cb_unauthorized).await;
    let purpose8 = extract_field(&resp8, "INTENT_PAYLOAD", "purpose");
    let reason8 = extract_field(&resp8, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose8, reason8);
    if purpose8.as_deref() != Some("quorum_rejected") || reason8.as_deref() != Some("not_authorized") {
        failures.push("scenario 8: circuit_breaker par un agent hors RestrictedCouncil doit etre refuse");
    }

    // ---- Scenario 9: circuit_breaker par le membre legitime (olivier) ----
    println!("[9/9] quorum_circuit_breaker par olivier (membre du RestrictedCouncil)...");
    let mut csprng2 = OsRng;
    let olivier_sk = SigningKey::generate(&mut csprng2);
    let olivier_pub = hex::encode(olivier_sk.verifying_key().to_bytes());
    let (_olivier_sk2, olivier_pub2) = register_agent(port, "olivier").await;
    // register_agent genere sa propre cle -- on reutilise cette cle pour signer
    // le circuit_breaker plutot que la cle jetable olivier_sk ci-dessus.
    let _ = (olivier_sk, olivier_pub); // cle jetable non utilisee, juste pour montrer qu'une cle different ne marcherait pas sans enregistrement
    let cb_authorized = build_signed("olivier", &olivier_pub2, &_olivier_sk2, "quorum_circuit_breaker", ", proposal_id=smoke_impersonation, reason=suspected_stall");
    let resp9 = send(port, &cb_authorized).await;
    let purpose9 = extract_field(&resp9, "INTENT_PAYLOAD", "purpose");
    println!("      purpose={:?}", purpose9);
    if purpose9.as_deref() != Some("quorum_circuit_breaker_activated") {
        failures.push("scenario 9: circuit_breaker par le membre legitime du RestrictedCouncil doit reussir");
    }
    // Un vote ulterieur sur cette proposition doit maintenant etre refuse.
    let vote_after_cb = build_signed("bob", &bob_pub, &bob_sk, "quorum_vote", ", proposal_id=smoke_impersonation, decision=yea");
    let resp9b = send(port, &vote_after_cb).await;
    let reason9b = extract_field(&resp9b, "INTENT_PAYLOAD", "reason");
    println!("      vote post-circuit-breaker: reason={:?}", reason9b);
    if reason9b.as_deref() != Some("circuit_breaker_active") {
        failures.push("scenario 9b: un vote apres activation du circuit breaker doit etre refuse (circuit_breaker_active)");
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios quorum sont conformes (y compris la persistance a travers des connexions TCP separees).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
