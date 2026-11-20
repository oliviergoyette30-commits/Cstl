//! src/server/action_authorization.rs -- Couche 9 : autorisation AVANT action
//! (2026-11-20).
//!
//! Probleme : CSTL ne se trouve pas sur le chemin des actions des agents (ils
//! agissent dans leurs propres processus). Les regles deontiques ne pouvaient
//! donc que JUGER APRES COUP un message deja envoye. Ici, l'agent demande
//! d'abord : `INTENT_PAYLOAD [purpose=action_request, action=<nom>]`, signe.
//! Le serveur repond `allowed` ou `denied` d'apres l'historique persiste des
//! obligations / interdictions, et inscrit la decision dans la chaine d'audit.
//!
//! Regles de decision (pures, sans E/S, testees ci-dessous) :
//! 1. identite : demande non signee, ou expediteur non enregistre avec une
//!    cle -> `denied` (autoriser une identite non prouvee n'a pas de sens) ;
//! 2. `action` absente ou hors de `[A-Za-z0-9_.-]{1,128}` -> `denied`
//!    (la valeur est recopiee dans la reponse wire : jamais de `,` ni de `]`) ;
//! 3. une INTERDICTION (MUST_NOT / FORBID) dans l'historique pour
//!    (sujet = expediteur, objet = action) -> `denied`, meme si une
//!    obligation contraire existe aussi : l'interdiction l'emporte, et le
//!    conflit est signale ;
//! 4. sinon, une OBLIGATION (MUST / REQUIRE) -> `allowed` (required) ;
//! 5. sinon -> politique par defaut : `allowed` (`CSTL_ACTION_DEFAULT` absent
//!    ou `allow`), ou `denied` (`CSTL_ACTION_DEFAULT=deny` : liste blanche,
//!    seule une obligation explicite autorise).

use std::collections::HashMap;

use crate::semantic::{FORBIDDEN_MODALITIES, REQUIRED_MODALITIES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultPolicy {
    Allow,
    Deny,
}

impl DefaultPolicy {
    pub fn from_env() -> Self {
        match std::env::var("CSTL_ACTION_DEFAULT") {
            Ok(v) if v.eq_ignore_ascii_case("deny") => DefaultPolicy::Deny,
            _ => DefaultPolicy::Allow,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub allowed: bool,
    /// Code court, sans `,` ni `]` (recopie tel quel dans la reponse wire).
    pub reason: &'static str,
    /// Modalite qui a tranche, si une regle de l'historique a tranche.
    pub rule: Option<String>,
    /// Une obligation ET une interdiction existent pour cette action.
    pub conflict: bool,
}

impl Decision {
    fn deny(reason: &'static str) -> Self {
        Decision { allowed: false, reason, rule: None, conflict: false }
    }
}

pub fn is_valid_action(action: &str) -> bool {
    !action.is_empty()
        && action.len() <= 128
        && action.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `history` : relations deontiques persistees (`AdnStore::deontic_relations_history`).
pub fn decide(
    sender: &str,
    action: Option<&str>,
    identity_proven: bool,
    sender_registered: bool,
    history: &[HashMap<String, String>],
    default: DefaultPolicy,
) -> Decision {
    if !sender_registered {
        return Decision::deny("sender_not_registered");
    }
    if !identity_proven {
        return Decision::deny("signature_required");
    }
    let Some(action) = action.filter(|a| is_valid_action(a)) else {
        return Decision::deny("missing_or_invalid_action");
    };

    let mut forbidden: Option<String> = None;
    let mut required: Option<String> = None;
    for rel in history {
        if rel.get("subject").map(String::as_str) != Some(sender) || rel.get("object").map(String::as_str) != Some(action) {
            continue;
        }
        let Some(modality) = rel.get("modality") else { continue };
        if FORBIDDEN_MODALITIES.contains(&modality.as_str()) {
            forbidden.get_or_insert_with(|| modality.clone());
        } else if REQUIRED_MODALITIES.contains(&modality.as_str()) {
            required.get_or_insert_with(|| modality.clone());
        }
    }
    let conflict = forbidden.is_some() && required.is_some();
    match (forbidden, required) {
        (Some(f), _) => Decision { allowed: false, reason: "forbidden", rule: Some(f), conflict },
        (None, Some(r)) => Decision { allowed: true, reason: "required", rule: Some(r), conflict },
        (None, None) => match default {
            DefaultPolicy::Allow => Decision { allowed: true, reason: "no_prohibition", rule: None, conflict },
            DefaultPolicy::Deny => Decision { allowed: false, reason: "default_deny_no_explicit_permission", rule: None, conflict },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(subject: &str, object: &str, modality: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("subject".into(), subject.into());
        m.insert("type".into(), "PERFORM".into());
        m.insert("object".into(), object.into());
        m.insert("modality".into(), modality.into());
        m
    }

    #[test]
    fn prohibition_denies_and_wins_over_obligation() {
        let h = vec![rel("bot", "wipe_db", "MUST"), rel("bot", "wipe_db", "MUST_NOT")];
        let d = decide("bot", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow);
        assert!(!d.allowed);
        assert_eq!((d.reason, d.rule.as_deref(), d.conflict), ("forbidden", Some("MUST_NOT"), true));
    }

    #[test]
    fn obligation_allows_and_unknown_action_follows_default() {
        let h = vec![rel("bot", "backup", "REQUIRE")];
        let d = decide("bot", Some("backup"), true, true, &h, DefaultPolicy::Deny);
        assert!(d.allowed && d.reason == "required");
        assert!(decide("bot", Some("read_logs"), true, true, &h, DefaultPolicy::Allow).allowed);
        let deny = decide("bot", Some("read_logs"), true, true, &h, DefaultPolicy::Deny);
        assert!(!deny.allowed && deny.reason == "default_deny_no_explicit_permission");
    }

    #[test]
    fn rules_are_scoped_to_the_sender() {
        let h = vec![rel("alice", "wipe_db", "MUST_NOT")];
        assert!(decide("bob", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow).allowed);
    }

    #[test]
    fn identity_and_action_are_checked_first() {
        let h = vec![rel("bot", "backup", "MUST")];
        assert_eq!(decide("bot", Some("backup"), false, false, &h, DefaultPolicy::Allow).reason, "sender_not_registered");
        assert_eq!(decide("bot", Some("backup"), false, true, &h, DefaultPolicy::Allow).reason, "signature_required");
        for bad in [None, Some(""), Some("a,b"), Some("x]y"), Some("with space")] {
            assert_eq!(decide("bot", bad, true, true, &h, DefaultPolicy::Allow).reason, "missing_or_invalid_action");
        }
    }
}
