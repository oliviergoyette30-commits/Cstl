//! src/execution_lab.rs — Couche 3b de l'architecture CSTL.
//! `ExecutionLab`: vérification de cohérence interne, computationnellement checkable,
//! SANS jugement de vérité empirique (ça, c'est le rôle de kb_verify / Wikidata).
//!
//! Deux checks réels, appliqués via `check_consistency_with_history` (le seul chemin
//! réellement appelé par le serveur, voir handler.rs) contre le payload NOUVEAU plus
//! tout l'historique persisté dans l'ADN store (pas seulement les relations reçues
//! dans la même requête TCP — mise à jour 2026-09-04, voir sa propre doc ci-dessous
//! pour la portée exacte) :
//!   1. Contradiction directe: un prédicat "fonctionnel" (au plus un objet valide par
//!      sujet — né quelque part une fois, capitale d'un seul pays, etc.) apparaît deux
//!      fois pour le même sujet avec deux objets différents.
//!   2. Cycle: une chaîne de prédicats transitifs (located_in / part_of) qui revient
//!      sur son point de départ (A part_of B, B part_of A).
//!
//! PAS encore fait (honnête, pas caché): détection de cycle temporel. Le quorum humain
//! (`RestrictedCouncil`, portée réduite v1 — un seul membre par défaut) est implémenté
//! ailleurs, voir src/restricted_council.rs, pas dans ce module.

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
/// Seule fonction publique de ce genre (2026-09-04): l'ancienne
/// `check_consistency(relations)`, qui ne regardait que le payload courant
/// sans historique, a été retirée -- elle n'avait plus aucun appelant réel
/// (le chemin serveur appelle uniquement `check_consistency_with_history`
/// depuis l'ajout de la vérification croisée avec l'ADN store). Un appel
/// équivalent à l'ancien comportement reste possible en passant `&[]` comme
/// `history_relations`.
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

/// Une violation de l'Axiome D SDL (¬(MUST p ∧ MUST_NOT p)) detectee entre
/// l'historique persiste et un payload NOUVEAU.
#[derive(Debug, Clone)]
pub struct DeonticViolation {
    pub subject: String,
    pub object: String,
    pub required_by: String, // le nom exact de la modalite obligatoire (MUST/REQUIRE)
    pub forbidden_by: String, // le nom exact de la modalite interdite (MUST_NOT/FORBID)
}

#[derive(Debug, Clone, Default)]
pub struct DeonticAuditReport {
    pub consistent: bool,
    pub violations: Vec<DeonticViolation>,
}

