//! src/adn_delta_detector.rs — Couche 5 ADN Delta Detector
//! Détecte les changements entre deux entrées ADN pour la résolution de conflits.
//!
//! Rationale: Quand deux agents proposent des faits conflictuels, la détection
//! delta vous dit COMMENT ils diffèrent:
//!   - Données (payload changed)
//!   - Confiance (sigma shift)
//!   - Priorité / modalité déontique (MUST vs MUST_NOT)
//!
//! Utilisé par Couche 2 (gouvernance) et Couche 4 (émergence) pour l'arbitrage.

use crate::adn_store::AdnStore;

/// Rapport détaillé des changements entre deux entrées ADN
#[derive(Debug, Clone)]
pub struct DeltaReport {
    /// Le payload lui-même a changé (structurellement ou textuellement)
    pub payload_changed: bool,

    /// Différence sigma: sigma_new - sigma_old
    pub sigma_delta: f64,

    /// Si la modalité déontique a changé, (old_modality, new_modality)
    /// Exemples: Some(("MUST", "MUST_NOT")), Some(("PERMISSION", "OBLIGATION"))
    pub modality_change: Option<(String, String)>,

    /// Lignes différentes du payload (si texte-based) ou résumé du byte diff
    pub line_diffs: Vec<String>,

    /// Nombre de bytes différents (si comparaison binaire)
    pub byte_diff_count: usize,

    /// Métadonnées pour audit
    pub old_hash: String,
    pub new_hash: String,
    pub conflict_severity: ConflictSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictSeverity {
    /// Aucun changement détecté
    NoChange,

    /// Changement mineur (sigma shift < 0.1, payload quasi-identique)
    Minor,

    /// Changement modéré (payload change, sigma shift modéré)
    Moderate,

    /// Changement majeur (changement de modalité déontique ou sigma shift > 0.3)
    Major,

    /// Conflit critique (MUST ↔ MUST_NOT ou sigma delta > 0.5)
    Critical,
}

pub struct ADNDeltaDetector {
    store: std::sync::Arc<AdnStore>,
}

impl ADNDeltaDetector {
    pub fn new(store: std::sync::Arc<AdnStore>) -> Self {
        Self { store }
    }

    /// Détecte les changements entre deux entrées ADN par leurs hashes.
    ///
    /// # Arguments
    /// - `old_hash`: Hash de l'entrée existante
    /// - `new_hash`: Hash de la nouvelle proposition
    ///
    /// # Returns
    /// `Result<DeltaReport, String>` avec le rapport détaillé des deltas
    pub fn detect_deltas(
        &self,
        old_hash: &str,
        new_hash: &str,
    ) -> Result<DeltaReport, String> {
        // Charger les deux entrées
        let old_entry = self
            .store
            .get(old_hash)
            .map_err(|e| format!("Erreur lecture old_hash: {}", e))?
            .ok_or_else(|| format!("old_hash non trouvé: {}", old_hash))?;

        let new_entry = self
            .store
            .get(new_hash)
            .map_err(|e| format!("Erreur lecture new_hash: {}", e))?
            .ok_or_else(|| format!("new_hash non trouvé: {}", new_hash))?;

        // Comparer les payloads
        let (payload_changed, line_diffs, byte_diff_count) =
            self.compare_payloads(&old_entry.payload, &new_entry.payload);

        // Comparer les valeurs sigma
        let sigma_delta = new_entry.sigma - old_entry.sigma;

        // Comparer les modalités déontiques
        let modality_change = self.compare_modalities(old_hash, new_hash)?;

        // Calculer la sévérité du conflit
        let severity = self.calculate_severity(payload_changed, sigma_delta, &modality_change);

        Ok(DeltaReport {
            payload_changed,
            sigma_delta: (sigma_delta * 1000.0).round() / 1000.0, // Arrondir à 3 décimales
            modality_change,
            line_diffs,
            byte_diff_count,
            old_hash: old_hash.to_string(),
            new_hash: new_hash.to_string(),
            conflict_severity: severity,
        })
    }

    /// Compare les payloads (texte ou binaire).
    /// Retourne (changed, line_diffs, byte_diff_count)
    fn compare_payloads(
        &self,
        old_payload: &str,
        new_payload: &str,
    ) -> (bool, Vec<String>, usize) {
        if old_payload == new_payload {
            return (false, vec![], 0);
        }

        // Essayer comparaison ligne-par-ligne (texte)
        let old_lines: Vec<&str> = old_payload.lines().collect();
        let new_lines: Vec<&str> = new_payload.lines().collect();

        if old_lines.len() > 0 && new_lines.len() > 0 {
            let line_diffs = self.diff_lines(&old_lines, &new_lines);
            // Aussi compter byte diff
            let byte_diff = self.count_byte_diffs(old_payload.as_bytes(), new_payload.as_bytes());
            return (true, line_diffs, byte_diff);
        }

        // Sinon, comparaison binaire
        let byte_diff = self.count_byte_diffs(old_payload.as_bytes(), new_payload.as_bytes());
        let diff_desc = format!("{} bytes changed out of {}", byte_diff, old_payload.len());
        (true, vec![diff_desc], byte_diff)
    }

