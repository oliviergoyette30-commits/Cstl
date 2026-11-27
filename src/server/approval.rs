//! Escalade vers un humain (2026-11-25).
//!
//! Une action listee dans `CSTL_APPROVAL_ACTIONS` (noms exacts ou `prefixe*`)
//! n'obtient son jeton qu'apres l'accord SIGNE d'un approbateur :
//!   1. l'agent demande l'action ; si les regles la permettent, la reponse est
//!      `decision=denied, reason=approval_required, approval_id=<id>` ;
//!   2. un approbateur (`CSTL_APPROVERS`, sinon les auteurs de politique de
//!      `CSTL_POLICY_AUTHORS`), jamais l'agent lui-meme, envoie
//!      `purpose=approval_decision, approval_id=<id>, decision=approve|reject` ;
//!   3. l'agent redemande la MEME action, pour le MEME montant, avec
//!      `approval_id=<id>` : les regles et l'arret d'urgence sont reevalues, et
//!      l'accord est consomme (usage unique). Il expire apres
//!      `CSTL_APPROVAL_TTL_SECS` (3600 par defaut) a compter de la demande.
//!
//! Sans approbateur configure, une action a accord n'est jamais permise
//! (prudence).

use crate::adn_store::ApprovalRow;

fn parse_patterns(v: Option<String>) -> Vec<String> {
    v.unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn matches(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => p == name,
    })
}

#[derive(Debug, Clone, Default)]
pub struct ApprovalPolicy {
    pub actions: Vec<String>,
    pub approvers: Vec<String>,
    pub ttl_secs: i64,
}

impl ApprovalPolicy {
    pub fn from_env() -> Self {
        let approvers = match parse_patterns(std::env::var("CSTL_APPROVERS").ok()) {
            v if !v.is_empty() => v,
            _ => parse_patterns(std::env::var("CSTL_POLICY_AUTHORS").ok()),
        };
        ApprovalPolicy {
            actions: parse_patterns(std::env::var("CSTL_APPROVAL_ACTIONS").ok()),
            approvers,
            ttl_secs: std::env::var("CSTL_APPROVAL_TTL_SECS").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(3600),
        }
    }

    pub fn requires(&self, action: &str) -> bool {
        matches(&self.actions, action)
    }

    pub fn is_approver(&self, name: &str) -> bool {
        matches(&self.approvers, name)
    }

    /// Un accord peut-il servir pour CETTE demande ? Err : la raison du refus.
    pub fn check(&self, row: &ApprovalRow, agent: &str, action: &str, amount: Option<f64>, now: i64) -> Result<(), &'static str> {
        let same_amount = match (row.amount.as_deref().and_then(|a| a.parse::<f64>().ok()), amount) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if row.agent != agent || row.action != action || !same_amount {
            Err("approval_mismatch")
        } else if now > row.requested_at + self.ttl_secs {
            Err("approval_expired")
        } else if row.status == "pending" {
            Err("approval_pending")
        } else if row.status != "approved" {
            Err("approval_rejected")
        } else if row.consumed {
            Err("approval_used")
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str, consumed: bool) -> ApprovalRow {
        ApprovalRow {
            id: "a".into(), agent: "bot".into(), action: "wipe_db".into(), amount: Some("80".into()),
            requested_at: 100, status: status.into(), decided_by: Some("alice".into()), reason: None, consumed,
        }
    }

    #[test]
    fn an_approval_serves_once_for_the_same_request_only() {
        let p = ApprovalPolicy { actions: vec!["wipe_*".into()], approvers: vec!["human_*".into()], ttl_secs: 60 };
        assert!(p.requires("wipe_db") && !p.requires("read_logs"));
        assert!(p.is_approver("human_alice") && !p.is_approver("bot"));
        assert_eq!(p.check(&row("approved", false), "bot", "wipe_db", Some(80.0), 120), Ok(()));
        assert_eq!(p.check(&row("approved", false), "bot", "wipe_db", Some(8000.0), 120), Err("approval_mismatch"));
        assert_eq!(p.check(&row("approved", false), "bot", "wipe_db", None, 120), Err("approval_mismatch"));
        assert_eq!(p.check(&row("approved", false), "other", "wipe_db", Some(80.0), 120), Err("approval_mismatch"));
        assert_eq!(p.check(&row("approved", false), "bot", "drop_db", Some(80.0), 120), Err("approval_mismatch"));
        assert_eq!(p.check(&row("approved", false), "bot", "wipe_db", Some(80.0), 161), Err("approval_expired"));
        assert_eq!(p.check(&row("pending", false), "bot", "wipe_db", Some(80.0), 120), Err("approval_pending"));
        assert_eq!(p.check(&row("rejected", false), "bot", "wipe_db", Some(80.0), 120), Err("approval_rejected"));
        assert_eq!(p.check(&row("approved", true), "bot", "wipe_db", Some(80.0), 120), Err("approval_used"));
    }

    #[test]
    fn no_approver_configured_means_nobody_approves() {
        let p = ApprovalPolicy { actions: vec!["wipe_db".into()], approvers: vec![], ttl_secs: 60 };
        assert!(!p.is_approver("anyone"));
    }
}
