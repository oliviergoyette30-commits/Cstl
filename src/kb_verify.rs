//! src/kb_verify.rs — Couche 3a de l'architecture CSTL (port Rust fidèle)
//! Vérification d'une relation CSTL (subject, predicate, object) contre Wikidata.
//! Port de orchestrator/cstl_verify_public_kb.py — même logique, même mapping,
//! mêmes bornes BFS, mêmes corrections empiriques (2026-08-06 / 08-10 / 08-12).

use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use serde::Serialize;
use serde_json::Value;
use crate::hypothesis_engine::{EntanglementHypothesis, overlap_coefficient};

const WIKIDATA_SEARCH_API: &str = "https://www.wikidata.org/w/api.php";
const WIKIDATA_SPARQL_ENDPOINT: &str = "https://query.wikidata.org/sparql";
const USER_AGENT: &str = "CSTL-Verifier/1.1 (research project; contact: orchestrator)";
const DISAMBIGUATION_PAGE_QID: &str = "Q4167410";

fn predicate_to_property(predicate: &str) -> Option<&'static str> {
    match predicate {
        "born_in" => Some("P19"),
        "died_in" => Some("P20"),
        "spouse" => Some("P26"),
        "child_of" => Some("P22"),
        "employer" => Some("P108"),
        "occupation" => Some("P106"),
        "nationality" => Some("P27"),
        "founder_of" => Some("P112"),
        "located_in" => Some("P131"),
        "part_of" => Some("P361"),
        "author_of" => Some("P50"),
        "capital_of" => Some("P36"),
        _ => None,
    }
}

/// Propriétés pour lesquelles une chaîne transitive à plusieurs sauts a un
/// sens sémantique réel (hiérarchies d'imbrication géographique/administrative).
///
/// Corrige une trouvaille majeure de l'audit multi-angle (2026-09-03):
/// cette fonction encodait independamment (`matches!(property_id, "P131" |
/// "P361")`) la MEME notion que `execution_lab::CHAINABLE_PREDICATES`
/// (`["part_of", "located_in"]`), dans un espace de cles different (ID
/// Wikidata ici, nom de predicat CSTL la-bas), sans aucune reference
/// croisee. Un ajout dans l'une sans l'autre desynchronisait silencieusement
/// la verification KB (Couche 3a) et le calcul de coherence interne
/// (ExecutionLab). Desormais derivee directement de
/// execution_lab::CHAINABLE_PREDICATES via predicate_to_property -- une
/// seule source de verite, plus de liste dupliquee a maintenir a la main.
fn is_chainable(property_id: &str) -> bool {
    crate::execution_lab::CHAINABLE_PREDICATES
        .iter()
        .any(|predicate| predicate_to_property(predicate) == Some(property_id))
}

#[derive(Debug, Serialize)]
pub struct VerificationResult {
    pub verified: String,
    pub source_url: Option<String>,
    pub reason: String,
    pub subject_qid: Option<String>,
    pub object_qid: Option<String>,
    pub check_method: Option<String>,
    pub chain: Option<Vec<(String, String)>>,
    pub property_id: Option<String>,
}

pub struct KbVerifier {
    client: reqwest::Client,
    search_api: String,
    sparql_endpoint: String,
}

