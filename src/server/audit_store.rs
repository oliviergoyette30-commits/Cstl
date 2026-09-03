/// Persistent Audit Store
/// Sauvegarde la chaîne dans SQLite, la charge au démarrage
/// Rejoint cstl_adn_store.py conceptuellement

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

    /// Sauvegarde une entrée (append-only)
    pub fn save(&self, entry: &AuditEntry) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "INSERT INTO audit_trail (seq, hash, parent_hash, sender, receiver, purpose)
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
        eprintln!("[AuditStore] Persisted seq={}", entry.seq);
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
}
