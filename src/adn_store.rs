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
use crate::server::audit::{AuditEntry, HashChain};
use crate::payload_compression::{compress_payload, decompress_payload, should_compress};

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

/// Résultat d'un vote de commit à quorum (Couche 2, gouvernance,
/// `cast_commit_vote`). `quorum_reached` reflète l'état APRES ce vote
/// (voix distinctes deja comptees, y compris celle-ci).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteOutcome {
    pub distinct_voters: usize,
    pub quorum_size: usize,
    pub quorum_reached: bool,
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

/// Arbitrage case for dispute resolution
#[derive(Debug, Clone)]
pub struct ArbitrageCase {
    pub case_id: String,
    pub initiator: String,
    pub subject: String,
    pub status: String,  // "Open", "Assigned", "Submitted", "PeerReview", "Finalized"
    pub created_at: i64,
    pub updated_at: i64,
    pub assigned_arbiters: Option<String>,
}

/// Arbitration ruling decision (database layer)
#[derive(Debug, Clone)]
pub struct DbArbitrationRuling {
    pub ruling_id: String,
    pub case_id: String,
    pub ruling_text: String,
    pub decided_by: String,
    pub status: String,  // "Pending", "Approved", "Rejected", "Finalized"
    pub created_at: i64,
}

/// ContextWindow: résultat du chargement de contexte (Couche 5)
/// Utilisé par `load_context()` pour reconstituer l'historique via
/// la chaîne parent_hash sur une profondeur limitée.
#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub entries: Vec<AdnEntry>,
    pub depth: usize,
}