    /// Implémente un simple diff de lignes (ligne présente/absente)
    fn diff_lines(&self, old_lines: &[&str], new_lines: &[&str]) -> Vec<String> {
        let mut diffs = Vec::new();

        let old_set: std::collections::HashSet<_> = old_lines.iter().collect();
        let new_set: std::collections::HashSet<_> = new_lines.iter().collect();

        // Lignes supprimées
        for line in &old_set {
            if !new_set.contains(line) {
                diffs.push(format!("- {}", line));
            }
        }

        // Lignes ajoutées
        for line in &new_set {
            if !old_set.contains(line) {
                diffs.push(format!("+ {}", line));
            }
        }

        diffs.sort();
        diffs.truncate(20); // Limiter à 20 diffs pour éviter le spam
        diffs
    }

    /// Compte le nombre de bytes différents
    fn count_byte_diffs(&self, old: &[u8], new: &[u8]) -> usize {
        old.iter()
            .zip(new.iter())
            .filter(|&(x, y)| x != y)
            .count()
            + (old.len() as isize - new.len() as isize).abs() as usize
    }

    /// Compare les modalités déontiques stockées en adn_relations
    fn compare_modalities(
        &self,
        old_hash: &str,
        new_hash: &str,
    ) -> Result<Option<(String, String)>, String> {
        // Récupérer les relations pour chaque hash
        let old_relations = self
            .store
            .get_relations_with_modality(old_hash)
            .map_err(|e| format!("Erreur lecture relations old: {}", e))?;

        let new_relations = self
            .store
            .get_relations_with_modality(new_hash)
            .map_err(|e| format!("Erreur lecture relations new: {}", e))?;

        // Chercher les modalités différentes
        for (key, old_modality) in old_relations {
            if let Some(new_modality) = new_relations.get(&key) {
                if old_modality != *new_modality {
                    // Trouver la première divergence
                    return Ok(Some((old_modality, new_modality.clone())));
                }
            }
        }

        Ok(None)
    }

    /// Calcule la sévérité du conflit basée sur les deltas
    fn calculate_severity(
        &self,
        payload_changed: bool,
        sigma_delta: f64,
        modality_change: &Option<(String, String)>,
    ) -> ConflictSeverity {
        use ConflictSeverity::*;

        // Conflit critique: changement de modalité MUST ↔ MUST_NOT
        if let Some((old_mod, new_mod)) = modality_change {
            if (old_mod == "MUST" && new_mod == "MUST_NOT")
                || (old_mod == "MUST_NOT" && new_mod == "MUST")
            {
                return Critical;
            }
            // Autre changement de modalité = majeur
            if old_mod != new_mod {
                return Major;
            }
        }

        // Sigma shift > 0.5 = critique
        if sigma_delta.abs() > 0.5 {
            return Critical;
        }

        // Sigma shift > 0.3 = majeur
        if sigma_delta.abs() > 0.3 {
            return Major;
        }

        // Payload changé mais sigma stable = modéré
        if payload_changed && sigma_delta.abs() < 0.1 {
            return Moderate;
        }

        // Payload changé et sigma shift modéré = modéré
        if payload_changed && sigma_delta.abs() < 0.3 {
            return Moderate;
        }

        // Aucun changement de payload mais sigma shift = mineur
        if !payload_changed && sigma_delta.abs() > 0.01 {
            return Minor;
        }

        NoChange
    }