/// Couche 8 (2026-09-04) — audit deontique HISTORIQUE: verifie les
/// relations portant une `modality` (MUST/MUST_NOT/REQUIRE/FORBID) d'un
/// payload NOUVEAU contre TOUT ce qui a ete persiste par des payloads
/// PRECEDENTS (potentiellement d'agents differents, a des moments
/// differents) -- pas seulement a l'interieur d'un seul payload (deja
/// couvert, et BLOQUANT, par `server/validator.rs::validate_deontic_
/// constraints` via `semantic.rs::check_axiom_d`, appele en STEP 2 avant
/// meme d'atteindre cette fonction).
///
/// Meme patron que `check_consistency_with_history` ci-dessus, pas
/// invente ici: une "position etablie" par (subject, object) est
/// construite depuis l'historique, mise a jour au fil du nouveau payload,
/// et une violation n'est rapportee QUE si une relation du NOUVEAU payload
/// contredit une position deja etablie -- deux entrees de l'historique qui
/// se contredisaient deja entre elles ne sont jamais re-signalees (elles
/// l'auraient ete au moment ou leur deuxieme moitie est arrivee).
///
/// Design assume, pas un oubli: contrairement au check intra-payload
/// (bloquant, E107 -- une auto-contradiction dans UN MEME payload est
/// presque toujours une erreur d'auteur), une contradiction a travers
/// l'HISTOIRE reste INFORMATIVE ici (jamais de rejet), exactement comme
/// `check_consistency_with_history` pour les faits: des agents differents,
/// ou le meme agent a des moments differents, peuvent legitimement changer
/// de position ou etre en desaccord -- ce n'est pas force une erreur de
/// protocole, contrairement a se contredire dans le meme souffle.
pub fn check_deontic_consistency_with_history(
    new_relations: &[HashMap<String, String>],
    history_relations: &[HashMap<String, String>],
) -> DeonticAuditReport {
    use crate::semantic::{FORBIDDEN_MODALITIES, REQUIRED_MODALITIES};

    // (subject, object) -> modalite (le premier des deux camps rencontre,
    // obligatoire OU interdit) etablie par l'historique, puis par ce
    // payload au fur et a mesure.
    let mut established: HashMap<(String, String), (bool, String)> = HashMap::new(); // bool: is_required

    let classify = |m: &str| -> Option<bool> {
        if REQUIRED_MODALITIES.contains(&m) { Some(true) }
        else if FORBIDDEN_MODALITIES.contains(&m) { Some(false) }
        else { None }
    };

    for rel in history_relations {
        let (Some(subject), Some(object), Some(modality)) =
            (rel.get("subject"), rel.get("object"), rel.get("modality"))
        else { continue };
        let Some(is_required) = classify(modality) else { continue };
        established.entry((subject.clone(), object.clone())).or_insert((is_required, modality.clone()));
    }

    let mut violations = Vec::new();
    for rel in new_relations {
        let (Some(subject), Some(object), Some(modality)) =
            (rel.get("subject"), rel.get("object"), rel.get("modality"))
        else { continue };
        let Some(is_required) = classify(modality) else { continue };
        let key = (subject.clone(), object.clone());
        match established.get(&key) {
            Some((prev_is_required, prev_modality)) if *prev_is_required != is_required => {
                let (required_by, forbidden_by) = if is_required {
                    (modality.clone(), prev_modality.clone())
                } else {
                    (prev_modality.clone(), modality.clone())
                };
                violations.push(DeonticViolation {
                    subject: subject.clone(),
                    object: object.clone(),
                    required_by,
                    forbidden_by,
                });
            }
            Some(_) => {}
            None => {
                established.insert(key, (is_required, modality.clone()));
            }
        }
    }

    DeonticAuditReport { consistent: violations.is_empty(), violations }
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
        let report = check_consistency_with_history(&[], &[]);
        assert!(report.consistent);
    }

    #[test]
    fn test_detects_functional_predicate_contradiction() {
        let relations = vec![
            rel("Marie Curie", "born_in", "Warsaw"),
            rel("Marie Curie", "born_in", "Paris"),
        ];
        let report = check_consistency_with_history(&relations, &[]);
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
        let report = check_consistency_with_history(&relations, &[]);
        assert!(report.consistent);
    }

    #[test]
    fn test_detects_two_node_cycle() {
        let relations = vec![
            rel("A", "part_of", "B"),
            rel("B", "part_of", "A"),
        ];
        let report = check_consistency_with_history(&relations, &[]);
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
        let report = check_consistency_with_history(&relations, &[]);
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
        let report = check_consistency_with_history(&relations, &[]);
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

    // ── check_deontic_consistency_with_history (Couche 8, 2026-09-04) ──

    fn deontic_rel(subject: &str, object: &str, modality: &str) -> HashMap<String, String> {
        let mut m = rel(subject, "PERFORM", object);
        m.insert("modality".to_string(), modality.to_string());
        m
    }

    #[test]
    fn test_no_history_no_violation() {
        let new_relations = vec![deontic_rel("agent_x", "backup_db", "MUST")];
        let report = check_deontic_consistency_with_history(&new_relations, &[]);
        assert!(report.consistent);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_new_must_contradicts_established_history_must_not() {
        // Un payload PASSE a etabli MUST_NOT(agent_x, delete_prod_db). Un
        // NOUVEAU payload (potentiellement d'un autre agent, ou du meme
        // agent plus tard) declare MUST sur la MEME (subject, object) --
        // c'est exactement le cas que la verification intra-payload
        // (Axiome D, bloquante) ne peut PAS voir, puisque les deux moities
        // n'ont jamais partage un seul payload.
        let history = vec![deontic_rel("agent_x", "delete_prod_db", "MUST_NOT")];
        let new_relations = vec![deontic_rel("agent_x", "delete_prod_db", "MUST")];
        let report = check_deontic_consistency_with_history(&new_relations, &history);
        assert!(!report.consistent);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].subject, "agent_x");
        assert_eq!(report.violations[0].object, "delete_prod_db");
        assert_eq!(report.violations[0].required_by, "MUST");
        assert_eq!(report.violations[0].forbidden_by, "MUST_NOT");
    }

    #[test]
    fn test_same_modality_repeated_is_not_a_violation() {
        // Deux payloads (l'historique et le nouveau) qui declarent la MEME
        // modalite sur la meme (subject, object) -- une repetition n'est
        // pas une contradiction.
        let history = vec![deontic_rel("agent_x", "backup_db", "MUST")];
        let new_relations = vec![deontic_rel("agent_x", "backup_db", "MUST")];
        let report = check_deontic_consistency_with_history(&new_relations, &history);
        assert!(report.consistent);
    }

    #[test]
    fn test_different_objects_no_violation() {
        let history = vec![deontic_rel("agent_x", "backup_db", "MUST")];
        let new_relations = vec![deontic_rel("agent_x", "delete_prod_db", "MUST_NOT")];
        let report = check_deontic_consistency_with_history(&new_relations, &history);
        assert!(report.consistent);
    }

    #[test]
    fn test_preexisting_history_contradiction_not_reported_again() {
        // Meme logique de dedup que check_consistency_with_history: si
        // l'HISTOIRE contenait deja une contradiction (les deux moities
        // sont deja persistees), un nouveau payload SANS RAPPORT ne doit
        // pas la re-signaler a chaque fois -- ce serait du bruit repete,
        // pas de l'information nouvelle (elle a deja ete rapportee quand
        // sa 2e moitie est arrivee, a l'epoque).
        let history = vec![
            deontic_rel("agent_x", "delete_prod_db", "MUST"),
            deontic_rel("agent_x", "delete_prod_db", "MUST_NOT"),
        ];
        let new_relations = vec![deontic_rel("agent_y", "other_action", "MUST")];
        let report = check_deontic_consistency_with_history(&new_relations, &history);
        assert!(report.consistent, "une contradiction deja presente dans l'historique ne doit pas etre re-signalee: {:?}", report.violations);
    }

    #[test]
    fn test_forbid_and_require_synonyms_also_detected() {
        // FORBID/REQUIRE sont les synonymes de MUST_NOT/MUST (memes listes
        // que semantic.rs::check_axiom_d, reutilisees ici -- pas dupliquees).
        let history = vec![deontic_rel("agent_x", "delete_prod_db", "FORBID")];
        let new_relations = vec![deontic_rel("agent_x", "delete_prod_db", "REQUIRE")];
        let report = check_deontic_consistency_with_history(&new_relations, &history);
        assert!(!report.consistent);
    }
}
