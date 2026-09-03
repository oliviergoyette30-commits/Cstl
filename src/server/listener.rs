/// TCP Listener for CSTL payloads
use tokio::net::{TcpListener, TcpStream};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::agent_discovery::AgentRegistry;
use crate::kb_verify::KbVerifier;
use crate::adn_store::AdnStore;
use super::audit::HashChain;
use super::handler;

pub async fn create_listener(addr: &str) -> Result<TcpListener, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(addr).await?;
    Ok(listener)
}

pub async fn accept_connections(
    listener: TcpListener,
    agent_registry: Arc<AgentRegistry>,
    chain: Arc<Mutex<HashChain>>,
    kb_verifier: Arc<KbVerifier>,
    adn_store: Arc<Mutex<AdnStore>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let (socket, addr) = listener.accept().await?;
        eprintln!("[Server] New connection from {}", addr);
        
        let registry = agent_registry.clone();
        let chain = chain.clone();
        let kb_verifier = kb_verifier.clone();
        let adn_store = adn_store.clone();
        
        tokio::spawn(async move {
            if let Err(e) = handler::handle_connection(socket, registry, chain, kb_verifier, adn_store).await {
                eprintln!("[Server] Error handling connection: {}", e);
            }
        });
    }
}
