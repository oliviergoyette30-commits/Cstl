//! src/adn_store.rs — Couche 5 de l'architecture CSTL (mémoire persistante / provenance)
//! Port Rust natif de ce que `cstl_adn_store.py` était censé être selon le README.
//! Constat honnête au moment d'écrire ce module: `cstl_adn_store.py` n'existe nulle
//! part dans ce repo (vérifié par recherche exhaustive le 2026-09-03) — ce n'est
//! donc PAS un port d'un fichier réel, c'est une reconstruction en Rust à partir de
//! la description du README (schéma des 3 tables, sémantique commit/revoke).
//!
//! Portée de cette version: schéma SQLite + CRUD + journal du conseil humain.
//! PAS encore fait (honnête, pas caché): retrieval TF-IDF, get_primer()/load_context(),
//! ADNDeltaDetector. Ces pièces restent à construire si on en a besoin plus tard.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AdnEntry {
    pub hash: String,
    pub payload: String,
    pub encoder: Option<String>,
    pub produced_by: Option<String>,
    pub sigma: f64,
    pub parent_hash: Option<String>,
    pub conversation_id: Option<String>,
    pub turn: Option<i64>,
    pub committed: bool,
    pub committed_by: Option<String>,
    pub committed_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AdnStats {
    pub total: u64,
    pub committed: u64,
    pub pending: u64,
}

/// Une ligne de `adn_council_log` — jusqu'ici la table etait ecriture seule
/// (`commit`/`revoke` y inserent) sans aucun moyen de la relire. Correction
/// honnete: le journal d'audit humain existait dans la DB mais nulle part
/// dans le code Rust.
#[derive(Debug, Clone)]
pub struct CouncilLogEntry {
    pub id: i64,
    pub hash: String,
    pub action: String,
    pub by_whom: String,
    pub note: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct EmergenceProof {
    pub id: i64,
    pub question: String,
    pub solo_answers: String,
    pub final_decision: String,
    pub position_changed_by: Option<String>,
    pub changed_to: Option<String>,
    pub delta_sigma: Option<f64>,
    pub timestamp: i64,
}

pub struct AdnStore {
    conn: Connection,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl AdnStore {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        // Les FK ne sont PAS appliquees par defaut en SQLite, meme avec la
        // syntaxe REFERENCES dans le CREATE TABLE -- il faut l'activer
        // explicitement par connexion. Sans cette ligne, les contraintes
        // ci-dessous sont silencieusement ignorees.
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS adn_store (
                hash TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                encoder TEXT,
                produced_by TEXT,
                sigma REAL NOT NULL,
                parent_hash TEXT,
                conversation_id TEXT,
                turn INTEGER,
                committed INTEGER NOT NULL DEFAULT 0,
                committed_by TEXT,
                committed_at INTEGER,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS adn_council_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hash TEXT NOT NULL REFERENCES adn_store(hash),
                action TEXT NOT NULL,
                by_whom TEXT NOT NULL,
                note TEXT,
                timestamp INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS emergence_proofs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                question TEXT NOT NULL,
                solo_answers TEXT NOT NULL,
                final_decision TEXT NOT NULL,
                position_changed_by TEXT,
                changed_to TEXT,
                delta_sigma REAL,
                timestamp INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS adn_relations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hash TEXT NOT NULL REFERENCES adn_store(hash),
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    /// Stocke un payload (ASSUMES / non-commité par défaut). Idempotent sur `hash`:
    /// un hash déjà présent n'est pas écrasé (append-only, comme la chaîne d'audit).
    #[allow(clippy::too_many_arguments)]
    pub fn put(
        &self,
        hash: &str,
        payload: &str,
        encoder: Option<&str>,
        produced_by: Option<&str>,
        sigma: f64,
        parent_hash: Option<&str>,
        conversation_id: Option<&str>,
        turn: Option<i64>,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR IGNORE INTO adn_store
                (hash, payload, encoder, produced_by, sigma, parent_hash, conversation_id, turn, committed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
            params![hash, payload, encoder, produced_by, sigma, parent_hash, conversation_id, turn, now_unix()],
        )?;
        Ok(())
    }

    pub fn get(&self, hash: &str) -> Result<Option<AdnEntry>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT hash, payload, encoder, produced_by, sigma, parent_hash, conversation_id, turn,
                        committed, committed_by, committed_at, created_at
                 FROM adn_store WHERE hash = ?1",
                params![hash],
                |row| {
                    Ok(AdnEntry {
                        hash: row.get(0)?,
                        payload: row.get(1)?,
                        encoder: row.get(2)?,
                        produced_by: row.get(3)?,
                        sigma: row.get(4)?,
                        parent_hash: row.get(5)?,
                        conversation_id: row.get(6)?,
                        turn: row.get(7)?,
                        committed: row.get::<_, i64>(8)? != 0,
                        committed_by: row.get(9)?,
                        committed_at: row.get(10)?,
                        created_at: row.get(11)?,
                    })
                },
            )
            .optional()
    }

