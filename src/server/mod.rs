/// CSTL-Native Server
/// Agent-to-agent communication natively in CSTL
/// 
/// Architecture:
/// 1. TCP Listener (incoming CSTL payloads)
/// 2. Parser (validates CSTL format)
/// 3. Validator (semantic checking)
/// 4. Router (finds destination agent)
/// 5. Audit Trail (immutable SHA-256 record)

pub mod listener;
pub mod handler;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent_discovery::AgentRegistry;
use crate::kb_verify::KbVerifier;
use crate::adn_store::AdnStore;
use crate::restricted_council::RestrictedCouncil;
use crate::telegram_council::TelegramNotifier;
use crate::obsidian_escalation::ObsidianEscalation;
use crate::governance::GovernanceTracker;

pub struct CstlNativeServer {
    pub port: u16,
    /// Mutable derriere un verrou depuis l'ajout de purpose=agent_register
    /// (2026-09-04) -- avant ca, le registre etait fige a la compilation
    /// (alice/bob en dur dans main.rs), aucun agent ne pouvait s'inscrire
    /// au runtime.
    pub agent_registry: Arc<Mutex<AgentRegistry>>,
    pub chain: Arc<Mutex<audit::HashChain>>,
    pub kb_verifier: Arc<KbVerifier>,
    pub adn_store: Arc<Mutex<AdnStore>>,
    pub restricted_council: Arc<RestrictedCouncil>,
    pub telegram: Option<Arc<TelegramNotifier>>,
    pub obsidian: Option<Arc<ObsidianEscalation>>,
    /// Couche 2 (gouvernance/resilience) -- circuit breaker + drift
    /// d'operateur, observation seule (voir src/governance.rs). Etat en
    /// memoire uniquement, perdu au redemarrage -- limite v1 assumee.
    pub governance: Arc<Mutex<GovernanceTracker>>,
}

impl CstlNativeServer {
    pub fn new(port: u16) -> Self {
        CstlNativeServer {
            port,
            agent_registry: Arc::new(Mutex::new(AgentRegistry::new())),
            chain: Arc::new(Mutex::new(audit::HashChain::new())),
            kb_verifier: Arc::new(KbVerifier::new()),
            adn_store: Arc::new(Mutex::new(AdnStore::open("cstl_adn.db").expect("failed to open cstl_adn.db"))),
            // Portee reduite v1, decision explicite de l'utilisateur: un seul membre
            // autorise pour bootstrap le systeme, pas le quorum 2/3 multi-personnes
            // decrit dans le README.
            restricted_council: Arc::new(RestrictedCouncil::single_member("Olivier")),
            // None si TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID absents de l'environnement -
            // degradation propre, le serveur marche pareil sans notification.
            telegram: TelegramNotifier::from_env().map(Arc::new),
            // None si OBSIDIAN_VAULT_PATH absent de l'environnement - degradation
            // propre, le serveur marche pareil sans escalade Obsidian.
            obsidian: ObsidianEscalation::from_env().map(Arc::new),
            governance: Arc::new(Mutex::new(GovernanceTracker::with_defaults())),
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("[CSTL-Native Server] Starting on port {}", self.port);

        if let Some(telegram) = &self.telegram {
            eprintln!("[CSTL-Native Server] Telegram poller actif");
            let telegram = telegram.clone();
            let adn_store = self.adn_store.clone();
            let restricted_council = self.restricted_council.clone();
            tokio::spawn(async move {
                crate::telegram_council::run_telegram_poller(telegram, adn_store, restricted_council).await;
            });
        } else {
            eprintln!("[CSTL-Native Server] Telegram desactive (TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID absents)");
        }
        
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = listener::create_listener(&addr).await?;
        
        eprintln!("[CSTL-Native Server] Listening on {}", addr);
        
        listener::accept_connections(listener, self.agent_registry.clone(), self.chain.clone(), self.kb_verifier.clone(), self.adn_store.clone(), self.restricted_council.clone(), self.telegram.clone(), self.obsidian.clone(), self.governance.clone()).await?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = CstlNativeServer::new(5000);
        assert_eq!(server.port, 5000);
    }
}

pub mod parser;

pub mod validator;

pub mod audit;

pub mod audit_store;
