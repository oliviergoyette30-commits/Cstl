/// Persistent Audit Store
/// Sauvegarde la chaîne dans SQLite, la charge au démarrage.
///
/// Trouvaille honnête (audit Couche 5, 2026-09-04): ce module était
/// ENTIÈREMENT du code mort avant cette passe -- `AuditStore::open` n'était
/// appelé NULLE PART en dehors de son propre test (`cfg(test)` ci-dessous).
/// Conséquence réelle, pas théorique: `CstlNativeServer` construisait
/// `chain: HashChain::new()` (vide, purement en mémoire) à CHAQUE démarrage,
/// alors que `adn_store.rs` persistait déjà les payloads (avec leur
/// `parent_hash`) dans `cstl_adn.db` sur disque. Un redémarrage réel
/// remettait donc `seq` à 0 et `parent_hash` à "root" pour le prochain
/// payload, alors que l'historique pré-redémarrage restait sur disque avec
/// sa propre lignée -- une rupture silencieuse de la garantie de
/// "provenance immuable en chaîne de hachage" (Couche 8) que rien ne
/// signalait. Corrigé: `CstlNativeServer::new`/`with_data_path`
/// (`server/mod.rs`) ouvre maintenant ce store et charge la chaîne
/// persistée au démarrage; `server/handler.rs` appelle `save()` a chaque
/// entree ajoutee. Vérifié en direct par un redémarrage réel du serveur
/// (`examples/audit_persistence_smoke_test.rs`), pas seulement par le test
/// unitaire ci-dessous qui existait déjà.
///
/// Portée assumée, pas encore faite: ceci reste une DEUXIEME connexion
/// SQLite (table `audit_trail`) pointée sur le MEME fichier que
/// `adn_store.rs` (même `path`, deux `Connection` distinctes) -- un
/// rapprochement réel ("un seul fichier" au lieu de deux systèmes de
/// stockage disjoints), mais pas encore la fusion complète en un seul
/// schéma/une seule `Connection`/un seul verrou que la formulation
/// originale du README visait ("pas encore unifiée dans un seul schéma").
/// Cette fusion plus profonde reste à faire si le besoin se confirme.
use rusqlite::{Connection, params};
use super::audit::{HashChain, AuditEntry};

pub struct AuditStore {
    conn: Connection,
}

