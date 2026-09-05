//! Verification EN DIRECT du mecanisme de budget mur-a-mur ajoute dans
//! `src/server/handler.rs` le 2026-09-05 (`KB_VERIFICATION_WALL_CLOCK_BUDGET`,
//! 8s) suite a un bug reel decouvert par l'utilisateur sur sa propre machine:
//! un `TimeoutError` cote client Python (`cstl_client.py`, timeout socket
//! fixe a 15s) alors que le serveur avait fini par repondre avec succes,
//! juste trop tard, a cause d'une verification KB (Wikidata) anormalement
//! lente (jusqu'a 40 appels SPARQL sequentiels possibles pour une seule
//! relation, cf. `find_property_chain`).
//!
//! Ce test NE reexecute PAS `handler.rs` tel quel (ca exigerait de monter tout
//! le serveur TCP + registre d'agents + adn_store pour une seule ligne de
//! logique) -- il exerce directement le meme mecanisme,
//! `tokio::time::timeout(KB_VERIFICATION_WALL_CLOCK_BUDGET,
//! kb_verifier.verify_relation(...))`, avec la MEME valeur de budget (8s),
//! contre un vrai serveur HTTP mock (`wiremock`) configure pour repondre
//! delibirement plus lentement que ce budget sur le premier appel reseau que
//! `verify_relation` effectue (`search_entity`, via `search_api`).
//!
//! Ce que ce test PROUVE reellement: que le futur `verify_relation(...)`,
//! une fois englobe dans `tokio::time::timeout(Duration::from_secs(8), ...)`,
//! se termine bien en Err (timeout) en un temps borne proche de 8s -- PAS en
//! attendant la latence complete du mock (10s) ni en bloquant indefiniment.
//! C'est exactement la garantie dont depend le correctif de `handler.rs`:
//! que le budget coupe court avant que le timeout socket du client (15s) ne
//! soit atteint, quelle que soit la lenteur du reseau Wikidata reel.
//!
//! Ce que ce test NE verifie PAS: le comportement du vrai `handler.rs` de
//! bout en bout (reponse TCP effectivement envoyee a temps) -- ca reste a
//! confirmer par l'utilisateur sur sa machine en relancant son scenario
//! Gemini qui avait initialement declenche le bug (turn 3, timeout cote
//! client). Wikidata.org est bloque depuis ce sandbox (voir l'en-tete de
//! `tests/kb_verify_mock_wikidata_test.rs`), donc aucun test ici ne peut
//! reproduire la latence reelle du vrai Wikidata, seulement le mecanisme de
//! coupure cote serveur.

use std::time::{Duration, Instant};

use cstl_parser::kb_verify::KbVerifier;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Meme valeur que `KB_VERIFICATION_WALL_CLOCK_BUDGET` dans
/// `src/server/handler.rs` -- dupliquee ici volontairement (la constante de
/// `handler.rs` est privee au module et le but de ce test est de verifier le
/// MECANISME `tokio::time::timeout` avec ce budget precis, pas d'importer un
/// detail d'implementation interne du handler).
const KB_VERIFICATION_WALL_CLOCK_BUDGET: Duration = Duration::from_secs(8);

#[tokio::test]
async fn test_wall_clock_timeout_cuts_off_a_slow_kb_verification_within_budget() {
    let mock_server = MockServer::start().await;

    // Le tout premier appel reseau de verify_relation() est search_entity()
    // sur le sujet, qui frappe search_api ("/w/api.php" par convention dans
    // ce projet, cf. tests/kb_verify_mock_wikidata_test.rs). On le fait
    // deliberement repondre APRES 10s -- strictement plus lent que le
    // budget de 8s -- pour garantir que verify_relation() seul n'aurait
    // jamais pu terminer a temps sans le wrapper de timeout.
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "search": [] }))
                .set_delay(Duration::from_secs(10)),
        )
        .mount(&mock_server)
        .await;

    let verifier = KbVerifier::with_endpoints(
        format!("{}/w/api.php", mock_server.uri()),
        format!("{}/sparql", mock_server.uri()),
    );

    let started = Instant::now();
    let outcome = tokio::time::timeout(
        KB_VERIFICATION_WALL_CLOCK_BUDGET,
        verifier.verify_relation("Montreal", "located_in", "Canada", "fr", 4, 40),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(
        outcome.is_err(),
        "le timeout aurait du se declencher (mock delibirement plus lent que le budget), \
         mais verify_relation() a termine a temps: {outcome:?}"
    );

    // Le budget est de 8s -- on tolere une marge (jusqu'a 9.5s) pour le
    // jitter du scheduler tokio/CI, mais PAS jusqu'aux 10s complets du mock:
    // ca prouverait que le timeout n'a rien coupe et qu'on a juste attendu
    // la reponse (lente) du mock.
    assert!(
        elapsed < Duration::from_millis(9500),
        "le timeout devait couper court pres de 8s, mais {elapsed:?} se sont ecoules \
         (proche ou au-dela des 10s du mock -- le wrapper timeout() n'a rien coupe)"
    );
    assert!(
        elapsed >= KB_VERIFICATION_WALL_CLOCK_BUDGET,
        "le timeout ne doit pas se declencher AVANT le budget annonce (8s), \
         obtenu {elapsed:?} -- ce serait un budget plus court que documente"
    );
}

#[tokio::test]
async fn test_no_timeout_when_kb_verification_is_faster_than_budget() {
    // Controle negatif: si le mock repond vite (bien avant les 8s), le
    // wrapper timeout() ne doit RIEN couper -- verify_relation() doit
    // terminer normalement et outcome doit etre Ok(...). Un mock qui repond
    // "not found" partout donne un VerificationResult "unverified"/"not
    // found", peu importe le contenu exact: seul le fait que
    // tokio::time::timeout() retourne Ok(_) (pas Err(Elapsed)) compte ici.
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "search": [] })))
        .mount(&mock_server)
        .await;

    let verifier = KbVerifier::with_endpoints(
        format!("{}/w/api.php", mock_server.uri()),
        format!("{}/sparql", mock_server.uri()),
    );

    let outcome = tokio::time::timeout(
        KB_VERIFICATION_WALL_CLOCK_BUDGET,
        verifier.verify_relation("Montreal", "located_in", "Canada", "fr", 4, 40),
    )
    .await;

    assert!(
        outcome.is_ok(),
        "avec un mock rapide, verify_relation() aurait du terminer bien avant le budget de 8s, \
         mais le timeout s'est declenche: {outcome:?}"
    );
}
