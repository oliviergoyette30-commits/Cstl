//! src/hypothesis_engine.rs — Level 4, moteur d'hypotheses ("Future Architecture").
//!
//! Portee honnete (2026-09-04, en reponse a une trouvaille d'audit: le README
//! documentait cette etape comme jamais demontree malgre l'entity-resolution/SPARQL
//! deja reelle dans kb_verify.rs -- Couche 3a). Ce module ajoute l'etape GENERATIVE
//! manquante decrite dans README.md ("Future Architecture — Level 4"): detection
//! d'entanglement sur un sous-graphe Wikidata borne -- deux entites dont les
//! VOISINAGES se recoupent fortement mais qui n'ont AUCUNE relation directe connue
//! sont un signal (pas une preuve) qu'une relation existe peut-etre entre elles. Le
//! moteur propose alors une relation CSTL speculative a sigma delibrement bas
//! (`ASSUMES`, jamais `KNOWS`/`BELIEVES`) -- a valider ensuite par
//! ExecutionLab/RestrictedCouncil, pas a accepter tel quel.
//!
//! Ce que ce module NE fait PAS et n'a jamais pretendu faire: deviner LE TYPE de
//! relation (predicat) entre deux entites -- seulement QU'une relation merite
//! d'etre investiguee. `to_cstl_relation()` emet volontairement `type=ASSUMES` sans
//! predicat specifique plus fin; c'est a un humain ou un check downstream de
//! qualifier la nature exacte du lien, si le signal est retenu.
//!
//! Verification honnete: la logique pure de ce fichier (coefficient de
//! recouvrement, formule de sigma, formatage CSTL) est testee ici sans reseau.
//! L'orchestration reseau (`KbVerifier::query_generic_neighbors`/`resolve_label`/
//! `has_direct_relation`/`detect_entanglement`, ajoutee a `src/kb_verify.rs`) ne
//! peut PAS etre verifiee en direct contre le VRAI wikidata.org depuis ce
//! sandbox: wikidata.org est bloque par la liste blanche reseau de cet
//! environnement (403 confirme sur le proxy sortant via un `curl` direct avant
//! d'ecrire ce module). **2026-09-05**: cette orchestration reseau EST
//! desormais verifiee en direct contre un vrai serveur HTTP local qui
//! reproduit le format de reponse SPARQL de Wikidata (`wiremock`,
//! `tests/kb_verify_mock_wikidata_test.rs`, 4 tests incluant un controle
//! positif, deux controles negatifs et deux scenarios de panne reseau) --
//! l'appel HTTP reel, la construction de requete et le parsing JSON sont donc
//! reellement exerces. Ca ne remplace pas une verification contre le vrai
//! wikidata.org (format de reponse reel, latence reelle, cas de donnees
//! imprevus) -- toujours a faire sur une machine avec acces reseau reel.
//! -- voir README.md/docs/ARCHITECTURE.md pour le statut exact ("structurellement
//! construit et teste hors-reseau, jamais execute en direct contre Wikidata").

use std::collections::HashSet;

/// Coefficient de recouvrement (overlap coefficient, PAS Jaccard): |A∩B| / min(|A|,|B|).
/// Prefere a Jaccard ici parce qu'un noeud a tres haut degre (ex: "pays") aurait un
/// Jaccard artificiellement bas avec tout le monde meme quand le recouvrement est
/// total du cote du noeud a faible degre -- le signal qu'on cherche ("ces deux
/// entites partagent presque tout leur petit voisinage") serait noye par la taille
/// du plus gros ensemble. Retourne 0.0 si l'un des deux ensembles est vide (rien a
/// comparer, jamais une division par zero).
pub fn overlap_coefficient(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let smaller = a.len().min(b.len());
    if smaller == 0 {
        return 0.0;
    }
    let common = a.intersection(b).count();
    common as f64 / smaller as f64
}

/// Sigma delibrement bas pour une hypothese generee -- jamais assez haut pour
/// atteindre le seuil KNOWS (>0.8, voir `semantic.rs::check_knows_calibration`,
/// W604) ni meme un BELIEVES usuel. Une hypothese generee reste une hypothese
/// jusqu'a validation par ExecutionLab/RestrictedCouncil, peu importe la force du
/// signal statistique qui l'a produite -- plafonnee a 0.35 meme pour un
/// recouvrement parfait (overlap=1.0), cf. le payload de reference de README.md
/// ("Future Architecture — Level 4": `ASSUMES ... σ=0.3`, jamais `KNOWS`).
pub fn sigma_for_overlap(overlap: f64) -> f64 {
    let overlap = overlap.clamp(0.0, 1.0);
    (0.15 + 0.20 * overlap).min(0.35)
}

/// Une hypothese d'entanglement generee: deux entites Wikidata dont le voisinage se
/// recoupe au-dessus d'un seuil, sans relation directe deja connue entre elles.
#[derive(Debug, Clone, PartialEq)]
pub struct EntanglementHypothesis {
    pub subject_qid: String,
    pub object_qid: String,
    pub subject_label: Option<String>,
    pub object_label: Option<String>,
    pub common_neighbors: usize,
    pub overlap_coefficient: f64,
    pub sigma: f64,
}

