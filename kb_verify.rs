//! src/kb_verify.rs — Couche 3a de l'architecture CSTL (port Rust fidèle)
//! Vérification d'une relation CSTL (subject, predicate, object) contre Wikidata.
//! Port de orchestrator/cstl_verify_public_kb.py — même logique, même mapping,
//! mêmes bornes BFS, mêmes corrections empiriques (2026-08-06 / 08-10 / 08-12).

use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use serde::Serialize;
use serde_json::Value;

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
fn is_chainable(property_id: &str) -> bool {
    matches!(property_id, "P131" | "P361")
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
        Self { client }
    }

    async fn is_disambiguation_page(&self, qid: &str) -> bool {
        let query = format!("ASK {{ wd:{qid} wdt:P31 wd:{DISAMBIGUATION_PAGE_QID} . }}");
        let resp = self.client.get(WIKIDATA_SPARQL_ENDPOINT)
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
            let resp = self.client.get(WIKIDATA_SEARCH_API)
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
        let resp = self.client.get(WIKIDATA_SPARQL_ENDPOINT)
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
        let resp = self.client.get(WIKIDATA_SPARQL_ENDPOINT)
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
        let resp = self.client.get(WIKIDATA_SPARQL_ENDPOINT)
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
}
