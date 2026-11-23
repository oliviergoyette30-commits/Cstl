//! Completude des ARBRES (2026-11-23).
//!
//! Un arbre (donnees imbriquees, `agent.send_tree` du SDK) arrive en
//! relations marquees `tree=<id>` : une par element, `object` = l'element
//! parent (`-` a la racine). Chaque element qui a des enfants porte
//! `tree_n=<nombre d'enfants directs>`, et la relation `tree_root` porte
//! `tree_n=<nombre d'elements a la racine>`.
//!
//! Le serveur verifie ces comptes lui-meme, a la reception : un arbre
//! auquel il manque une branche (element saute par un modele, client qui ne
//! passe pas par le SDK, message tronque) est refuse en entier, avant
//! d'etre stocke. Les comptes sont ensuite signes et audites avec le reste :
//! un lecteur peut prouver plus tard que l'arbre relu est complet.
//!
//! Limite : les comptes sont ceux de l'AUTEUR. Ils prouvent que l'arbre est
//! complet par rapport a ce que l'auteur a annonce, pas que l'auteur dit
//! vrai ; un auteur malveillant peut annoncer les comptes de l'arbre qu'il
//! envoie. Ce que l'auteur signe, lui, ne peut plus etre change.

use std::collections::HashMap;

/// Premier defaut de completude des arbres de ces relations, ou None.
pub fn violation(relations: &[HashMap<String, String>]) -> Option<String> {
    let mut trees: HashMap<&str, Vec<&HashMap<String, String>>> = HashMap::new();
    for r in relations {
        if let Some(t) = r.get("tree") {
            trees.entry(t.as_str()).or_default().push(r);
        }
    }
    let mut ids: Vec<&&str> = trees.keys().collect();
    ids.sort();
    for id in ids {
        let rels = &trees[*id];
        let is_root = |r: &HashMap<String, String>| r.get("type").map(String::as_str) == Some("tree_root");
        let mut subjects: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut kids: HashMap<&str, usize> = HashMap::new();
        for r in rels.iter().filter(|r| !is_root(r)) {
            let subject = r.get("subject").map(String::as_str).unwrap_or("");
            if !subjects.insert(subject) {
                return Some(format!("arbre {id} : element {subject} en double"));
            }
            *kids.entry(r.get("object").map(String::as_str).unwrap_or("")).or_default() += 1;
        }
        for r in rels.iter().filter(|r| !is_root(r)) {
            let object = r.get("object").map(String::as_str).unwrap_or("");
            if object != "-" && !subjects.contains(object) {
                let subject = r.get("subject").map(String::as_str).unwrap_or("?");
                return Some(format!("arbre {id} : parent {object} absent pour {subject}"));
            }
        }
        for r in rels.iter() {
            let Some(n) = r.get("tree_n") else { continue };
            let Ok(want) = n.parse::<usize>() else {
                return Some(format!("arbre {id} : tree_n illisible : {n}"));
            };
            let node = if is_root(r) { "-" } else { r.get("subject").map(String::as_str).unwrap_or("") };
            let got = kids.get(node).copied().unwrap_or(0);
            if got != want {
                let who = if is_root(r) { "la racine".to_string() } else { node.to_string() };
                return Some(format!("arbre {id} : {who} annonce {want} enfant(s), {got} recu(s)"));
            }
        }
    }
    None
}

/// Empreinte canonique d'un arbre : ses relations (tree=<id>) dans l'ordre
/// d'envoi, chacune en `cle=valeur` triees par cle, separees par 0x1f ; les
/// relations separees par 0x1e ; SHA-256 en hexadecimal, prefixe `sha256:`.
/// Le SDK Python calcule la meme (`cstl_compact.tree_hash`).
pub fn canonical_hash(relations: &[HashMap<String, String>], tree: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let mut first = true;
    for r in relations.iter().filter(|r| r.get("tree").map(String::as_str) == Some(tree)) {
        if !first {
            h.update([0x1e]);
        }
        first = false;
        let mut keys: Vec<&String> = r.keys().filter(|k| k.as_str() != "block").collect();
        keys.sort();
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                h.update([0x1f]);
            }
            h.update(k.as_bytes());
            h.update(b"=");
            h.update(r[*k].as_bytes());
        }
    }
    format!("sha256:{}", hex::encode(h.finalize()))
}