impl EntanglementHypothesis {
    pub fn new(
        subject_qid: impl Into<String>,
        object_qid: impl Into<String>,
        subject_label: Option<String>,
        object_label: Option<String>,
        common_neighbors: usize,
        overlap_coefficient: f64,
    ) -> Self {
        Self {
            subject_qid: subject_qid.into(),
            object_qid: object_qid.into(),
            subject_label,
            object_label,
            common_neighbors,
            overlap_coefficient,
            sigma: sigma_for_overlap(overlap_coefficient),
        }
    }

    /// Formate la relation CSTL speculative telle que decrite dans README.md
    /// (section "Future Architecture — Level 4"): `ASSUMES ... [sigma=...
    /// source=hypothesis_engine derived_from=common_neighbour_pattern]`. Utilise le
    /// label resolu si disponible, sinon le QID brut -- jamais d'echec silencieux,
    /// le sujet/objet doivent toujours designer quelque chose d'identifiable.
    pub fn to_cstl_relation(&self) -> String {
        let subject = self.subject_label.clone().unwrap_or_else(|| self.subject_qid.clone());
        let object = self.object_label.clone().unwrap_or_else(|| self.object_qid.clone());
        format!(
            "RELATION [type=ASSUMES, subject={subject}, object={object}, sigma={:.2}, \
             source=hypothesis_engine, derived_from=common_neighbour_pattern, \
             subject_qid={}, object_qid={}, common_neighbors={}]",
            self.sigma, self.subject_qid, self.object_qid, self.common_neighbors
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_overlap_coefficient_full_overlap() {
        let a = set(&["Q1", "Q2", "Q3"]);
        let b = set(&["Q1", "Q2"]);
        assert_eq!(overlap_coefficient(&a, &b), 1.0);
    }

    #[test]
    fn test_overlap_coefficient_no_overlap() {
        let a = set(&["Q1", "Q2"]);
        let b = set(&["Q3", "Q4"]);
        assert_eq!(overlap_coefficient(&a, &b), 0.0);
    }

    #[test]
    fn test_overlap_coefficient_empty_set_is_zero_not_nan() {
        let a: HashSet<String> = HashSet::new();
        let b = set(&["Q1"]);
        assert_eq!(overlap_coefficient(&a, &b), 0.0);
        assert_eq!(overlap_coefficient(&a, &a), 0.0);
    }

    #[test]
    fn test_overlap_coefficient_partial() {
        let a = set(&["Q1", "Q2", "Q3", "Q4"]);
        let b = set(&["Q3", "Q4", "Q5", "Q6"]);
        // commun=2, min(4,4)=4 -> 0.5
        assert_eq!(overlap_coefficient(&a, &b), 0.5);
    }

    #[test]
    fn test_sigma_never_reaches_believes_or_knows_threshold() {
        // Meme pour un recouvrement parfait, sigma doit rester nettement sous 0.8
        // (seuil KNOWS, semantic.rs::check_knows_calibration) -- une hypothese
        // generee ne doit jamais se faire passer pour une conviction forte.
        assert!(sigma_for_overlap(1.0) <= 0.35);
        assert!(sigma_for_overlap(1.0) < 0.8);
    }

    #[test]
    fn test_sigma_monotonic_in_overlap() {
        assert!(sigma_for_overlap(0.9) > sigma_for_overlap(0.1));
        assert!(sigma_for_overlap(0.0) < sigma_for_overlap(0.5));
    }

    #[test]
    fn test_sigma_clamps_out_of_range_input() {
        assert_eq!(sigma_for_overlap(-1.0), sigma_for_overlap(0.0));
        assert_eq!(sigma_for_overlap(5.0), sigma_for_overlap(1.0));
    }

    #[test]
    fn test_to_cstl_relation_uses_labels_when_available() {
        let h = EntanglementHypothesis::new(
            "Q7186", "Q37463",
            Some("Marie Curie".to_string()), Some("Irène Joliot-Curie".to_string()),
            5, 0.8,
        );
        let rel = h.to_cstl_relation();
        assert!(rel.contains("type=ASSUMES"));
        assert!(rel.contains("subject=Marie Curie"));
        assert!(rel.contains("object=Irène Joliot-Curie"));
        assert!(rel.contains("source=hypothesis_engine"));
        assert!(rel.contains("derived_from=common_neighbour_pattern"));
        assert!(!rel.contains("KNOWS"));
        assert!(!rel.contains("BELIEVES"));
    }

    #[test]
    fn test_to_cstl_relation_falls_back_to_qid_without_label() {
        let h = EntanglementHypothesis::new("Q1", "Q2", None, None, 3, 0.5);
        let rel = h.to_cstl_relation();
        assert!(rel.contains("subject=Q1"));
        assert!(rel.contains("object=Q2"));
    }

    #[test]
    fn test_common_neighbors_and_overlap_are_carried_through() {
        let h = EntanglementHypothesis::new("Q1", "Q2", None, None, 7, 0.42);
        assert_eq!(h.common_neighbors, 7);
        assert_eq!(h.overlap_coefficient, 0.42);
    }
}
