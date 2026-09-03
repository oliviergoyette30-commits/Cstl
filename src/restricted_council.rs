//! src/restricted_council.rs — RestrictedCouncil (portée réduite, v1)
//!
//! Le README décrit un quorum humain 2/3 entre plusieurs membres. Cette
//! version est délibérément plus simple, par décision explicite de
//! l'utilisateur (2026-09-03): un seul membre autorisé, quorum=1.
//! Ce n'est PAS l'implémentation complète du quorum multi-personnes décrit
//! dans le README — à étendre si un vrai quorum est nécessaire plus tard.
//! Ce que ce module fait déjà de réel: c'est la seule porte d'entrée qui
//! permet à un `AdnStore::commit()`/`revoke()` d'être appelé par le serveur
//! en cours d'exécution — avant ce module, ces fonctions existaient mais
//! n'étaient jamais invoquées par aucun chemin de code.

pub struct RestrictedCouncil {
    authorized_members: Vec<String>,
}

impl RestrictedCouncil {
    pub fn new(authorized_members: Vec<String>) -> Self {
        Self { authorized_members }
    }

    /// Conseil à un seul membre — le cas d'usage actuel (bootstrap).
    pub fn single_member(name: &str) -> Self {
        Self { authorized_members: vec![name.to_string()] }
    }

    pub fn is_authorized(&self, sender: &str) -> bool {
        self.authorized_members.iter().any(|m| m == sender)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_member_authorizes_itself() {
        let council = RestrictedCouncil::single_member("Olivier");
        assert!(council.is_authorized("Olivier"));
    }

    #[test]
    fn test_single_member_rejects_others() {
        let council = RestrictedCouncil::single_member("Olivier");
        assert!(!council.is_authorized("quelqu_un_dautre"));
    }

    #[test]
    fn test_multi_member_council() {
        let council = RestrictedCouncil::new(vec!["a".to_string(), "b".to_string()]);
        assert!(council.is_authorized("a"));
        assert!(council.is_authorized("b"));
        assert!(!council.is_authorized("c"));
    }
}