impl Default for KbVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl KbVerifier {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            search_api: WIKIDATA_SEARCH_API.to_string(),
            sparql_endpoint: WIKIDATA_SPARQL_ENDPOINT.to_string(),
        }
    }

    /// Comme `new()`, mais pointe vers des endpoints arbitraires au lieu des
    /// constantes Wikidata codees en dur -- seul moyen d'exercer reellement
    /// l'orchestration HTTP (`detect_entanglement` et les methodes qu'il
    /// appelle) contre un serveur mock local plutot que le vrai wikidata.org.
    /// `new()` n'est pas modifie et continue a viser les vraies URLs de
    /// production -- ce constructeur est strictement additif.
    pub fn with_endpoints(search_api: impl Into<String>, sparql_endpoint: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            search_api: search_api.into(),
            sparql_endpoint: sparql_endpoint.into(),
        }
    }

    async fn is_disambiguation_page(&self, qid: &str) -> bool {
        let query = format!("ASK {{ wd:{qid} wdt:P31 wd:{DISAMBIGUATION_PAGE_QID} . }}");
        let resp = self.client.get(&self.sparql_endpoint)
            .query(&[("query", query.as_str()), ("format", "json")])
            .timeout(Duration::from_secs(10))
            .send().await;
        match resp {
            Ok(r) => match r.json::<Value>().await {
                Ok(j) => j.get("boolean").and_then(Value::as_bool).unwrap_or(false),
                Err(_) => false,
            },
            // en cas de doute, ne pas bloquer sur ce filtre seul (comportement Python conservé)
            Err(_) => false,
        }
    }

    /// Retourne (qid, label_trouve_ou_raison). L'anglais est essayé EN PREMIER,
    /// indépendamment de `lang` (correction du 2026-08-10 : la recherche
    /// anglaise résout mieux les entités communes sur l'index Wikidata).
    async fn search_entity(&self, label: &str, lang: &str, max_candidates: usize) -> (Option<String>, String) {
        let mut languages_to_try = vec!["en"];
        if lang != "en" {
            languages_to_try.push(lang);
        }

        for language in languages_to_try {
            let resp = self.client.get(&self.search_api)
                .query(&[
                    ("action", "wbsearchentities"),
                    ("search", label),
                    ("language", language),
                    ("format", "json"),
                    ("limit", &max_candidates.to_string()),
                ])
                .timeout(Duration::from_secs(10))
                .send().await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => return (None, format!("network_error: {e}")),
            };
            let json: Value = match resp.json().await {
                Ok(j) => j,
                Err(e) => return (None, format!("network_error: {e}")),
            };

            let results = json.get("search").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut had_results = false;
            for candidate in &results {
                had_results = true;
                let qid = candidate.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                if qid.is_empty() { continue; }
                if !self.is_disambiguation_page(&qid).await {
                    let found_label = candidate.get("label").and_then(Value::as_str)
                        .unwrap_or(label).to_string();
                    return (Some(qid), found_label);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            if had_results { continue; }
        }
        (None, "no_valid_non_disambiguation_entity_found".to_string())
    }

    async fn query_specific_property(&self, subject_qid: &str, object_qid: &str, property_id: &str)
        -> (Option<bool>, Option<String>)
    {
        let query = format!(
            "ASK {{ {{ wd:{subject_qid} wdt:{property_id} wd:{object_qid} . }} UNION \
             {{ wd:{object_qid} wdt:{property_id} wd:{subject_qid} . }} }}"
        );
        let resp = self.client.get(&self.sparql_endpoint)
            .query(&[("query", query.as_str()), ("format", "json")])
            .timeout(Duration::from_secs(15))
            .send().await;
        match resp {
            Ok(r) => match r.json::<Value>().await {
                Ok(j) => (Some(j.get("boolean").and_then(Value::as_bool).unwrap_or(false)), None),
                Err(e) => (None, Some(format!("network_error: {e}"))),
            },
            Err(e) => (None, Some(format!("network_error: {e}"))),
        }
    }

    async fn query_property_neighbors(&self, qid: &str, property_id: &str, limit: usize) -> Vec<String> {
        let query = format!(
            "SELECT DISTINCT ?n WHERE {{ {{ wd:{qid} wdt:{property_id} ?n . }} UNION \
             {{ ?n wdt:{property_id} wd:{qid} . }} }} LIMIT {limit}"
        );
        let resp = self.client.get(&self.sparql_endpoint)
            .query(&[("query", query.as_str()), ("format", "json")])
            .timeout(Duration::from_secs(15))
            .send().await;
        let json: Value = match resp {
            Ok(r) => match r.json().await { Ok(j) => j, Err(_) => return vec![] },
            Err(_) => return vec![],
        };
        json.pointer("/results/bindings").and_then(Value::as_array).cloned().unwrap_or_default()
            .iter()
            .filter_map(|b| b.pointer("/n/value").and_then(Value::as_str))
            .filter_map(|uri| uri.rsplit('/').next())
            .filter(|qid| qid.starts_with('Q'))
            .map(String::from)
            .collect()
    }

    /// BFS bornée par max_hops (profondeur) ET max_expansions (nœuds explorés,
    /// donc appels SPARQL) — deux caps indépendants, exactement comme l'original.
    /// Retourne (chemin, exhausted). exhausted=false => résultat NON CONCLUANT,
    /// jamais à traiter comme une preuve d'absence de chaîne.
    async fn find_property_chain(&self, subject_qid: &str, object_qid: &str, property_id: &str,
        max_hops: usize, max_expansions: usize) -> (Option<Vec<(String, String)>>, bool)
    {
        if subject_qid == object_qid {
            return (Some(vec![]), true);
        }
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(subject_qid.to_string());
        let mut queue: VecDeque<(String, Vec<(String, String)>)> = VecDeque::new();
        queue.push_back((subject_qid.to_string(), vec![]));
        let mut expansions = 0usize;

        while let Some((current_qid, path)) = queue.pop_front() {
            if expansions >= max_expansions {
                return (None, false); // budget épuisé, pas prouvé négatif
            }
            if path.len() >= max_hops {
                continue;
            }
            expansions += 1;
            for neighbor_qid in self.query_property_neighbors(&current_qid, property_id, 50).await {
                if neighbor_qid == object_qid {
                    let mut full_path = path.clone();
                    full_path.push((current_qid.clone(), neighbor_qid));
                    return (Some(full_path), true);
                }
                if !visited.contains(&neighbor_qid) {
                    visited.insert(neighbor_qid.clone());
                    let mut new_path = path.clone();
                    new_path.push((current_qid.clone(), neighbor_qid.clone()));
                    queue.push_back((neighbor_qid, new_path));
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        (None, true) // queue vidée naturellement : recherche complète
    }

    async fn query_any_relation_exists(&self, subject_qid: &str, object_qid: &str)
        -> (Option<bool>, Option<String>)
    {
        let query = format!(
            "SELECT ?prop WHERE {{ {{ wd:{subject_qid} ?prop wd:{object_qid} . }} UNION \
             {{ wd:{object_qid} ?prop wd:{subject_qid} . }} }} LIMIT 5"
        );
        let resp = self.client.get(&self.sparql_endpoint)
            .query(&[("query", query.as_str()), ("format", "json")])
            .timeout(Duration::from_secs(15))
            .send().await;
        match resp {
            Ok(r) => match r.json::<Value>().await {
                Ok(j) => {
                    let bindings = j.pointer("/results/bindings").and_then(Value::as_array)
                        .cloned().unwrap_or_default();
                    match bindings.first() {
                        Some(first) => (Some(true),
                            first.pointer("/prop/value").and_then(Value::as_str).map(String::from)),
                        None => (Some(false), None),
                    }
                }
                Err(e) => (None, Some(format!("network_error: {e}"))),
            },
            Err(e) => (None, Some(format!("network_error: {e}"))),
        }
    }

    fn build_result(&self, found: bool, subject_qid: &str, object_qid: &str,
        check_method: &str, chain: Option<Vec<(String, String)>>, property_id: Option<&str>
    ) -> VerificationResult {
        let property_id = property_id.map(String::from);
        if found {
            VerificationResult {
                verified: "confirmed_external_source".into(),
                source_url: Some(format!("https://www.wikidata.org/wiki/{subject_qid}")),
                reason: format!("property_confirmed_via_{check_method}"),
                subject_qid: Some(subject_qid.into()),
                object_qid: Some(object_qid.into()),
                check_method: Some(check_method.into()),
                chain, property_id,
            }
        } else {
            VerificationResult {
                verified: "unchallenged_unproven".into(),
                source_url: Some(format!("https://www.wikidata.org/wiki/{subject_qid}")),
                reason: format!("entities_resolved_but_relation_not_confirmed_via_{check_method}"),
                subject_qid: Some(subject_qid.into()),
                object_qid: Some(object_qid.into()),
                check_method: Some(check_method.into()),
                chain: None, property_id,
            }
        }
    }

    /// Voisinage generique d'une entite (toute propriete, pas seulement les 12
    /// mappees dans `predicate_to_property`) -- utilise par le moteur d'hypotheses
    /// (Level 4, `src/hypothesis_engine.rs`) pour detecter un recouvrement de
    /// voisinage entre deux entites, pas pour verifier une relation CSTL precise
    /// comme `query_property_neighbors` ci-dessus.
    pub async fn query_generic_neighbors(&self, qid: &str, limit: usize) -> HashSet<String> {
        let query = format!(
            "SELECT DISTINCT ?n WHERE {{ {{ wd:{qid} ?p ?n . }} UNION {{ ?n ?p wd:{qid} . }} \
             FILTER(STRSTARTS(STR(?n), \"http://www.wikidata.org/entity/Q\")) }} LIMIT {limit}"
        );
        let resp = self.client.get(&self.sparql_endpoint)
            .query(&[("query", query.as_str()), ("format", "json")])
            .timeout(Duration::from_secs(15))
            .send().await;
        let json: Value = match resp {
            Ok(r) => match r.json().await { Ok(j) => j, Err(_) => return HashSet::new() },
            Err(_) => return HashSet::new(),
        };
        json.pointer("/results/bindings").and_then(Value::as_array).cloned().unwrap_or_default()
            .iter()
            .filter_map(|b| b.pointer("/n/value").and_then(Value::as_str))
            .filter_map(|uri| uri.rsplit('/').next())
            .filter(|qid| qid.starts_with('Q'))
            .map(String::from)
            .collect()
    }

    /// Label lisible d'un QID (`rdfs:label`, langue demandee sinon anglais en
    /// repli) -- utilise pour rendre une hypothese generee lisible par un humain
    /// plutot que de n'afficher que des QID.
    pub async fn resolve_label(&self, qid: &str, lang: &str) -> Option<String> {
        let query = format!(
            "SELECT ?label WHERE {{ wd:{qid} rdfs:label ?label . \
             FILTER(lang(?label) = \"{lang}\" || lang(?label) = \"en\") }} LIMIT 1"
        );
        let resp = self.client.get(&self.sparql_endpoint)
            .query(&[("query", query.as_str()), ("format", "json")])
            .timeout(Duration::from_secs(10))
            .send().await.ok()?;
        let json: Value = resp.json().await.ok()?;
        json.pointer("/results/bindings/0/label/value").and_then(Value::as_str).map(String::from)
    }

    /// `Some(true)`/`Some(false)` si une relation directe (n'importe quelle
    /// propriete) existe deja entre les deux entites -- reutilise
    /// `query_any_relation_exists`. `None` sur echec reseau: jamais traite comme
    /// "pas de relation" par l'appelant, pour ne jamais generer une hypothese a
    /// partir d'une incertitude reseau plutot que d'un vrai signal.
    pub async fn has_direct_relation(&self, a: &str, b: &str) -> Option<bool> {
        self.query_any_relation_exists(a, b).await.0
    }

    /// Point d'entree du moteur d'hypotheses (Level 4, "Future Architecture" --
    /// voir `src/hypothesis_engine.rs`). Pour chaque candidat, calcule le
    /// recouvrement de voisinage avec `subject_qid` et propose une
    /// `EntanglementHypothesis` quand (a) aucune relation directe connue n'existe
    /// deja entre les deux entites et (b) le nombre de voisins communs atteint
    /// `min_common_neighbors`. Ne propose RIEN sur un echec/vide reseau partiel --
    /// un voisinage vide ou une verification de relation directe indisponible fait
    /// sauter le candidat plutot que de generer une hypothese depuis une
    /// incertitude. Non verifiable en direct depuis ce sandbox (wikidata.org
    /// bloque par la liste blanche reseau) -- voir l'en-tete de
    /// `hypothesis_engine.rs`.
    pub async fn detect_entanglement(
        &self,
        subject_qid: &str,
        candidate_qids: &[String],
        neighbor_limit: usize,
        min_common_neighbors: usize,
        lang: &str,
    ) -> Vec<EntanglementHypothesis> {
        let subject_neighbors = self.query_generic_neighbors(subject_qid, neighbor_limit).await;
        if subject_neighbors.is_empty() {
            return Vec::new();
        }
        let mut hypotheses = Vec::new();
        for object_qid in candidate_qids {
            if object_qid == subject_qid {
                continue;
            }
            match self.has_direct_relation(subject_qid, object_qid).await {
                Some(false) => {}
                Some(true) | None => continue, // deja lie, ou incertitude reseau -- rien a proposer
            }
            let object_neighbors = self.query_generic_neighbors(object_qid, neighbor_limit).await;
            if object_neighbors.is_empty() {
                continue;
            }
            let common = subject_neighbors.intersection(&object_neighbors).count();
            if common < min_common_neighbors {
                continue;
            }
            let overlap = overlap_coefficient(&subject_neighbors, &object_neighbors);
            let subject_label = self.resolve_label(subject_qid, lang).await;
            let object_label = self.resolve_label(object_qid, lang).await;
            hypotheses.push(EntanglementHypothesis::new(
                subject_qid, object_qid.as_str(), subject_label, object_label, common, overlap,
            ));
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        hypotheses
    }

    /// Point d'entrée principal — couche 3a.
    pub async fn verify_relation(&self, subject: &str, predicate: &str, object: &str,
        lang: &str, max_hops: usize, max_expansions: usize) -> VerificationResult
    {
        let (subject_qid, subject_info) = self.search_entity(subject, lang, 3).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (object_qid, object_info) = self.search_entity(object, lang, 3).await;

        let (subject_qid, object_qid) = match (subject_qid, object_qid) {
            (Some(s), Some(o)) => (s, o),
            _ => return VerificationResult {
                verified: "unchallenged_unproven".into(),
                source_url: None,
                reason: format!(
                    "subject_or_object_not_resolved (subject: {subject_info}, object: {object_info})"
                ),
                subject_qid: None, object_qid: None, check_method: None,
                chain: None, property_id: None,
            },
        };

        tokio::time::sleep(Duration::from_millis(300)).await;
        let property_id = predicate_to_property(predicate);

        let (found, error, check_method, chain) = if let Some(pid) = property_id {
            let (f, e) = self.query_specific_property(&subject_qid, &object_qid, pid).await;
            let mut method = format!("targeted_property_{pid}");
            let mut chain = None;
            let mut found = f;

            if found == Some(false) && is_chainable(pid) {
                let (chain_hops, exhausted) = self.find_property_chain(
                    &subject_qid, &object_qid, pid, max_hops, max_expansions
                ).await;
                if let Some(hops) = chain_hops {
                    method = format!("transitive_chain_{pid}_{}_hops", hops.len());
                    chain = Some(hops);
                    found = Some(true);
                } else if !exhausted {
                    method = format!("transitive_chain_{pid}_incomplete_expansion_budget_exceeded");
                }
            }
            (found, e, method, chain)
        } else {
            let (f, e) = self.query_any_relation_exists(&subject_qid, &object_qid).await;
            (f, e, "generic_fallback_any_property_less_reliable".to_string(), None)
        };

        match found {
            None => VerificationResult {
                verified: "unchallenged_unproven".into(),
                source_url: None,
                reason: format!("sparql_query_failed: {}", error.unwrap_or_default()),
                subject_qid: Some(subject_qid), object_qid: Some(object_qid),
                check_method: Some(check_method), chain: None,
                property_id: property_id.map(String::from),
            },
            Some(b) => self.build_result(b, &subject_qid, &object_qid, &check_method, chain, property_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predicate_mapping_matches_python_table() {
        assert_eq!(predicate_to_property("born_in"), Some("P19"));
        assert_eq!(predicate_to_property("located_in"), Some("P131"));
        assert_eq!(predicate_to_property("capital_of"), Some("P36"));
        assert_eq!(predicate_to_property("unknown_predicate_xyz"), None);
    }

    #[test]
    fn test_chainable_properties() {
        assert!(is_chainable("P131"));
        assert!(is_chainable("P361"));
        assert!(!is_chainable("P19"));
    }

    /// Garde-fou anti-desync (audit multi-angle 2026-09-03, finding majeure):
    /// verifie mecaniquement que TOUT predicat CSTL liste dans
    /// `execution_lab::CHAINABLE_PREDICATES` se mappe, via `predicate_to_property`,
    /// vers un ID Wikidata que `is_chainable` accepte. Si quelqu'un ajoute un
    /// predicat a l'un sans penser a l'autre, ce test casse immediatement --
    /// au lieu d'une desynchronisation silencieuse entre Couche 3a (kb_verify)
    /// et ExecutionLab.
    #[test]
    fn test_chainable_predicates_stay_in_sync_with_execution_lab() {
        for &predicate in crate::execution_lab::CHAINABLE_PREDICATES {
            let property = predicate_to_property(predicate).unwrap_or_else(|| {
                panic!(
                    "predicat '{}' present dans execution_lab::CHAINABLE_PREDICATES \
                     mais absent de la table predicate_to_property de kb_verify.rs",
                    predicate
                )
            });
            assert!(
                is_chainable(property),
                "predicat '{}' (-> {}) est dans execution_lab::CHAINABLE_PREDICATES \
                 mais is_chainable('{}') retourne false -- desync",
                predicate,
                property,
                property
            );
        }
    }
}
