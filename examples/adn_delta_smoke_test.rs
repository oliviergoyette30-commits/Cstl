/// examples/adn_delta_smoke_test.rs -- verification live du cablage de
/// `adn_delta_detector.rs` (Couche 5, 2026-10-01) sur le pipeline reel de
/// stockage ADN (`handler.rs` STEP 3e-delta).
///
/// Contexte: `detect_deltas` (529 lignes, 6 tests unitaires reels) existait
/// depuis des mois et avait meme deja un test d'INTEGRATION
/// (`tests/couche5_persistence_e2e_test.rs`) qui l'appelle -- mais le
/// README affirmait "v5.1 COMPLETE" alors que rien dans `handler.rs` ou
/// `main.rs` ne l'invoquait jamais sur le chemin TCP live: zero reference
/// externe a `adn_delta_detector::` en dehors de ce fichier de test.
///
/// Cable ici: a chaque payload stocke avec succes dans adn_store,
/// `compute_delta_report` compare la nouvelle entree (`entry.hash`) a son
/// parent direct dans la chaine de hachage (`entry.parent_hash`) -- le
/// tout premier message d'une chaine (`parent_hash="root"`, jamais dans
/// adn_store) echoue silencieusement, pas une erreur. Informatif
/// seulement: jamais de rejet, une ligne `ADN_DELTA [...]` n'est ajoutee a
/// la reponse que si un changement reel est detecte (severity != NoChange).
///
/// Bug de conformite wire trouve et corrige en cablant live: la version
/// d'origine de `format_cstl` produisait un bloc MULTI-LIGNES
/// (`ADN_DELTA [\nold_hash=...\n...]`), qui ne respecte pas la convention
/// `NOM [inner]` sur une seule ligne du reste du wire format CSTL -- jamais
/// remarque avant parce que cette fonction n'etait jamais appelee en
/// dehors de son propre test unitaire isole. Pire: ses champs
/// `old_hash=`/`new_hash=` entraient en collision de sous-chaine avec
/// `AUDIT [hash=...]` pour tout extracteur naif cherchant "hash=" (prouve
/// en cassant reellement tests/emergence_production_path_test.rs en
/// cablant ce commit) -- renommes `delta_old_ref=`/`delta_new_ref=`.
///
/// Scenarios verifies:
/// 1. Premier message d'une chaine (parent_hash="root") -> PAS de ligne
///    ADN_DELTA dans la reponse (rien a comparer, comportement attendu,
///    pas une erreur).
/// 2. Deuxieme message, payload DIFFERENT du premier, meme sender ->
///    ADN_DELTA presente dans la reponse, sur une seule ligne,
///    payload_changed=true, severity != NoChange.
/// 3. Troisieme message, payload IDENTIQUE au deuxieme (meme texte
///    normalise) -> PAS de ligne ADN_DELTA (severity=NoChange, rien a
///    signaler).
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::CstlNativeServer;
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

fn make_test_server(port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "adn_delta_smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("olivier"));
    server
}

fn payload_with_subject(subject: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=AdnDeltaSmokeTest, produced_by=AdnDeltaSmokeTest]\n\
         INTENT_PAYLOAD [purpose=inform, sender=adn_delta_smoke_agent, receiver=server, subject={subject}]\n\
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

    // ---- Scenario 1: premier message de la chaine (parent_hash=root) ----
    println!("[1/3] Premier message (parent_hash=root) -> pas de ADN_DELTA...");
    let resp1 = send(port, &payload_with_subject("adn_delta_smoke_subject_A")).await;
    let has_delta_1 = resp1.contains("ADN_DELTA");
    println!("      ADN_DELTA present={}", has_delta_1);
    if has_delta_1 {
        failures.push("scenario 1: aucune ligne ADN_DELTA attendue sur le tout premier message de la chaine (parent inconnu d'adn_store)".to_string());
    }

    // ---- Scenario 2: deuxieme message, payload different -> delta reel ----
    println!("[2/3] Deuxieme message (payload different) -> ADN_DELTA reel, une seule ligne...");
    let resp2 = send(port, &payload_with_subject("adn_delta_smoke_subject_B_completely_different")).await;
    let has_delta_2 = resp2.contains("ADN_DELTA");
    println!("      ADN_DELTA present={}", has_delta_2);
    if !has_delta_2 {
        failures.push(format!("scenario 2: une ligne ADN_DELTA etait attendue (payload different du parent), reponse: {resp2}"));
    } else {
        let delta_line = resp2.lines().find(|l| l.starts_with("ADN_DELTA")).unwrap_or("");
        println!("      {}", delta_line);
        if !delta_line.contains("payload_changed=true") {
            failures.push(format!("scenario 2: payload_changed=true attendu dans la ligne ADN_DELTA: {delta_line}"));
        }
        if delta_line.contains("delta_old_ref=") == false || delta_line.contains("delta_new_ref=") == false {
            failures.push(format!("scenario 2: delta_old_ref=/delta_new_ref= attendus (pas old_hash=/new_hash=, collision de sous-chaine avec AUDIT hash=): {delta_line}"));
        }
        // Conformite wire: un seul bloc ADN_DELTA sur sa propre ligne (pas de
        // bloc multi-lignes -- voir le commentaire en tete de fichier).
        if resp2.matches("ADN_DELTA").count() != 1 {
            failures.push(format!("scenario 2: exactement 1 occurrence de \"ADN_DELTA\" attendue, recu {}", resp2.matches("ADN_DELTA").count()));
        }
    }

    // ---- Scenario 3: troisieme message, payload identique au deuxieme ----
    println!("[3/3] Troisieme message (payload identique au precedent) -> pas de ADN_DELTA (NoChange)...");
    let resp3 = send(port, &payload_with_subject("adn_delta_smoke_subject_B_completely_different")).await;
    let has_delta_3 = resp3.contains("ADN_DELTA");
    println!("      ADN_DELTA present={}", has_delta_3);
    if has_delta_3 {
        failures.push(format!("scenario 3: aucune ligne ADN_DELTA attendue (payload identique au parent, severity=NoChange), reponse: {resp3}"));
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios adn_delta_detector sont conformes (cablage live sur le stockage ADN reel, conformite wire single-line verifiee).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
