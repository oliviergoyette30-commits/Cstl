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
                hash TEXT NOT NULL,
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

    pub fn stats(&self) -> Result<AdnStats, rusqlite::Error> {
        let total: u64 = self.conn.query_row("SELECT COUNT(*) FROM adn_store", [], |r| r.get(0))?;
        let committed: u64 =
            self.conn.query_row("SELECT COUNT(*) FROM adn_store WHERE committed = 1", [], |r| r.get(0))?;
        Ok(AdnStats { total, committed, pending: total - committed })
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
}
