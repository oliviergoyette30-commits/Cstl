/// examples/governance_smoke_test.rs -- verification live de la Couche 2
/// (gouvernance/resilience: circuit breaker + quorum 2/3, observation seule)
/// ET de la Couche 3b (RestrictedCouncil, quorum multi-membres reellement
/// securise, durci le 2026-09-04).
///
/// Demarre deux instances de CstlNativeServer EN PROCESS (memes conventions
/// que examples/kb_verify_smoke_test.rs), chacune avec son propre AdnStore
/// en memoire (":memory:") pour ne jamais toucher a un vrai cstl_adn.db, et
/// sans TELEGRAM_BOT_TOKEN/OBSIDIAN_VAULT_PATH dans l'environnement -- donc
/// telegram/obsidian restent None, aucun appel reseau reel.
///
/// Trouvaille et fix du 2026-09-04 (Couche 3b, en reponse a la demande
/// explicite d'un quorum "reellement securise", pas juste arithmetiquement
/// correct): avant ce fix, `council_decision` n'etait autorise que par
/// correspondance de NOM (`RestrictedCouncil::is_authorized`) -- des qu'un
/// deuxieme membre existe (desormais possible via CSTL_COUNCIL_MEMBERS,
/// restricted_council.rs::from_env()), n'importe qui connectant au TCP
/// pouvait fabriquer 2 votes non authentifies ("sender=Olivier",
/// "sender=Alice") et atteindre seul le quorum -- theatre de securite pur.
/// `handler.rs` exige desormais, en plus de `is_authorized()`: (1) une
/// signature Ed25519 valide sur CE message, ET (2) que la cle publique
/// EMBARQUEE dans le message corresponde EXACTEMENT a celle enregistree
/// (via agent_register) pour ce nom -- pas seulement que le message soit
/// signe par N'IMPORTE QUELLE cle (ce que STEP 2a verifiait deja, mais sans
/// jamais comparer au registre).
///
/// Scenarios verifies:
/// 1. Un payload normal et coherent -> GOVERNANCE [circuit=closed, ...],
///    status=processed inchange (aucune regression sur le trafic normal).
/// 2. Une meme incoherence repetee -> GOVERNANCE [circuit=open,
///    breaker_trips=3, ...] apparait, ET le payload reste status=processed
///    avec un AUDIT normal (preuve qu'il n'est jamais rejete).
/// 3. Council a 2 membres, votes SIGNES: membre 1 vote commit -> quorum=1/2,
///    committed=false ; membre 2 distinct vote -> quorum=2/2, committed=true.
/// 4. Council a 1 membre (config par defaut), vote SIGNE -> committed=true
///    immediatement, comportement inchange.
/// 5. Council a 2 membres, vote NON SIGNE d'un nom autorise -> rejete
///    (reason=signature_required) -- preuve que la regression theorique
///    decrite ci-dessus est bien fermee.
/// 6. Council a 2 membres, vote signe par la cle d'un IMPOSTEUR (differente
///    de celle enregistree pour "alice_h") mais avec sender=alice_h -> rejete
///    des STEP 2a (reason=public_key_mismatch, purpose=signature_rejected --
///    intercepte avant meme d'atteindre le bloc council_decision, depuis
///    l'extension du scenario 7/8 ci-dessous) -- preuve qu'une signature
///    valide-mais-non-liee-au-registre ne suffit plus.
/// 7. Trafic ORDINAIRE (pas un vote): "alice_h" envoie un payload normal
///    signe avec SA VRAIE cle -> traite normalement (aucune regression sur
///    le trafic legitime d'un agent deja enregistre).
/// 8. Meme trafic ordinaire, mais signe par la cle d'un IMPOSTEUR tout en
///    pretendant sender=alice_h -> rejete (public_key_mismatch) -- preuve
///    que la protection ajoutee le 2026-09-04 (deuxieme passe) couvre bien
///    TOUT le trafic signe, pas seulement council_decision.
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

