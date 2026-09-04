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

    /// Charge la liste des membres autorisés depuis `CSTL_COUNCIL_MEMBERS`
    /// (noms séparés par des virgules), même patron que
    /// `TelegramNotifier::from_env()`. Absent/vide -> `single_member("Olivier")`,
    /// comportement identique à avant cette fonction (aucune régression sur
    /// la config par défaut mono-membre).
    ///
    /// Note honnête (2026-09-04): ceci ne suffit PAS a lui seul a rendre le
    /// quorum multi-membres reellement securise -- ajouter un deuxieme nom
    /// ici sans plus n'aurait ete que du theatre de securite, puisque
    /// `is_authorized()` compare une simple chaine de caracteres.
    /// `handler.rs::handle_connection` (bloc `council_decision`) exige
    /// desormais, en plus de `is_authorized()`, une signature Ed25519
    /// valide ET que la cle publique embarquee dans le message corresponde
    /// EXACTEMENT a celle enregistree pour ce nom via `agent_register` --
    /// c'est cette combinaison, pas seulement la liste de noms ici, qui
    /// rend un vote de conseil reellement infalsifiable par un tiers
    /// connaissant simplement les noms des membres.
    pub fn from_env() -> Self {
        match std::env::var("CSTL_COUNCIL_MEMBERS") {
            Ok(raw) => Self::from_members_str(&raw),
            Err(_) => Self::single_member("Olivier"),
        }
    }

    /// Logique pure de `from_env()`, extraite pour etre testable sans
    /// manipuler de variables d'environnement (source de flakiness dans des
    /// tests paralleles).
    fn from_members_str(raw: &str) -> Self {
        let members: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if members.is_empty() {
            Self::single_member("Olivier")
        } else {
            Self::new(members)
        }
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

    #[test]
    fn test_from_members_str_parses_comma_separated_names() {
        let council = RestrictedCouncil::from_members_str("Olivier, Alice ,Bob");
        assert_eq!(council.member_count(), 3);
        assert!(council.is_authorized("Olivier"));
        assert!(council.is_authorized("Alice"));
        assert!(council.is_authorized("Bob"));
        assert_eq!(council.quorum_size(), 2);
    }

    #[test]
    fn test_from_members_str_empty_falls_back_to_single_olivier() {
        // Meme comportement que single_member("Olivier") si la variable
        // d'environnement existe mais est vide/blanche -- pas de regression
        // silencieuse vers un conseil a zero membre (quorum jamais atteignable).
        let council = RestrictedCouncil::from_members_str("");
        assert_eq!(council.member_count(), 1);
        assert!(council.is_authorized("Olivier"));

        let council2 = RestrictedCouncil::from_members_str("  ,  ,");
        assert_eq!(council2.member_count(), 1);
        assert!(council2.is_authorized("Olivier"));
    }
}
