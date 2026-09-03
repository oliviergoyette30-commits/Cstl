//! src/execution_lab.rs — Couche 3b (partielle) de l'architecture CSTL.
//! `ExecutionLab`: vérification de cohérence interne, computationnellement checkable,
//! SANS jugement de vérité empirique (ça, c'est le rôle de kb_verify / Wikidata).
//!
//! Portée honnête de cette version: cohérence DANS un seul payload reçu (les relations
//! qui arrivent ensemble dans une même requête TCP). Deux checks réels:
//!   1. Contradiction directe: un prédicat "fonctionnel" (au plus un objet valide par
//!      sujet — né quelque part une fois, capitale d'un seul pays, etc.) apparaît deux
//!      fois pour le même sujet avec deux objets différents.
//!   2. Cycle: une chaîne de prédicats transitifs (located_in / part_of) qui revient
//!      sur son point de départ (A part_of B, B part_of A).
//! PAS encore fait (honnête, pas caché): `RestrictedCouncil` (quorum humain 2/3),
//! détection de cycle temporel, consistance croisée avec l'historique complet de
//! l'ADN store (seulement la portée du payload courant pour l'instant).

use std::collections::{HashMap, HashSet};

/// Prédicats à valeur unique par sujet: en voir deux avec des objets différents
/// pour le même sujet, dans le même payload, est une contradiction directe.
pub const FUNCTIONAL_PREDICATES: &[&str] = &["born_in", "died_in", "spouse", "capital_of"];

/// Prédicats transitifs pour lesquels un cycle est une incohérence structurelle
/// (une hiérarchie d'imbrication ne peut pas revenir sur elle-même).
pub const CHAINABLE_PREDICATES: &[&str] = &["part_of", "located_in"];

#[derive(Debug, Clone)]
pub struct Contradiction {
    pub subject: String,
    pub predicate: String,
    pub object_a: String,
    pub object_b: String,
}

