/// TCP Listener for CSTL payloads
use tokio::net::{TcpListener, TcpStream};
use std::sync::Arc;
use crate::agent_discovery::AgentRegistry;
use super::handler;

pub async fn create_listener(addr: &str) -> Result<TcpListener, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(addr).await?;
    Ok(listener)
}

pub async fn accept_connections(
    listener: TcpListener,
    agent_registry: Arc<AgentRegistry>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let (socket, addr) = listener.accept().await?;
        eprintln!("[Server] New connection from {}", addr);
        
        let registry = agent_registry.clone();
        
        tokio::spawn(async move {
            if let Err(e) = handler::handle_connection(socket, registry).await {
                eprintln!("[Server] Error handling connection: {}", e);
            }
        });
    }
}