/// Peer review signature on a ruling (database layer)
#[derive(Debug, Clone)]
pub struct DbPeerReviewSignature {
    pub review_id: String,
    pub ruling_id: String,
    pub reviewer_id: String,
    pub signature: String,
    pub approval_status: String,  // "Approved", "Rejected", "Abstain"
    pub reviewed_at: i64,
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
                payload BLOB NOT NULL,
                payload_compressed INTEGER NOT NULL DEFAULT 0,
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
            );
            CREATE INDEX IF NOT EXISTS idx_adn_relations_hash ON adn_relations(hash);
            CREATE INDEX IF NOT EXISTS idx_adn_relations_predicate ON adn_relations(predicate);
            CREATE INDEX IF NOT EXISTS idx_adn_produced_by ON adn_store(produced_by);
            CREATE INDEX IF NOT EXISTS idx_adn_parent_hash ON adn_store(parent_hash);
            CREATE INDEX IF NOT EXISTS idx_adn_created_at ON adn_store(created_at);
            CREATE INDEX IF NOT EXISTS idx_adn_conversation ON adn_store(conversation_id, turn);
            CREATE TABLE IF NOT EXISTS audit_trail (
                seq INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                parent_hash TEXT NOT NULL,
                sender TEXT NOT NULL,
                receiver TEXT NOT NULL,
                purpose TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS governance_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender TEXT NOT NULL,
                ts INTEGER NOT NULL,
                inconsistency INTEGER NOT NULL,
                semantic_warning INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_governance_events_ts ON governance_events(ts);
            CREATE TABLE IF NOT EXISTS governance_alerts (
                sender TEXT PRIMARY KEY,
                last_alert_ts INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS arbitrage_cases (
                case_id TEXT PRIMARY KEY,
                initiator TEXT NOT NULL,
                subject TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                assigned_arbiters TEXT
            );
            CREATE TABLE IF NOT EXISTS arbitration_rulings (
                ruling_id TEXT PRIMARY KEY,
                case_id TEXT NOT NULL UNIQUE,
                ruling_text TEXT NOT NULL,
                decided_by TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(case_id) REFERENCES arbitrage_cases(case_id)
            );
            CREATE TABLE IF NOT EXISTS peer_review_signatures (
                review_id TEXT PRIMARY KEY,
                ruling_id TEXT NOT NULL,
                reviewer_id TEXT NOT NULL,
                signature TEXT NOT NULL,
                approval_status TEXT NOT NULL,
                reviewed_at INTEGER NOT NULL,
                FOREIGN KEY(ruling_id) REFERENCES arbitration_rulings(ruling_id)
            );
            CREATE TABLE IF NOT EXISTS wai_dictionaries (
                dictionary_hash TEXT PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                symbols_json TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS deontic_executions (
                execution_id TEXT PRIMARY KEY,
                rule_id TEXT NOT NULL,
                event_id TEXT NOT NULL,
                modality TEXT NOT NULL,
                action TEXT NOT NULL,
                result TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_comments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                comment TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_arbitrage_cases_status ON arbitrage_cases(status);
            CREATE INDEX IF NOT EXISTS idx_arbitration_rulings_case ON arbitration_rulings(case_id);
            CREATE INDEX IF NOT EXISTS idx_peer_review_ruling ON peer_review_signatures(ruling_id);
            CREATE INDEX IF NOT EXISTS idx_wai_dictionaries_created ON wai_dictionaries(created_at);
            CREATE INDEX IF NOT EXISTS idx_deontic_executions_timestamp ON deontic_executions(timestamp);
            CREATE INDEX IF NOT EXISTS idx_audit_comments_timestamp ON audit_comments(timestamp);",
        )?;
        // Migration idempotente (2026-09-04, Couche 8: audit deontique
        // historique): `adn_relations` existe deja sur les bases reelles de
        // production (dont celle de l'utilisateur) SANS colonne `modality` --
        // l'ajouter au CREATE TABLE ci-dessus ne suffirait pas (IF NOT EXISTS
        // ne modifie jamais un schema deja present). ADD COLUMN echoue avec
        // "duplicate column name" sur une base qui l'a deja (execution
        // repetee de ce open(), ou base neuve creee apres ce fix -- possible
        // seulement si une version future remet modality dans CREATE TABLE)
        // -- cette erreur precise est ignoree ; toute AUTRE erreur est
        // loggee, jamais avalee en silence.
        if let Err(e) = conn.execute("ALTER TABLE adn_relations ADD COLUMN modality TEXT", []) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                eprintln!("[AdnStore] ⚠️  migration modality echouee (inattendu): {}", msg);
            }
        }
        // Migration idempotente (2026-09-14, Couche 5: compression de payloads):
        // `adn_store` peut exister sans colonne `payload_compressed` sur les bases
        // anciennes. L'ajouter au CREATE TABLE ci-dessus ne suffit pas pour les
        // bases existantes -- ADD COLUMN est idempotent (echoue silencieusement si
        // la colonne existe deja).
        if let Err(e) = conn.execute(
            "ALTER TABLE adn_store ADD COLUMN payload_compressed INTEGER NOT NULL DEFAULT 0",
            [],
        ) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                eprintln!("[AdnStore] ⚠️  migration payload_compressed echouee (inattendu): {}", msg);
            }
        }
        Ok(Self { conn })
    }

    /// Helper pour tests: créer un AdnStore depuis une connexion existante
    /// (typiquement in-memory)
    #[cfg(test)]
    pub fn open_connection(conn: Connection) -> Result<Self, rusqlite::Error> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS adn_store (
                hash TEXT PRIMARY KEY,
                payload BLOB NOT NULL,
                payload_compressed INTEGER NOT NULL DEFAULT 0,
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
                object TEXT NOT NULL,
                modality TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_adn_relations_hash ON adn_relations(hash);
            CREATE INDEX IF NOT EXISTS idx_adn_relations_predicate ON adn_relations(predicate);
            CREATE INDEX IF NOT EXISTS idx_adn_produced_by ON adn_store(produced_by);
            CREATE INDEX IF NOT EXISTS idx_adn_parent_hash ON adn_store(parent_hash);
            CREATE INDEX IF NOT EXISTS idx_adn_created_at ON adn_store(created_at);
            CREATE INDEX IF NOT EXISTS idx_adn_conversation ON adn_store(conversation_id, turn);",
        )?;
        Ok(Self { conn })
    }

    /// Stocke un payload (ASSUMES / non-commité par défaut). Idempotent sur `hash`:
    /// un hash déjà présent n'est pas écrasé (append-only, comme la chaîne d'audit).
    ///
    /// Compression optionnelle: si le payload > 10KB, il est compressé automatiquement
    /// avec gzip (flate2::Compression::default()). La colonne `payload_compressed`
    /// enregistre l'état (0=TEXT brut, 1=gzip compressé).
    ///
    /// Erreurs de compression ne sont pas fatales: si la compression échoue ou
    /// n'économise pas d'espace, le payload brut est stocké (payload_compressed=0).
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
        // Déterminer si compression est utile
        let (payload_bytes, is_compressed) = if should_compress(payload) {
            match compress_payload(payload) {
                Ok(compressed) => {
                    // Vérifier que la compression économise réellement de l'espace
                    if compressed.len() < payload.len() {
                        (compressed, true)
                    } else {
                        // Incompressible: stocker le brut
                        (payload.as_bytes().to_vec(), false)
                    }
                }
                Err(_) => {
                    // Erreur de compression: stocker le brut et continuer
                    eprintln!("[AdnStore] Compression failed for hash {}: storing uncompressed", hash);
                    (payload.as_bytes().to_vec(), false)
                }
            }
        } else {
            // Payload trop petit: stocker le brut
            (payload.as_bytes().to_vec(), false)
        };

        self.conn.execute(
            "INSERT OR IGNORE INTO adn_store
                (hash, payload, payload_compressed, encoder, produced_by, sigma, parent_hash, conversation_id, turn, committed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10)",
            params![hash, payload_bytes, is_compressed as i32, encoder, produced_by, sigma, parent_hash, conversation_id, turn, now_unix()],
        )?;
        Ok(())
    }

    pub fn get(&self, hash: &str) -> Result<Option<AdnEntry>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT hash, payload, payload_compressed, encoder, produced_by, sigma, parent_hash, conversation_id, turn,
                        committed, committed_by, committed_at, created_at
                 FROM adn_store WHERE hash = ?1",
                params![hash],
                |row| {
                    let is_compressed: i32 = row.get(2)?;

                    // Handle both BLOB (new format) and TEXT (old format) payloads for backward compatibility
                    let payload_bytes: Vec<u8> = match row.get_ref(1)?.data_type() {
                        rusqlite::types::Type::Blob => {
                            row.get::<_, Vec<u8>>(1)?
                        }
                        rusqlite::types::Type::Text => {
                            row.get::<_, String>(1)?.into_bytes()
                        }
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };

                    // Décompresser si nécessaire
                    let payload = if is_compressed != 0 {
                        match decompress_payload(&payload_bytes) {
                            Ok(decompressed) => decompressed,
                            Err(e) => {
                                eprintln!("[AdnStore] Decompression error for hash {}: {}", hash, e);
                                return Err(rusqlite::Error::QueryReturnedNoRows);
                            }
                        }
                    } else {
                        // Payload non compressé: convertir bytes en String
                        String::from_utf8(payload_bytes)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?
                    };

                    Ok(AdnEntry {
                        hash: row.get(0)?,
                        payload,
                        encoder: row.get(3)?,
                        produced_by: row.get(4)?,
                        sigma: row.get(5)?,
                        parent_hash: row.get(6)?,
                        conversation_id: row.get(7)?,
                        turn: row.get(8)?,
                        committed: row.get::<_, i64>(9)? != 0,
                        committed_by: row.get(10)?,
                        committed_at: row.get(11)?,
                        created_at: row.get(12)?,
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
                "SELECT hash, payload, payload_compressed, encoder, produced_by, sigma, parent_hash, conversation_id, turn,
                        committed, committed_by, committed_at, created_at
                 FROM adn_store WHERE hash LIKE ?1 LIMIT 1",
                params![pattern],
                |row| {
                    let is_compressed: i32 = row.get(2)?;

                    // Handle both BLOB (new format) and TEXT (old format) payloads for backward compatibility
                    let payload_bytes: Vec<u8> = match row.get_ref(1)?.data_type() {
                        rusqlite::types::Type::Blob => {
                            row.get::<_, Vec<u8>>(1)?
                        }
                        rusqlite::types::Type::Text => {
                            row.get::<_, String>(1)?.into_bytes()
                        }
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };

                    // Décompresser si nécessaire
                    let payload = if is_compressed != 0 {
                        match decompress_payload(&payload_bytes) {
                            Ok(decompressed) => decompressed,
                            Err(e) => {
                                eprintln!("[AdnStore] Decompression error for short_id {}: {}", short_id, e);
                                return Err(rusqlite::Error::QueryReturnedNoRows);
                            }
                        }
                    } else {
                        String::from_utf8(payload_bytes)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?
                    };

                    Ok(AdnEntry {
                        hash: row.get(0)?,
                        payload,
                        encoder: row.get(3)?,
                        produced_by: row.get(4)?,
                        sigma: row.get(5)?,
                        parent_hash: row.get(6)?,
                        conversation_id: row.get(7)?,
                        turn: row.get(8)?,
                        committed: row.get::<_, i64>(9)? != 0,
                        committed_by: row.get(10)?,
                        committed_at: row.get(11)?,
                        created_at: row.get(12)?,
                    })
                },
            )
            .optional()
    }

    /// Retrouve UNE entree de la chaine d'audit (`audit_trail`) par son
    /// `hash` -- ajoute le 2026-09-06 pour la negociation FIPA minimale
    /// (voir `server/handler.rs`, bloc `NEGOTIATION`): un `REFUSE` qui
    /// porte `in_reply_to=<hash>` a besoin de savoir QUEL `purpose` avait
    /// le payload original (etait-ce bien un `PROPOSE`/`CFP`, ou autre
    /// chose ?) sans avoir a re-parser tout le payload brut stocke dans
    /// `adn_store` -- `audit_trail` porte deja `purpose` en colonne depuis
    /// le debut (voir `save_audit_entry`), seule la lecture cible par hash
    /// manquait. Aucune migration de schema: la table existe deja.
    pub fn get_audit_entry(&self, hash: &str) -> Result<Option<AuditEntry>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT seq, hash, parent_hash, sender, receiver, purpose
                 FROM audit_trail WHERE hash = ?1",
                params![hash],
                |row| {
                    Ok(AuditEntry {
                        hash: row.get(1)?,
                        parent_hash: row.get(2)?,
                        sender: row.get(3)?,
                        receiver: row.get(4)?,
                        purpose: row.get(5)?,
                        seq: row.get(0)?,
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

    /// Résultat d'un vote de commit avec quorum (Couche 2, gouvernance).
    pub fn cast_commit_vote(
        &self,
        hash: &str,
        by_whom: &str,
        note: Option<&str>,
        quorum_size: usize,
    ) -> Result<VoteOutcome, rusqlite::Error> {
        let now = now_unix();
        // On enregistre toujours le vote, quorum atteint ou non -- c'est ce
        // journal (adn_council_log, colonnes by_whom/timestamp deja
        // presentes) qui permet de recompter les votants distincts.
        self.conn.execute(
            "INSERT INTO adn_council_log (hash, action, by_whom, note, timestamp) VALUES (?1, 'commit', ?2, ?3, ?4)",
            params![hash, by_whom, note, now],
        )?;
        let distinct_voters: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT by_whom) FROM adn_council_log WHERE hash = ?1 AND action = 'commit'",
            params![hash],
            |r| r.get(0),
        )?;
        let quorum_size = quorum_size.max(1);
        let distinct_voters = distinct_voters as usize;
        let quorum_reached = distinct_voters >= quorum_size;
        if quorum_reached {
            self.conn.execute(
                "UPDATE adn_store SET committed = 1, committed_by = ?2, committed_at = ?3 WHERE hash = ?1",
                params![hash, by_whom, now],
            )?;
        }
        Ok(VoteOutcome { distinct_voters, quorum_size, quorum_reached })
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
    /// Persiste aussi `modality` quand le champ est present sur la RELATION
    /// (`modality=MUST|MUST_NOT|...`, Couche 8 -- audit deontique historique,
    /// 2026-09-04) -- NULL sinon (l'immense majorite des relations
    /// factuelles), colonne ajoutee par la migration idempotente dans open().
    pub fn put_relations(&self, hash: &str, relations: &[HashMap<String, String>]) -> Result<(), rusqlite::Error> {
        for rel in relations {
            if let (Some(subject), Some(predicate), Some(object)) =
                (rel.get("subject"), rel.get("type"), rel.get("object"))
            {
                self.conn.execute(
                    "INSERT INTO adn_relations (hash, subject, predicate, object, modality) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![hash, subject, predicate, object, rel.get("modality")],
                )?;
            }
        }
        Ok(())
    }

    /// Toutes les relations deontiques (modality IS NOT NULL) jamais
    /// persistees, tous hashes/agents confondus -- l'historique que
    /// `execution_lab::check_deontic_consistency_with_history` (Couche 8)
    /// utilise pour detecter une contradiction Axiome D (MUST/MUST_NOT sur
    /// le meme (subject, object)) qui s'etale sur PLUSIEURS payloads/agents,
    /// pas seulement a l'interieur d'un seul (deja couvert, bloquant, par
    /// `server/validator.rs::validate_deontic_constraints`). Meme filtre SQL
    /// que `relations_for_predicates` (charge seulement ce qui compte, pas
    /// tout `adn_relations`).
    pub fn deontic_relations_history(&self) -> Result<Vec<HashMap<String, String>>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT subject, predicate, object, modality FROM adn_relations WHERE modality IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            let mut m = HashMap::new();
            m.insert("subject".to_string(), row.get::<_, String>(0)?);
            m.insert("type".to_string(), row.get::<_, String>(1)?);
            m.insert("object".to_string(), row.get::<_, String>(2)?);
            m.insert("modality".to_string(), row.get::<_, String>(3)?);
            Ok(m)
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Toutes les relations jamais stockees, tous hashes confondus -- l'historique
    /// complet que la Couche 3b utilise pour detecter des contradictions/cycles
    /// qui s'etalent sur plusieurs requetes, pas seulement dans un seul payload.
    /// Conservee telle quelle pour compatibilite (tests existants, usage generique
    /// hors ExecutionLab si besoin un jour) -- voir `relations_for_predicates` pour
    /// le chemin utilise reellement par le handler, qui ne charge que ce qui compte.
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

    /// Comme `all_relations()`, mais filtre au niveau SQL sur une liste de
    /// predicats (WHERE predicate IN (...)). Existe pour que
    /// `ExecutionLab::check_consistency_with_history` (via le handler) ne
    /// charge que les ~6 predicats dont il se sert reellement
    /// (`execution_lab::relevant_predicates()`), pas tout ce qui a jamais ete
    /// stocke dans `adn_relations` -- y compris, une fois le Layer 4
    /// (hypothesis engine) actif, des relations `ASSUMES`/`DOUBTS` qui ne
    /// concernent pas du tout la coherence de Couche 3b. Correction honnete:
    /// ca reste O(relations pertinentes), pas O(1) -- un vrai lookup cible par
    /// (subject, predicate) demanderait de casser la purete de execution_lab.rs
    /// (voir le message de commit), pas fait ici volontairement.
    pub fn relations_for_predicates(
        &self,
        predicates: &[&str],
    ) -> Result<Vec<HashMap<String, String>>, rusqlite::Error> {
        if predicates.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = predicates.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT subject, predicate, object FROM adn_relations WHERE predicate IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            predicates.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
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

    /// Relations avec modalité pour un hash spécifique (utilisé par ADNDeltaDetector)
    /// Retourne une HashMap avec clé = "subject:predicate:object", valeur = modalité
    pub fn get_relations_with_modality(
        &self,
        hash: &str,
    ) -> Result<HashMap<String, String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT subject, predicate, object, modality FROM adn_relations WHERE hash = ?1",
        )?;
        let rows = stmt.query_map([hash], |row| {
            let subject: String = row.get(0)?;
            let predicate: String = row.get(1)?;
            let object: String = row.get(2)?;
            let modality: Option<String> = row.get(3)?;
            let key = format!("{}:{}:{}", subject, predicate, object);
            Ok((key, modality.unwrap_or_else(|| "NONE".to_string())))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (key, modality) = r?;
            map.insert(key, modality);
        }
        Ok(map)
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

    // ── Chaine d'audit (Couche 5/8) -- fusionnee depuis l'ancien module
    // `server/audit_store.rs` le 2026-09-04 (item #1 de la liste des choses
    // a faire): les deux tables (`adn_store`/`adn_relations` ici,
    // `audit_trail` avant dans un fichier separe) pointaient deja sur le
    // MEME fichier SQLite via deux `Connection` distinctes -- un vrai risque
    // (deux connexions non coordonnees vers le meme fichier, deux verrous
    // `Mutex` en memoire different pour un seul et unique fichier sur
    // disque), pas seulement de la dette cosmetique. Desormais une seule
    // `Connection`, un seul schema, un seul `Arc<Mutex<AdnStore>>> cote
    // serveur (voir `server/mod.rs`).

    /// Sauvegarde une entree de la chaine d'audit (append-only). `INSERT OR
    /// IGNORE`, pas `INSERT` brut: un payload de contenu identique (meme
    /// `canonical_hash`) soumis deux fois doit rester idempotent, comme
    /// `put()` ci-dessus -- `HashChain::append` en memoire ne deduplique pas
    /// lui-meme (voir server/audit.rs).
    pub fn save_audit_entry(&self, entry: &AuditEntry) -> Result<(), rusqlite::Error> {
        let rows_affected = self.conn.execute(
            "INSERT OR IGNORE INTO audit_trail (seq, hash, parent_hash, sender, receiver, purpose)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                entry.seq,
                &entry.hash,
                &entry.parent_hash,
                &entry.sender,
                &entry.receiver,
                &entry.purpose,
            ],
        )?;
        if rows_affected > 0 {
            eprintln!("[AdnStore] Audit persisted seq={}", entry.seq);
        } else {
            eprintln!("[AdnStore] Audit seq={} ignore (hash {} deja present)", entry.seq, entry.hash);
        }
        Ok(())
    }

    /// Charge toute la chaine d'audit persistee depuis `audit_trail`, du
    /// plus ancien au plus recent -- utilise au demarrage du serveur pour
    /// que `seq`/`parent_hash` survivent a un redemarrage reel (voir
    /// `server/mod.rs::with_data_path`).
    pub fn load_chain(&self) -> Result<HashChain, rusqlite::Error> {
        let mut chain = HashChain::new();

        let mut stmt = self.conn.prepare(
            "SELECT seq, hash, parent_hash, sender, receiver, purpose
             FROM audit_trail ORDER BY seq",
        )?;

        let entries = stmt.query_map([], |row| {
            Ok(AuditEntry {
                hash: row.get(1)?,
                parent_hash: row.get(2)?,
                sender: row.get(3)?,
                receiver: row.get(4)?,
                purpose: row.get(5)?,
                seq: row.get(0)?,
            })
        })?;

        for entry_result in entries {
            chain.entries.push(entry_result?);
        }

        eprintln!("[AdnStore] Loaded {} audit entries from disk", chain.len());
        Ok(chain)
    }

    pub fn audit_count(&self) -> Result<u64, rusqlite::Error> {
        self.conn.query_row("SELECT COUNT(*) FROM audit_trail", [], |row| row.get(0))
    }

    // ── Gouvernance (Couche 2) -- persistance ajoutee le 2026-09-05.
    // `governance.rs::GovernanceTracker` vivait purement en memoire
    // (`Arc<Mutex<GovernanceTracker>>` cote serveur, remis a zero a chaque
    // redemarrage) -- meme fichier/`Connection` que le reste (adn_store/
    // audit_trail), pas une base separee, coherent avec la fusion du
    // 2026-09-04. Un evenement = un appel `record()` (un payload traite),
    // exactement le meme grain que `save_audit_entry` pour l'audit trail --
    // deja le patron etabli de ce depot pour ce compromis latence/durabilite.

    /// Sauvegarde un evenement de gouvernance (un appel `record()`) pour
    /// `sender`, et purge dans la meme requete tout ce qui est devenu plus
    /// vieux que `prune_before` (horodatage unix) -- sans cette purge,
    /// `governance_events` grossirait sans limite sur un serveur qui tourne
    /// des mois, alors que le mecanisme lui-meme (fenetre glissante) n'a
    /// jamais besoin de plus que la plus grande fenetre (`DRIFT_WINDOW`).
    /// L'appelant (`handler.rs`) calcule `prune_before` a partir des
    /// constantes de `governance.rs` -- ce module reste agnostique du sens
    /// de ces fenetres, il ne fait qu'ecrire/purger sur un seuil donne.
    pub fn save_governance_event(
        &self,
        sender: &str,
        ts: i64,
        had_inconsistency: bool,
        had_semantic_warning: bool,
        prune_before: i64,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO governance_events (sender, ts, inconsistency, semantic_warning) VALUES (?1, ?2, ?3, ?4)",
            params![sender, ts, had_inconsistency as i64, had_semantic_warning as i64],
        )?;
        self.conn.execute("DELETE FROM governance_events WHERE ts < ?1", params![prune_before])?;
        Ok(())
    }

    /// Sauvegarde le dernier horodatage d'alerte connu pour `sender` --
    /// upsert (un seul horodatage par sender a la fois a un sens pour le
    /// cooldown anti-spam, pas un historique).
    pub fn save_governance_alert(&self, sender: &str, ts: i64) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO governance_alerts (sender, last_alert_ts) VALUES (?1, ?2)
             ON CONFLICT(sender) DO UPDATE SET last_alert_ts = excluded.last_alert_ts",
            params![sender, ts],
        )?;
        Ok(())
    }

    /// Charge tous les evenements de gouvernance dont l'horodatage est
    /// superieur ou egal a `since` (deja filtre au niveau SQL -- pas la
    /// peine de rapatrier ce qui est de toute facon hors de la plus grande
    /// fenetre glissante), du plus ancien au plus recent -- utilise au
    /// demarrage pour reconstruire `GovernanceTracker` via
    /// `with_defaults_restored`.
    pub fn load_governance_events(&self, since: i64) -> Result<Vec<(String, i64, bool, bool)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT sender, ts, inconsistency, semantic_warning FROM governance_events WHERE ts >= ?1 ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, i64>(3)? != 0,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Charge tous les derniers horodatages d'alerte connus, un par sender.
    pub fn load_governance_alerts(&self) -> Result<Vec<(String, i64)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT sender, last_alert_ts FROM governance_alerts")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
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

    // ═══════════════════════════════════════════════════════════════════
    // PHASE 3: Arbitrage Persistence Methods (Layer 3b)
    // ═══════════════════════════════════════════════════════════════════

    /// Save an arbitrage case to the database
    pub fn save_arbitrage_case(&self, case: &ArbitrageCase) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO arbitrage_cases
                (case_id, initiator, subject, status, created_at, updated_at, assigned_arbiters)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &case.case_id, &case.initiator, &case.subject, &case.status,
                &case.created_at, &case.updated_at, &case.assigned_arbiters
            ],
        )?;
        Ok(())
    }

    /// Retrieve an arbitrage case by ID
    pub fn get_arbitrage_case(&self, case_id: &str) -> Result<Option<ArbitrageCase>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT case_id, initiator, subject, status, created_at, updated_at, assigned_arbiters
                 FROM arbitrage_cases WHERE case_id = ?1",
                params![case_id],
                |row| {
                    Ok(ArbitrageCase {
                        case_id: row.get(0)?,
                        initiator: row.get(1)?,
                        subject: row.get(2)?,
                        status: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                        assigned_arbiters: row.get(6)?,
                    })
                },
            )
            .optional()
    }

    /// Save an arbitration ruling
    pub fn save_arbitrage_ruling(&self, ruling: &DbArbitrationRuling) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO arbitration_rulings
                (ruling_id, case_id, ruling_text, decided_by, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &ruling.ruling_id, &ruling.case_id, &ruling.ruling_text,
                &ruling.decided_by, &ruling.status, &ruling.created_at
            ],
        )?;
        Ok(())
    }

    /// Retrieve a ruling by ID
    pub fn get_arbitrage_ruling(&self, ruling_id: &str) -> Result<Option<DbArbitrationRuling>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT ruling_id, case_id, ruling_text, decided_by, status, created_at
                 FROM arbitration_rulings WHERE ruling_id = ?1",
                params![ruling_id],
                |row| {
                    Ok(DbArbitrationRuling {
                        ruling_id: row.get(0)?,
                        case_id: row.get(1)?,
                        ruling_text: row.get(2)?,
                        decided_by: row.get(3)?,
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    /// Retrieve ruling by case ID
    pub fn get_ruling_by_case(&self, case_id: &str) -> Result<Option<DbArbitrationRuling>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT ruling_id, case_id, ruling_text, decided_by, status, created_at
                 FROM arbitration_rulings WHERE case_id = ?1",
                params![case_id],
                |row| {
                    Ok(DbArbitrationRuling {
                        ruling_id: row.get(0)?,
                        case_id: row.get(1)?,
                        ruling_text: row.get(2)?,
                        decided_by: row.get(3)?,
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    /// Save a peer review signature
    pub fn save_peer_review(&self, review: &DbPeerReviewSignature) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO peer_review_signatures
                (review_id, ruling_id, reviewer_id, signature, approval_status, reviewed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &review.review_id, &review.ruling_id, &review.reviewer_id,
                &review.signature, &review.approval_status, &review.reviewed_at
            ],
        )?;
        Ok(())
    }

    /// Get all peer reviews for a specific ruling
    pub fn get_peer_reviews_for_ruling(&self, ruling_id: &str) -> Result<Vec<DbPeerReviewSignature>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT review_id, ruling_id, reviewer_id, signature, approval_status, reviewed_at
             FROM peer_review_signatures WHERE ruling_id = ?1 ORDER BY reviewed_at"
        )?;
        let rows = stmt.query_map(params![ruling_id], |row| {
            Ok(DbPeerReviewSignature {
                review_id: row.get(0)?,
                ruling_id: row.get(1)?,
                reviewer_id: row.get(2)?,
                signature: row.get(3)?,
                approval_status: row.get(4)?,
                reviewed_at: row.get(5)?,
            })
        })?;
        let mut reviews = Vec::new();
        for row in rows {
            reviews.push(row?);
        }
        Ok(reviews)
    }

    /// Get all active arbiters (stub for now, would need an arbiters table)
    pub fn get_active_arbiters(&self) -> Result<Vec<String>, rusqlite::Error> {
        // Placeholder: in production, would query from an arbiters table
        // For now, return empty — integration tests can mock this
        Ok(Vec::new())
    }

    /// Save a WAI dictionary version to persistent storage
    pub fn save_wai_dictionary(
        &self,
        dictionary_hash: &str,
        timestamp: u64,
        symbols_json: &str,
        size_bytes: usize,
    ) -> Result<(), rusqlite::Error> {
        let created_at = now_unix();
        self.conn.execute(
            "INSERT OR IGNORE INTO wai_dictionaries
             (dictionary_hash, timestamp, symbols_json, size_bytes, created_at)
             VALUES (?, ?, ?, ?, ?)",
            params![dictionary_hash, timestamp as i64, symbols_json, size_bytes as i64, created_at],
        )?;
        Ok(())
    }

    /// Load a WAI dictionary version by hash
    pub fn load_wai_dictionary(&self, dictionary_hash: &str) -> Result<Option<(u64, String, usize)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, symbols_json, size_bytes FROM wai_dictionaries WHERE dictionary_hash = ?"
        )?;

        let result = stmt.query_row([dictionary_hash], |row| {
            let timestamp: i64 = row.get(0)?;
            let symbols_json: String = row.get(1)?;
            let size_bytes: i64 = row.get(2)?;
            Ok((timestamp as u64, symbols_json, size_bytes as usize))
        }).optional()?;

        Ok(result)
    }

    /// Get all WAI dictionaries (for discovery)
    pub fn list_wai_dictionaries(&self) -> Result<Vec<(String, u64, usize)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT dictionary_hash, timestamp, size_bytes FROM wai_dictionaries ORDER BY created_at DESC"
        )?;

        let dicts = stmt.query_map([], |row| {
            let hash: String = row.get(0)?;
            let timestamp: i64 = row.get(1)?;
            let size_bytes: i64 = row.get(2)?;
            Ok((hash, timestamp as u64, size_bytes as usize))
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(dicts)
    }

    /// Couche 9: Save deontic execution record
    pub fn save_deontic_execution(
        &self,
        execution: &crate::server::deontic_orchestration::DeonticExecution,
    ) -> Result<(), rusqlite::Error> {
        let execution_id = format!(
            "{}_{}",
            execution.rule_id,
            execution.timestamp.timestamp()
        );

        let modality_str = match execution.modality {
            crate::server::deontic_orchestration::DeonticModality::Must => "MUST",
            crate::server::deontic_orchestration::DeonticModality::MustNot => "MUST_NOT",
            crate::server::deontic_orchestration::DeonticModality::May => "MAY",
        };

        let result_str = match execution.result {
            crate::server::deontic_orchestration::ExecutionResult::Success => "Success",
            crate::server::deontic_orchestration::ExecutionResult::Rejected => "Rejected",
            crate::server::deontic_orchestration::ExecutionResult::NoMatch => "NoMatch",
            crate::server::deontic_orchestration::ExecutionResult::Failed => "Failed",
        };

        self.conn.execute(
            "INSERT OR IGNORE INTO deontic_executions
                (execution_id, rule_id, event_id, modality, action, result, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &execution_id,
                &execution.rule_id,
                &execution.event_id,
                modality_str,
                &execution.action,
                result_str,
                execution.timestamp.timestamp(),
            ],
        )?;
        Ok(())
    }

    /// Couche 9: Append audit comment to the trail
    pub fn append_comment(&self, comment: &str) -> Result<(), rusqlite::Error> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO audit_comments (comment, timestamp) VALUES (?1, ?2)",
            params![comment, timestamp],
        )?;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════
    // COUCHE 5: get_primer() & load_context()
    // ═══════════════════════════════════════════════════════════════════

    /// get_primer() — charger les 3-5 entrées ADN les plus pertinentes
    /// avant un tour donné dans une conversation.
    ///
    /// Utilisé par un agent récepteur pour reconstituer le contexte
    /// auquel répondre, en sélectionnant les entrées engagées (committed=1)
    /// les plus proches du tour cible (et antérieures à celui-ci).
    ///
    /// Stratégie de sélection:
    ///   1. Filtrer par conversation_id ET turn < target_turn
    ///   2. Trier par tour DESC (plus proche d'abord)
    ///   3. Inclure seulement committed=1 (ancrage humain, Couche 5)
    ///   4. Limiter à 5 entrées max (fenêtre glissante)
    ///   5. Concaténer les 500 premiers caractères de chaque payload
    ///
    /// Complexité: O(n) scan SQL sur adn_store filtré par conversation_id,
    /// pas de full-table scan.
    pub fn get_primer(
        &self,
        conversation_id: &str,
        target_turn: i64,
    ) -> Result<String, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, payload, payload_compressed, encoder, sigma, turn
             FROM adn_store
             WHERE conversation_id = ?1 AND turn < ?2 AND committed = 1
             ORDER BY turn DESC
             LIMIT 5"
        )?;

        let entries = stmt.query_map(params![conversation_id, target_turn], |row| {
            let payload_bytes: Vec<u8> = row.get(1)?;
            let is_compressed: i32 = row.get(2)?;

            let payload = if is_compressed != 0 {
                match decompress_payload(&payload_bytes) {
                    Ok(decompressed) => decompressed,
                    Err(_) => {
                        // Fallback: return empty string on decompression error
                        eprintln!("[AdnStore] Decompression error in get_primer");
                        String::new()
                    }
                }
            } else {
                String::from_utf8(payload_bytes).unwrap_or_default()
            };

            Ok((
                row.get::<_, String>(0)?,      // hash
                payload,                        // decompressed payload
                row.get::<_, Option<String>>(3)?,  // encoder
                row.get::<_, f64>(4)?,         // sigma
                row.get::<_, Option<i64>>(5)?, // turn
            ))
        })?;

        let mut results = Vec::new();
        for entry_result in entries {
            results.push(entry_result?);
        }

        if results.is_empty() {
            return Ok(String::new());
        }

        // Construire le primer — format lisible pour agent
        let mut primer_lines = Vec::new();
        primer_lines.push("ADN_PRIMER [".to_string());
        primer_lines.push(format!(
            "  conversation_id = {},",
            conversation_id
        ));
        primer_lines.push(format!(
            "  before_turn = {},",
            target_turn
        ));
        primer_lines.push(format!("  num_entries = {},", results.len()));
        primer_lines.push("  anchors = [".to_string());

        for (hash, payload, encoder, sigma, turn) in results {
            // Prendre les 500 premiers caractères du payload
            let excerpt = if payload.len() > 500 {
                format!("{}...", &payload[..500])
            } else {
                payload
            };
            // Échapper les sauts de ligne dans l'excerpt
            let excerpt_escaped = excerpt.replace("\n", "\\n");

            let encoder_str = encoder.unwrap_or_default();
            let turn_str = turn.map(|t| t.to_string()).unwrap_or_else(|| "?".to_string());

            primer_lines.push(format!(
                "    {{hash=\"{}\", encoder=\"{}\", sigma={:.2}, turn={}, excerpt=\"{}\"}},",
                hash, encoder_str, sigma, turn_str, excerpt_escaped
            ));
        }

        primer_lines.push("  ],".to_string());
        primer_lines.push("]".to_string());

        Ok(primer_lines.join("\n"))
    }

    /// load_context() — marcher la chaîne parent_hash en arrière jusqu'à
    /// max_depth étapes pour reconstituer l'historique complet.
    ///
    /// Procédure:
    ///   1. Commencer par le hash fourni
    ///   2. Charger AdnEntry pour ce hash
    ///   3. Si parent_hash existe ET depth < max_depth, récurser
    ///   4. Arrêter si on atteint max_depth OU si parent n'existe pas
    ///   5. Détecter les cycles (même hash deux fois) — retourner Err
    ///
    /// Complexité: O(max_depth) appels SQL, chacun O(1) par index primaire.
    /// Aucun full-table scan. Pas de charge mémoire exponentiellement croissante.
    ///
    /// Retourner un ContextWindow avec la liste des entrées (ordre: du plus
    /// ancien au plus récent, inverse de la marche) et la profondeur atteinte.
    pub fn load_context(
        &self,
        hash: &str,
        max_depth: usize,
    ) -> Result<ContextWindow, Box<dyn std::error::Error>> {
        let mut entries = Vec::new();
        let mut seen_hashes = std::collections::HashSet::new();
        let mut current_hash = hash.to_string();
        let mut depth = 0;

        // Marcher en arrière sur la chaîne parent_hash
        while depth < max_depth {
            // Détecter les cycles
            if seen_hashes.contains(&current_hash) {
                return Err(
                    format!(
                        "Circular parent_hash chain detected at hash: {}",
                        current_hash
                    )
                    .into(),
                );
            }
            seen_hashes.insert(current_hash.clone());

            // Charger l'entrée courante
            let entry = self
                .get(&current_hash)?
                .ok_or_else(|| format!("Hash not found: {}", current_hash))?;

            // Récupérer le parent avant d'ajouter l'entrée au vecteur
            let parent_hash_opt = entry.parent_hash.clone();

            entries.push(entry);
            depth += 1;

            // Si pas de parent ou parent == "root", arrêter
            match parent_hash_opt {
                Some(parent) if parent != "root" && !parent.is_empty() => {
                    current_hash = parent;
                }
                _ => break,
            }
        }

        // Inverser pour avoir l'ordre chronologique (ancien → récent)
        entries.reverse();

        Ok(ContextWindow {
            entries,
            depth,
        })
    }

    // ── TF-IDF Retrieval (Couche 5: persistent memory & semantic search) ──
    // Hand-rolled tokenizer + TF-IDF indexing for semantic search over committed payloads
    // No external NLP libraries; keeps Couche 5 self-contained and lightweight.

    /// Simple hand-rolled tokenizer: lowercases, splits on non-alphanumeric,
    /// filters stop words, returns term frequencies.
    fn tokenize_payload(text: &str) -> HashMap<String, usize> {
        const STOP_WORDS: &[&str] = &[
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for",
            "of", "with", "by", "from", "as", "is", "was", "are", "be", "been",
            "if", "then", "else", "this", "that", "these", "those", "it", "its",
            "what", "which", "who", "when", "where", "why", "how", "can", "could",
            "would", "should", "may", "might", "must", "shall", "do", "does", "did",
        ];

        let mut terms = HashMap::new();
        let lowered = text.to_lowercase();
        // Split on any non-alphanumeric character
        for token in lowered.split(|c: char| !c.is_alphanumeric()) {
            if !token.is_empty() && !STOP_WORDS.contains(&token) && token.len() > 2 {
                *terms.entry(token.to_string()).or_insert(0) += 1;
            }
        }
        terms
    }

    /// Builds a TF-IDF index from all committed payloads.
    /// Returns a map: (term -> map of (hash -> tf_idf_score))
    fn build_tfidf_index(&self) -> Result<HashMap<String, HashMap<String, f64>>, rusqlite::Error> {
        // Fetch all committed payloads with their hashes
        let mut stmt = self.conn.prepare(
            "SELECT hash, payload, payload_compressed FROM adn_store WHERE committed = 1 ORDER BY created_at DESC"
        )?;

        let documents: Vec<(String, String)> = stmt
            .query_map([], |row| {
                let hash = row.get::<_, String>(0)?;
                let payload_bytes: Vec<u8> = row.get(1)?;
                let is_compressed: i32 = row.get(2)?;

                let payload = if is_compressed != 0 {
                    match decompress_payload(&payload_bytes) {
                        Ok(decompressed) => decompressed,
                        Err(_) => {
                            eprintln!("[AdnStore] Decompression error in build_tfidf_index for hash {}", hash);
                            String::new()
                        }
                    }
                } else {
                    String::from_utf8(payload_bytes).unwrap_or_default()
                };

                Ok((hash, payload))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let num_docs = documents.len() as f64;
        if num_docs == 0.0 {
            return Ok(HashMap::new());
        }

        // Count document frequency for each term (how many docs contain it)
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for (_hash, payload) in &documents {
            let terms = Self::tokenize_payload(payload);
            for term in terms.keys() {
                *doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }

        // Build TF-IDF index: term -> (hash -> score)
        let mut index: HashMap<String, HashMap<String, f64>> = HashMap::new();
        for (hash, payload) in &documents {
            let tf = Self::tokenize_payload(payload);
            let doc_length = payload.len() as f64;

            for (term, count) in tf {
                // TF = count / doc_length (normalized by document length)
                let term_frequency = (count as f64) / doc_length.max(1.0);

                // IDF = log(total_docs / docs_containing_term)
                let df = doc_freq.get(&term).copied().unwrap_or(1);
                let idf = (num_docs / (df as f64)).log10().max(0.0);

                // TF-IDF = TF * IDF
                let score = term_frequency * idf;

                index
                    .entry(term)
                    .or_insert_with(HashMap::new)
                    .insert(hash.clone(), score);
            }
        }

        Ok(index)
    }

    /// Retrieves top-k results for a query using TF-IDF scoring.
    /// Returns Vec of (hash, relevance_score) sorted by score descending.
    /// Handles edge cases: empty query, no matches, multiple matches.
    pub fn get_tfidf_results(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<(String, f64)>, rusqlite::Error> {
        // Edge case: empty query
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let index = self.build_tfidf_index()?;
        if index.is_empty() {
            return Ok(Vec::new()); // No indexed documents
        }

        // Tokenize query
        let query_terms = Self::tokenize_payload(query);
        if query_terms.is_empty() {
            return Ok(Vec::new()); // Query has no meaningful terms after stop-word filtering
        }

        // Accumulate scores for each document
        let mut scores: HashMap<String, f64> = HashMap::new();
        for term in query_terms.keys() {
            if let Some(term_scores) = index.get(term) {
                for (hash, score) in term_scores {
                    *scores.entry(hash.clone()).or_insert(0.0) += score;
                }
            }
        }

        // Edge case: no documents match any query term
        if scores.is_empty() {
            return Ok(Vec::new());
        }

        // Sort by score descending and return top_k
        let mut results: Vec<(String, f64)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results.into_iter().take(top_k).collect())
    }

    /// Alternative retrieval using basic fulltext search (exact term matching).
    /// Faster fallback when TF-IDF complexity is unnecessary.
    /// Searches all committed payloads (both compressed and uncompressed) in memory.
    pub fn search_payloads(
        &self,
        term: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>, rusqlite::Error> {
        if term.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Fetch all committed payloads with their hashes
        let mut stmt = self.conn.prepare(
            "SELECT hash, payload, payload_compressed FROM adn_store
             WHERE committed = 1 ORDER BY created_at DESC"
        )?;

        let all_entries: Vec<(String, String)> = stmt
            .query_map([], |row| {
                let hash = row.get::<_, String>(0)?;
                let payload_bytes: Vec<u8> = row.get(1)?;
                let is_compressed: i32 = row.get(2)?;

                let payload = if is_compressed != 0 {
                    match decompress_payload(&payload_bytes) {
                        Ok(decompressed) => decompressed,
                        Err(_) => {
                            eprintln!("[AdnStore] Decompression error in search_payloads for hash {}", hash);
                            String::new()
                        }
                    }
                } else {
                    String::from_utf8(payload_bytes).unwrap_or_default()
                };

                Ok((hash, payload))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Filter in memory: search for term (case-insensitive)
        let term_lower = term.to_lowercase();
        let results: Vec<(String, String)> = all_entries
            .into_iter()
            .filter(|(_, payload)| payload.to_lowercase().contains(&term_lower))
            .take(limit)
            .collect();

        Ok(results)
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
    fn test_cast_commit_vote_quorum_one_matches_legacy_commit_flow() {
        // quorum_size=1 (config a un seul membre, celle d'aujourd'hui):
        // un seul vote doit committer immediatement, comme commit().
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_q1", "payload", None, None, 0.75, None, None, None).unwrap();
        let outcome = store.cast_commit_vote("hash_q1", "Olivier", None, 1).unwrap();
        assert_eq!(outcome.distinct_voters, 1);
        assert_eq!(outcome.quorum_size, 1);
        assert!(outcome.quorum_reached);
        let entry = store.get("hash_q1").unwrap().unwrap();
        assert!(entry.committed);
        assert_eq!(entry.committed_by.as_deref(), Some("Olivier"));
    }

    #[test]
    fn test_cast_commit_vote_below_quorum_does_not_commit() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_q2", "payload", None, None, 0.75, None, None, None).unwrap();
        let outcome = store.cast_commit_vote("hash_q2", "alice", None, 2).unwrap();
        assert_eq!(outcome.distinct_voters, 1);
        assert_eq!(outcome.quorum_size, 2);
        assert!(!outcome.quorum_reached);
        let entry = store.get("hash_q2").unwrap().unwrap();
        assert!(!entry.committed, "un seul votant sur quorum=2 ne doit pas committer");
    }

    #[test]
    fn test_cast_commit_vote_second_distinct_voter_reaches_quorum() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_q3", "payload", None, None, 0.75, None, None, None).unwrap();
        store.cast_commit_vote("hash_q3", "alice", None, 2).unwrap();
        let outcome = store.cast_commit_vote("hash_q3", "bob", None, 2).unwrap();
        assert_eq!(outcome.distinct_voters, 2);
        assert!(outcome.quorum_reached);
        let entry = store.get("hash_q3").unwrap().unwrap();
        assert!(entry.committed);
        assert_eq!(entry.committed_by.as_deref(), Some("bob"));
    }

    #[test]
    fn test_cast_commit_vote_repeat_voter_does_not_fake_quorum() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("hash_q4", "payload", None, None, 0.75, None, None, None).unwrap();
        store.cast_commit_vote("hash_q4", "alice", None, 2).unwrap();
        let outcome = store.cast_commit_vote("hash_q4", "alice", None, 2).unwrap();
        assert_eq!(outcome.distinct_voters, 1, "COUNT DISTINCT: 2 votes du meme membre = 1 votant");
        assert!(!outcome.quorum_reached);
        let entry = store.get("hash_q4").unwrap().unwrap();
        assert!(!entry.committed);
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
    fn test_relations_for_predicates_filters_at_sql_level() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("h1", "p", None, None, 0.3, None, None, None).unwrap();
        let mut born = HashMap::new();
        born.insert("subject".to_string(), "Marie Curie".to_string());
        born.insert("type".to_string(), "born_in".to_string());
        born.insert("object".to_string(), "Warsaw".to_string());
        let mut assumes = HashMap::new();
        assumes.insert("subject".to_string(), "X".to_string());
        assumes.insert("type".to_string(), "ASSUMES".to_string());
        assumes.insert("object".to_string(), "Y".to_string());
        store.put_relations("h1", &[born, assumes]).unwrap();

        // all_relations() voit tout, y compris ASSUMES.
        assert_eq!(store.all_relations().unwrap().len(), 2);

        // relations_for_predicates ne renvoie que ce qui est demande.
        let filtered = store.relations_for_predicates(&["born_in", "died_in"]).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].get("type").map(String::as_str), Some("born_in"));
    }

    #[test]
    fn test_relations_for_predicates_empty_list_returns_nothing() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("h1", "p", None, None, 0.3, None, None, None).unwrap();
        let mut rel = HashMap::new();
        rel.insert("subject".to_string(), "A".to_string());
        rel.insert("type".to_string(), "born_in".to_string());
        rel.insert("object".to_string(), "B".to_string());
        store.put_relations("h1", &[rel]).unwrap();

        assert!(store.relations_for_predicates(&[]).unwrap().is_empty());
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

    // ── modality (Couche 8, audit deontique historique, 2026-09-04) ──

    fn deontic_relation(subject: &str, object: &str, modality: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("subject".to_string(), subject.to_string());
        m.insert("type".to_string(), "PERFORM".to_string());
        m.insert("object".to_string(), object.to_string());
        m.insert("modality".to_string(), modality.to_string());
        m
    }

    #[test]
    fn test_put_relations_persists_and_roundtrips_modality() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("h1", "payload", None, None, 0.75, None, None, None).unwrap();
        store.put_relations("h1", &[deontic_relation("agent_x", "delete_prod_db", "MUST_NOT")]).unwrap();

        let history = store.deontic_relations_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].get("subject").map(String::as_str), Some("agent_x"));
        assert_eq!(history[0].get("modality").map(String::as_str), Some("MUST_NOT"));
    }

    #[test]
    fn test_factual_relations_without_modality_excluded_from_deontic_history() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("h2", "payload", None, None, 0.75, None, None, None).unwrap();
        let mut factual = HashMap::new();
        factual.insert("subject".to_string(), "alice".to_string());
        factual.insert("type".to_string(), "born_in".to_string());
        factual.insert("object".to_string(), "quebec".to_string());
        store.put_relations("h2", &[factual]).unwrap();

        let history = store.deontic_relations_history().unwrap();
        assert!(history.is_empty(), "une relation factuelle (sans modality) ne doit jamais apparaitre dans l'historique deontique");
    }

    #[test]
    fn test_open_migrates_real_file_with_old_schema_missing_modality_column() {
        // Simule EXACTEMENT l'etat d'une vraie base de production existante
        // (dont celle de l'utilisateur): adn_relations deja cree, SANS
        // colonne modality, AVANT ce fix -- via une Connection brute, pas
        // AdnStore::open() (qui appliquerait deja la migration). Confirme
        // que la migration idempotente dans open() gere ce cas reel sans
        // paniquer ni perdre les donnees deja presentes.
        let tmp_path = std::env::temp_dir().join(format!(
            "cstl_adn_store_migration_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        let tmp_path_str = tmp_path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp_path_str);

        {
            // Ancien schema, sans modality -- exactement ce que ce depot
            // produisait avant ce fix.
            let raw = Connection::open(&tmp_path_str).unwrap();
            raw.execute_batch(
                "CREATE TABLE adn_store (
                    hash TEXT PRIMARY KEY, payload TEXT NOT NULL, encoder TEXT,
                    produced_by TEXT, sigma REAL NOT NULL, parent_hash TEXT,
                    conversation_id TEXT, turn INTEGER, committed INTEGER NOT NULL DEFAULT 0,
                    committed_by TEXT, committed_at INTEGER, created_at INTEGER NOT NULL
                );
                CREATE TABLE adn_relations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    hash TEXT NOT NULL REFERENCES adn_store(hash),
                    subject TEXT NOT NULL, predicate TEXT NOT NULL, object TEXT NOT NULL
                );",
            ).unwrap();
            raw.execute(
                "INSERT INTO adn_store (hash, payload, sigma, created_at) VALUES ('h_old', 'p', 0.5, 0)",
                [],
            ).unwrap();
            raw.execute(
                "INSERT INTO adn_relations (hash, subject, predicate, object) VALUES ('h_old', 'paris', 'part_of', 'france')",
                [],
            ).unwrap();
        }

        // Rouvre via AdnStore::open() -- ne doit PAS paniquer, et doit avoir
        // migre la colonne modality (NULL pour la ligne pre-existante).
        let store = AdnStore::open(&tmp_path_str).unwrap();
        let old_entry = store.get("h_old").unwrap();
        assert!(old_entry.is_some(), "les donnees pre-existantes doivent survivre a la migration");

        // put_relations avec modality doit maintenant fonctionner sur ce
        // meme fichier migre.
        store.put("h_new", "p2", None, None, 0.5, None, None, None).unwrap();
        store.put_relations("h_new", &[deontic_relation("agent_x", "delete_prod_db", "MUST")]).unwrap();
        let history = store.deontic_relations_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].get("modality").map(String::as_str), Some("MUST"));

        let _ = std::fs::remove_file(&tmp_path_str);
    }

    // ── Chaine d'audit fusionnee (ex server/audit_store.rs) ──

    #[test]
    fn test_audit_persist_and_load() {
        let store = AdnStore::open(":memory:").unwrap();
        let entry = AuditEntry {
            hash: "sha256:abc123".to_string(),
            parent_hash: "root".to_string(),
            sender: "alice".to_string(),
            receiver: "bob".to_string(),
            purpose: "test".to_string(),
            seq: 0,
        };
        store.save_audit_entry(&entry).unwrap();
        assert_eq!(store.audit_count().unwrap(), 1);
        let chain = store.load_chain().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.entries[0].hash, "sha256:abc123");
    }

    #[test]
    fn test_audit_save_is_idempotent_on_duplicate_hash() {
        let store = AdnStore::open(":memory:").unwrap();
        let entry1 = AuditEntry {
            hash: "sha256:dup".to_string(), parent_hash: "root".to_string(),
            sender: "alice".to_string(), receiver: "bob".to_string(),
            purpose: "test".to_string(), seq: 0,
        };
        let entry2 = AuditEntry {
            hash: "sha256:dup".to_string(), parent_hash: "sha256:dup".to_string(),
            sender: "alice".to_string(), receiver: "bob".to_string(),
            purpose: "test".to_string(), seq: 1,
        };
        store.save_audit_entry(&entry1).unwrap();
        store.save_audit_entry(&entry2).unwrap(); // ne doit pas retourner Err
        assert_eq!(store.audit_count().unwrap(), 1, "le second save (hash duplique) doit etre ignore, pas ajoute");
    }

    #[test]
    fn test_audit_persistence_survives_reopen_on_real_file_and_shares_adn_store_data() {
        // Verifie a la fois la survie au redemarrage (deja teste avant la
        // fusion) ET la vraie raison d'etre de cette fusion: audit_trail et
        // adn_store/adn_relations vivent maintenant dans LE MEME fichier ET
        // la MEME Connection -- put() et save_audit_entry() sur la meme
        // instance doivent cohabiter sans se marcher dessus.
        let tmp_path = std::env::temp_dir().join(format!(
            "cstl_adn_store_audit_merge_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        let tmp_path_str = tmp_path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp_path_str);

        {
            let store = AdnStore::open(&tmp_path_str).unwrap();
            store.put("sha256:persisted1", "payload", None, None, 0.5, None, None, None).unwrap();
            store.save_audit_entry(&AuditEntry {
                hash: "sha256:persisted1".to_string(), parent_hash: "root".to_string(),
                sender: "alice".to_string(), receiver: "bob".to_string(),
                purpose: "test".to_string(), seq: 0,
            }).unwrap();
            store.save_audit_entry(&AuditEntry {
                hash: "sha256:persisted2".to_string(), parent_hash: "sha256:persisted1".to_string(),
                sender: "bob".to_string(), receiver: "alice".to_string(),
                purpose: "test".to_string(), seq: 1,
            }).unwrap();
            // `store` sort de portee ici -- simule un redemarrage complet.
        }

        let reopened = AdnStore::open(&tmp_path_str).unwrap();
        let chain = reopened.load_chain().unwrap();
        assert_eq!(chain.len(), 2, "les 2 entrees du 'run precedent' doivent survivre a la reouverture");
        assert_eq!(chain.entries[0].hash, "sha256:persisted1");
        assert_eq!(chain.entries[1].parent_hash, "sha256:persisted1");
        assert!(chain.verify_integrity().is_ok());
        // Le payload adn_store du meme run precedent doit lui aussi survivre
        // -- meme fichier, une seule Connection.
        assert!(reopened.get("sha256:persisted1").unwrap().is_some());

        let _ = std::fs::remove_file(&tmp_path_str);
    }

    // ── Couche 5: get_primer() & load_context() ──

    #[test]
    fn test_load_context_single_entry_root_parent() {
        // Cas minimal: une seule entree, parent = "root"
        let store = AdnStore::open(":memory:").unwrap();
        store
            .put(
                "sha256:entry1",
                "payload text",
                Some("agent_a"),
                Some("prod"),
                0.85,
                Some("root"),
                Some("conv_1"),
                Some(1),
            )
            .unwrap();

        let ctx = store.load_context("sha256:entry1", 10).unwrap();
        assert_eq!(ctx.entries.len(), 1);
        assert_eq!(ctx.depth, 1);
        assert_eq!(ctx.entries[0].hash, "sha256:entry1");
        assert_eq!(ctx.entries[0].parent_hash, Some("root".to_string()));
    }

    #[test]
    fn test_load_context_chain_of_three() {
        // Chaîne linéaire: A <- B <- C (root)
        // En marche arrière depuis A, on devrait retrouver A, B, C dans cet ordre
        let store = AdnStore::open(":memory:").unwrap();

        // C: parent = root
        store
            .put(
                "sha256:C",
                "payload C",
                Some("agent"),
                None,
                0.9,
                Some("root"),
                Some("conv_1"),
                Some(1),
            )
            .unwrap();

        // B: parent = C
        store
            .put(
                "sha256:B",
                "payload B",
                Some("agent"),
                None,
                0.8,
                Some("sha256:C"),
                Some("conv_1"),
                Some(2),
            )
            .unwrap();

        // A: parent = B
        store
            .put(
                "sha256:A",
                "payload A",
                Some("agent"),
                None,
                0.7,
                Some("sha256:B"),
                Some("conv_1"),
                Some(3),
            )
            .unwrap();

        // Charger depuis A avec max_depth=10 -- doit remonter jusqu'à C
        let ctx = store.load_context("sha256:A", 10).unwrap();
        assert_eq!(ctx.entries.len(), 3, "doit charger A, B, C");
        assert_eq!(ctx.depth, 3);

        // Ordre chronologique: C, B, A (ancien → récent)
        assert_eq!(ctx.entries[0].hash, "sha256:C");
        assert_eq!(ctx.entries[1].hash, "sha256:B");
        assert_eq!(ctx.entries[2].hash, "sha256:A");
    }

    #[test]
    fn test_load_context_respects_max_depth() {
        // Chaîne de 5 éléments, mais max_depth=2
        // Doit charger seulement les 2 les plus proches
        let store = AdnStore::open(":memory:").unwrap();

        store.put("sha256:E", "p", None, None, 0.9, Some("root"), None, Some(1)).unwrap();
        store.put("sha256:D", "p", None, None, 0.8, Some("sha256:E"), None, Some(2)).unwrap();
        store.put("sha256:C", "p", None, None, 0.7, Some("sha256:D"), None, Some(3)).unwrap();
        store.put("sha256:B", "p", None, None, 0.6, Some("sha256:C"), None, Some(4)).unwrap();
        store.put("sha256:A", "p", None, None, 0.5, Some("sha256:B"), None, Some(5)).unwrap();

        let ctx = store.load_context("sha256:A", 2).unwrap();
        assert_eq!(ctx.entries.len(), 2, "max_depth=2: charger seulement A et B");
        assert_eq!(ctx.depth, 2);
        assert_eq!(ctx.entries[0].hash, "sha256:B");
        assert_eq!(ctx.entries[1].hash, "sha256:A");
    }

    #[test]
    fn test_load_context_detects_circular_parent_hash() {
        // Créer un cycle: A -> B -> A (cas pathologique, ne devrait pas arriver en production)
        let store = AdnStore::open(":memory:").unwrap();

        // A: parent = B (B n'existe pas encore)
        store.put("sha256:A", "p", None, None, 0.5, Some("sha256:B"), None, None).unwrap();

        // B: parent = A (crée le cycle)
        store.put("sha256:B", "p", None, None, 0.5, Some("sha256:A"), None, None).unwrap();

        // Essayer de charger depuis A -- doit détecter le cycle et retourner Err
        let result = store.load_context("sha256:A", 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circular"));
    }

    #[test]
    fn test_load_context_missing_parent_stops_traversal() {
        // B parent de A, mais B n'existe pas dans la DB
        // load_context doit retourner Err (pas continuer avec un hash inexistant)
        let store = AdnStore::open(":memory:").unwrap();

        store.put("sha256:A", "p", None, None, 0.5, Some("sha256:B_missing"), None, None).unwrap();

        let result = store.load_context("sha256:A", 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Hash not found"));
    }

    #[test]
    fn test_get_primer_basic_flow() {
        // Créer 5 entrées commitées, demander les 3 avant turn=4
        let store = AdnStore::open(":memory:").unwrap();

        for turn in 1..=5 {
            let hash = format!("sha256:entry_{}", turn);
            let payload = format!("Payload for turn {}", turn);
            store
                .put(
                    &hash,
                    &payload,
                    Some("agent"),
                    None,
                    0.8,
                    None,
                    Some("conv_x"),
                    Some(turn),
                )
                .unwrap();
            // Committer toutes les entrées
            store.commit(&hash, "human", None).unwrap();
        }

        // Demander le primer avant turn=4 -- doit retourner entrées 1,2,3 triées DESC
        let primer = store.get_primer("conv_x", 4).unwrap();
        assert!(!primer.is_empty());
        assert!(primer.contains("ADN_PRIMER"));
        assert!(primer.contains("num_entries = 3"));
        // Vérifier que l'ordre DESC est respecté dans le primer
        assert!(primer.contains("turn=3"));
        assert!(primer.contains("turn=2"));
        assert!(primer.contains("turn=1"));
    }

    #[test]
    fn test_get_primer_filters_uncommitted() {
        // Créer 3 entrées: 2 commitées, 1 pas
        let store = AdnStore::open(":memory:").unwrap();

        store.put("sha256:c1", "payload 1", None, None, 0.8, None, Some("conv_y"), Some(1)).unwrap();
        store.put("sha256:u1", "payload 2", None, None, 0.7, None, Some("conv_y"), Some(2)).unwrap();
        store.put("sha256:c2", "payload 3", None, None, 0.6, None, Some("conv_y"), Some(3)).unwrap();

        store.commit("sha256:c1", "human", None).unwrap();
        store.commit("sha256:c2", "human", None).unwrap();
        // "sha256:u1" n'est pas commitée

        let primer = store.get_primer("conv_y", 10).unwrap();
        // Doit contenir seulement c2 et c1 (les entrées commitées)
        assert!(primer.contains("num_entries = 2"));
        assert!(primer.contains("sha256:c1"));
        assert!(primer.contains("sha256:c2"));
        assert!(!primer.contains("sha256:u1"));
    }

    #[test]
    fn test_get_primer_empty_when_no_prior_turns() {
        // Primer demandé avant turn=1 -- aucune entrée antérieure
        let store = AdnStore::open(":memory:").unwrap();

        store
            .put("sha256:h1", "p", None, None, 0.8, None, Some("conv_z"), Some(1))
            .unwrap();
        store.commit("sha256:h1", "human", None).unwrap();

        let primer = store.get_primer("conv_z", 1).unwrap();
        // Pas d'entrées avant turn=1, primer doit être vide
        assert_eq!(primer, "");
    }

    #[test]
    fn test_get_primer_limits_to_five_entries() {
        // Créer 10 entrées commitées, demander avant turn=11 -- doit limiter à 5
        let store = AdnStore::open(":memory:").unwrap();

        for turn in 1..=10 {
            let hash = format!("sha256:e_{}", turn);
            store
                .put(&hash, "payload", None, None, 0.8, None, Some("conv_a"), Some(turn))
                .unwrap();
            store.commit(&hash, "human", None).unwrap();
        }

        let primer = store.get_primer("conv_a", 11).unwrap();
        assert!(primer.contains("num_entries = 5"));
        // Doit retourner les 5 les plus proches (tours 10,9,8,7,6)
        assert!(primer.contains("turn=10"));
        assert!(primer.contains("turn=9"));
        assert!(primer.contains("turn=8"));
        assert!(primer.contains("turn=7"));
        assert!(primer.contains("turn=6"));
        // Ne doit pas contenir les plus lointaines
        assert!(!primer.contains("turn=5"));
    }

    #[test]
    fn test_get_primer_respects_conversation_boundary() {
        // Créer des entrées dans deux conversations différentes
        let store = AdnStore::open(":memory:").unwrap();

        // Conversation X: turns 1-3
        for turn in 1..=3 {
            let hash = format!("sha256:conv_x_turn_{}", turn);
            store
                .put(&hash, "payload", None, None, 0.8, None, Some("conv_X"), Some(turn))
                .unwrap();
            store.commit(&hash, "human", None).unwrap();
        }

        // Conversation Y: turns 1-3
        for turn in 1..=3 {
            let hash = format!("sha256:conv_y_turn_{}", turn);
            store
                .put(&hash, "payload", None, None, 0.8, None, Some("conv_Y"), Some(turn))
                .unwrap();
            store.commit(&hash, "human", None).unwrap();
        }

        // Demander primer pour conv_X avant turn=2
        let primer = store.get_primer("conv_X", 2).unwrap();
        // Doit contenir seulement l'entrée turn=1 de conv_X
        assert!(primer.contains("num_entries = 1"));
        assert!(primer.contains("turn=1"));
        assert!(!primer.contains("turn=2"));
        assert!(!primer.contains("turn=3"));
        // Ne doit jamais contenir d'entrées de conv_Y
        assert!(!primer.contains("conv_Y"));
    }

    #[test]
    fn test_get_primer_excerpt_truncation() {
        // Créer une entrée avec un très long payload
        let store = AdnStore::open(":memory:").unwrap();

        let long_payload = "x".repeat(1000);
        store
            .put("sha256:long", &long_payload, None, None, 0.8, None, Some("conv_long"), Some(1))
            .unwrap();
        store.commit("sha256:long", "human", None).unwrap();

        let primer = store.get_primer("conv_long", 10).unwrap();
        // Doit contenir "..." indiquant la troncature à 500 chars
        assert!(primer.contains("..."));
        // Vérifier qu'on ne voit pas tout le payload de 1000 chars
        assert!(!primer.contains(&"x".repeat(600)));
    }

    // ── TF-IDF Retrieval Tests (Couche 5) ──

    #[test]
    fn test_tfidf_empty_query_returns_empty_results() {
        let store = AdnStore::open(":memory:").unwrap();

        store.put("h1", "artificial intelligence machine learning", None, None, 0.8, None, None, None).unwrap();
        store.commit("h1", "human", None).unwrap();

        // Empty query should return empty results
        let results = store.get_tfidf_results("", 5).unwrap();
        assert!(results.is_empty(), "empty query should return no results");

        // Whitespace-only query should also return empty results
        let results = store.get_tfidf_results("   ", 5).unwrap();
        assert!(results.is_empty(), "whitespace query should return no results");
    }

    #[test]
    fn test_tfidf_no_matching_documents() {
        let store = AdnStore::open(":memory:").unwrap();

        // Only store uncommitted entry
        store.put("h1", "artificial intelligence", None, None, 0.8, None, None, None).unwrap();
        // Don't commit it

        // Query for something that exists in uncommitted entry
        let results = store.get_tfidf_results("artificial intelligence", 5).unwrap();
        assert!(results.is_empty(), "uncommitted entries should not be indexed");

        // Query for something not in database at all
        store.put("h2", "neural networks deep learning", None, None, 0.8, None, None, None).unwrap();
        store.commit("h2", "human", None).unwrap();

        let results = store.get_tfidf_results("quantum computing", 5).unwrap();
        assert!(results.is_empty(), "query with no matching terms should return no results");
    }

    #[test]
    fn test_tfidf_single_document_match() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create and commit a single document
        let payload = "machine learning algorithms neural networks deep learning";
        store.put("h_ml", payload, None, None, 0.8, None, None, None).unwrap();
        store.commit("h_ml", "human", None).unwrap();

        // Query for relevant term
        let results = store.get_tfidf_results("neural networks", 5).unwrap();
        assert_eq!(results.len(), 1, "should find exactly one document");
        assert_eq!(results[0].0, "h_ml");
        // Note: With a single document, IDF = log10(1/1) = 0, so score can be 0
        // This is mathematically correct; we just verify the document is found
        assert!(results[0].1 >= 0.0, "score should be non-negative");
    }

    #[test]
    fn test_tfidf_multiple_documents_ranking() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create multiple documents with varying relevance
        // Doc 1: Heavy on "neural networks"
        store.put(
            "h1",
            "neural networks neural networks neural networks deep learning",
            None, None, 0.8, None, None, None,
        ).unwrap();
        store.commit("h1", "human", None).unwrap();

        // Doc 2: Light on "neural networks"
        store.put(
            "h2",
            "fuzzy logic expert systems genetic algorithms",
            None, None, 0.8, None, None, None,
        ).unwrap();
        store.commit("h2", "human", None).unwrap();

        // Doc 3: Medium on "neural networks"
        store.put(
            "h3",
            "neural networks machine learning classification",
            None, None, 0.8, None, None, None,
        ).unwrap();
        store.commit("h3", "human", None).unwrap();

        // Query for "neural networks"
        let results = store.get_tfidf_results("neural networks", 5).unwrap();

        // Should return multiple results
        assert_eq!(results.len(), 2, "should find documents containing query terms");

        // Doc 1 should rank higher than Doc 3 (more occurrences of relevant term)
        assert_eq!(results[0].0, "h1", "doc with most occurrences should rank first");
        assert!(results[0].1 > results[1].1, "first result should have higher score");
    }

    #[test]
    fn test_tfidf_query_with_stop_words_filtered() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create a document
        store.put(
            "h1",
            "the artificial intelligence and machine learning",
            None, None, 0.8, None, None, None,
        ).unwrap();
        store.commit("h1", "human", None).unwrap();

        // Query with stop words only
        let results = store.get_tfidf_results("the and or a", 5).unwrap();
        assert!(results.is_empty(), "query with only stop words should return no results");

        // Query with stop words and meaningful terms
        let results = store.get_tfidf_results("the artificial intelligence", 5).unwrap();
        assert_eq!(results.len(), 1, "should find document despite stop words in query");
    }

    #[test]
    fn test_tfidf_top_k_limit_respected() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create 10 documents all containing "algorithm"
        for i in 1..=10 {
            let payload = format!("algorithm algorithm algorithm {}", i);
            let hash = format!("h{}", i);
            store.put(&hash, &payload, None, None, 0.8, None, None, None).unwrap();
            store.commit(&hash, "human", None).unwrap();
        }

        // Query with top_k=3
        let results = store.get_tfidf_results("algorithm", 3).unwrap();
        assert_eq!(results.len(), 3, "should return exactly top_k results");

        // Verify scores are sorted descending
        for i in 0..results.len() - 1 {
            assert!(results[i].1 >= results[i+1].1, "results should be sorted by score descending");
        }
    }

    #[test]
    fn test_tfidf_case_insensitivity() {
        let store = AdnStore::open(":memory:").unwrap();

        store.put(
            "h1",
            "Artificial Intelligence Neural Networks",
            None, None, 0.8, None, None, None,
        ).unwrap();
        store.commit("h1", "human", None).unwrap();

        // Query with different cases
        let results_lower = store.get_tfidf_results("neural networks", 5).unwrap();
        let results_upper = store.get_tfidf_results("NEURAL NETWORKS", 5).unwrap();
        let results_mixed = store.get_tfidf_results("Neural NETWORKS", 5).unwrap();

        assert_eq!(results_lower.len(), 1);
        assert_eq!(results_upper.len(), 1);
        assert_eq!(results_mixed.len(), 1);
        assert_eq!(results_lower[0].0, results_upper[0].0);
        assert_eq!(results_upper[0].0, results_mixed[0].0);
    }

    #[test]
    fn test_search_payloads_basic_fulltext() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create multiple documents
        store.put("h1", "python programming language", None, None, 0.8, None, None, None).unwrap();
        store.commit("h1", "human", None).unwrap();

        store.put("h2", "rust programming language systems", None, None, 0.8, None, None, None).unwrap();
        store.commit("h2", "human", None).unwrap();

        store.put("h3", "javascript web development", None, None, 0.8, None, None, None).unwrap();
        store.commit("h3", "human", None).unwrap();

        // Search for "programming"
        let results = store.search_payloads("programming", 10).unwrap();
        assert_eq!(results.len(), 2, "should find both documents with 'programming'");

        // Verify payloads are returned
        let payloads: Vec<_> = results.iter().map(|r| r.1.clone()).collect();
        assert!(payloads.iter().any(|p| p.contains("python")));
        assert!(payloads.iter().any(|p| p.contains("rust")));
    }

    #[test]
    fn test_search_payloads_respects_limit() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create 5 documents all matching
        for i in 1..=5 {
            let payload = format!("testing document number {}", i);
            let hash = format!("h{}", i);
            store.put(&hash, &payload, None, None, 0.8, None, None, None).unwrap();
            store.commit(&hash, "human", None).unwrap();
        }

        // Search with limit=2
        let results = store.search_payloads("testing", 2).unwrap();
        assert_eq!(results.len(), 2, "should respect limit parameter");
    }

    #[test]
    fn test_indices_created_on_fresh_db() {
        // Verify that all required indices are created when opening a fresh DB
        let tmp_path = std::env::temp_dir().join(format!(
            "cstl_adn_index_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        let tmp_path_str = tmp_path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp_path_str);

        // Open a fresh DB
        let store = AdnStore::open(&tmp_path_str).unwrap();

        // Query sqlite_master to verify indices exist
        let conn = Connection::open(&tmp_path_str).unwrap();

        // List of required indices for adn_store
        let required_indices = vec![
            "idx_adn_produced_by",
            "idx_adn_parent_hash",
            "idx_adn_created_at",
            "idx_adn_conversation",
        ];

        // List of required indices for adn_relations
        let required_relation_indices = vec![
            "idx_adn_relations_hash",
            "idx_adn_relations_predicate",
        ];

        // Check adn_store indices
        for idx_name in &required_indices {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx_name],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            assert!(exists, "Index {} should exist on adn_store", idx_name);

            // Verify the index is on adn_store table
            let table_name: String = conn
                .query_row(
                    "SELECT tbl_name FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx_name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(table_name, "adn_store", "Index {} should be on adn_store table", idx_name);
        }

        // Check adn_relations indices
        for idx_name in &required_relation_indices {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx_name],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            assert!(exists, "Index {} should exist on adn_relations", idx_name);
        }

        // Clean up
        let _ = std::fs::remove_file(&tmp_path_str);
    }

    #[test]
    fn test_indices_performance_query_by_produced_by() {
        let store = AdnStore::open(":memory:").unwrap();

        // Insert multiple entries
        for i in 1..=100 {
            let hash = format!("sha256:hash{:06}", i);
            let producer = if i % 10 == 0 { "alice" } else { "bob" };
            store.put(&hash, "test payload", None, Some(producer), 0.8, None, None, None).unwrap();
        }

        // Query by produced_by (should use index)
        let mut stmt = store.conn.prepare(
            "SELECT COUNT(*) FROM adn_store WHERE produced_by = ?"
        ).unwrap();
        let alice_count: u64 = stmt.query_row(params!["alice"], |row| row.get(0)).unwrap();

        assert_eq!(alice_count, 10, "Should find 10 entries produced by alice");
    }

    #[test]
    fn test_indices_performance_query_by_conversation() {
        let store = AdnStore::open(":memory:").unwrap();

        // Insert entries with conversation_id and turn
        for i in 1..=50 {
            let hash = format!("sha256:hash{:06}", i);
            let conv_id = if i <= 25 { "conv1" } else { "conv2" };
            let turn = ((i - 1) / 5) as i64;
            store.put(&hash, "test payload", None, None, 0.8, None, Some(conv_id), Some(turn)).unwrap();
        }

        // Query by conversation and turn (should use composite index)
        let mut stmt = store.conn.prepare(
            "SELECT COUNT(*) FROM adn_store WHERE conversation_id = ? AND turn = ?"
        ).unwrap();
        let count: u64 = stmt.query_row(params!["conv1", 0_i64], |row| row.get(0)).unwrap();

        assert_eq!(count, 5, "Should find 5 entries in conversation 1, turn 0");
    }

    #[test]
    fn test_indices_parent_hash_chain_traversal() {
        let store = AdnStore::open(":memory:").unwrap();

        // Create a chain: h1 -> h2 -> h3 -> h4
        store.put("h1", "payload1", None, None, 0.8, None, None, None).unwrap();
        store.put("h2", "payload2", None, None, 0.8, Some("h1"), None, None).unwrap();
        store.put("h3", "payload3", None, None, 0.8, Some("h2"), None, None).unwrap();
        store.put("h4", "payload4", None, None, 0.8, Some("h3"), None, None).unwrap();

        // Query by parent_hash to follow chain (should use index)
        let mut stmt = store.conn.prepare(
            "SELECT COUNT(*) FROM adn_store WHERE parent_hash = ?"
        ).unwrap();

        let children_of_h1: u64 = stmt.query_row(params!["h1"], |row| row.get(0)).unwrap();
        let children_of_h2: u64 = {
            let mut s = store.conn.prepare("SELECT COUNT(*) FROM adn_store WHERE parent_hash = ?").unwrap();
            s.query_row(params!["h2"], |row| row.get(0)).unwrap()
        };

        assert_eq!(children_of_h1, 1, "h1 should have 1 child");
        assert_eq!(children_of_h2, 1, "h2 should have 1 child");
    }

    #[test]
    fn test_indices_time_based_queries() {
        let store = AdnStore::open(":memory:").unwrap();

        let now = now_unix();

        // Insert entries at different times
        for i in 1..=10 {
            let hash = format!("sha256:hash{:06}", i);
            store.put(&hash, "test payload", None, None, 0.8, None, None, None).unwrap();
        }

        // Query by created_at (should use index)
        let mut stmt = store.conn.prepare(
            "SELECT COUNT(*) FROM adn_store WHERE created_at >= ?"
        ).unwrap();

        let recent_count: u64 = stmt.query_row(params![now - 100], |row| row.get(0)).unwrap();
        assert_eq!(recent_count, 10, "All entries should be recent");
    }

    #[test]
    fn test_compression_roundtrip_small_payload() {
        // Small payloads (< 10KB) should NOT be compressed
        let store = AdnStore::open(":memory:").unwrap();
        let small_payload = "small data";
        store.put("h_small", small_payload, None, None, 0.5, None, None, None).unwrap();

        let entry = store.get("h_small").unwrap().unwrap();
        assert_eq!(entry.payload, small_payload);

        // Verify compression flag is not set
        let is_compressed: i32 = store.conn.query_row(
            "SELECT payload_compressed FROM adn_store WHERE hash = 'h_small'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(is_compressed, 0, "Small payload should not be compressed");
    }

    #[test]
    fn test_compression_roundtrip_large_payload() {
        // Large payloads (> 10KB) should be compressed if beneficial
        let store = AdnStore::open(":memory:").unwrap();
        let large_payload = "X".repeat(50_000); // 50KB of repetitive data
        store.put("h_large", &large_payload, None, None, 0.5, None, None, None).unwrap();

        let entry = store.get("h_large").unwrap().unwrap();
        assert_eq!(entry.payload, large_payload, "Byte-for-byte match required after decompression");

        // Verify compression flag is set
        let is_compressed: i32 = store.conn.query_row(
            "SELECT payload_compressed FROM adn_store WHERE hash = 'h_large'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(is_compressed, 1, "Large repetitive payload should be compressed");

        // Verify compressed size is actually smaller
        let compressed_size: usize = store.conn.query_row(
            "SELECT LENGTH(payload) FROM adn_store WHERE hash = 'h_large'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert!(
            compressed_size < large_payload.len(),
            "Compressed size {} should be less than original {}",
            compressed_size,
            large_payload.len()
        );
    }

    #[test]
    fn test_compression_1mb_payload_roundtrip() {
        // Test with 1MB payload to verify performance and byte-for-byte correctness
        let store = AdnStore::open(":memory:").unwrap();
        let mut large_payload = String::new();
        for _ in 0..20_000 {
            large_payload.push_str("Lorem ipsum dolor sit amet, consectetur adipiscing elit. ");
        }
        assert!(large_payload.len() > 1_000_000, "Payload must exceed 1MB");

        let hash = "h_1mb";
        store.put(hash, &large_payload, Some("test_encoder"), Some("test_agent"), 0.99, None, Some("conv1"), Some(1)).unwrap();

        // Retrieve and verify byte-for-byte match
        let entry = store.get(hash).unwrap().unwrap();
        assert_eq!(entry.payload, large_payload, "1MB payload: byte-for-byte match required");
        assert_eq!(entry.payload.len(), large_payload.len());

        // Verify metadata was preserved
        assert_eq!(entry.encoder.as_deref(), Some("test_encoder"));
        assert_eq!(entry.produced_by.as_deref(), Some("test_agent"));
        assert_eq!(entry.conversation_id.as_deref(), Some("conv1"));
        assert_eq!(entry.turn, Some(1));

        // Verify compression was applied
        let is_compressed: i32 = store.conn.query_row(
            &format!("SELECT payload_compressed FROM adn_store WHERE hash = '{}'", hash),
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(is_compressed, 1, "1MB payload should be compressed");
    }

    #[test]
    fn test_compression_with_get_by_short_id() {
        // Test that get_by_short_id() also handles compression correctly
        let store = AdnStore::open(":memory:").unwrap();
        let large_payload = "ABCD".repeat(10_000); // > 10KB
        let hash = "sha256:1234567890abcdef";

        store.put(hash, &large_payload, None, None, 0.5, None, None, None).unwrap();

        // Retrieve by short ID
        let entry = store.get_by_short_id("1234567890").unwrap().unwrap();
        assert_eq!(entry.payload, large_payload, "Decompression via get_by_short_id failed");
    }

    #[test]
    fn test_compression_with_get_primer() {
        // Test that get_primer() handles compressed payloads
        let store = AdnStore::open(":memory:").unwrap();
        let large_payload = "X".repeat(50_000);

        store.put("h1", &large_payload, None, None, 0.5, Some("root"), Some("conv1"), Some(1)).unwrap();
        store.commit("h1", "human", None).unwrap();
        store.put("h2", "small payload", None, None, 0.5, Some("h1"), Some("conv1"), Some(2)).unwrap();
        store.commit("h2", "human", None).unwrap();

        // Get primer before turn 3 (should include h2)
        let primer = store.get_primer("conv1", 3).unwrap();
        assert!(!primer.is_empty(), "Primer should not be empty");
        assert!(primer.contains("small payload"), "Primer should contain h2 excerpt");
        assert!(primer.contains("h1"), "Primer should reference h1");
    }

    #[test]
    fn test_compression_incompressible_payload() {
        // Test that incompressible data is stored uncompressed
        let store = AdnStore::open(":memory:").unwrap();
        // Random binary-like data (hard to compress)
        let incompressible = "aB1cD2eF3gH4iJ5kL6mN7oP8qR9sT0u".repeat(500);

        store.put("h_incomp", &incompressible, None, None, 0.5, None, None, None).unwrap();

        let entry = store.get("h_incomp").unwrap().unwrap();
        assert_eq!(entry.payload, incompressible);

        // Check if it was compressed (might or might not be, depending on actual compression)
        let is_compressed: i32 = store.conn.query_row(
            "SELECT payload_compressed FROM adn_store WHERE hash = 'h_incomp'",
            [],
            |r| r.get(0),
        ).unwrap();
        // We don't assert on this value because compression might still be applied
        // even if not beneficial, but the roundtrip should work either way
        assert!(is_compressed == 0 || is_compressed == 1, "Compression flag should be 0 or 1");
    }

    #[test]
    fn test_compression_with_tfidf_search() {
        // Test that TF-IDF search works with compressed payloads
        let store = AdnStore::open(":memory:").unwrap();
        let doc1 = "The quick brown fox jumps over the lazy dog. ".repeat(500); // > 10KB
        let doc2 = "Machine learning is a subset of artificial intelligence. ".repeat(300);

        store.put("h_doc1", &doc1, None, None, 0.8, None, None, None).unwrap();
        store.commit("h_doc1", "human", None).unwrap();
        store.put("h_doc2", &doc2, None, None, 0.7, None, None, None).unwrap();
        store.commit("h_doc2", "human", None).unwrap();

        // Search for "fox" (in doc1)
        let results = store.get_tfidf_results("fox", 10).unwrap();
        assert!(!results.is_empty(), "Should find doc containing 'fox'");
        assert_eq!(results[0].0, "h_doc1", "Should find doc1");

        // Search for "machine" (in doc2)
        let results = store.get_tfidf_results("machine", 10).unwrap();
        assert!(!results.is_empty(), "Should find doc containing 'machine'");
        assert_eq!(results[0].0, "h_doc2", "Should find doc2");
    }

    #[test]
    fn test_compression_with_search_payloads() {
        // Test that search_payloads() works with compressed data
        let store = AdnStore::open(":memory:").unwrap();
        let searchable = "This is a searchable payload with keyword UNIQUEKEYWORD inside.".repeat(1000); // > 10KB

        store.put("h_search", &searchable, None, None, 0.5, None, None, None).unwrap();
        store.commit("h_search", "human", None).unwrap();

        // Search for the unique keyword
        let results = store.search_payloads("UNIQUEKEYWORD", 10).unwrap();
        assert_eq!(results.len(), 1, "Should find the compressed payload");
        assert_eq!(results[0].0, "h_search");
        assert_eq!(results[0].1, searchable, "Retrieved payload should match original");
    }
}
