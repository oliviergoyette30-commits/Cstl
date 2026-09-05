//! Verification EN DIRECT de l'orchestration reseau de
//! `KbVerifier::detect_entanglement` (et des methodes qu'elle appelle:
//! `query_generic_neighbors`, `has_direct_relation`/`query_any_relation_exists`,
//! `resolve_label`) -- Level 4, `src/hypothesis_engine.rs`.
//!
//! Ce que ce test verifie REELLEMENT: un vrai serveur HTTP (`wiremock`) tourne
//! sur `127.0.0.1`, `KbVerifier` lui envoie de vraies requetes HTTP (via
//! `reqwest`, meme client que la production, seule l'URL de base change via
//! `KbVerifier::with_endpoints`), et les reponses JSON sont reellement
//! deserialisees par le code de production dans `src/kb_verify.rs`. L'appel
//! reseau, le parsing de la query SPARQL entrante (cote mock, pour distinguer
//! quelle des 3 requetes SPARQL differentes arrive) et la deserialisation JSON
//! sortante sont donc tous les trois reellement exerces -- ce n'est PAS un
//! stub en memoire qui court-circuite reqwest.
//!
//! Ce que ce test NE verifie PAS: que ceci reproduit fidelement le VRAI
//! wikidata.org. Le format JSON mocke ici est ecrit a la main d'apres la
//! lecture du code de `src/kb_verify.rs` (SPARQL JSON results format standard),
//! pas capture depuis une vraie reponse de wikidata.org (impossible depuis ce
//! sandbox, wikidata.org y est bloque -- voir l'en-tete de
//! `src/hypothesis_engine.rs`). Le vrai Wikidata peut repondre avec une
//! latence, des donnees, des erreurs partielles (timeouts intermittents,
//! throttling) ou des variations de format que ce mock ne reproduit pas.
//! Reste a verifier sur une machine avec acces reseau reel a wikidata.org.

use cstl_parser::kb_verify::KbVerifier;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Extrait tous les identifiants "QNNN" apparaissant apres un prefixe "wd:"
/// dans le texte d'une requete SPARQL -- suffisant pour distinguer, cote mock,
/// quelle entite (ou quelle paire d'entites) une requete concerne, sans avoir
/// besoin d'un vrai parseur SPARQL.
fn extract_qids(query: &str) -> Vec<String> {
    let mut qids = Vec::new();
    let mut rest = query;
    let mut consumed = 0usize;
    while let Some(pos) = rest.find("wd:") {
        let abs_start = consumed + pos + 3;
        let tail = &query[abs_start..];
        let end = tail.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(tail.len());
        if end > 0 {
            qids.push(tail[..end].to_string());
        }
        let advance = pos + 3 + end.max(1);
        rest = &rest[advance.min(rest.len())..];
        consumed += advance.min(query.len() - consumed);
    }
    qids
}

#[test]
fn test_extract_qids_finds_all_entities_in_query() {
    let q = "SELECT ?prop WHERE { { wd:Q1 ?prop wd:Q2 . } UNION { wd:Q2 ?prop wd:Q1 . } } LIMIT 5";
    assert_eq!(extract_qids(q), vec!["Q1", "Q2", "Q2", "Q1"]);
}

/// Petit graphe simule, ecrit a la main pour ce test:
/// - Q1 (sujet) a pour voisins {Q10,Q11,Q12,Q13}
/// - Q2 a pour voisins {Q10,Q11,Q12,Q99} (3 voisins communs avec Q1, overlap
///   0.75, AUCUNE relation directe connue avec Q1) -> DOIT declencher une
///   hypothese.
/// - Q3 a pour voisins {Q50,Q51} (0 voisin commun avec Q1) -> controle
///   negatif: recouvrement insuffisant, ne doit RIEN declencher.
/// - Q4 a pour voisins {Q10,Q11,Q12,Q13} (recouvrement total avec Q1) MAIS une
///   relation directe deja connue avec Q1 -> deuxieme controle negatif: meme
///   un recouvrement parfait ne doit rien declencher quand une relation
///   directe existe deja.
struct SparqlResponder;

impl Respond for SparqlResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let query = request
            .url
            .query_pairs()
            .find(|(k, _)| k == "query")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();

        if query.contains("rdfs:label") {
            let qids = extract_qids(&query);
            let label = match qids.first().map(String::as_str) {
                Some("Q1") => Some("Marie Curie (mock)"),
                Some("Q2") => Some("Irene Joliot-Curie (mock)"),
                _ => None,
            };
            let body = match label {
                Some(l) => serde_json::json!({
                    "results": { "bindings": [ { "label": { "value": l } } ] }
                }),
                None => serde_json::json!({ "results": { "bindings": [] } }),
            };
            return ResponseTemplate::new(200).set_body_json(body);
        }

        if query.contains("?prop WHERE") {
            // query_any_relation_exists(subject_qid, object_qid): les deux
            // QID apparaissent, le sujet (Q1 dans ce test) toujours en
            // premier -- on cherche l'autre pour savoir quelle paire.
            let qids = extract_qids(&query);
            let other = qids.iter().find(|q| q.as_str() != "Q1");
            let already_related = matches!(other.map(String::as_str), Some("Q4"));
            let bindings = if already_related {
                serde_json::json!([ { "prop": { "value": "http://www.wikidata.org/prop/direct/P1" } } ])
            } else {
                serde_json::json!([])
            };
            return ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "results": { "bindings": bindings } }));
        }

        if query.contains("SELECT DISTINCT ?n WHERE") {
            let qids = extract_qids(&query);
            let neighbors: &[&str] = match qids.first().map(String::as_str) {
                Some("Q1") => &["Q10", "Q11", "Q12", "Q13"],
                Some("Q2") => &["Q10", "Q11", "Q12", "Q99"],
                Some("Q3") => &["Q50", "Q51"],
                Some("Q4") => &["Q10", "Q11", "Q12", "Q13"],
                _ => &[],
            };
            let bindings: Vec<_> = neighbors
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "n": { "value": format!("http://www.wikidata.org/entity/{n}") }
                    })
                })
                .collect();
            return ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "results": { "bindings": bindings } }));
        }

        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "results": { "bindings": [] } }))
    }
}

