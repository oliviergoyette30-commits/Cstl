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
pub mod router;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent_discovery::AgentRegistry;

pub struct CstlNativeServer {
    pub port: u16,
    pub agent_registry: Arc<AgentRegistry>,
    pub chain: Arc<Mutex<audit::HashChain>>,
}

impl CstlNativeServer {
    pub fn new(port: u16) -> Self {
        CstlNativeServer {
            port,
            agent_registry: Arc::new(AgentRegistry::new()),
            chain: Arc::new(Mutex::new(audit::HashChain::new())),
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("[CSTL-Native Server] Starting on port {}", self.port);
        
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = listener::create_listener(&addr).await?;
        
        eprintln!("[CSTL-Native Server] Listening on {}", addr);
        
        listener::accept_connections(listener, self.agent_registry.clone(), self.chain.clone()).await?;
        
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