impl AuditStore {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS audit_trail (
                seq INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                parent_hash TEXT NOT NULL,
                sender TEXT NOT NULL,
                receiver TEXT NOT NULL,
                purpose TEXT NOT NULL
            )",
            [],
        )?;

        eprintln!("[AuditStore] Initialized at {}", path);
        Ok(AuditStore { conn })
    }

    /// Sauvegarde une entrée (append-only). `INSERT OR IGNORE`, pas `INSERT`
    /// brut: un payload de contenu identique (donc de même `canonical_hash`)
    /// soumis deux fois doit se comporter comme dans `adn_store.put`
    /// (idempotent, silencieux) -- avant ce fix, `HashChain::append` en
    /// mémoire n'appliquait de toute façon AUCUNE déduplication (il pousse
    /// deux `AuditEntry` avec le même hash mais des `seq` différents sans se
    /// plaindre), donc un `INSERT` strict ici aurait fait planter la
    /// persistance (violation de la contrainte `UNIQUE` sur `hash`) dès le
    /// premier renvoi d'un payload identique -- un vrai smoke-test l'a
    /// confirmé avant ce correctif.
    pub fn save(&self, entry: &AuditEntry) -> Result<(), Box<dyn std::error::Error>> {
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
        // rows_affected==0 signifie que OR IGNORE a bel et bien ignore la
        // ligne (hash deja present) -- log honnete plutot que "Persisted"
        // dans les deux cas, ce qui aurait laisse croire qu'une ecriture a
        // reellement eu lieu la ou rien n'a change sur disque.
        if rows_affected > 0 {
            eprintln!("[AuditStore] Persisted seq={}", entry.seq);
        } else {
            eprintln!("[AuditStore] seq={} ignore (hash {} deja present)", entry.seq, entry.hash);
        }
        Ok(())
    }

    /// Charge toute la chaîne depuis la base
    pub fn load_chain(&self) -> Result<HashChain, Box<dyn std::error::Error>> {
        let mut chain = HashChain::new();
        
        let mut stmt = self.conn.prepare(
            "SELECT seq, hash, parent_hash, sender, receiver, purpose 
             FROM audit_trail ORDER BY seq"
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
            let entry = entry_result?;
            chain.entries.push(entry);
        }

        eprintln!("[AuditStore] Loaded {} entries from disk", chain.len());
        Ok(chain)
    }

    pub fn count(&self) -> Result<u64, rusqlite::Error> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM audit_trail",
            [],
            |row| row.get(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persist_and_load() {
        let store = AuditStore::open(":memory:").unwrap();
        
        let entry = AuditEntry {
            hash: "sha256:abc123".to_string(),
            parent_hash: "root".to_string(),
            sender: "alice".to_string(),
            receiver: "bob".to_string(),
            purpose: "test".to_string(),
            seq: 0,
        };

        store.save(&entry).unwrap();
        let count = store.count().unwrap();
        assert_eq!(count, 1);

        let chain = store.load_chain().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.entries[0].hash, "sha256:abc123");
    }

    #[test]
    fn test_save_is_idempotent_on_duplicate_hash() {
        // HashChain::append (en memoire) ne deduplique PAS: un payload de
        // contenu identique soumis deux fois produit deux AuditEntry avec le
        // MEME hash mais des seq differents. Avant le passage a
        // INSERT OR IGNORE, ce test aurait paniqué sur une violation de la
        // contrainte UNIQUE(hash) au deuxieme save().
        let store = AuditStore::open(":memory:").unwrap();
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
        store.save(&entry1).unwrap();
        store.save(&entry2).unwrap(); // ne doit pas retourner Err
        assert_eq!(store.count().unwrap(), 1, "le second save (hash duplique) doit etre ignore, pas ajoute");
    }

    #[test]
    fn test_persistence_survives_reopen_on_real_file() {
        // :memory: ne partage jamais d'etat entre deux Connection distinctes
        // -- ce test utilise un vrai fichier temporaire pour verifier ce que
        // le smoke-test live (examples/audit_persistence_smoke_test.rs)
        // confirme ensuite contre un vrai redemarrage de CstlNativeServer:
        // fermer la Connection et en rouvrir une nouvelle sur le MEME chemin
        // doit retrouver la chaine persistee.
        let tmp_path = std::env::temp_dir().join(format!(
            "cstl_audit_store_test_{}_{}.db",
            std::process::id(),
            now_unix_for_test()
        ));
        let tmp_path_str = tmp_path.to_str().unwrap().to_string();

        {
            let store = AuditStore::open(&tmp_path_str).unwrap();
            store.save(&AuditEntry {
                hash: "sha256:persisted1".to_string(), parent_hash: "root".to_string(),
                sender: "alice".to_string(), receiver: "bob".to_string(),
                purpose: "test".to_string(), seq: 0,
            }).unwrap();
            store.save(&AuditEntry {
                hash: "sha256:persisted2".to_string(), parent_hash: "sha256:persisted1".to_string(),
                sender: "bob".to_string(), receiver: "alice".to_string(),
                purpose: "test".to_string(), seq: 1,
            }).unwrap();
            // `store` (et sa Connection) sort de portee ici -- simule un
            // redemarrage complet du processus serveur.
        }

        let reopened = AuditStore::open(&tmp_path_str).unwrap();
        let chain = reopened.load_chain().unwrap();
        assert_eq!(chain.len(), 2, "les 2 entrees du 'run precedent' doivent survivre a la reouverture");
        assert_eq!(chain.entries[0].hash, "sha256:persisted1");
        assert_eq!(chain.entries[1].parent_hash, "sha256:persisted1");
        assert!(chain.verify_integrity().is_ok());

        let _ = std::fs::remove_file(&tmp_path_str);
    }

    fn now_unix_for_test() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }
}
