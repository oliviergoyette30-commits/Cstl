//! CSTL-Native Server
//! Main entry point - starts TCP listener on port 5050 (5000 est souvent pris par AirPlay Receiver sur macOS)

use cstl_parser::server::CstlNativeServer;
use cstl_parser::agent_discovery::AgentCard;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Backend reel pour log::info!/warn!/error! (arbitrage.rs, quorum_wire.rs,
    // handler.rs) -- sans ca ces appels ne vont nulle part (trouvaille de
    // l'audit du 15 octobre 2026, Cargo.toml 2026-10-29). RUST_LOG=info par
    // defaut si la variable d'environnement n'est pas definie, pour que les
    // logs apparaissent sans configuration prealable.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    eprintln!("🚀 CSTL-Native Server v1.0");
    eprintln!("===========================");

    // Create server with registry -- try_with_data_path plutot que new()/with_data_path
    // (qui paniquent avec un backtrace Rust brut) pour que le processus puisse sortir
    // proprement avec un message actionnable si la base ADN est corrompue/verrouillee
    // au demarrage (trouvaille de l'audit du repo, 2026-09-04).
    // Ports configurables (2026-11-20) : CSTL_PORT (TCP, defaut 5050) et
    // CSTL_REST_PORT (API REST, defaut 8000). Codes en dur avant : deux
    // serveurs ne pouvaient pas tourner sur la meme machine.
    let port_from_env = |name: &str, default: u16| -> u16 {
        match std::env::var(name) {
            Ok(v) if !v.is_empty() => v.parse().unwrap_or_else(|_| {
                eprintln!("❌ {name}={v} n'est pas un port valide");
                std::process::exit(1);
            }),
            _ => default,
        }
    };
    // `cstl_parser --rotate-server-key` (2026-11-21) : remplace la cle du
    // serveur, puis s'arrete. L'historique est d'abord verifie avec la cle
    // actuelle (try_with_data_path refuse un historique compromis) ; les
    // maillons deja ecrits restent verifiables avec l'ancienne cle, les
    // suivants seront signes par la nouvelle.
    if std::env::args().any(|a| a == "--rotate-server-key") {
        let server = match CstlNativeServer::try_with_data_path(0, "cstl_adn.db") {
            Ok(s) => s,
            Err(msg) => {
                eprintln!("❌ Rotation impossible : {msg}");
                std::process::exit(1);
            }
        };
        let at_seq = server.chain.lock().await.entries.len() as u64;
        let result = server.server_key.read_audit_marker().and_then(|marker| {
            let new = server.server_key.rotate(at_seq)?;
            if let Some(m) = marker {
                new.write_audit_marker(&m)?;
            }
            Ok(new)
        });
        match result {
            Ok(new) => {
                eprintln!("✅ Cle du serveur remplacee a seq={at_seq}");
                eprintln!("   ancienne : {}", server.server_key.public_hex());
                eprintln!("   nouvelle : {}", new.public_hex());
                eprintln!("   A mettre a jour : CSTL_SERVER_PUBLIC_KEY, la cle des gardes (CstlGuard) et celle du temoin (--server-key).");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("❌ Rotation impossible : {e}");
                std::process::exit(1);
            }
        }
    }
    let tcp_port = port_from_env("CSTL_PORT", 5050);
    let rest_port = port_from_env("CSTL_REST_PORT", 8000);
    let server = match CstlNativeServer::try_with_data_path(tcp_port, "cstl_adn.db") {
        Ok(server) => server,
        Err(msg) => {
            eprintln!("❌ Demarrage impossible: {msg}");
            std::process::exit(1);
        }
    };
    // Agents de demonstration, SEULEMENT en mode developpement (2026-11-21),
    // et AJOUTES au registre recharge depuis la base. Avant, ce bloc
    // REMPLACAIT ce registre : apres chaque redemarrage du binaire, tous les
    // agents enregistres avec une cle etaient oublies, et n'importe qui
    // pouvait reprendre leur nom avec sa propre cle (verifie en direct).
    if cstl_parser::server::dev_mode() {
        let mut registry = server.agent_registry.write().await;
        for (name, caps, trust) in [("alice", ["communication", "arbitration"], 0.95), ("bob", ["communication", "fact_checking"], 0.85)] {
            if !registry.agents.iter().any(|a| a.name == name) {
                registry.register(AgentCard {
                    name: name.to_string(),
                    version: "5.0.0".to_string(),
                    capabilities: caps.iter().map(|c| c.to_string()).collect(),
                    trust_score: trust,
                    // Sans cle : agents de demonstration, mode developpement uniquement.
                    public_key: None,
                });
            }
        }
        eprintln!("✅ CSTL_DEV_MODE : agents de demonstration alice, bob (sans cle)");
    }
    let known = server.agent_registry.read().await.agents.len();
    eprintln!("✅ {known} agent(s) connu(s) (recharges depuis la base)");

    eprintln!("📡 Starting server on port {tcp_port}...");
    eprintln!("💬 Ready to receive CSTL payloads\n");

    // Couche 5c: Start REST API server on port 8000 in parallel with TCP server
    // This allows HTTP access to audit trail while TCP server handles CSTL payloads
    let adn_store_for_rest = server.adn_store.clone();
    let chain_for_rest = server.chain.clone();
    let deontic_for_rest = server.deontic.clone();
    let wai_registry_for_rest = server.wai_registry.clone();
    let sigma_calibrator_for_rest = server.sigma_calibrator.clone();

    eprintln!("🌐 Starting REST API server on port {rest_port}...");
    eprintln!("   GET /health → health check");
    eprintln!("   POST /v1/message → message JSON (meme traitement que le fil CSTL)");
    eprintln!("   GET /audit/{{case_id}} → audit trail JSON");
    eprintln!("   POST /audit/query → filter audit by date/entity/action");
    eprintln!("   GET /audit/stats → audit statistics");
    eprintln!("   GET /graphify/export → graph export (nodes/edges JSON)");
    eprintln!("   GET /graphify/stats → graph node/edge counts");
    eprintln!("   GET /graphify/filter → filter nodes/edges by type");
    eprintln!("   GET /graphify/search → search nodes by text");
    eprintln!("   GET /graphify/traverse → BFS subgraph from a node");
    eprintln!("   GET /deontic/executions → deontic rule execution log");
    eprintln!("   GET /wai/dictionaries → WAI dictionary versions");
    eprintln!("   GET /wai/dictionaries/latest → latest WAI dictionary");
    eprintln!("   GET /wai/stats → WAI dictionary registry stats");
    eprintln!("   GET /calibration/agents → all EWMA calibrations");
    eprintln!("   GET /calibration/agents/{{agent_name}} → one agent's calibration\n");

    let rest_api_handle = tokio::spawn(async move {
        if let Err(e) = cstl_parser::server::rest_api::start_rest_api(
            adn_store_for_rest,
            chain_for_rest,
            deontic_for_rest,
            wai_registry_for_rest,
            sigma_calibrator_for_rest,
            "127.0.0.1",
            rest_port,
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