    /// Résout un hash court (les 16 premiers caractères hex après "sha256:") en
    /// entrée complète — utile pour les callback_data Telegram, limités à 64
    /// octets, bien trop court pour un sha256 complet ("sha256:" + 64 hex).
    pub fn get_by_short_id(&self, short_id: &str) -> Result<Option<AdnEntry>, rusqlite::Error> {
        let pattern = format!("sha256:{}%", short_id);
        self.conn
            .query_row(
                "SELECT hash, payload, encoder, produced_by, sigma, parent_hash, conversation_id, turn,
                        committed, committed_by, committed_at, created_at
                 FROM adn_store WHERE hash LIKE ?1 LIMIT 1",
                params![pattern],
                |row| {
                    Ok(AdnEntry {
                        hash: row.get(0)?,
                        payload: row.get(1)?,
                        encoder: row.get(2)?,
                        produced_by: row.get(3)?,
                        sigma: row.get(4)?,
                        parent_hash: row.get(5)?,
                        conversation_id: row.get(6)?,
                        turn: row.get(7)?,
                        committed: row.get::<_, i64>(8)? != 0,
                        committed_by: row.get(9)?,
                        committed_at: row.get(10)?,
                        created_at: row.get(11)?,
                    })
                },
            )
            .optional()
    }

    /// Ancrage humain (RestrictedCouncil). Rien n'est ancré sans ce commit explicite —
    /// aucune logique de quorum n'appelle encore cette fonction automatiquement:
    /// le quorum 2/3 humain (RestrictedCouncil) n'est pas construit dans cette passe.
    pub fn commit(&self, hash: &str, by_whom: &str, note: Option<&str>) -> Result<(), rusqlite::Error> {
        let now = now_unix();
        self.conn.execute(
            "UPDATE adn_store SET committed = 1, committed_by = ?2, committed_at = ?3 WHERE hash = ?1",
            params![hash, by_whom, now],
        )?;
        self.conn.execute(
            "INSERT INTO adn_council_log (hash, action, by_whom, note, timestamp) VALUES (?1, 'commit', ?2, ?3, ?4)",
            params![hash, by_whom, note, now],
        )?;
        Ok(())
    }

    pub fn revoke(&self, hash: &str, by_whom: &str, note: Option<&str>) -> Result<(), rusqlite::Error> {
        let now = now_unix();
        self.conn.execute(
            "UPDATE adn_store SET committed = 0, committed_by = NULL, committed_at = NULL WHERE hash = ?1",
            params![hash],
        )?;
        self.conn.execute(
            "INSERT INTO adn_council_log (hash, action, by_whom, note, timestamp) VALUES (?1, 'revoke', ?2, ?3, ?4)",
            params![hash, by_whom, note, now],
        )?;
        Ok(())
    }

