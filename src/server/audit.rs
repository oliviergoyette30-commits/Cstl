/// Audit Trail — canonical hashing + hash chain
///
/// Principe: le hash est calcule par l'ORCHESTRATEUR (ce serveur),
/// jamais par un LLM. Un LLM ne peut pas produire de SHA-256 reel.
/// PARENT_HASH=root ou unverified_no_hash_tool_available cote agent,
/// remplace ici par le vrai hash calcule.
///
/// Note honnete (audit multi-angle, 2026-09-03 -- decouverte en creusant
/// le fix NFC de src/canonical.rs): CE canonical_hash() ci-dessous, et non
/// canonical::canonical_hash(), est celui reellement utilise par le
/// serveur TCP en production (via HashChain::append, appele depuis
/// server/handler.rs pour chaque payload recu -- c'est la cle primaire de
/// adn_store). src/canonical.rs implemente un algorithme DIFFERENT
/// (canonicalisation de texte brut, documente comme normatif dans
/// CSTL_SPEC_v5_0.md section 15) mais n'est appele NULLE PART dans ce
/// depot -- corriger canonical.rs (fait plus tot dans cette session, ajout
/// de la normalisation NFC) n'avait donc AUCUN effet sur le hash reel
/// produit par le serveur qui tourne. Les deux fonctions partagent le nom
/// canonical_hash et l'intention (determinisme, immutabilite) mais
/// operent sur des donnees differentes (texte brut CSTL vs CstlPayload
/// deja parse) et ne sont PAS interchangeables -- corriger celle-ci ne
/// dispense pas de garder canonical.rs a jour si un jour un appelant
/// reel apparait pour elle.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;
use super::parser::CstlPayload;

/// Applique NFC avant de pousser une chaine dans le buffer canonique.
/// Corrige la meme trouvaille NFC que src/canonical.rs (ratifiee
/// tripartite depuis la Session #5, jamais appliquee ici): deux
/// representations Unicode canoniquement equivalentes d'une valeur de
/// champ (ex: "café" precompose vs decompose) produisaient auparavant
/// deux hash d'audit differents pour un payload semantiquement identique.
fn nfc(s: &str) -> String {
    s.nfc().collect()
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub hash: String,
    pub parent_hash: String,
    pub sender: String,
    pub receiver: String,
    pub purpose: String,
    pub seq: u64,
}

pub struct HashChain {
    pub entries: Vec<AuditEntry>,
}