/// Serveur qui retourne systematiquement une erreur HTTP 500 -- verifie que
/// l'orchestration degrade proprement (aucune hypothese generee, pas de
/// panic) quand le reseau echoue completement, au lieu de confondre "reseau
/// en panne" avec "pas de relation" (ce que le commentaire de
/// `KbVerifier::has_direct_relation` promet explicitement).
struct AlwaysServerErrorResponder;

impl Respond for AlwaysServerErrorResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        ResponseTemplate::new(500).set_body_string("internal server error (mock)")
    }
}

/// Serveur qui retourne du 200 mais un corps qui n'est PAS du JSON valide --
/// verifie que la deserialisation echouee (`resp.json::<Value>()` en erreur)
/// est bien absorbee sans panic, exactement comme une vraie panne partielle
/// de service pourrait le produire.
struct MalformedJsonResponder;

impl Respond for MalformedJsonResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_string("{not valid json ]")
    }
}

#[tokio::test]
async fn test_detect_entanglement_against_mock_wikidata_positive_and_negative() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sparql"))
        .respond_with(SparqlResponder)
        .mount(&mock_server)
        .await;

    // wbsearchentities n'est pas appele par detect_entanglement -- l'URL de
    // recherche pointe vers le meme mock par simplicite, jamais interrogee ici.
    let verifier = KbVerifier::with_endpoints(
        format!("{}/w/api.php", mock_server.uri()),
        format!("{}/sparql", mock_server.uri()),
    );

    let candidates = vec!["Q2".to_string(), "Q3".to_string(), "Q4".to_string()];
    let hypotheses = verifier
        .detect_entanglement("Q1", &candidates, 50, 2, "fr")
        .await;

    assert_eq!(
        hypotheses.len(),
        1,
        "attendu exactement 1 hypothese (Q2), obtenu: {hypotheses:#?}"
    );

    let h = &hypotheses[0];
    assert_eq!(h.subject_qid, "Q1");
    assert_eq!(h.object_qid, "Q2");
    assert_eq!(h.subject_label.as_deref(), Some("Marie Curie (mock)"));
    assert_eq!(h.object_label.as_deref(), Some("Irene Joliot-Curie (mock)"));
    assert_eq!(h.common_neighbors, 3); // {Q10,Q11,Q12}
    assert!((h.overlap_coefficient - 0.75).abs() < 1e-9); // 3 / min(4,4)
    assert!(h.sigma <= 0.35, "sigma ne doit jamais depasser le plafond ASSUMES");

    let relation = h.to_cstl_relation();
    assert!(relation.contains("type=ASSUMES"));
    assert!(relation.contains("subject=Marie Curie (mock)"));
    assert!(relation.contains("object=Irene Joliot-Curie (mock)"));

    // Controles negatifs verifies implicitement par assert_eq!(len(), 1) ci-dessus:
    // Q3 (recouvrement nul) et Q4 (relation directe deja connue malgre un
    // recouvrement total) n'ont produit AUCUNE hypothese.
}

#[tokio::test]
async fn test_detect_entanglement_degrades_cleanly_on_http_500() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sparql"))
        .respond_with(AlwaysServerErrorResponder)
        .mount(&mock_server)
        .await;

    let verifier = KbVerifier::with_endpoints(
        format!("{}/w/api.php", mock_server.uri()),
        format!("{}/sparql", mock_server.uri()),
    );

    let candidates = vec!["Q2".to_string()];
    // Ne doit PAS paniquer: query_generic_neighbors(subject) echoue (500),
    // detect_entanglement voit un voisinage vide et retourne vide.
    let hypotheses = verifier
        .detect_entanglement("Q1", &candidates, 50, 2, "fr")
        .await;
    assert!(hypotheses.is_empty());
}

#[tokio::test]
async fn test_detect_entanglement_degrades_cleanly_on_malformed_json() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sparql"))
        .respond_with(MalformedJsonResponder)
        .mount(&mock_server)
        .await;

    let verifier = KbVerifier::with_endpoints(
        format!("{}/w/api.php", mock_server.uri()),
        format!("{}/sparql", mock_server.uri()),
    );

    let candidates = vec!["Q2".to_string()];
    // Ne doit pas paniquer sur un corps 200 mais non-JSON: resp.json() echoue,
    // query_generic_neighbors retourne un HashSet vide -> pas d'hypothese.
    let hypotheses = verifier
        .detect_entanglement("Q1", &candidates, 50, 2, "fr")
        .await;
    assert!(hypotheses.is_empty());
}
