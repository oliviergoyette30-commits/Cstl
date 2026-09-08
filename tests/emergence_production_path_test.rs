//! Verification EN DIRECT du chemin de PRODUCTION de `purpose=detect_emergence`
//! (src/server/handler.rs, STEP 2c) -- pas des tests unitaires de
//! `src/emergence.rs`/`src/adn_store.rs` qui appellent `detect_revisions()` /
//! `put_emergence_proof()` directement en mémoire (`AdnStore::open(":memory:")`).
//!
//! Trouvaille qui motive ce test (2026-09-08): `RevisionOrchestrator`
//! (src/emergence.rs) et `emergence_proofs` (src/adn_store.rs) étaient déjà
//! honnêtement conçus et déjà cablés dans `handler.rs` via `purpose=
//! detect_emergence` (commit 3326f91) -- mais AUCUN test n'exerçait ce chemin
//! par le vrai port TCP contre un vrai fichier SQLite. Les tests existants de
//! emergence.rs appellent `detect_revisions()` en Rust direct sur un store
//! `:memory:`; ils prouvent que la fonction marche, pas que le SERVEUR
//! l'expose reellement a un client TCP qui écrit dans un fichier réel.
//!
//! Ce que ce test verifie REELLEMENT:
//! 1. Un vrai `CstlNativeServer` tourne sur un vrai port TCP local, adossé à
//!    un fichier SQLite temporaire réel (pas `:memory:`).
//! 2. Deux réponses "solo" (Agent_CLAUDE, Agent_GPT) et une décision
//!    "tripartite" finale sont envoyées comme de VRAIS payloads CSTL sur le
//!    socket, exactement comme le ferait un humain qui relaie manuellement des
//!    réponses de plusieurs LLM (design documenté de emergence.rs -- aucun
//!    appel API n'est fait ici non plus).
//! 3. Un QUATRIEME payload real `purpose=detect_emergence` est envoyé sur ce
//!    même socket -- c'est le point d'entrée de production, pas un appel
//!    direct à `emergence::detect_revisions`.
//! 4. La réponse TCP est vérifiée champ par champ (agents_compared,
//!    revisions_detected, EMERGENCE_REPORT par agent).
//! 5. La preuve matérielle: `AdnStore::get_emergence_proofs()` est appelé sur
//!    le MEME fichier SQLite APRES réouverture d'une connexion fraîche sur ce
//!    fichier -- pas seulement relu depuis la connexion en mémoire du
//!    process serveur -- pour écarter tout artefact "ça marche seulement
//!    parce que c'est encore chaud en mémoire".
//!
//! Ce que ce test NE verifie PAS: le cas multi-connexions concurrentes sur le
//! même fichier (écritures simultanées depuis deux processus séparés) --
//! hors de portée de ce scénario, qui reste fidèle au flux réel (un seul
//! opérateur humain relayant séquentiellement).

use cstl_parser::adn_store::AdnStore;
use cstl_parser::agent_discovery::AgentCard;
use cstl_parser::server::{listener, CstlNativeServer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Envoie `payload` sur une nouvelle connexion TCP vers `addr` et accumule la
/// réponse jusqu'à voir `---END---` -- même contrat de framing que le serveur
/// applique en réception (`find_message_end` dans handler.rs), pour ne pas
/// couper une réponse qui arriverait en plusieurs `read()` TCP.
async fn send_and_receive(addr: std::net::SocketAddr, payload: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connexion TCP au serveur de test");
    stream.write_all(payload.as_bytes()).await.expect("envoi du payload");

    let mut accumulated = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await.expect("lecture de la reponse");
        assert!(n > 0, "connexion fermee avant ---END--- -- reponse incomplete: {:?}", String::from_utf8_lossy(&accumulated));
        accumulated.extend_from_slice(&buf[..n]);
        if accumulated.windows(9).any(|w| w == b"---END---") {
            break;
        }
    }
    String::from_utf8_lossy(&accumulated).to_string()
}

/// Extrait la valeur d'un champ `cle=valeur` (separe par `,`, `]` ou fin de
/// ligne) dans une reponse wire CSTL -- assez pour lire `hash=` dans la ligne
/// AUDIT sans écrire un parseur CSTL complet côté test.
fn extract_field<'a>(response: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{}=", key);
    let start = response.find(&needle)? + needle.len();
    let rest = &response[start..];
    let end = rest.find([',', ']', '\n']).unwrap_or(rest.len());
    Some(&rest[..end])
}