    /// Journal d'audit complet pour un hash donne (commit/revoke, par qui, quand,
    /// note eventuelle), du plus ancien au plus recent. Premiere methode de
    /// lecture pour `adn_council_log` -- avant cette fonction, rien dans le
    /// code Rust ne pouvait relire ce que `commit()`/`revoke()` y ecrivent.
    pub fn council_log_for(&self, hash: &str) -> Result<Vec<CouncilLogEntry>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hash, action, by_whom, note, timestamp
             FROM adn_council_log WHERE hash = ?1 ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![hash], |row| {
            Ok(CouncilLogEntry {
                id: row.get(0)?,
                hash: row.get(1)?,
                action: row.get(2)?,
                by_whom: row.get(3)?,
                note: row.get(4)?,
                timestamp: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn stats(&self) -> Result<AdnStats, rusqlite::Error> {
        let total: u64 = self.conn.query_row("SELECT COUNT(*) FROM adn_store", [], |r| r.get(0))?;
        let committed: u64 =
            self.conn.query_row("SELECT COUNT(*) FROM adn_store WHERE committed = 1", [], |r| r.get(0))?;
        Ok(AdnStats { total, committed, pending: total - committed })
    }

    /// Persiste les relations d'un payload deja stocke (via `put`), pour que
    /// `ExecutionLab::check_consistency_with_history` puisse les retrouver lors
    /// d'une requete future. Appelee separement de `put()`: un payload sans
    /// relations (purpose=council_decision, etc.) n'a rien a inserer ici.
    pub fn put_relations(&self, hash: &str, relations: &[HashMap<String, String>]) -> Result<(), rusqlite::Error> {
        for rel in relations {
            if let (Some(subject), Some(predicate), Some(object)) =
                (rel.get("subject"), rel.get("type"), rel.get("object"))
            {
                self.conn.execute(
                    "INSERT INTO adn_relations (hash, subject, predicate, object) VALUES (?1, ?2, ?3, ?4)",
                    params![hash, subject, predicate, object],
                )?;
            }
        }
        Ok(())
    }

    /// Toutes les relations jamais stockees, tous hashes confondus -- l'historique
    /// complet que la Couche 3b utilise pour detecter des contradictions/cycles
    /// qui s'etalent sur plusieurs requetes, pas seulement dans un seul payload.
    pub fn all_relations(&self) -> Result<Vec<HashMap<String, String>>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT subject, predicate, object FROM adn_relations")?;
        let rows = stmt.query_map([], |row| {
            let mut m = HashMap::new();
            m.insert("subject".to_string(), row.get::<_, String>(0)?);
            m.insert("type".to_string(), row.get::<_, String>(1)?);
            m.insert("object".to_string(), row.get::<_, String>(2)?);
            Ok(m)
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Enregistre un `emergence_proof` (Level 4): la reponse solo de chaque
    /// modele face a une question, la decision collective finale, et si/comment
    /// une position a change. Portee honnete: aucun code de ce repo ne genere
    /// ces donnees automatiquement -- le debat multi-modele qui produit
    /// `solo_answers` se fait aujourd'hui manuellement, hors de ce serveur.
    /// Cette methode existe pour que la table serve reellement des qu'un vrai
    /// flux (orchestrateur ou saisie manuelle) l'appelle, plutot que de rester
    /// un schema sans aucun code Rust autour.
    #[allow(clippy::too_many_arguments)]
    pub fn put_emergence_proof(
        &self,
        question: &str,
        solo_answers: &str,
        final_decision: &str,
        position_changed_by: Option<&str>,
        changed_to: Option<&str>,
        delta_sigma: Option<f64>,
    ) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO emergence_proofs
                (question, solo_answers, final_decision, position_changed_by, changed_to, delta_sigma, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![question, solo_answers, final_decision, position_changed_by, changed_to, delta_sigma, now_unix()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Tous les emergence_proofs enregistres, du plus ancien au plus recent.
    pub fn get_emergence_proofs(&self) -> Result<Vec<EmergenceProof>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, question, solo_answers, final_decision, position_changed_by, changed_to, delta_sigma, timestamp
             FROM emergence_proofs ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(EmergenceProof {
                id: row.get(0)?,
                question: row.get(1)?,
                solo_answers: row.get(2)?,
                final_decision: row.get(3)?,
                position_changed_by: row.get(4)?,
                changed_to: row.get(5)?,
                delta_sigma: row.get(6)?,
                timestamp: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_get_roundtrip() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash1", "payload text", Some("enc"), Some("agent_a"), 0.3, None, None, None).unwrap();
        let entry = store.get("hash1").unwrap().unwrap();
        assert_eq!(entry.sigma, 0.3);
        assert!(!entry.committed);
    }

    #[test]
    fn test_commit_flow() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash2", "payload", None, None, 0.75, None, None, None).unwrap();
        store.commit("hash2", "human_arbiter", Some("quorum reached")).unwrap();
        let entry = store.get("hash2").unwrap().unwrap();
        assert!(entry.committed);
        assert_eq!(entry.committed_by.as_deref(), Some("human_arbiter"));
    }

    #[test]
    fn test_put_is_idempotent_append_only() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash3", "v1", None, None, 0.3, None, None, None).unwrap();
        store.put("hash3", "v2_should_be_ignored", None, None, 0.9, None, None, None).unwrap();
        let entry = store.get("hash3").unwrap().unwrap();
        assert_eq!(entry.payload, "v1");
    }

    #[test]
    fn test_get_by_short_id() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("sha256:abcdef0123456789fedcba", "payload", None, None, 0.5, None, None, None).unwrap();
        let entry = store.get_by_short_id("abcdef0123456789").unwrap().unwrap();
        assert_eq!(entry.hash, "sha256:abcdef0123456789fedcba");
    }

    #[test]
    fn test_stats() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("a", "p", None, None, 0.3, None, None, None).unwrap();
        store.put("b", "p", None, None, 0.3, None, None, None).unwrap();
        store.commit("a", "human", None).unwrap();
        let stats = store.stats().unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.committed, 1);
        assert_eq!(stats.pending, 1);
    }

    #[test]
    fn test_council_log_for_empty_when_never_committed() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_x", "p", None, None, 0.3, None, None, None).unwrap();
        assert!(store.council_log_for("hash_x").unwrap().is_empty());
    }

    #[test]
    fn test_council_log_for_records_commit_then_revoke_in_order() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_y", "p", None, None, 0.3, None, None, None).unwrap();
        store.commit("hash_y", "alice", Some("quorum ok")).unwrap();
        store.revoke("hash_y", "bob", Some("erreur")).unwrap();

        let log = store.council_log_for("hash_y").unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].action, "commit");
        assert_eq!(log[0].by_whom, "alice");
        assert_eq!(log[0].note.as_deref(), Some("quorum ok"));
        assert_eq!(log[1].action, "revoke");
        assert_eq!(log[1].by_whom, "bob");
    }