#[derive(Debug, Clone)]
pub struct Cycle {
    pub predicate: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConsistencyReport {
    pub consistent: bool,
    pub contradictions: Vec<Contradiction>,
    pub cycles: Vec<Cycle>,
}

impl ConsistencyReport {
    /// sigma appliqué selon le diagramme du README: 0.75 si validé cohérent,
    /// ~0.09 si contradiction/cycle détecté (jamais supprimé, juste dégradé).
    pub fn sigma_adjustment(&self) -> f64 {
        if self.consistent { 0.75 } else { 0.09 }
    }
}

fn find_contradictions(relations: &[HashMap<String, String>]) -> Vec<Contradiction> {
    // (subject, predicate) -> premier objet vu
    let mut seen: HashMap<(String, String), String> = HashMap::new();
    let mut out = Vec::new();

    for rel in relations {
        let (Some(subject), Some(predicate), Some(object)) =
            (rel.get("subject"), rel.get("type"), rel.get("object"))
        else { continue };

        if !FUNCTIONAL_PREDICATES.contains(&predicate.as_str()) {
            continue;
        }
        let key = (subject.clone(), predicate.clone());
        match seen.get(&key) {
            Some(prev_object) if prev_object != object => {
                out.push(Contradiction {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    object_a: prev_object.clone(),
                    object_b: object.clone(),
                });
            }
            Some(_) => {} // même objet répété — pas une contradiction
            None => {
                seen.insert(key, object.clone());
            }
        }
    }
    out
}

fn find_cycles(relations: &[HashMap<String, String>]) -> Vec<Cycle> {
    let mut by_predicate: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for rel in relations {
        let (Some(subject), Some(predicate), Some(object)) =
            (rel.get("subject"), rel.get("type"), rel.get("object"))
        else { continue };
        if CHAINABLE_PREDICATES.contains(&predicate.as_str()) {
            by_predicate.entry(predicate.clone()).or_default().push((subject.clone(), object.clone()));
        }
    }

    let mut cycles = Vec::new();
    for (predicate, edges) in &by_predicate {
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for (s, o) in edges {
            adjacency.entry(s.as_str()).or_default().push(o.as_str());
        }
        // Un cycle A->B->A est trouve une fois en partant de A ET une fois en
        // partant de B (meme cycle, deux points de depart). On evite le doublon
        // en retirant du pool de departs tout noeud deja couvert par un cycle trouve.
        let mut already_in_a_cycle: HashSet<&str> = HashSet::new();
        for (&start, _) in adjacency.iter() {
            if already_in_a_cycle.contains(start) {
                continue;
            }
            if let Some(path) = dfs_find_cycle(start, &adjacency) {
                for &node in &path {
                    already_in_a_cycle.insert(node);
                }
                cycles.push(Cycle {
                    predicate: predicate.clone(),
                    path: path.into_iter().map(String::from).collect(),
                });
            }
        }
    }
    cycles
}

/// Corrige un bug critique trouve par l'audit multi-angle (2026-09-03): l'ancienne
/// version ne suivait que `adjacency[current].first()` -- des qu'un noeud a plus
/// d'un successeur via un predicat chainable (legal, ce ne sont pas des
/// FUNCTIONAL_PREDICATES), un vrai cycle passant par un AUTRE successeur que le
/// premier devenait invisible, sans backtracking pour essayer les autres. Cas
/// reproduit par l'audit: A->B, A->C, C->A (cycle A->C->A reel) rate completement
/// car le chemin part toujours vers B en premier et n'a plus de successeur.
///
/// Nouvelle version: vraie recherche en profondeur avec backtracking explicite --
/// on essaie CHAQUE successeur du noeud courant, pas seulement le premier, et on
/// revient en arriere (path.pop()/on_path.remove()) si une branche n'aboutit pas.
fn dfs_find_cycle<'a>(start: &'a str, adjacency: &HashMap<&'a str, Vec<&'a str>>) -> Option<Vec<&'a str>> {
    fn dfs<'a>(
        current: &'a str,
        start: &'a str,
        adjacency: &HashMap<&'a str, Vec<&'a str>>,
        path: &mut Vec<&'a str>,
        on_path: &mut HashSet<&'a str>,
    ) -> Option<Vec<&'a str>> {
        let neighbors = adjacency.get(current)?;
        for &next in neighbors {
            if next == start {
                path.push(next);
                return Some(path.clone());
            }
            if on_path.contains(next) {
                // Deja sur CE chemin (pas `start`) -- cette branche ne peut pas
                // fermer le cycle qu'on cherche depuis `start`. On essaie le
                // successeur suivant plutot que d'abandonner toute la recherche.
                continue;
            }
            path.push(next);
            on_path.insert(next);
            if let Some(found) = dfs(next, start, adjacency, path, on_path) {
                return Some(found);
            }
            path.pop();
            on_path.remove(next);
        }
        None
    }

    let mut path: Vec<&str> = vec![start];
    let mut on_path: HashSet<&str> = HashSet::new();
    on_path.insert(start);
    dfs(start, start, adjacency, &mut path, &mut on_path)
}

/// Union de FUNCTIONAL_PREDICATES et CHAINABLE_PREDICATES — les seuls
/// prédicats dont `check_consistency_with_history` se sert. Existe pour que
/// l'appelant (le handler, via `AdnStore::relations_for_predicates`) puisse
/// ne charger que ça depuis la DB, sans dupliquer la liste à la main et
/// risquer qu'elle diverge des constantes ci-dessus si l'une des deux change.
pub fn relevant_predicates() -> Vec<&'static str> {
    FUNCTIONAL_PREDICATES
        .iter()
        .chain(CHAINABLE_PREDICATES.iter())
        .copied()
        .collect()
}

/// Point d'entrée principal — vérifie la cohérence interne de toutes les
/// relations d'un même payload.
pub fn check_consistency(relations: &[HashMap<String, String>]) -> ConsistencyReport {
    let contradictions = find_contradictions(relations);
    let cycles = find_cycles(relations);
    ConsistencyReport {
        consistent: contradictions.is_empty() && cycles.is_empty(),
        contradictions,
        cycles,
    }
}

fn edges_set(relations: &[HashMap<String, String>], predicates: &[&str]) -> HashSet<(String, String, String)> {
    // (predicate, subject, object)
    let mut set = HashSet::new();
    for rel in relations {
        if let (Some(subject), Some(predicate), Some(object)) =
            (rel.get("subject"), rel.get("type"), rel.get("object"))
        {
            if predicates.contains(&predicate.as_str()) {
                set.insert((predicate.clone(), subject.clone(), object.clone()));
            }
        }
    }
    set
}

