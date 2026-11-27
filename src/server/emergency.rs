//! Arret d'urgence et revocation des cles (2026-11-25).
//!
//! Arret d'urgence : tant qu'il est en vigueur, toute `action_request` qu'il
//! couvre est refusee (`reason=emergency_stop`), quelles que soient les
//! regles. Deux sources :
//!   - un auteur de politique (`CSTL_POLICY_AUTHORS`), par un message signe
//!     `purpose=emergency_stop, scope=..., state=on|off, policy_seq=...` ;
//!     portee `all`, `agent:<nom>` ou `action:<motif>` (`refund_*`) ;
//!   - l'operateur de la machine, par un FICHIER : s'il existe, tout est
//!     arrete. Aucun agent ne peut le lever, meme avec une cle d'auteur volee.
//!     Par defaut `<base>.emergency_stop` (a cote de `<base>.server_key`), ou
//!     `CSTL_EMERGENCY_STOP_FILE`.
//!
//! Revocation : voir `handler.rs` (`purpose=key_revoke`, `key_enroll`).

use std::path::{Path, PathBuf};

/// Fichier d'arret de l'operateur.
pub fn stop_file(server_key_path: Option<&Path>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CSTL_EMERGENCY_STOP_FILE") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let key = server_key_path?.to_str()?;
    Some(PathBuf::from(format!("{}.emergency_stop", key.strip_suffix(".server_key").unwrap_or(key))))
}

/// Raison de l'arret de l'operateur (premiere ligne du fichier), s'il existe.
pub fn operator_stop(file: Option<&Path>) -> Option<String> {
    let file = file?;
    if !file.exists() {
        return None;
    }
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let first: String = text.lines().next().unwrap_or("").chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
        .take(120).collect();
    Some(if first.trim().is_empty() { "operator".to_string() } else { first.trim().to_string() })
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `all`, `agent:<nom>` ou `action:<nom>` / `action:<prefixe>*`.
pub fn valid_scope(scope: &str) -> bool {
    if scope == "all" {
        return true;
    }
    if let Some(a) = scope.strip_prefix("agent:") {
        return valid_name(a);
    }
    if let Some(p) = scope.strip_prefix("action:") {
        let base = p.strip_suffix('*').unwrap_or(p);
        return valid_name(base) || (p == "*");
    }
    false
}

/// Premier arret qui couvre cet agent et cette action.
pub fn covering<'a>(scopes: impl IntoIterator<Item = &'a str>, agent: &str, action: Option<&str>) -> Option<&'a str> {
    scopes.into_iter().find(|s| applies(s, agent, action))
}

pub fn applies(scope: &str, agent: &str, action: Option<&str>) -> bool {
    if scope == "all" {
        return true;
    }
    if let Some(a) = scope.strip_prefix("agent:") {
        return a == agent;
    }
    if let Some(p) = scope.strip_prefix("action:") {
        // Action absente ou invalide : un arret par action s'applique (prudence).
        let Some(action) = action else { return true };
        return match p.strip_suffix('*') {
            Some(prefix) => action.starts_with(prefix),
            None => p == action,
        };
    }
    false
}

/// Detection de comportement anormal (2026-11-25) : plus de
/// `CSTL_ANOMALY_MAX_PER_MIN` demandes d'action d'un MEME agent en 60 s
/// declenchent un arret d'urgence `agent:<nom>`, pose par
/// `anomaly_detector` et persiste. Seul un auteur de politique le leve.
pub fn anomaly_max_per_min() -> Option<usize> {
    std::env::var("CSTL_ANOMALY_MAX_PER_MIN").ok().and_then(|v| v.parse().ok()).filter(|v: &usize| *v > 0)
}

static WINDOWS: std::sync::Mutex<Option<std::collections::HashMap<String, std::collections::VecDeque<i64>>>> =
    std::sync::Mutex::new(None);

/// Un auteur leve l'arret de `agent` : son compteur repart de zero (sinon la
/// demande suivante le redeclencherait aussitot).
pub fn reset_window(agent: &str) {
    let mut guard = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_mut() {
        map.remove(agent);
    }
}

/// Compte une demande de `agent` a `now` ; true si la limite est depassee.
pub fn record_request(agent: &str, now: i64, max: usize) -> bool {
    let mut guard = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(Default::default);
    let w = map.entry(agent.to_string()).or_default();
    while w.front().is_some_and(|t| *t <= now - 60) {
        w.pop_front();
    }
    w.push_back(now);
    while w.len() > max + 1 {
        w.pop_front();
    }
    w.len() > max
}

#[cfg(test)]
mod tests {
    #[test]
    fn too_many_requests_in_a_minute_trip_the_detector() {
        let a = format!("anomaly_test_{}", std::process::id());
        for i in 0..3 {
            assert!(!record_request(&a, 1000 + i, 3));
        }
        assert!(record_request(&a, 1003, 3), "4e demande en 60 s");
        assert!(!record_request(&a, 2000, 3), "une minute plus tard : fenetre videe");
        assert!(!record_request("someone_else", 1003, 3));
    }

    use super::*;

    #[test]
    fn scopes_are_validated_and_matched() {
        for ok in ["all", "agent:bot_1", "action:refund_order", "action:refund_*", "action:*"] {
            assert!(valid_scope(ok), "{ok}");
        }
        for bad in ["", "ALL", "agent:", "agent:a b", "action:", "action:x,y", "role:x"] {
            assert!(!valid_scope(bad), "{bad}");
        }
        assert!(applies("all", "a", Some("x")));
        assert!(applies("agent:a", "a", Some("x")));
        assert!(!applies("agent:a", "b", Some("x")));
        assert!(applies("action:refund_*", "a", Some("refund_order")));
        assert!(!applies("action:refund_*", "a", Some("read_logs")));
        assert!(applies("action:wipe_db", "a", None), "action inconnue : l'arret s'applique");
        assert_eq!(covering(["agent:b", "action:wipe_db"], "a", Some("wipe_db")), Some("action:wipe_db"));
    }

    #[test]
    fn operator_file_stops_everything_and_gives_its_reason() {
        let dir = std::env::temp_dir().join(format!("cstl_stop_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("cstl_adn.db.server_key");
        let file = stop_file(Some(&key)).unwrap();
        assert_eq!(file, dir.join("cstl_adn.db.emergency_stop"));
        assert_eq!(operator_stop(Some(&file)), None);
        std::fs::write(&file, "incident_42, \"quote\"]\nautre ligne").unwrap();
        assert_eq!(operator_stop(Some(&file)).as_deref(), Some("incident_42 quote"));
        std::fs::write(&file, "").unwrap();
        assert_eq!(operator_stop(Some(&file)).as_deref(), Some("operator"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