fn born_in_payload(sender: &str, subject: &str, city: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=governance_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

/// Meme payload que born_in_payload, mais avec public_key/signature -- pour
/// verifier la protection cle<->registre sur du trafic ORDINAIRE (pas un
/// council_decision), scenarios 7/8.
fn born_in_draft(sender: &str, subject: &str, city: &str, pubkey_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=governance_smoke, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

fn born_in_signed(sender: &str, subject: &str, city: &str, pubkey_hex: &str, sig_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=governance_smoke, sender={sender}, receiver=server, signature={sig_hex}]\n\
         RELATION [type=born_in, subject={subject}, object={city}]\n\
         ---END---\n"
    )
}

fn sign_born_in(sk: &SigningKey, sender: &str, subject: &str, city: &str, pubkey_hex: &str) -> String {
    let draft = born_in_draft(sender, subject, city, pubkey_hex);
    let parsed = parse_payload(&draft).expect("le brouillon born_in doit parser");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    born_in_signed(sender, subject, city, pubkey_hex, &sig_hex)
}

/// Brouillon non signe d'un council_decision, avec public_key embarquee.
fn council_decision_draft(sender: &str, target_hash: &str, decision: &str, pubkey_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=council_decision, sender={sender}, receiver=server, target_hash={target_hash}, decision={decision}]\n\
         ---END---\n"
    )
}

/// Meme payload, avec une signature ajoutee au bout de INTENT_PAYLOAD.
fn council_decision_signed(sender: &str, target_hash: &str, decision: &str, pubkey_hex: &str, sig_hex: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=council_decision, sender={sender}, receiver=server, target_hash={target_hash}, decision={decision}, signature={sig_hex}]\n\
         ---END---\n"
    )
}

