//! src/obsidian_escalation.rs — Escalade vers un vault Obsidian (Layer 6, portion réelle)
//!
//! Quand `ExecutionLab` détecte une contradiction ou un cycle, ce module écrit une
//! entrée dans un fichier markdown du vault Obsidian de l'utilisateur — visible
//! immédiatement dans son Obsidian, sans avoir à interroger la base ou Telegram.
//! Équivalent Rust de ce que faisait l'ancien `server_app.py` (Python, supprimé
//! lors du nettoyage de la session Gemini plus tôt dans ce projet), mais intégré
//! au serveur réel plutôt qu'un script FastAPI local séparé.
//!
//! Portée honnête: écriture seule, append-only, un seul fichier
//! (`CSTL_Restricted_Council.md`), pas de lecture/réconciliation avec ce que
//! l'utilisateur a pu modifier dans Obsidian entre-temps.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ObsidianEscalation {
    file_path: PathBuf,
}

impl ObsidianEscalation {
    /// `None` si `OBSIDIAN_VAULT_PATH` absent de l'environnement — le serveur
    /// continue de fonctionner sans escalade Obsidian, dégradation propre
    /// plutôt qu'un crash au démarrage.
    pub fn from_env() -> Option<Self> {
        let vault = std::env::var("OBSIDIAN_VAULT_PATH").ok()?;
        let mut path = PathBuf::from(vault);
        path.push("CSTL_Restricted_Council.md");
        Some(Self { file_path: path })
    }

    pub fn escalate(&self, hash: &str, sigma: f64, details: &str) -> std::io::Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let entry = format!(
            "\n---\n## ⚠️ Escalade CSTL — unix:{}\n**hash**: `{}`\n**sigma**: {}\n\n```\n{}\n```\n",
            now,
            hash,
            sigma,
            details.trim()
        );
        let mut file = OpenOptions::new().create(true).append(true).open(&self.file_path)?;
        file.write_all(entry.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Un seul test pour les deux comportements: env::set_var/remove_var est
    // global au processus, et cargo test fait tourner les tests en parallele
    // sur des threads du meme processus -> deux tests separes touchant la
    // meme variable d'environnement se marchent dessus (flaky par design).
    // Les regrouper dans un seul test sequentiel evite la course.
    #[test]
    fn test_from_env_and_escalate() {
        std::env::remove_var("OBSIDIAN_VAULT_PATH");
        assert!(ObsidianEscalation::from_env().is_none());

        let dir = std::env::temp_dir().join(format!("cstl_obsidian_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("OBSIDIAN_VAULT_PATH", dir.to_str().unwrap());

        let escalation = ObsidianEscalation::from_env().unwrap();
        escalation.escalate("sha256:abc", 0.09, "contradiction: X vs Y").unwrap();
        escalation.escalate("sha256:def", 0.09, "cycle: A -> B -> A").unwrap();

        let content = std::fs::read_to_string(dir.join("CSTL_Restricted_Council.md")).unwrap();
        assert!(content.contains("sha256:abc"));
        assert!(content.contains("sha256:def"));
        assert!(content.contains("contradiction: X vs Y"));

        std::fs::remove_dir_all(&dir).ok();
        std::env::remove_var("OBSIDIAN_VAULT_PATH");
    }
}
