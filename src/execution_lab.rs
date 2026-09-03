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
const FUNCTIONAL_PREDICATES: &[&str] = &["born_in", "died_in", "spouse", "capital_of"];

/// Prédicats transitifs pour lesquels un cycle est une incohérence structurelle
/// (une hiérarchie d'imbrication ne peut pas revenir sur elle-même).
const CHAINABLE_PREDICATES: &[&str] = &["part_of", "located_in"];

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

fn dfs_find_cycle<'a>(start: &'a str, adjacency: &HashMap<&'a str, Vec<&'a str>>) -> Option<Vec<&'a str>> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut path: Vec<&str> = vec![start];
    let mut current = start;

    loop {
        visited.insert(current);
        let next = adjacency.get(current)?.first().copied()?;
        if next == start {
            path.push(next);
            return Some(path);
        }
        if visited.contains(next) {
            return None; // boucle qui ne revient pas sur `start` — pas le cycle qu'on cherche ici
        }
        path.push(next);
        current = next;
    }
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
    fn test_valid_chain_is_not_a_cycle() {
        let relations = vec![
            rel("Paris", "part_of", "France"),
            rel("France", "part_of", "Europe"),
        ];
        let report = check_consistency(&relations);
        assert!(report.consistent);
        assert_eq!(report.sigma_adjustment(), 0.75);
    }
}