#[tokio::test]
async fn detect_emergence_purpose_writes_a_real_row_via_the_tcp_production_path() {
    let db_path = std::env::temp_dir().join(format!(
        "cstl_emergence_e2e_{}_{}.db",
        std::process::id(),
        now_nanos()
    ));
    let db_path_str = db_path.to_str().expect("chemin UTF-8").to_string();
    let _ = std::fs::remove_file(&db_path); // au cas ou un run precedent aurait laisse un residu

    let server = CstlNativeServer::with_data_path(0, &db_path_str);

    let tcp_listener = listener::create_listener("127.0.0.1:0").await.expect("bind 127.0.0.1:0");
    let addr = tcp_listener.local_addr().expect("adresse locale du listener");

    // On garde une reference separee vers le MEME Arc<Mutex<AdnStore>> que le
    // serveur utilise -- pour verifier apres coup sans passer par le socket.
    let adn_store_handle = server.adn_store.clone();

    // Enregistre un agent "communication" directement dans le registre en
    // memoire (pas via purpose=agent_register -- ca exigerait une signature
    // Ed25519, hors-sujet ici) pour que STEP 4 du handler (route vers un
    // agent) trouve une cible et emette une reponse AVEC la ligne AUDIT --
    // sans ca, un payload normal recoit "purpose=error, status=no_agent" et
    // n'a jamais de hash a extraire, meme s'il a bien ete persiste plus haut
    // dans le pipeline (STEP 3d, avant le routage).
    server.agent_registry.lock().await.register(AgentCard {
        name: "TestRouter".to_string(),
        version: "test".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });

    tokio::spawn(async move {
        let _ = listener::accept_connections(
            tcp_listener,
            server.agent_registry,
            server.chain,
            server.kb_verifier,
            server.adn_store,
            server.restricted_council,
            server.telegram,
            server.obsidian,
            server.governance,
        )
        .await;
    });

    // -- 1) Reponse SOLO de Agent_CLAUDE : "option_C" (va diverger du trio) --
    let solo_claude = "#!CSTL v5.0.0 MODE=A\n\
        META [encoder=TestHarness, produced_by=Agent_CLAUDE, status=draft]\n\
        INTENT_PAYLOAD [purpose=solo_answer, sender=Agent_CLAUDE, receiver=Server]\n\
        DECISION: option_C [sigma=0.82]\n\
        ---END---\n";
    let resp_claude = send_and_receive(addr, solo_claude).await;
    let hash_claude = extract_field(&resp_claude, "hash")
        .unwrap_or_else(|| panic!("pas de hash AUDIT dans la reponse solo Claude: {}", resp_claude))
        .to_string();

    // -- 2) Reponse SOLO de Agent_GPT : "option_D" (deja alignee sur le trio) --
    let solo_gpt = "#!CSTL v5.0.0 MODE=A\n\
        META [encoder=TestHarness, produced_by=Agent_GPT, status=draft]\n\
        INTENT_PAYLOAD [purpose=solo_answer, sender=Agent_GPT, receiver=Server]\n\
        DECISION: option_D [sigma=0.84]\n\
        ---END---\n";
    let resp_gpt = send_and_receive(addr, solo_gpt).await;
    let hash_gpt = extract_field(&resp_gpt, "hash")
        .unwrap_or_else(|| panic!("pas de hash AUDIT dans la reponse solo GPT: {}", resp_gpt))
        .to_string();

    // -- 3) Decision TRIPARTITE finale : "option_D" retenu --
    let trio = "#!CSTL v5.0.0 MODE=A\n\
        META [encoder=TestHarness, produced_by=Council, status=final]\n\
        INTENT_PAYLOAD [purpose=tripartite_decision, sender=Council, receiver=Server]\n\
        DECISION: option_D [sigma=0.91]\n\
        ---END---\n";
    let resp_trio = send_and_receive(addr, trio).await;
    let hash_trio = extract_field(&resp_trio, "hash")
        .unwrap_or_else(|| panic!("pas de hash AUDIT dans la reponse trio: {}", resp_trio))
        .to_string();

    assert_ne!(hash_claude, hash_gpt, "deux payloads de contenu different doivent avoir un hash different");
    assert_ne!(hash_claude, hash_trio);
    assert_ne!(hash_gpt, hash_trio);

    // -- 4) LE POINT D'ENTREE DE PRODUCTION TESTE ICI : purpose=detect_emergence
    // envoye sur le socket, pas un appel direct a emergence::detect_revisions.
    let detect = format!(
        "#!CSTL v5.0.0 MODE=A\n\
        META [encoder=TestHarness, produced_by=Olivier, status=draft]\n\
        INTENT_PAYLOAD [purpose=detect_emergence, sender=Olivier, receiver=Server, trio_hash={}, solo_hashes=Agent_CLAUDE:{};Agent_GPT:{}, question=Q_integration_e2e]\n\
        ---END---\n",
        hash_trio, hash_claude, hash_gpt
    );
    let resp_detect = send_and_receive(addr, &detect).await;

    assert!(
        resp_detect.contains("purpose=detect_emergence_result"),
        "reponse inattendue au purpose=detect_emergence: {}",
        resp_detect
    );
    assert!(resp_detect.contains("agents_compared=2"), "reponse: {}", resp_detect);
    assert!(resp_detect.contains("revisions_detected=1"), "reponse: {}", resp_detect);
    assert!(
        resp_detect.contains("EMERGENCE_REPORT [agent=Agent_CLAUDE, revised=true"),
        "Agent_CLAUDE (option_C -> option_D) doit etre signale comme revise: {}",
        resp_detect
    );
    assert!(
        resp_detect.contains("EMERGENCE_REPORT [agent=Agent_GPT, revised=false"),
        "Agent_GPT (deja sur option_D) ne doit PAS etre signale comme revise: {}",
        resp_detect
    );

    // -- 5) LA VRAIE PREUVE -- pas juste "la reponse TCP dit revisions_detected=1",
    // mais une ligne reellement lisible dans emergence_proofs via
    // get_emergence_proofs(), sur le store PARTAGE avec le serveur.
    let proofs_live = adn_store_handle
        .lock()
        .await
        .get_emergence_proofs()
        .expect("get_emergence_proofs sur le store partage avec le serveur");
    let matching_live: Vec<_> = proofs_live.iter().filter(|p| p.question == "Q_integration_e2e").collect();
    assert_eq!(
        matching_live.len(),
        1,
        "une seule ligne emergence_proofs attendue pour cette question, questions vues: {:?}",
        proofs_live.iter().map(|p| p.question.clone()).collect::<Vec<_>>()
    );
    let proof = matching_live[0];
    assert_eq!(proof.position_changed_by.as_deref(), Some("Agent_CLAUDE"));
    assert_eq!(proof.changed_to.as_deref(), Some("option_D"));
    assert_eq!(proof.final_decision, "option_D");
    assert!(proof.solo_answers.contains("option_C"), "solo_answers doit garder la position initiale de Claude: {}", proof.solo_answers);
    assert!(proof.solo_answers.contains("option_D"), "solo_answers doit garder la position initiale de GPT: {}", proof.solo_answers);
    assert!(proof.delta_sigma.is_some());

    // Libere le lock/l'Arc local avant de rouvrir une connexion independante
    // sur le meme fichier -- exclut l'hypothese "ca ne marche que parce que
    // c'est encore la meme Connection SQLite en memoire".
    drop(adn_store_handle);
    let reopened = AdnStore::open(&db_path_str).expect("reouverture d'une connexion fraiche sur le meme fichier SQLite");
    let proofs_reopened = reopened.get_emergence_proofs().expect("get_emergence_proofs apres reouverture du fichier");
    assert!(
        proofs_reopened.iter().any(|p| p.question == "Q_integration_e2e" && p.position_changed_by.as_deref() == Some("Agent_CLAUDE")),
        "la ligne emergence_proofs doit survivre a la fermeture/reouverture du fichier SQLite reel"
    );

    let _ = std::fs::remove_file(&db_path);
}

/// Suffixe de nom de fichier temporaire unique -- `process::id()` seul ne
/// suffit pas si ce test tournait deux fois dans le meme process (peu probable
/// avec `cargo test`, mais evite toute collision de fichier entre un run
/// precedent mal nettoye et celui-ci).
fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
