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
//!
//! AUTORITE ET REVISION (2026-11-20, apres le banc d'essai d'equipe
//! `scripts/team_task_benchmark.py`, ou la couche 9 a refuse 3 actions
//! correctes) : avec `CSTL_POLICY_AUTHORS=nom1,nom2,prefixe*`,
//! - seules comptent les regles affirmees par un auteur de cette liste, dans
//!   un message dont la signature est valide ET faite avec la cle enregistree
//!   pour cet auteur (`Rule::authorized`, calcule par l'appelant) ; les autres
//!   sont ignorees et comptees (`ignored`) ;
//! - la regle autorisee la PLUS RECENTE decide (une interdiction peut donc
//!   etre levee par l'autorite, et une obligation retiree) ; `superseded`
//!   signale qu'une regle contraire plus ancienne a ete remplacee.
//!
//! Sans `CSTL_POLICY_AUTHORS`, comportement d'origine : toute regle compte et
//! l'interdiction l'emporte toujours. La revision n'est PAS activee dans ce
//! mode : si n'importe qui pouvait reviser, n'importe qui pourrait lever une
//! interdiction en envoyant simplement un MUST plus recent.

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

/// Qui peut fixer les regles qui comptent pour la couche 9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAuthors {
    /// Toute regle compte ; l'interdiction l'emporte (comportement d'origine).
    Anyone,
    /// Seules comptent les regles prouvees de ces auteurs (`nom` exact ou
    /// `prefixe*`) ; la plus recente decide.
    Only(Vec<String>),
}

impl PolicyAuthors {
    pub fn from_env() -> Self {
        Self::parse(std::env::var("CSTL_POLICY_AUTHORS").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        let names: Vec<String> = value
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if names.is_empty() { PolicyAuthors::Anyone } else { PolicyAuthors::Only(names) }
    }

    /// Le nom fait-il partie des auteurs de politique ? (`Anyone` : oui.)
    pub fn includes(&self, name: &str) -> bool {
        match self {
            PolicyAuthors::Anyone => true,
            PolicyAuthors::Only(list) => list.iter().any(|p| match p.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => p == name,
            }),
        }
    }
}

/// Une regle deontique persistee, dans l'ordre ou elle a ete affirmee
/// (la plus ancienne d'abord).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub subject: String,
    pub object: String,
    pub modality: String,
    /// Auteur dans `PolicyAuthors` ET identite prouvee (signature valide,
    /// cle = cle enregistree de l'auteur). Ignore en mode `Anyone`.
    pub authorized: bool,
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
    /// Mode `Only` : une regle contraire plus ancienne a ete remplacee.
    pub superseded: bool,
    /// Mode `Only` : regles sur cette action ignorees (auteur non autorise
    /// ou identite non prouvee).
    pub ignored: usize,
}

impl Decision {
    fn deny(reason: &'static str) -> Self {
        Decision { allowed: false, reason, rule: None, conflict: false, superseded: false, ignored: 0 }
    }

    fn from_rule(modality: String, conflict: bool, superseded: bool, ignored: usize) -> Self {
        let allowed = !FORBIDDEN_MODALITIES.contains(&modality.as_str());
        let reason = if allowed { "required" } else { "forbidden" };
        Decision { allowed, reason, rule: Some(modality), conflict, superseded, ignored }
    }
}

fn is_deontic(modality: &str) -> bool {
    FORBIDDEN_MODALITIES.contains(&modality) || REQUIRED_MODALITIES.contains(&modality)
}

