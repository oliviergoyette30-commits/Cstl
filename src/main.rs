//! CSTL-Native Server
//! Main entry point - starts TCP listener on port 5050 (5000 est souvent pris par AirPlay Receiver sur macOS)

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

    // Create server with registry -- try_with_data_path plutot que new()/with_data_path
    // (qui paniquent avec un backtrace Rust brut) pour que le processus puisse sortir
    // proprement avec un message actionnable si la base ADN est corrompue/verrouillee
    // au demarrage (trouvaille de l'audit du repo, 2026-09-04).
    let mut server = match CstlNativeServer::try_with_data_path(5050, "cstl_adn.db") {
        Ok(server) => server,
        Err(msg) => {
            eprintln!("❌ Demarrage impossible: {msg}");
            std::process::exit(1);
        }
    };
    server.agent_registry = Arc::new(Mutex::new(registry));

    eprintln!("📡 Starting server on port 5050...");
    eprintln!("💬 Ready to receive CSTL payloads\n");

    // Couche 5c: Start REST API server on port 8000 in parallel with TCP server
    // This allows HTTP access to audit trail while TCP server handles CSTL payloads
    let adn_store_for_rest = server.adn_store.clone();

    eprintln!("🌐 Starting REST API server on port 8000...");
    eprintln!("   GET /health → health check");
    eprintln!("   GET /audit/{{case_id}} → audit trail JSON");
    eprintln!("   POST /audit/query → filter audit by date/entity/action");
    eprintln!("   GET /audit/stats → audit statistics\n");

    let rest_api_handle = tokio::spawn(async move {
        if let Err(e) = cstl_parser::server::rest_api::start_rest_api(
            adn_store_for_rest,
            "127.0.0.1",
            8000,
        ).await {
            eprintln!("❌ REST API server error: {}", e);
        }
    });

    // Start TCP server (blocks until shutdown)
    let tcp_result = server.start().await;

    // Cancel REST API server if TCP server shuts down
    rest_api_handle.abort();

    tcp_result
}