    /// Formatte le rapport en CSTL pour audit (optionnel)
    pub fn format_cstl(&self, report: &DeltaReport) -> String {
        let mut s = format!(
            "ADN_DELTA [\n\
             old_hash={}\n\
             new_hash={}\n\
             payload_changed={}\n\
             sigma_delta={:.3}\n\
             severity={:?}\n",
            &report.old_hash[..16.min(report.old_hash.len())],
            &report.new_hash[..16.min(report.new_hash.len())],
            report.payload_changed,
            report.sigma_delta,
            report.conflict_severity
        );

        if let Some((old_mod, new_mod)) = &report.modality_change {
            s.push_str(&format!(
                "modality_change=({} -> {})\n",
                old_mod, new_mod
            ));
        }

        if !report.line_diffs.is_empty() {
            s.push_str("diffs=[\n");
            for diff in &report.line_diffs {
                s.push_str(&format!("  {}\n", diff));
            }
            s.push_str("]\n");
        }

        if report.byte_diff_count > 0 {
            s.push_str(&format!("byte_diffs={}\n", report.byte_diff_count));
        }

        s.push_str("]\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn create_test_store() -> AdnStore {
        let conn = Connection::open_in_memory().expect("in-memory DB");
        let store = AdnStore::open_connection(conn).expect("store creation");
        store
    }

    #[test]
    fn test_no_change() {
        let store = Arc::new(create_test_store());
        let detector = ADNDeltaDetector::new(Arc::clone(&store));

        // Créer une entrée
        store
            .put(
                "hash1",
                "payload content",
                Some("encoder1"),
                Some("agent1"),
                0.85,
                None,
                Some("conv1"),
                Some(1),
            )
            .expect("put");

        // Comparer avec elle-même
        let report = detector
            .detect_deltas("hash1", "hash1")
            .expect("detect_deltas");

        assert!(!report.payload_changed);
        assert_eq!(report.sigma_delta, 0.0);
        assert_eq!(report.modality_change, None);
        assert!(report.line_diffs.is_empty());
        assert_eq!(report.conflict_severity, ConflictSeverity::NoChange);
    }

    #[test]
    fn test_payload_change() {
        let store = Arc::new(create_test_store());
        let detector = ADNDeltaDetector::new(Arc::clone(&store));

        // Créer deux entrées
        store
            .put(
                "hash_old",
                "line 1\nline 2\nline 3",
                Some("encoder1"),
                Some("agent1"),
                0.85,
                None,
                Some("conv1"),
                Some(1),
            )
            .expect("put");

        store
            .put(
                "hash_new",
                "line 1\nline 2 modified\nline 3",
                Some("encoder1"),
                Some("agent1"),
                0.85,
                None,
                Some("conv1"),
                Some(2),
            )
            .expect("put");

        let report = detector
            .detect_deltas("hash_old", "hash_new")
            .expect("detect_deltas");

        assert!(report.payload_changed);
        assert_eq!(report.sigma_delta, 0.0);
        assert!(!report.line_diffs.is_empty());
        assert_eq!(report.conflict_severity, ConflictSeverity::Moderate);
    }

    #[test]
    fn test_sigma_change_minor() {
        let store = Arc::new(create_test_store());
        let detector = ADNDeltaDetector::new(Arc::clone(&store));

        store
            .put(
                "hash_old",
                "payload",
                Some("encoder1"),
                Some("agent1"),
                0.80,
                None,
                Some("conv1"),
                Some(1),
            )
            .expect("put");

        store
            .put(
                "hash_new",
                "payload",
                Some("encoder1"),
                Some("agent1"),
                0.85,
                None,
                Some("conv1"),
                Some(2),
            )
            .expect("put");

        let report = detector
            .detect_deltas("hash_old", "hash_new")
            .expect("detect_deltas");

        assert!(!report.payload_changed);
        assert_eq!(report.sigma_delta, 0.05);
        assert_eq!(report.conflict_severity, ConflictSeverity::Minor);
    }

    #[test]
    fn test_sigma_change_major() {
        let store = Arc::new(create_test_store());
        let detector = ADNDeltaDetector::new(Arc::clone(&store));

        store
            .put(
                "hash_old",
                "payload",
                Some("encoder1"),
                Some("agent1"),
                0.50,
                None,
                Some("conv1"),
                Some(1),
            )
            .expect("put");

        store
            .put(
                "hash_new",
                "payload",
                Some("encoder1"),
                Some("agent1"),
                0.85,
                None,
                Some("conv1"),
                Some(2),
            )
            .expect("put");

        let report = detector
            .detect_deltas("hash_old", "hash_new")
            .expect("detect_deltas");

        assert!(!report.payload_changed);
        assert_eq!(report.sigma_delta, 0.35);
        assert_eq!(report.conflict_severity, ConflictSeverity::Major);
    }

    #[test]
    fn test_combined_changes() {
        let store = Arc::new(create_test_store());
        let detector = ADNDeltaDetector::new(Arc::clone(&store));

        store
            .put(
                "hash_old",
                "original payload\nwith multiple lines",
                Some("encoder1"),
                Some("agent1"),
                0.70,
                None,
                Some("conv1"),
                Some(1),
            )
            .expect("put");

        store
            .put(
                "hash_new",
                "modified payload\nwith different content",
                Some("encoder1"),
                Some("agent2"),
                0.90,
                None,
                Some("conv1"),
                Some(2),
            )
            .expect("put");

        let report = detector
            .detect_deltas("hash_old", "hash_new")
            .expect("detect_deltas");

        assert!(report.payload_changed);
        assert_eq!(report.sigma_delta, 0.20);
        assert!(!report.line_diffs.is_empty());
        assert_eq!(report.conflict_severity, ConflictSeverity::Moderate);
    }

    #[test]
    fn test_format_cstl() {
        let report = DeltaReport {
            payload_changed: true,
            sigma_delta: 0.123,
            modality_change: Some(("MUST".to_string(), "PERMISSION".to_string())),
            line_diffs: vec!["- old line".to_string(), "+ new line".to_string()],
            byte_diff_count: 42,
            old_hash: "abc123def456".to_string(),
            new_hash: "xyz789uvw123".to_string(),
            conflict_severity: ConflictSeverity::Moderate,
        };

        let detector = ADNDeltaDetector::new(Arc::new(create_test_store()));
        let cstl = detector.format_cstl(&report);

        assert!(cstl.contains("ADN_DELTA"));
        assert!(cstl.contains("abc123def456"));
        assert!(cstl.contains("xyz789uvw123"));
        assert!(cstl.contains("payload_changed=true"));
        assert!(cstl.contains("MUST -> PERMISSION"));
        assert!(cstl.contains("42"));
    }
}
