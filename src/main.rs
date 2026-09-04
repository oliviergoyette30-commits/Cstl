/// CSTL-Native Server
/// Main entry point - starts TCP listener on port 5050 (5000 est souvent pris par AirPlay Receiver sur macOS)

use cstl_parser::server::CstlNativeServer;
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("🚀 CSTL-Native Server v1.0");
    eprintln!("===========================");

    // Create registry and register agents
    let mut registry = AgentRegistry::new();
    
    registry.register(AgentCard {
        name: "alice".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string(), "arbitration".to_string()],
        trust_score: 0.95,
        // Legacy, non signe -- ces deux agents bootstrap n'ont jamais eu de
        // cle: aucune signature n'est exigee pour eux (voir src/signing.rs).
        public_key: None,
    });

    registry.register(AgentCard {
        name: "bob".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string(), "fact_checking".to_string()],
        trust_score: 0.85,
        public_key: None,
    });

    eprintln!("✅ Registered agents: alice, bob");

    // Create server with registry
    let mut server = CstlNativeServer::new(5050);
    server.agent_registry = Arc::new(Mutex::new(registry));

    eprintln!("📡 Starting server on port 5050...");
    eprintln!("💬 Ready to receive CSTL payloads\n");

    // Start server
    server.start().await?;

    Ok(())
}
