//! CSTL v5.0.0 — Ontologies de domaine (port Rust de cstl_domains.py)
//! 18 domaines avec opérateurs fixes et types d'entités.
//! Port Rust de la source Python -- mêmes clés, mêmes domaines.
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
//!
//! Opérateurs traduits en anglais (2026-10-05, demande explicite
//! d'Olivier) : la liste d'origine portait les verbes en français
//! (`PRESCRIRE`, `NÉGOCIER`, etc.) alors que `OFFICIAL_OPERATORS`
//! (`semantic.rs`, le noyau 38 opérateurs) est entièrement en anglais --
//! incohérence corrigée ici. Vérifié avant traduction : `with_domain()`
//! (seul point d'entrée reel de ce module, via `check_operator_whitelist`)
//! n'est appelé nulle part dans le chemin serveur TCP (`grep -rn
//! with_domain` ne retourne que sa propre définition et son propre test
//! unitaire) -- donc aucun payload en production, aucun fichier de test
//! du dépôt (`tests/test_medical.cstl` notamment, vérifié orphelin,
//! référencé par aucun test) et aucun autre module ne dépendent des
//! anciens verbes français. Traduction sans impact fonctionnel connu.
//! Les clés de domaine (`"médical"`, `"juridique"`, etc.) ne sont PAS
//! traduites ici -- seules les VALEURS (listes d'opérateurs) le sont ;
//! demande explicite portait sur les opérateurs, pas les noms de domaine.
//! Les paires accent/sans-accent de la liste française (ex: `NÉGOCIER`/
//! `NEGOCIER`) n'ont plus de raison d'être en anglais (pas d'accents) --
//! elles sont donc fusionnées en une seule entrée, pas dupliquées.

/// Retourne les opérateurs officiels d'un domaine sous forme de slice statique.
/// Fonction interne réutilisée par domain_operators() et is_domain_operator().
fn get_domain_operators_slice(domain: &str) -> &'static [&'static str] {
    match domain.to_lowercase().as_str() {
        "diplomatique" => &[
            "NEGOTIATE", "RATIFY", "SIGN", "DISCLOSE", "SANCTION", "MEDIATE",
            "PROTEST", "RECOGNIZE", "OBTAIN", "EXPEL", "RECALL",
        ],
        "juridique" => &[
            "CONTEST", "TERMINATE", "NOTIFY", "SUE", "PLEAD", "CONVICT",
            "ACQUIT", "DISCLOSE", "SIGN", "OBTAIN", "MANDATE", "CLAIM",
        ],
        "médical" | "medical" => &[
            "PRESCRIBE", "DIAGNOSE", "CONTRAINDICATE", "ADMINISTER", "OPERATE",
            "MONITOR", "REFER", "HOSPITALIZE", "TREAT", "VACCINATE", "PREVENT",
        ],
        "corporate" => &[
            "APPROVE", "REJECT", "DELEGATE", "POSTPONE", "BUDGET", "AUDIT",
            "MERGE", "ACQUIRE", "DISMISS", "RECRUIT", "EVALUATE",
        ],
        "archéologique" | "archeologique" => &[
            "DISCOVER", "SURVEY", "DATE", "CATALOG", "PRESERVE", "PUBLISH",
            "CONTEST", "ATTRIBUTE", "RESTORE", "EXCAVATE",
        ],
        "astronomique" => &[
            "OBSERVE", "DETECT", "MEASURE", "CATALOG", "NAME", "CONFIRM",
            "REFUTE", "PUBLISH", "SIMULATE",
        ],
        "financier" => &[
            "INVEST", "LIQUIDATE", "AUDIT", "GUARANTEE", "HEDGE", "FINANCE",
            "BORROW", "REPAY", "APPRAISE", "ACQUIRE", "DIVEST", "CONSOLIDATE",
            "PROVISION",
        ],
        "cyber_securite" => &[
            "BREACH", "PATCH", "MONITOR", "ALERT", "ENCRYPT", "DECRYPT",
            "AUTHENTICATE", "BLOCK", "DETECT", "NEUTRALIZE", "EXFILTRATE",
            "COMPROMISE",
        ],
        "reglementaire" => &[
            "CERTIFY", "SANCTION", "NOTIFY", "REPEAL", "HOMOLOGATE", "INSPECT",
            "AUTHORIZE", "PROHIBIT", "DECLARE", "AUDIT", "COMPLY", "POSTPONE",
        ],
        "supply_chain" => &[
            "DELIVER", "ROUTE", "BLOCK", "SOURCE", "TRACK", "STORE", "SHIP",
            "RECEIVE", "RETURN", "APPROVE", "ORDER",
        ],
        "rh" => &[
            "RECRUIT", "EVALUATE", "DISMISS", "PROMOTE", "TRAIN", "TRANSFER",
            "COMPENSATE", "SANCTION", "ONBOARD", "OFFBOARD",
        ],
        "recherche" => &[
            "HYPOTHESIZE", "VALIDATE", "REFUTE", "PUBLISH", "CITE", "REPRODUCE",
            "RETRACT", "FUND", "COLLABORATE", "SUBMIT",
        ],
        "marketing" => &[
            "TARGET", "SEGMENT", "CONVERT", "RETAIN", "ACTIVATE", "DEACTIVATE",
            "PERSONALIZE", "MEASURE", "TEST", "OPTIMIZE",
        ],
        "immobilier" => &[
            "ACQUIRE", "LEASE", "MORTGAGE", "APPRAISE", "SELL", "MANAGE",
            "RENOVATE", "TERMINATE", "NOTARIZE",
        ],
        "assurance" => &[
            "SUBSCRIBE", "INDEMNIFY", "TERMINATE", "ASSESS", "DECLARE",
            "COVER", "EXCLUDE", "REIMBURSE", "EVALUATE",
        ],
        "education" => &[
            "TEACH", "EVALUATE", "CERTIFY", "GUIDE", "ENROLL", "EXPEL",
            "DELIBERATE", "VALIDATE", "GRADE",
        ],
        "journalisme" => &[
            "SOURCE", "VERIFY", "PUBLISH", "CORRECT", "INVESTIGATE", "CITE",
            "REVEAL", "DENY", "COMMENT",
        ],
        "energie" => &[
            "PRODUCE", "DISTRIBUTE", "STORE", "PRICE", "CONNECT", "DISCONNECT",
            "REGULATE", "OPTIMIZE", "FORECAST",
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
    fn test_medical_prescribe_recognized() {
        // Operateur traduit en anglais le 2026-10-05 (etait "PRESCRIRE").
        assert!(is_domain_operator("PRESCRIBE", "médical"));
    }

    #[test]
    fn test_medical_domain_ascii_fallback() {
        assert!(is_domain_operator("PRESCRIBE", "medical"));
    }

    #[test]
    fn test_unknown_operator_rejected() {
        assert!(!is_domain_operator("INVENT", "juridique"));
    }

    #[test]
    fn test_unknown_domain_returns_no_operators() {
        assert!(!is_domain_operator("PRESCRIBE", "domaine_inexistant"));
    }

    #[test]
    fn test_cyber_breach_recognized() {
        assert!(is_domain_operator("BREACH", "cyber_securite"));
    }
}
