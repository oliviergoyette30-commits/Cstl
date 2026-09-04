/// TCP Listener for CSTL payloads
use tokio::net::TcpListener;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::agent_discovery::AgentRegistry;
use crate::kb_verify::KbVerifier;
use crate::adn_store::AdnStore;
use crate::restricted_council::RestrictedCouncil;
use crate::telegram_council::TelegramNotifier;
use crate::obsidian_escalation::ObsidianEscalation;
use crate::governance::GovernanceTracker;
use super::audit::HashChain;
use super::audit_store::AuditStore;
use super::handler;

pub async fn create_listener(addr: &str) -> Result<TcpListener, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(addr).await?;
    Ok(listener)
}

pub async fn accept_connections(
    listener: TcpListener,
    agent_registry: Arc<Mutex<AgentRegistry>>,
    chain: Arc<Mutex<HashChain>>,
    audit_store: Arc<Mutex<AuditStore>>,
    kb_verifier: Arc<KbVerifier>,
    adn_store: Arc<Mutex<AdnStore>>,
    restricted_council: Arc<RestrictedCouncil>,
    telegram: Option<Arc<TelegramNotifier>>,
    obsidian: Option<Arc<ObsidianEscalation>>,
    governance: Arc<Mutex<GovernanceTracker>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let (socket, addr) = listener.accept().await?;
        eprintln!("[Server] New connection from {}", addr);

        let registry = agent_registry.clone();
        let chain = chain.clone();
        let audit_store = audit_store.clone();
        let kb_verifier = kb_verifier.clone();
        let adn_store = adn_store.clone();
        let restricted_council = restricted_council.clone();
        let telegram = telegram.clone();
        let obsidian = obsidian.clone();
        let governance = governance.clone();

        tokio::spawn(async move {
            if let Err(e) = handler::handle_connection(socket, registry, chain, audit_store, kb_verifier, adn_store, restricted_council, telegram, obsidian, governance).await {
                eprintln!("[Server] Error handling connection: {}", e);
            }
        });
    }
}