/// Hash canonique deterministe: champs tries, aucune dependance a l'ordre d'arrivee.
pub fn canonical_hash(payload: &CstlPayload) -> String {
    let mut canon = String::new();

    canon.push_str("VERSION|");
    canon.push_str(&nfc(&payload.version));
    canon.push_str("\nMODE|");
    canon.push_str(&nfc(&payload.mode));

    // BTreeMap => tri lexicographique garanti. Trie sur les cles BRUTES
    // (non normalisees) -- deux cles NFC-equivalentes mais bytes-differentes
    // seraient de toute facon deux entrees HashMap distinctes en amont, la
    // normalisation ici ne change donc pas l'ORDRE de tri, seulement les
    // bytes hashes pour chaque valeur (et chaque cle, par coherence).
    let meta: BTreeMap<_, _> = payload.meta.iter().collect();
    canon.push_str("\nMETA");
    for (k, v) in meta {
        // PARENT_HASH exclu du hash: sinon dependance circulaire
        if k == "PARENT_HASH" {
            continue;
        }
        canon.push('|');
        canon.push_str(&nfc(k));
        canon.push('=');
        canon.push_str(&nfc(v));
    }

    let intent: BTreeMap<_, _> = payload.intent.iter().collect();
    canon.push_str("\nINTENT");
    for (k, v) in intent {
        canon.push('|');
        canon.push_str(&nfc(k));
        canon.push('=');
        canon.push_str(&nfc(v));
    }

    // Relations: chaque bloc trie en interne, puis les blocs tries entre eux
    let mut rel_strings: Vec<String> = payload
        .relations
        .iter()
        .map(|r| {
            let sorted: BTreeMap<_, _> = r.iter().collect();
            sorted
                .iter()
                .map(|(k, v)| format!("{}={}", nfc(k), nfc(v)))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    rel_strings.sort();

    canon.push_str("\nRELATIONS");
    for r in rel_strings {
        canon.push('|');
        canon.push_str(&r);
    }

    let mut hasher = Sha256::new();
    hasher.update(canon.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

impl HashChain {
    pub fn new() -> Self {
        HashChain { entries: Vec::new() }
    }

    /// Append-only. Retourne le hash reel calcule pour ce payload.
    pub fn append(&mut self, payload: &CstlPayload) -> AuditEntry {
        let parent_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| "root".to_string());

        let hash = canonical_hash(payload);

        let entry = AuditEntry {
            hash,
            parent_hash,
            sender: payload.intent.get("sender").cloned().unwrap_or_else(|| "unknown".into()),
            receiver: payload.intent.get("receiver").cloned().unwrap_or_else(|| "unknown".into()),
            purpose: payload.intent.get("purpose").cloned().unwrap_or_else(|| "unknown".into()),
            seq: self.entries.len() as u64,
        };

        eprintln!(
            "[Audit] seq={} hash={} parent={}",
            entry.seq,
            &entry.hash[..23.min(entry.hash.len())],
            if entry.parent_hash == "root" { "root".to_string() } else { entry.parent_hash[..23.min(entry.parent_hash.len())].to_string() }
        );

        self.entries.push(entry.clone());
        entry
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn last_hash(&self) -> String {
        self.entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| "root".to_string())
    }

    /// Verifie que chaque maillon pointe bien sur le precedent.
    pub fn verify_integrity(&self) -> Result<(), String> {
        let mut expected_parent = "root".to_string();
        for e in &self.entries {
            if e.parent_hash != expected_parent {
                return Err(format!(
                    "Chain broken at seq={}: parent={} expected={}",
                    e.seq, e.parent_hash, expected_parent
                ));
            }
            expected_parent = e.hash.clone();
        }
        Ok(())
    }
}

impl Default for HashChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mk(sender: &str, purpose: &str) -> CstlPayload {
        let mut meta = HashMap::new();
        meta.insert("encoder".to_string(), "Agent_CLAUDE".to_string());
        meta.insert("produced_by".to_string(), "Claude".to_string());

        let mut intent = HashMap::new();
        intent.insert("purpose".to_string(), purpose.to_string());
        intent.insert("sender".to_string(), sender.to_string());
        intent.insert("receiver".to_string(), "bob".to_string());

        CstlPayload {
            version: "v5.0.0".to_string(),
            mode: "A".to_string(),
            meta,
            intent,
            relations: vec![],
            raw: String::new(),
        }
    }

    /// Corrige la meme trouvaille NFC que src/canonical.rs, mais cette fois
    /// sur le HASH REELLEMENT UTILISE EN PRODUCTION (via HashChain::append,
    /// la cle primaire de adn_store). Deux payloads dont un champ INTENT
    /// contient la meme valeur visible mais dans deux representations
    /// Unicode differentes (café precompose vs decompose) doivent produire
    /// EXACTEMENT le meme hash d'audit.
    #[test]
    fn test_nfc_equivalent_field_values_produce_same_audit_hash() {
        let mut a = mk("alice", "query");
        let mut b = mk("alice", "query");
        a.intent.insert("note".to_string(), "caf\u{00E9}".to_string());
        b.intent.insert("note".to_string(), "cafe\u{0301}".to_string());
        assert_ne!(a.intent.get("note"), b.intent.get("note"), "bytes bruts differents avant normalisation");
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn test_hash_is_deterministic() {
        let a = mk("alice", "query");
        let b = mk("alice", "query");
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn test_hash_changes_with_content() {
        let a = mk("alice", "query");
        let b = mk("alice", "command");
        assert_ne!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn test_parent_hash_excluded_from_hash() {
        let a = mk("alice", "query");
        let mut b = mk("alice", "query");
        b.meta.insert("PARENT_HASH".to_string(), "sha256:deadbeef".to_string());
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn test_chain_links_correctly() {
        let mut chain = HashChain::new();
        let e1 = chain.append(&mk("alice", "query"));
        let e2 = chain.append(&mk("bob", "reply"));

        assert_eq!(e1.parent_hash, "root");
        assert_eq!(e2.parent_hash, e1.hash);
        assert_eq!(chain.len(), 2);
        assert!(chain.verify_integrity().is_ok());
    }

    #[test]
    fn test_integrity_detects_break() {
        let mut chain = HashChain::new();
        chain.append(&mk("alice", "query"));
        chain.append(&mk("bob", "reply"));
        chain.entries[1].parent_hash = "sha256:tampered".to_string();
        assert!(chain.verify_integrity().is_err());
    }
}