/// Les arbres de ces relations qui se reclament d'une SOURCE
/// (`tree_source=<agent>` sur leur relation tree_root) : (id, source).
pub fn claimed_sources(relations: &[HashMap<String, String>]) -> Vec<(String, String)> {
    relations
        .iter()
        .filter(|r| r.get("type").map(String::as_str) == Some("tree_root"))
        .filter_map(|r| Some((r.get("tree")?.clone(), r.get("tree_source")?.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn tree() -> Vec<HashMap<String, String>> {
        vec![
            rel(&[("type", "tree_root"), ("subject", "tree_root"), ("object", "-"), ("tree", "t1"), ("tree_n", "2")]),
            rel(&[("type", "orders"), ("subject", "o1"), ("object", "-"), ("tree", "t1"), ("tree_n", "2")]),
            rel(&[("type", "lines"), ("subject", "o1.0"), ("object", "o1"), ("tree", "t1")]),
            rel(&[("type", "lines"), ("subject", "o1.1"), ("object", "o1"), ("tree", "t1")]),
            rel(&[("type", "orders"), ("subject", "o2"), ("object", "-"), ("tree", "t1")]),
        ]
    }

    #[test]
    fn canonical_hash_matches_the_python_sdk() {
        // Valeur calculee par cstl_compact.tree_hash(tree_to_relations(
        // {"orders": [{"id": "o1", "lines": [{"q": 1}, {"q": 2}]}, {"id": "o2"}]}, "t1"), "t1")
        // -- voir test_tree_hash_is_the_servers dans test_cstl_compact.py.
        let mut t = tree();
        t[2].insert("q".into(), "1".into());
        t[2].insert("tree_types".into(), "q:int".into());
        t[3].insert("q".into(), "2".into());
        t[3].insert("tree_types".into(), "q:int".into());
        t[0].insert("tree_kind".into(), "tables".into());
        t[1].insert("tree_id".into(), "1".into());
        t[4].insert("tree_id".into(), "1".into());
        assert_eq!(canonical_hash(&t, "t1"), PYTHON_HASH);
        // L'ordre des freres compte, une valeur changee aussi.
        t.swap(2, 3);
        assert_ne!(canonical_hash(&t, "t1"), PYTHON_HASH);
    }

    const PYTHON_HASH: &str = "sha256:f3a92ea780204ab228c0c413ad11595615f81a7708a918a248711b000a157db5";

    #[test]
    fn complete_tree_passes() {
        assert_eq!(violation(&tree()), None);
        // Sans tree=, rien a verifier.
        assert_eq!(violation(&[rel(&[("type", "STATE"), ("subject", "a"), ("object", "b")])]), None);
    }

    #[test]
    fn missing_child_or_root_element_is_caught() {
        let mut t = tree();
        t.remove(3);
        assert_eq!(violation(&t).unwrap(), "arbre t1 : o1 annonce 2 enfant(s), 1 recu(s)");
        let mut t = tree();
        t.remove(4);
        assert_eq!(violation(&t).unwrap(), "arbre t1 : la racine annonce 2 enfant(s), 1 recu(s)");
    }

    #[test]
    fn orphans_duplicates_and_bad_counts_are_caught() {
        let mut t = tree();
        t.remove(1); // o1 retire : ses lignes n'ont plus de parent
        assert!(violation(&t).unwrap().contains("parent o1 absent"));
        let mut t = tree();
        t.push(rel(&[("type", "orders"), ("subject", "o2"), ("object", "-"), ("tree", "t1")]));
        assert!(violation(&t).unwrap().contains("o2 en double"));
        let mut t = tree();
        t[1].insert("tree_n".into(), "deux".into());
        assert!(violation(&t).unwrap().contains("tree_n illisible"));
    }
}
