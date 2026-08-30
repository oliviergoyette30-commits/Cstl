/// CSTL-Native Server
/// Main entry point - starts TCP listener on port 5000

use cstl_parser::server::CstlNativeServer;
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use std::sync::Arc;

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
    });

    registry.register(AgentCard {
        name: "bob".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string(), "fact_checking".to_string()],
        trust_score: 0.85,
    });

    eprintln!("✅ Registered agents: alice, bob");

    // Create server with registry
    let mut server = CstlNativeServer::new(5000);
    server.agent_registry = Arc::new(registry);

    eprintln!("📡 Starting server on port 5000...");
    eprintln!("💬 Ready to receive CSTL payloads\n");

    // Start server
    server.start().await?;

    Ok(())
}
