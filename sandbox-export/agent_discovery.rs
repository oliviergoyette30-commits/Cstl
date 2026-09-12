//! Layer 7: Agent Discovery & Routing (CSTL Native)

pub struct AgentCard {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub trust_score: f64,
    /// Cle publique Ed25519 (hex, 64 caracteres) de cet agent -- `None` pour
    /// un agent legacy non signe (ex: alice/bob enregistres en dur dans
    /// main.rs). Voir src/signing.rs: quand cette cle est presente, une
    /// signature valide devient obligatoire sur chaque message de cet
    /// expediteur (Couche 2/securite, 2026-09-03).
    pub public_key: Option<String>,
}

pub struct AgentRegistry {
    pub agents: Vec<AgentCard>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        AgentRegistry { agents: vec![] }
    }

    /// Upsert par nom: un agent qui se reenregistre (ex: rotation de cle)
    /// remplace son entree existante plutot que d'en creer une seconde.
    /// Necessaire des que l'enregistrement devient dynamique (purpose=
    /// agent_register) -- avec l'ancien `push` inconditionnel, des doublons
    /// auraient rendu `route()` (max par trust_score) incoherent.
    /// Limite v1 assumee: aucune verification que le REENREGISTREMENT est
    /// autorise par le detenteur de l'ANCIENNE cle -- seule la possession
    /// de la NOUVELLE cle est prouvee (voir handler.rs, bloc agent_register).
    pub fn register(&mut self, card: AgentCard) {
        if let Some(existing) = self.agents.iter_mut().find(|a| a.name == card.name) {
            *existing = card;
        } else {
            self.agents.push(card);
        }
    }

    pub fn discover(&self, capability: &str) -> Vec<&AgentCard> {
        self.agents
            .iter()
            .filter(|a| a.capabilities.contains(&capability.to_string()))
            .collect()
    }

    pub fn route(&self, capability: &str) -> Option<&AgentCard> {
        self.discover(capability)
            .into_iter()
            .max_by(|a, b| a.trust_score.partial_cmp(&b.trust_score).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_discovery() {
        let mut registry = AgentRegistry::new();
        registry.register(AgentCard {
            name: "alice".to_string(),
            version: "5.0.0".to_string(),
            capabilities: vec!["fact_checking".to_string()],
            trust_score: 0.92,
            public_key: None,
        });

        let discovered = registry.discover("fact_checking");
        assert_eq!(discovered.len(), 1);

        let best = registry.route("fact_checking");
        assert_eq!(best.unwrap().name, "alice");
    }

    #[test]
    fn test_register_is_upsert_by_name_not_duplicate() {
        let mut registry = AgentRegistry::new();
        registry.register(AgentCard {
            name: "carol".to_string(),
            version: "1.0.0".to_string(),
            capabilities: vec!["communication".to_string()],
            trust_score: 0.5,
            public_key: Some("aaaa".to_string()),
        });
        registry.register(AgentCard {
            name: "carol".to_string(),
            version: "1.1.0".to_string(),
            capabilities: vec!["communication".to_string()],
            trust_score: 0.6,
            public_key: Some("bbbb".to_string()),
        });
        assert_eq!(registry.agents.len(), 1, "un reenregistrement ne doit pas dupliquer l'entree");
        let entry = registry.agents.iter().find(|a| a.name == "carol").unwrap();
        assert_eq!(entry.public_key.as_deref(), Some("bbbb"), "la derniere version doit gagner (rotation de cle)");
        assert_eq!(entry.trust_score, 0.6);
    }

    #[test]
    fn test_register_distinct_names_coexist() {
        let mut registry = AgentRegistry::new();
        registry.register(AgentCard {
            name: "dave".to_string(), version: "1.0.0".to_string(),
            capabilities: vec![], trust_score: 0.5, public_key: None,
        });
        registry.register(AgentCard {
            name: "erin".to_string(), version: "1.0.0".to_string(),
            capabilities: vec![], trust_score: 0.5, public_key: None,
        });
        assert_eq!(registry.agents.len(), 2);
    }
}
