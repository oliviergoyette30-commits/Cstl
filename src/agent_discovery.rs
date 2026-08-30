/// Layer 7: Agent Discovery & Routing (CSTL Native)

pub struct AgentCard {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub trust_score: f64,
}

pub struct AgentRegistry {
    pub agents: Vec<AgentCard>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        AgentRegistry { agents: vec![] }
    }

    pub fn register(&mut self, card: AgentCard) {
        self.agents.push(card);
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
        });

        let discovered = registry.discover("fact_checking");
        assert_eq!(discovered.len(), 1);

        let best = registry.route("fact_checking");
        assert_eq!(best.unwrap().name, "alice");
    }
}