/// Signe un council_decision avec la cle donnee -- reproduit exactement ce
/// qu'un vrai client ferait (parser son propre brouillon, signer, injecter).
fn sign_council_decision(sk: &SigningKey, sender: &str, target_hash: &str, decision: &str, pubkey_hex: &str) -> String {
    let draft = council_decision_draft(sender, target_hash, decision, pubkey_hex);
    let parsed = parse_payload(&draft).expect("le brouillon council_decision doit parser");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    council_decision_signed(sender, target_hash, decision, pubkey_hex, &sig_hex)
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

/// Cree un serveur de test avec un conseil donne, et enregistre chaque
/// (nom, cle publique) fourni dans son AgentRegistry -- c'est cette
/// inscription qui rend un vote de ce nom verifiable contre une cle
/// specifique (pas seulement "une cle quelconque").
fn make_test_server(port: u16, council: RestrictedCouncil, council_members: &[(&str, &str)]) -> CstlNativeServer {
    // with_data_path(":memory:") plutot que new()+override: depuis que
    // audit_store/chain sont aussi seedes au demarrage (Couche 5, 2026-09-04),
    // new() ferait un vrai open+load sur le fichier "cstl_adn.db" reel du
    // repertoire courant AVANT que .adn_store soit ecrase plus bas -- ce
    // chargement (contrairement a adn_store avant ce fix) alimenterait .chain
    // avec un historique reel potentiellement present sur disque, rendant ce
    // smoke-test non-deterministe. with_data_path evite ce probleme a la racine.
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "smoke_agent".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    for (name, pubkey_hex) in council_members {
        registry.register(AgentCard {
            name: name.to_string(),
            version: "5.0.0".to_string(),
            capabilities: vec!["council".to_string()],
            trust_score: 0.9,
            public_key: Some(pubkey_hex.to_string()),
        });
    }
    server.agent_registry = Arc::new(Mutex::new(registry));
    server.restricted_council = Arc::new(council);
    server
}

#[tokio::main]
async fn main() {
    let mut csprng = OsRng;

    // Cles reelles pour chaque membre de conseil implique dans ce test,
    // + une cle "imposteur" pour le scenario 6.
    let sk_olivier = SigningKey::generate(&mut csprng);
    let pk_olivier = hex::encode(sk_olivier.verifying_key().to_bytes());
    let sk_alice = SigningKey::generate(&mut csprng);
    let pk_alice = hex::encode(sk_alice.verifying_key().to_bytes());
    let sk_bob = SigningKey::generate(&mut csprng);
    let pk_bob = hex::encode(sk_bob.verifying_key().to_bytes());
    let sk_impostor = SigningKey::generate(&mut csprng);
    let pk_impostor = hex::encode(sk_impostor.verifying_key().to_bytes());

    // --- Serveur A: config par defaut (1 membre, "Olivier") -- scenarios 1, 2, 4.
    let port_a: u16 = 15150;
    let server_a = make_test_server(
        port_a,
        RestrictedCouncil::single_member("Olivier"),
        &[("Olivier", &pk_olivier)],
    );
    tokio::spawn(async move {
        server_a.start().await.expect("server_a start");
    });

    // --- Serveur B: council a 3 membres (dont "carol_h", jamais enregistree
    // via agent_register -- expres, pour le scenario 5) -- scenarios 3, 5, 6.
    // ceil(2/3 * 3) = 2, identique au quorum_size() d'un council a 2 membres:
    // les assertions "1/2"/"2/2" des scenarios 3/4 restent valides.
    let port_b: u16 = 15151;
    let server_b = make_test_server(
        port_b,
        RestrictedCouncil::new(vec!["alice_h".to_string(), "bob_h".to_string(), "carol_h".to_string()]),
        &[("alice_h", &pk_alice), ("bob_h", &pk_bob)], // carol_h volontairement PAS enregistree
    );
    tokio::spawn(async move {
        server_b.start().await.expect("server_b start");
    });

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: payload normal et coherent ----
    println!("[1/8] Payload normal, coherent...");
    let resp1 = send(port_a, &born_in_payload("agent_gov_1", "governance_smoke_subject_1", "Varsovie")).await;
    let status1 = extract_field(&resp1, "META", "status");
    let circuit1 = extract_field(&resp1, "GOVERNANCE", "circuit");
    println!("      status={:?} circuit={:?}", status1, circuit1);
    if status1.as_deref() != Some("processed") || circuit1.as_deref() != Some("closed") {
        failures.push("scenario 1: attendu status=processed, circuit=closed".to_string());
    }

    // ---- Scenario 2: incoherence repetee -> breaker ouvert, jamais rejete ----
    println!("[2/8] Incoherence repetee (meme sujet, villes differentes)...");
    let subject2 = "governance_smoke_subject_2";
    let _ = send(port_a, &born_in_payload("agent_gov_2", subject2, "Varsovie")).await; // baseline
    let _ = send(port_a, &born_in_payload("agent_gov_2", subject2, "Paris")).await; // trip 1
    let _ = send(port_a, &born_in_payload("agent_gov_2", subject2, "Berlin")).await; // trip 2
    let resp2 = send(port_a, &born_in_payload("agent_gov_2", subject2, "Londres")).await; // trip 3 -> open
    let status2 = extract_field(&resp2, "META", "status");
    let circuit2 = extract_field(&resp2, "GOVERNANCE", "circuit");
    let trips2 = extract_field(&resp2, "GOVERNANCE", "breaker_trips");
    let has_audit2 = resp2.contains("AUDIT [hash=");
    println!("      status={:?} circuit={:?} breaker_trips={:?} audit_present={}", status2, circuit2, trips2, has_audit2);
    if status2.as_deref() != Some("processed") {
        failures.push("scenario 2: le payload a ete rejete -- la couche 2 ne doit JAMAIS bloquer".to_string());
    }
    if circuit2.as_deref() != Some("open") || trips2.as_deref() != Some("3") {
        failures.push("scenario 2: circuit breaker attendu ouvert avec 3 trips".to_string());
    }
    if !has_audit2 {
        failures.push("scenario 2: bloc AUDIT absent -- le payload aurait du etre traite normalement".to_string());
    }

    // ---- Scenario 3: quorum 2/3 sur serveur B, votes SIGNES ----
    println!("[3/8] Quorum 2/3 (council a 2 membres, votes signes)...");
    let resp_seed = send(port_b, &born_in_payload("agent_gov_3", "governance_smoke_subject_3", "Rome")).await;
    let target_hash = extract_field(&resp_seed, "AUDIT", "hash").expect("hash pour scenario 3");
    let vote1 = send(port_b, &sign_council_decision(&sk_alice, "alice_h", &target_hash, "commit", &pk_alice)).await;
    let purpose1 = extract_field(&vote1, "INTENT_PAYLOAD", "purpose");
    let quorum1 = extract_field(&vote1, "INTENT_PAYLOAD", "quorum");
    let committed1 = extract_field(&vote1, "INTENT_PAYLOAD", "committed");
    println!("      vote 1 (alice_h): purpose={:?} quorum={:?} committed={:?}", purpose1, quorum1, committed1);
    if purpose1.as_deref() != Some("council_decision_recorded") || quorum1.as_deref() != Some("1/2") || committed1.as_deref() != Some("false") {
        failures.push("scenario 3: premier vote aurait du etre 'recorded', quorum=1/2, committed=false".to_string());
    }
    let vote2 = send(port_b, &sign_council_decision(&sk_bob, "bob_h", &target_hash, "commit", &pk_bob)).await;
    let purpose2 = extract_field(&vote2, "INTENT_PAYLOAD", "purpose");
    let quorum2 = extract_field(&vote2, "INTENT_PAYLOAD", "quorum");
    let committed2 = extract_field(&vote2, "INTENT_PAYLOAD", "committed");
    println!("      vote 2 (bob_h):   purpose={:?} quorum={:?} committed={:?}", purpose2, quorum2, committed2);
    if purpose2.as_deref() != Some("council_decision_applied") || quorum2.as_deref() != Some("2/2") || committed2.as_deref() != Some("true") {
        failures.push("scenario 3: deuxieme vote distinct aurait du atteindre le quorum et committer".to_string());
    }

    // ---- Scenario 4: config a 1 membre (aujourd'hui), vote SIGNE -- aucune regression ----
    println!("[4/8] Config a 1 membre, vote signe (comportement d'avant ce changement)...");
    let resp_seed4 = send(port_a, &born_in_payload("agent_gov_4", "governance_smoke_subject_4", "Tokyo")).await;
    let target_hash4 = extract_field(&resp_seed4, "AUDIT", "hash").expect("hash pour scenario 4");
    let vote4 = send(port_a, &sign_council_decision(&sk_olivier, "Olivier", &target_hash4, "commit", &pk_olivier)).await;
    let purpose4 = extract_field(&vote4, "INTENT_PAYLOAD", "purpose");
    let committed4 = extract_field(&vote4, "INTENT_PAYLOAD", "committed");
    println!("      purpose={:?} committed={:?}", purpose4, committed4);
    if purpose4.as_deref() != Some("council_decision_applied") || committed4.as_deref() != Some("true") {
        failures.push("scenario 4: un seul commit signe en config a 1 membre doit committer immediatement (regression)".to_string());
    }

    // ---- Scenario 5: vote NON SIGNE d'un membre autorise mais JAMAIS
    // enregistre (carol_h) -> rejete par la verification PROPRE au bloc
    // council_decision (pas par STEP 2a, qui ne force une signature que
    // pour un sender DEJA enregistre avec une cle -- ce n'est justement PAS
    // le cas ici, donc si le bloc council ne verifiait pas lui-meme, ce
    // vote non authentifie passerait).
    println!("[5/8] Vote NON signe d'un membre autorise mais jamais enregistre (doit etre rejete)...");
    let resp_seed5 = send(port_b, &born_in_payload("agent_gov_5", "governance_smoke_subject_5", "Vienne")).await;
    let target_hash5 = extract_field(&resp_seed5, "AUDIT", "hash").expect("hash pour scenario 5");
    let unsigned_vote = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=council_decision, sender=carol_h, receiver=server, target_hash={target_hash5}, decision=commit]\n\
         ---END---\n"
    );
    let resp5 = send(port_b, &unsigned_vote).await;
    let purpose5 = extract_field(&resp5, "INTENT_PAYLOAD", "purpose");
    let reason5 = extract_field(&resp5, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose5, reason5);
    if purpose5.as_deref() != Some("council_decision_rejected") || reason5.as_deref() != Some("signature_required") {
        failures.push("scenario 5: un council_decision non signe d'un membre autorise-mais-non-enregistre doit etre rejete par le bloc council lui-meme (signature_required)".to_string());
    }

    // ---- Scenario 6: vote signe par la cle d'un IMPOSTEUR, sender usurpe ----
    // Depuis l'extension du 2026-09-04 (meme jour, deuxieme passe) de la
    // comparaison cle<->registre a TOUT le trafic signe (pas seulement
    // council_decision), ce cas est desormais intercepte encore plus tot,
    // par STEP 2a elle-meme (purpose=signature_rejected), avant meme
    // d'atteindre le bloc council_decision -- le reason reste identique
    // (public_key_mismatch), seul le purpose change par rapport a avant
    // cette extension.
    println!("[6/8] Vote signe par une cle d'imposteur, sender=alice_h usurpe (doit etre rejete)...");
    let resp_seed6 = send(port_b, &born_in_payload("agent_gov_6", "governance_smoke_subject_6", "Madrid")).await;
    let target_hash6 = extract_field(&resp_seed6, "AUDIT", "hash").expect("hash pour scenario 6");
    // sk_impostor signe valablement -- le message EST auto-coherent (sa
    // signature correspond a SA cle, pk_impostor, embarquee dans META) --
    // mais pk_impostor != la cle enregistree pour "alice_h" (pk_alice).
    let impostor_vote = sign_council_decision(&sk_impostor, "alice_h", &target_hash6, "commit", &pk_impostor);
    let resp6 = send(port_b, &impostor_vote).await;
    let purpose6 = extract_field(&resp6, "INTENT_PAYLOAD", "purpose");
    let reason6 = extract_field(&resp6, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose6, reason6);
    if purpose6.as_deref() != Some("signature_rejected") || reason6.as_deref() != Some("public_key_mismatch") {
        failures.push("scenario 6: un vote signe par une cle differente de celle enregistree pour ce nom doit etre rejete (public_key_mismatch, intercepte des STEP 2a)".to_string());
    }

    // ---- Scenario 7: trafic ORDINAIRE, alice_h signe avec SA VRAIE cle ----
    println!("[7/8] Trafic ordinaire signe correctement par un agent enregistre (aucune regression)...");
    let resp7 = send(port_b, &sign_born_in(&sk_alice, "alice_h", "governance_smoke_subject_7", "Lisbonne", &pk_alice)).await;
    let status7 = extract_field(&resp7, "META", "status");
    println!("      status={:?}", status7);
    if status7.as_deref() != Some("processed") {
        failures.push("scenario 7: un payload ordinaire correctement signe par un agent deja enregistre doit etre traite normalement".to_string());
    }

    // ---- Scenario 8: trafic ORDINAIRE, IMPOSTEUR usurpe sender=alice_h ----
    // Preuve directe de l'extension du 2026-09-04 (deuxieme passe): la
    // protection cle<->registre ne se limite plus a council_decision.
    println!("[8/8] Trafic ordinaire signe par un imposteur usurpant sender=alice_h (doit etre rejete)...");
    let resp8 = send(port_b, &sign_born_in(&sk_impostor, "alice_h", "governance_smoke_subject_8", "Athenes", &pk_impostor)).await;
    let purpose8 = extract_field(&resp8, "INTENT_PAYLOAD", "purpose");
    let reason8 = extract_field(&resp8, "INTENT_PAYLOAD", "reason");
    println!("      purpose={:?} reason={:?}", purpose8, reason8);
    if purpose8.as_deref() != Some("signature_rejected") || reason8.as_deref() != Some("public_key_mismatch") {
        failures.push("scenario 8: un payload ordinaire signe par la cle d'un imposteur usurpant un sender deja enregistre doit etre rejete (public_key_mismatch)".to_string());
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios de gouvernance sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