/// Vérifie la cohérence d'un payload NOUVEAU contre tout l'historique de
/// l'ADN store, pas seulement les relations reçues dans la même requête.
/// Portée honnête et ce qui change par rapport à `check_consistency`:
///
///   - Contradictions: la valeur "établie" pour un (sujet, prédicat)
///     fonctionnel vient d'abord de l'historique, puis est mise à jour par les
///     relations du nouveau payload dans l'ordre. Une contradiction n'est
///     rapportée QUE si une relation du NOUVEAU payload contredit une valeur
///     déjà établie (par l'historique ou plus tôt dans ce même payload) —
///     deux entrées de l'historique qui se contredisaient déjà entre elles
///     ne sont jamais re-signalées ici: si c'était une vraie contradiction,
///     elle a déjà été rapportée au moment où sa deuxième moitié est arrivée.
///     Répéter ce signalement à chaque requête future non liée serait du
///     bruit, pas de l'information nouvelle.
///   - Cycles: le graphe complet (historique + nouveau) est utilisé pour la
///     détection — un cycle est une propriété structurelle de tout le graphe,
///     pas seulement de l'arête qui le referme — mais un cycle n'est
///     rapporté QUE s'il contient au moins une arête introduite par CE
///     payload. Un cycle qui existait déjà entièrement dans l'historique
///     aurait déjà été rapporté quand son arête de fermeture est arrivée.
pub fn check_consistency_with_history(
    new_relations: &[HashMap<String, String>],
    history_relations: &[HashMap<String, String>],
) -> ConsistencyReport {
    let mut established: HashMap<(String, String), String> = HashMap::new();
    for rel in history_relations {
        if let (Some(subject), Some(predicate), Some(object)) =
            (rel.get("subject"), rel.get("type"), rel.get("object"))
        {
            if FUNCTIONAL_PREDICATES.contains(&predicate.as_str()) {
                established.insert((subject.clone(), predicate.clone()), object.clone());
            }
        }
    }

    let mut contradictions = Vec::new();
    for rel in new_relations {
        let (Some(subject), Some(predicate), Some(object)) =
            (rel.get("subject"), rel.get("type"), rel.get("object"))
        else { continue };
        if !FUNCTIONAL_PREDICATES.contains(&predicate.as_str()) {
            continue;
        }
        let key = (subject.clone(), predicate.clone());
        match established.get(&key) {
            Some(prev_object) if prev_object != object => {
                contradictions.push(Contradiction {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    object_a: prev_object.clone(),
                    object_b: object.clone(),
                });
            }
            Some(_) => {}
            None => {
                established.insert(key, object.clone());
            }
        }
    }

    let new_edges = edges_set(new_relations, CHAINABLE_PREDICATES);
    let mut combined: Vec<HashMap<String, String>> = history_relations.to_vec();
    combined.extend(new_relations.iter().cloned());
    let cycles: Vec<Cycle> = find_cycles(&combined)
        .into_iter()
        .filter(|cycle| {
            cycle
                .path
                .windows(2)
                .any(|edge| new_edges.contains(&(cycle.predicate.clone(), edge[0].clone(), edge[1].clone())))
        })
        .collect();

    ConsistencyReport {
        consistent: contradictions.is_empty() && cycles.is_empty(),
        contradictions,
        cycles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(subject: &str, predicate: &str, object: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("subject".to_string(), subject.to_string());
        m.insert("type".to_string(), predicate.to_string());
        m.insert("object".to_string(), object.to_string());
        m
    }

    #[test]
    fn test_relevant_predicates_contains_all_six_and_nothing_else() {
        let preds = relevant_predicates();
        assert_eq!(preds.len(), 6);
        for p in ["born_in", "died_in", "spouse", "capital_of", "part_of", "located_in"] {
            assert!(preds.contains(&p), "predicat manquant: {p}");
        }
    }

    #[test]
    fn test_no_relations_is_consistent() {
        let report = check_consistency(&[]);
        assert!(report.consistent);
    }

    #[test]
    fn test_detects_functional_predicate_contradiction() {
        let relations = vec![
            rel("Marie Curie", "born_in", "Warsaw"),
            rel("Marie Curie", "born_in", "Paris"),
        ];
        let report = check_consistency(&relations);
        assert!(!report.consistent);
        assert_eq!(report.contradictions.len(), 1);
        assert_eq!(report.sigma_adjustment(), 0.09);
    }

    #[test]
    fn test_same_object_repeated_is_not_a_contradiction() {
        let relations = vec![
            rel("Marie Curie", "born_in", "Warsaw"),
            rel("Marie Curie", "born_in", "Warsaw"),
        ];
        let report = check_consistency(&relations);
        assert!(report.consistent);
    }

    #[test]
    fn test_detects_two_node_cycle() {
        let relations = vec![
            rel("A", "part_of", "B"),
            rel("B", "part_of", "A"),
        ];
        let report = check_consistency(&relations);
        assert!(!report.consistent);
        assert_eq!(report.cycles.len(), 1);
    }



    #[test]
    fn test_detects_cycle_missed_by_first_successor_only_traversal() {
        // Cas exact trouve par l'audit multi-angle (2026-09-03): A a DEUX
        // successeurs (B et C). L'ancienne version suivait toujours B en
        // premier (ordre d'insertion), B n'a pas de successeur -> impasse ->
        // le cycle reel A->C->A n'etait jamais essaye. Avec backtracking, la
        // branche B echoue mais C est ensuite essaye et ferme le cycle.
        let relations = vec![
            rel("A", "part_of", "B"),
            rel("A", "part_of", "C"),
            rel("C", "part_of", "A"),
        ];
        let report = check_consistency(&relations);
        assert!(!report.consistent, "le cycle A->C->A doit etre detecte malgre le successeur B qui n'aboutit pas");
        assert_eq!(report.cycles.len(), 1);
        assert_eq!(report.sigma_adjustment(), 0.09);
    }

    #[test]
    fn test_no_false_positive_when_multiple_successors_and_no_real_cycle() {
        // Meme forme (un noeud a deux successeurs) mais SANS cycle reel --
        // verifie que le backtracking ne cree pas de faux positifs.
        let relations = vec![
            rel("A", "part_of", "B"),
            rel("A", "part_of", "C"),
            rel("C", "part_of", "D"),
        ];
        let report = check_consistency(&relations);
        assert!(report.consistent);
        assert!(report.cycles.is_empty());
    }

    #[test]
    fn test_history_contradicts_new_relation() {
        let history = vec![rel("Marie Curie", "born_in", "Warsaw")];
        let new_relations = vec![rel("Marie Curie", "born_in", "Paris")];
        let report = check_consistency_with_history(&new_relations, &history);
        assert!(!report.consistent);
        assert_eq!(report.contradictions.len(), 1);
        assert_eq!(report.contradictions[0].object_a, "Warsaw");
        assert_eq!(report.contradictions[0].object_b, "Paris");
    }

    #[test]
    fn test_history_contradicting_itself_is_not_reported_again() {
        // Deux entrees d'historique deja contradictoires entre elles (auraient
        // ete rapportees quand la deuxieme est arrivee) ne doivent pas
        // resurgir juste parce qu'une requete sans rapport arrive ensuite.
        let history = vec![
            rel("Marie Curie", "born_in", "Warsaw"),
            rel("Marie Curie", "born_in", "Paris"),
        ];
        let new_relations = vec![rel("Albert Einstein", "born_in", "Ulm")];
        let report = check_consistency_with_history(&new_relations, &history);
        assert!(report.consistent);
        assert_eq!(report.contradictions.len(), 0);
    }

    #[test]
    fn test_new_relation_matching_history_is_consistent() {
        let history = vec![rel("Marie Curie", "born_in", "Warsaw")];
        let new_relations = vec![rel("Marie Curie", "born_in", "Warsaw")];
        let report = check_consistency_with_history(&new_relations, &history);
        assert!(report.consistent);
    }

    #[test]
    fn test_history_closes_cycle_with_new_edge() {
        let history = vec![rel("A", "part_of", "B")];
        let new_relations = vec![rel("B", "part_of", "A")];
        let report = check_consistency_with_history(&new_relations, &history);
        assert!(!report.consistent);
        assert_eq!(report.cycles.len(), 1);
    }

    #[test]
    fn test_preexisting_history_cycle_not_reported_again() {
        let history = vec![rel("A", "part_of", "B"), rel("B", "part_of", "A")];
        let new_relations = vec![rel("X", "part_of", "Y")];
        let report = check_consistency_with_history(&new_relations, &history);
        assert!(report.consistent);
        assert_eq!(report.cycles.len(), 0);
    }
}
