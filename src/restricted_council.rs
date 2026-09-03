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

    /// Nombre de membres autorisés.
    pub fn member_count(&self) -> usize {
        self.authorized_members.len()
    }

    /// Taille du quorum 2/3 (Couche 2, gouvernance) — ceil(2/3 * n).
    /// n=1 (config actuelle, un seul membre "Olivier") -> quorum_size()=1,
    /// identique au comportement d'avant cette fonction: aucune régression.
    /// ceil(a/b) = (a+b-1)/b avec a=2n, b=3.
    pub fn quorum_size(&self) -> usize {
        let n = self.member_count().max(1);
        (2 * n + 2) / 3
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

    #[test]
    fn test_quorum_size_single_member_is_one_no_regression() {
        // n=1 -> quorum_size()=1: doit rester identique au comportement
        // "un commit suffit" d'avant l'ajout du quorum multi-membres.
        let council = RestrictedCouncil::single_member("Olivier");
        assert_eq!(council.quorum_size(), 1);
    }

    #[test]
    fn test_quorum_size_two_members_is_two() {
        let council = RestrictedCouncil::new(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(council.quorum_size(), 2);
    }

    #[test]
    fn test_quorum_size_three_members_is_two() {
        let council = RestrictedCouncil::new(
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(council.quorum_size(), 2);
    }

    #[test]
    fn test_quorum_size_four_members_is_three() {
        let council = RestrictedCouncil::new(vec![
            "a".to_string(), "b".to_string(), "c".to_string(), "d".to_string(),
        ]);
        assert_eq!(council.quorum_size(), 3);
    }
}
