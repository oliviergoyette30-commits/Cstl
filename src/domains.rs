//! CSTL v5.0.0 — Ontologies de domaine (port Rust de cstl_domains.py)
//! 18 domaines avec opérateurs fixes et types d'entités.
//! Miroir exact de la source Python — mêmes clés, mêmes opérateurs, français.
//! Auteur : Olivier Goyette + Claude Sonnet 5
//! Date   : 9 juillet 2026
//!
//! Mise à jour honnête (2026-09-04): `validator.rs` (racine) et
//! `validator_semantic.rs`, les deux anciens appelants cités ici, ont été
//! supprimés (code mort, jamais réellement invoqué par le chemin serveur —
//! voir l'audit du repo du même jour). Le seul appelant réel restant est
//! `crate::semantic::SemanticValidator::check_operator_whitelist`, qui
//! délègue à `is_domain_operator` ci-dessous — c'est la seule fonction
//! publique de ce module qui reste réellement branchée sur le chemin TCP
//! live. `list_domains`/`is_known_domain`/`domain_operators`/
//! `get_domain_operators` ont été retirées à la même date: aucun appelant
//! réel, seulement leurs propres tests (voir CHANGELOG).

/// Retourne les opérateurs officiels d'un domaine sous forme de slice statique.
/// Fonction interne réutilisée par domain_operators() et is_domain_operator().
fn get_domain_operators_slice(domain: &str) -> &'static [&'static str] {
    match domain.to_lowercase().as_str() {
        "diplomatique" => &[
            "NÉGOCIER", "NEGOCIER", "RATIFIER", "SIGNER", "DIVULGUER", "DIVULGUE",
            "SANCTIONNER", "MÉDIER", "MEDIER", "PROTESTER", "RECONNAÎTRE", "RECONNAITRE",
            "OBTENIR", "OBTAIN", "EXPULSER", "RAPPELER",
        ],
        "juridique" => &[
            "CONTESTER", "RÉSILIER", "RESILIER", "NOTIFIER", "ESTER", "PLAIDER",
            "CONDAMNER", "ACQUITTER", "DIVULGUER", "DIVULGUE", "SIGNER", "OBTENIR",
            "OBTAIN", "MANDATER", "RÉCLAMER", "RECLAMER",
        ],
        "médical" | "medical" => &[
            "PRESCRIRE", "DIAGNOSTIQUER", "CONTRA_INDIQUER", "ADMINISTRER", "OPÉRER",
            "OPERER", "SURVEILLER", "RÉFÉRER", "REFERER", "HOSPITALISER", "TRAITER",
            "VACCINER", "PRÉVENIR", "PREVENIR",
        ],
        "corporate" => &[
            "APPROUVER", "REJETER", "DÉLÉGUER", "DELEGUER", "REPORTER", "BUDGÉTER",
            "BUDGETER", "AUDITER", "FUSIONNER", "ACQUÉRIR", "ACQUERIR", "LICENCIER",
            "RECRUTER", "ÉVALUER", "EVALUER",
        ],
        "archéologique" | "archeologique" => &[
            "DÉCOUVRIR", "DECOUVRIR", "FOUILLER", "DATER", "CATALOGUER", "PRÉSERVER",
            "PRESERVER", "PUBLIER", "CONTESTER", "ATTRIBUER", "RESTAURER", "EXCAVÉR",
            "EXCAVER",
        ],
        "astronomique" => &[
            "OBSERVER", "DÉTECTER", "DETECTER", "MESURER", "CATALOGUER", "NOMMER",
            "CONFIRMER", "RÉFUTER", "REFUTER", "PUBLIER", "SIMULER",
        ],
        "financier" => &[
            "INVESTIR", "LIQUIDER", "AUDITER", "GARANTIR", "COUVRIR", "FINANCER",
            "EMPRUNTER", "REMBOURSER", "ÉVALUER", "EVALUER", "ACQUÉRIR", "ACQUERIR",
            "CÉDER", "CEDER", "CONSOLIDER", "PROVISIONNER",
        ],
        "cyber_securite" => &[
            "BREACH", "PATCH", "MONITOR", "ALERT", "CHIFFRER", "DÉCHIFFRER",
            "DECHIFFRER", "AUTHENTIFIER", "BLOQUER", "DÉTECTER", "DETECTER",
            "NEUTRALISER", "PATCHER", "SURVEILLER", "EXFILTRER", "COMPROMETTRE",
        ],
        "reglementaire" => &[
            "CERTIFIER", "SANCTIONNER", "NOTIFIER", "ABROGER", "HOMOLOGUER",
            "CONTRÔLER", "CONTROLER", "AUTORISER", "INTERDIRE", "DÉCLARER",
            "DECLARER", "AUDITER", "CONFORMER", "REPORTER",
        ],
        "supply_chain" => &[
            "LIVRER", "ROUTER", "BLOQUER", "SOURCER", "TRACER", "STOCKER",
            "EXPÉDIER", "EXPEDIER", "RÉCEPTIONNER", "RECEPTIONNER", "RETOURNER",
            "APPROUVER", "COMMANDER",
        ],
        "rh" => &[
            "RECRUTER", "ÉVALUER", "EVALUER", "LICENCIER", "PROMOUVOIR", "FORMER",
            "MUTER", "RÉMUNÉRER", "REMUNERER", "SANCTIONNER", "INTÉGRER",
            "INTEGRER", "OFFBOARDER",
        ],
        "recherche" => &[
            "HYPOTHÉSER", "HYPOTHESER", "VALIDER", "RÉFUTER", "REFUTER", "PUBLIER",
            "CITER", "REPRODUIRE", "RÉTRACTER", "RETRACTER", "FINANCER",
            "COLLABORER", "SOUMETTRE",
        ],
        "marketing" => &[
            "CIBLER", "SEGMENTER", "CONVERTIR", "FIDÉLISER", "FIDELISER", "ACTIVER",
            "DÉSACTIVER", "DESACTIVER", "PERSONNALISER", "MESURER", "TESTER",
            "OPTIMISER",
        ],
        "immobilier" => &[
            "ACQUÉRIR", "ACQUERIR", "LOUER", "HYPOTHÉQUER", "HYPOTHEQUER",
            "ÉVALUER", "EVALUER", "VENDRE", "GÉRER", "GERER", "RÉNOVER",
            "RENOVER", "RÉSILIER", "RESILIER", "NOTARIER",
        ],
        "assurance" => &[
            "SOUSCRIRE", "INDEMNISER", "RÉSILIER", "RESILIER", "EXPERTISER",
            "DÉCLARER", "DECLARER", "COUVRIR", "EXCLURE", "REMBOURSER",
            "ÉVALUER", "EVALUER",
        ],
        "education" => &[
            "ENSEIGNER", "ÉVALUER", "EVALUER", "CERTIFIER", "ORIENTER", "INSCRIRE",
            "EXCLURE", "DÉLIBÉRER", "DELIBERER", "VALIDER", "NOTER",
        ],
        "journalisme" => &[
            "SOURCER", "VÉRIFIER", "VERIFIER", "PUBLIER", "RECTIFIER", "ENQUÊTER",
            "ENQUETER", "CITER", "RÉVÉLER", "REVELER", "DÉMENTIR", "DEMENTIR",
            "COMMENTER",
        ],
        "energie" => &[
            "PRODUIRE", "DISTRIBUER", "STOCKER", "TARIFER", "CONNECTER",
            "DÉCONNECTER", "DECONNECTER", "RÉGULER", "REGULER", "OPTIMISER",
            "PRÉVOIR", "PREVOIR",
        ],
        _ => &[],
    }
}

/// Vérifie si un opérateur est valide pour un domaine donné (extension seulement,
/// ne teste pas le noyau officiel — c'est la responsabilité des validateurs).
/// Seule fonction publique de ce module réellement appelée par le chemin serveur
/// (voir semantic.rs::check_operator_whitelist).
pub fn is_domain_operator(operator: &str, domain: &str) -> bool {
    get_domain_operators_slice(domain).contains(&operator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_medical_prescrire_recognized() {
        assert!(is_domain_operator("PRESCRIRE", "médical"));
    }

    #[test]
    fn test_medical_domain_ascii_fallback() {
        assert!(is_domain_operator("PRESCRIRE", "medical"));
    }

    #[test]
    fn test_unknown_operator_rejected() {
        assert!(!is_domain_operator("INVENTER", "juridique"));
    }

    #[test]
    fn test_unknown_domain_returns_no_operators() {
        assert!(!is_domain_operator("PRESCRIRE", "domaine_inexistant"));
    }

    #[test]
    fn test_cyber_breach_recognized() {
        assert!(is_domain_operator("BREACH", "cyber_securite"));
    }
}