pub fn is_valid_action(action: &str) -> bool {
    !action.is_empty()
        && action.len() <= 128
        && action.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `rules` : regles deontiques persistees, la plus ancienne d'abord
/// (`AdnStore::deontic_rules_with_authors`).
pub fn decide(
    sender: &str,
    action: Option<&str>,
    identity_proven: bool,
    sender_registered: bool,
    rules: &[Rule],
    default: DefaultPolicy,
    authors: &PolicyAuthors,
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
    let relevant = rules
        .iter()
        .filter(|r| r.subject == sender && r.object == action && is_deontic(&r.modality));

    let fallback = |conflict, ignored| {
        let (allowed, reason) = match default {
            DefaultPolicy::Allow => (true, "no_prohibition"),
            DefaultPolicy::Deny => (false, "default_deny_no_explicit_permission"),
        };
        Decision { allowed, reason, rule: None, conflict, superseded: false, ignored }
    };

    match authors {
        PolicyAuthors::Anyone => {
            let mut forbidden: Option<String> = None;
            let mut required: Option<String> = None;
            for r in relevant {
                if FORBIDDEN_MODALITIES.contains(&r.modality.as_str()) {
                    forbidden.get_or_insert_with(|| r.modality.clone());
                } else {
                    required.get_or_insert_with(|| r.modality.clone());
                }
            }
            let conflict = forbidden.is_some() && required.is_some();
            match forbidden.or(required) {
                Some(m) => Decision::from_rule(m, conflict, false, 0),
                None => fallback(conflict, 0),
            }
        }
        PolicyAuthors::Only(_) => {
            let (kept, ignored): (Vec<&Rule>, Vec<&Rule>) = relevant.partition(|r| r.authorized);
            let Some(latest) = kept.last() else { return fallback(false, ignored.len()) };
            let latest_forbids = FORBIDDEN_MODALITIES.contains(&latest.modality.as_str());
            let superseded = kept
                .iter()
                .any(|r| FORBIDDEN_MODALITIES.contains(&r.modality.as_str()) != latest_forbids);
            Decision::from_rule(latest.modality.clone(), false, superseded, ignored.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(subject: &str, object: &str, modality: &str) -> Rule {
        Rule { subject: subject.into(), object: object.into(), modality: modality.into(), authorized: true }
    }

    fn by(authorized: bool, r: Rule) -> Rule {
        Rule { authorized, ..r }
    }

    const ANY: PolicyAuthors = PolicyAuthors::Anyone;

    #[test]
    fn prohibition_denies_and_wins_over_obligation() {
        let h = vec![rel("bot", "wipe_db", "MUST"), rel("bot", "wipe_db", "MUST_NOT")];
        let d = decide("bot", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY);
        assert!(!d.allowed);
        assert_eq!((d.reason, d.rule.as_deref(), d.conflict), ("forbidden", Some("MUST_NOT"), true));
        // Mode d'origine : un MUST PLUS RECENT ne leve pas l'interdiction.
        let h = vec![rel("bot", "wipe_db", "MUST_NOT"), rel("bot", "wipe_db", "MUST")];
        assert!(!decide("bot", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY).allowed);
    }

    #[test]
    fn obligation_allows_and_unknown_action_follows_default() {
        let h = vec![rel("bot", "backup", "REQUIRE")];
        let d = decide("bot", Some("backup"), true, true, &h, DefaultPolicy::Deny, &ANY);
        assert!(d.allowed && d.reason == "required");
        assert!(decide("bot", Some("read_logs"), true, true, &h, DefaultPolicy::Allow, &ANY).allowed);
        let deny = decide("bot", Some("read_logs"), true, true, &h, DefaultPolicy::Deny, &ANY);
        assert!(!deny.allowed && deny.reason == "default_deny_no_explicit_permission");
    }

    #[test]
    fn rules_are_scoped_to_the_sender() {
        let h = vec![rel("alice", "wipe_db", "MUST_NOT")];
        assert!(decide("bob", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY).allowed);
    }

    #[test]
    fn identity_and_action_are_checked_first() {
        let h = vec![rel("bot", "backup", "MUST")];
        assert_eq!(decide("bot", Some("backup"), false, false, &h, DefaultPolicy::Allow, &ANY).reason, "sender_not_registered");
        assert_eq!(decide("bot", Some("backup"), false, true, &h, DefaultPolicy::Allow, &ANY).reason, "signature_required");
        for bad in [None, Some(""), Some("a,b"), Some("x]y"), Some("with space")] {
            assert_eq!(decide("bot", bad, true, true, &h, DefaultPolicy::Allow, &ANY).reason, "missing_or_invalid_action");
        }
    }

    #[test]
    fn authority_can_lift_a_prohibition_with_a_newer_rule() {
        // Cas 4 et 10 du banc d'equipe.
        let only = PolicyAuthors::parse(Some("policy_owner"));
        let h = vec![rel("bot", "rotate_keys", "MUST_NOT"), rel("bot", "rotate_keys", "MUST")];
        let d = decide("bot", Some("rotate_keys"), true, true, &h, DefaultPolicy::Allow, &only);
        assert!(d.allowed && d.reason == "required" && d.superseded && !d.conflict, "{d:?}");
        // ... et la reimposer ensuite.
        let h = [h, vec![rel("bot", "rotate_keys", "FORBID")]].concat();
        let d = decide("bot", Some("rotate_keys"), true, true, &h, DefaultPolicy::Allow, &only);
        assert!(!d.allowed && d.rule.as_deref() == Some("FORBID"));
    }

    #[test]
    fn rules_from_unauthorized_or_unproven_authors_are_ignored() {
        // Cas 11 (interdiction posee par ops_bot) et cas 3 (obligation posee par ops_bot).
        let only = PolicyAuthors::parse(Some("policy_owner"));
        let h = vec![rel("bot", "check_backups", "MUST"), by(false, rel("bot", "check_backups", "MUST_NOT"))];
        let d = decide("bot", Some("check_backups"), true, true, &h, DefaultPolicy::Allow, &only);
        assert!(d.allowed && d.ignored == 1 && !d.superseded, "{d:?}");
        let h = vec![rel("bot", "deploy_prod", "MUST_NOT"), by(false, rel("bot", "deploy_prod", "MUST"))];
        assert!(!decide("bot", Some("deploy_prod"), true, true, &h, DefaultPolicy::Allow, &only).allowed);
        // Seules des regles ignorees : la politique par defaut s'applique.
        let h = vec![by(false, rel("bot", "x", "MUST"))];
        let d = decide("bot", Some("x"), true, true, &h, DefaultPolicy::Deny, &only);
        assert!(!d.allowed && d.reason == "default_deny_no_explicit_permission" && d.ignored == 1);
    }

    #[test]
    fn policy_authors_parsing() {
        assert_eq!(PolicyAuthors::parse(None), PolicyAuthors::Anyone);
        assert_eq!(PolicyAuthors::parse(Some(" , ")), PolicyAuthors::Anyone);
        let p = PolicyAuthors::parse(Some("policy_owner, redacteur_*"));
        assert!(p.includes("policy_owner") && p.includes("redacteur_c1") && !p.includes("ops_bot"));
        assert!(!p.includes("policy_owner2"));
    }
}