    #[test]
    fn test_council_log_for_is_scoped_to_its_hash() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_z1", "p", None, None, 0.3, None, None, None).unwrap();
        store.put("hash_z2", "p", None, None, 0.3, None, None, None).unwrap();
        store.commit("hash_z1", "alice", None).unwrap();
        store.commit("hash_z2", "bob", None).unwrap();

        let log = store.council_log_for("hash_z1").unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].by_whom, "alice");
    }

    #[test]
    fn test_put_relations_rejects_orphan_hash_via_foreign_key() {
        let store = AdnStore::open(":memory:").unwrap();
        let mut rel = HashMap::new();
        rel.insert("subject".to_string(), "A".to_string());
        rel.insert("type".to_string(), "part_of".to_string());
        rel.insert("object".to_string(), "B".to_string());
        // "hash_never_stored" n'a jamais ete put() dans adn_store -- la FK
        // doit refuser l'insertion plutot que la laisser passer en silence.
        let result = store.put_relations("hash_never_stored", &[rel]);
        assert!(result.is_err());
    }

    #[test]
    fn test_put_relations_and_all_relations_roundtrip() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_a", "payload", None, None, 0.3, None, None, None).unwrap();
        let mut rel = HashMap::new();
        rel.insert("subject".to_string(), "Marie Curie".to_string());
        rel.insert("type".to_string(), "born_in".to_string());
        rel.insert("object".to_string(), "Warsaw".to_string());
        store.put_relations("hash_a", &[rel]).unwrap();

        let all = store.all_relations().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].get("subject").map(String::as_str), Some("Marie Curie"));
        assert_eq!(all[0].get("object").map(String::as_str), Some("Warsaw"));
    }

    #[test]
    fn test_all_relations_accumulates_across_multiple_put_relations_calls() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_1", "payload", None, None, 0.3, None, None, None).unwrap();
        store.put("hash_2", "payload", None, None, 0.3, None, None, None).unwrap();
        let mut rel1 = HashMap::new();
        rel1.insert("subject".to_string(), "A".to_string());
        rel1.insert("type".to_string(), "part_of".to_string());
        rel1.insert("object".to_string(), "B".to_string());
        store.put_relations("hash_1", &[rel1]).unwrap();

        let mut rel2 = HashMap::new();
        rel2.insert("subject".to_string(), "B".to_string());
        rel2.insert("type".to_string(), "part_of".to_string());
        rel2.insert("object".to_string(), "C".to_string());
        store.put_relations("hash_2", &[rel2]).unwrap();

        let all = store.all_relations().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_put_and_get_emergence_proof_roundtrip() {
        let store = AdnStore::open(":memory:").unwrap();
        let id = store
            .put_emergence_proof(
                "Is the sky blue?",
                r#"{"claude":"yes","gpt":"yes","gemini":"mostly"}"#,
                "yes",
                Some("gemini"),
                Some("yes"),
                Some(0.12),
            )
            .unwrap();
        assert!(id > 0);

        let all = store.get_emergence_proofs().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].question, "Is the sky blue?");
        assert_eq!(all[0].position_changed_by.as_deref(), Some("gemini"));
        assert_eq!(all[0].delta_sigma, Some(0.12));
    }

    #[test]
    fn test_emergence_proof_without_position_change() {
        let store = AdnStore::open(":memory:").unwrap();
        store
            .put_emergence_proof(
                "2+2?",
                r#"{"claude":"4","gpt":"4"}"#,
                "4",
                None,
                None,
                None,
            )
            .unwrap();
        let all = store.get_emergence_proofs().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].position_changed_by.is_none());
        assert!(all[0].delta_sigma.is_none());
    }
}
