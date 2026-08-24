//! CSTL v5.0.0 — Ontologies de domaine (port Rust de cstl_domains.py)
//! 18 domaines avec opérateurs fixes et types d'entités.
//! Miroir exact de la source Python — mêmes clés, mêmes opérateurs, français.
//! Auteur : Olivier Goyette + Claude Sonnet 5
//! Date   : 9 juillet 2026
//!
//! Signatures alignées sur l'usage réel trouvé dans le projet :
//! - validator.rs:219-220     use crate::domains::is_known_domain;
//! - validator_semantic.rs:368 .map(crate::domains::domain_operators)
//!   (nécessite Vec<&'static str>, pas une slice, pour .chain() derrière)

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

/// Retourne les opérateurs officiels d'un domaine sous forme de Vec possédé.
/// Signature exacte attendue par validator_semantic.rs (usage avec .chain()).
pub fn domain_operators(domain: &str) -> Vec<&'static str> {
    get_domain_operators_slice(domain).to_vec()
}

/// Alias slice, conservé pour usage direct sans allocation si besoin ailleurs.
pub fn get_domain_operators(domain: &str) -> &'static [&'static str] {
    get_domain_operators_slice(domain)
}

/// Vérifie si un nom de domaine correspond à l'un des 18 domaines connus.
/// Signature exacte attendue par validator.rs:219-220.
pub fn is_known_domain(domain: &str) -> bool {
    !get_domain_operators_slice(domain).is_empty() || list_domains().contains(&domain.to_lowercase().as_str())
}

/// Vérifie si un opérateur est valide pour un domaine donné (extension seulement,
/// ne teste pas le noyau officiel — c'est la responsabilité des validateurs).
pub fn is_domain_operator(operator: &str, domain: &str) -> bool {
    get_domain_operators_slice(domain).contains(&operator)
}

/// Liste tous les noms de domaines connus (miroir de list_domains() en Python).
pub fn list_domains() -> &'static [&'static str] {
    &[
        "diplomatique", "juridique", "médical", "corporate", "archéologique",
        "astronomique", "financier", "cyber_securite", "reglementaire",
        "supply_chain", "rh", "recherche", "marketing", "immobilier",
        "assurance", "education", "journalisme", "energie",
    ]
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
    fn test_unknown_domain_returns_empty() {
        assert!(get_domain_operators("domaine_inexistant").is_empty());
    }

    #[test]
    fn test_cyber_breach_recognized() {
        assert!(is_domain_operator("BREACH", "cyber_securite"));
    }

    #[test]
    fn test_list_domains_count() {
        assert_eq!(list_domains().len(), 18);
    }

    #[test]
    fn test_domain_operators_returns_vec() {
        let ops = domain_operators("médical");
        assert!(ops.contains(&"PRESCRIRE"));
    }

    #[test]
    fn test_is_known_domain_true() {
        assert!(is_known_domain("juridique"));
        assert!(is_known_domain("MÉDICAL"));
    }

    #[test]
    fn test_is_known_domain_false() {
        assert!(!is_known_domain("domaine_bidon"));
    }
}
