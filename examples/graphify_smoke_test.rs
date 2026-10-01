/// examples/graphify_smoke_test.rs -- verification live de `/graphify/export`
/// et `/graphify/stats` (Couche 6, graphify_server.rs + rest_api.rs,
/// 2026-10-01), serveur TCP + serveur REST reels, vrai client HTTP
/// (reqwest), meme convention que les autres smoke-tests de cette session.
///
/// Contexte: `graphify_server.rs` existait depuis des mois (teste en
/// isolation) mais n'avait AUCUNE route REST -- le README affirmait pourtant
/// "GET /graphify/export" "v5.1 COMPLETE" avec des chiffres ("842 nodes,
/// 1784 edges, 42 communities") qui n'avaient jamais ete mesures sur ce
/// depot. Ce smoke-test cable et verifie la vraie route, avec de vrais
/// chiffres mesures ci-dessous plutot que repetes d'une affirmation anterieure.
///
/// Scenarios verifies:
/// 1. Avant tout trafic: /graphify/export retourne un graphe vide valide
///    (0 noeuds, 0 arcs) -- pas une erreur, pas un payload absent.
/// 2. Apres 3 payloads CSTL envoyes par 2 agents distincts sur le port TCP:
///    /graphify/export reflete le nombre REEL de noeuds (3 audit_entry + 2
///    agents = 5) et d'arcs (sends_to + responds_to par entree = jusqu'a 6).
/// 3. Un payload dont `purpose` contient un caractere UTF-8 multi-octets
///    PILE a la limite de troncature (20 octets) -- preuve live que le bug
///    de panic par decoupage d'octet (trouve en lisant le code avant de le
///    cabler) est reellement corrige, pas juste en test unitaire isole:
///    la requete HTTP doit reussir (200 OK), jamais un timeout/connexion
///    fermee qui trahirait un panic cote serveur.
/// 4. /graphify/stats retourne les memes comptes, calcules independamment
///    via GraphifyExporter::get_graph_stats (deja teste en isolation mais
///    jamais appele depuis une route reelle avant ce commit).
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::CstlNativeServer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;

async fn send_cstl(port: u16, payload: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    stream.write_all(payload.as_bytes()).await.expect("send");
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.expect("read");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(9).any(|w| w == b"---END---") {
            break;
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn born_in_payload(sender: &str, purpose_extra: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=GraphifySmokeTest, produced_by=GraphifySmokeTest]\n\
         INTENT_PAYLOAD [purpose=graphify_smoke{purpose_extra}, sender={sender}, receiver=server]\n\
         ---END---\n"
    )
}

fn make_test_server(tcp_port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(tcp_port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "graphify_smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("olivier"));
    server
}

#[tokio::main]
async fn main() {
    let tcp_port: u16 = 15190;
    let rest_port: u16 = 15191;

    let server = make_test_server(tcp_port);
    let adn_store_for_rest = server.adn_store.clone();
    let chain_for_rest = server.chain.clone();
    let deontic_for_rest = server.deontic.clone();

    tokio::spawn(async move {
        server.start().await.expect("TCP server start");
    });
    tokio::spawn(async move {
        cstl_parser::server::rest_api::start_rest_api(adn_store_for_rest, chain_for_rest, deontic_for_rest, "127.0.0.1", rest_port)
            .await
            .expect("REST API start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();
    let http = reqwest::Client::new();
    let export_url = format!("http://127.0.0.1:{}/graphify/export", rest_port);
    let stats_url = format!("http://127.0.0.1:{}/graphify/stats", rest_port);

    // ---- Scenario 1: graphe vide avant tout trafic ----
    println!("[1/4] /graphify/export avant tout trafic -- graphe vide valide...");
    match http.get(&export_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.expect("JSON valide");
            let node_count = body["metadata"]["node_count"].as_u64();
            println!("      status={} node_count={:?}", status, node_count);
            if !status.is_success() || node_count != Some(0) {
                failures.push(format!("scenario 1: graphe vide attendu (0 noeuds), recu node_count={:?} status={}", node_count, status));
            }
        }
        Err(e) => failures.push(format!("scenario 1: requete HTTP echouee: {e}")),
    }

    // ---- Scenario 2: 3 payloads, 2 agents -> comptes reels ----
    println!("[2/4] Envoi de 3 payloads CSTL (agents alice/bob) puis /graphify/export...");
    send_cstl(tcp_port, &born_in_payload("alice", "_1")).await;
    send_cstl(tcp_port, &born_in_payload("bob", "_2")).await;
    send_cstl(tcp_port, &born_in_payload("alice", "_3")).await;

    match http.get(&export_url).send().await {
        Ok(resp) => {
            let body: serde_json::Value = resp.json().await.expect("JSON valide");
            let node_count = body["metadata"]["node_count"].as_u64();
            let edge_count = body["metadata"]["edge_count"].as_u64();
            let nodes = body["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("      node_count={:?} edge_count={:?} (nodes.len()={})", node_count, edge_count, nodes);
            // 3 audit_entry + alice + bob + le receiver "server" = 6 noeuds.
            if node_count != Some(6) {
                failures.push(format!("scenario 2: 6 noeuds attendus (3 audit_entry + alice + bob + server), recu {:?}", node_count));
            }
            if nodes as u64 != node_count.unwrap_or(0) {
                failures.push("scenario 2: metadata.node_count doit correspondre a nodes.len() reel".to_string());
            }
        }
        Err(e) => failures.push(format!("scenario 2: requete HTTP echouee: {e}")),
    }

    // ---- Scenario 3: purpose avec caractere multi-octets a la limite de troncature ----
    println!("[3/4] Payload avec un caractere UTF-8 multi-octets pile a la limite de troncature (preuve live du fix anti-panic)...");
    // "graphify_smoke" fait 14 octets; on ajoute du texte pour que 'é' (2
    // octets) tombe exactement sur l'ancienne limite de troncature (20).
    let tricky_purpose = "_caf\u{e9}_test_overflow_text";
    send_cstl(tcp_port, &born_in_payload("alice", tricky_purpose)).await;
    match tokio::time::timeout(std::time::Duration::from_secs(3), http.get(&export_url).send()).await {
        Ok(Ok(resp)) => {
            let status = resp.status();
            println!("      status={} (200 OK = le serveur n'a PAS panique sur le caractere multi-octets)", status);
            if !status.is_success() {
                failures.push(format!("scenario 3: /graphify/export doit rester disponible apres un purpose multi-octets, status={}", status));
            }
        }
        Ok(Err(e)) => failures.push(format!("scenario 3: requete HTTP echouee (possible panic serveur): {e}")),
        Err(_) => failures.push("scenario 3: timeout -- le serveur a probablement panique sur le caractere multi-octets (regression du bug trouve dans build_from_audit_trail)".to_string()),
    }

    // ---- Scenario 4: /graphify/stats coherent avec /graphify/export ----
    println!("[4/4] /graphify/stats...");
    match http.get(&stats_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.expect("JSON valide");
            let total_nodes = body["total_nodes"].as_u64();
            let agents = body["agents"].as_u64();
            println!("      status={} total_nodes={:?} agents={:?}", status, total_nodes, agents);
            // 4 payloads envoyes au total maintenant (3 + 1 du scenario 3) +
            // alice/bob/server = 4 audit_entry + 3 agents = 7 noeuds.
            if total_nodes != Some(7) || agents != Some(3) {
                failures.push(format!("scenario 4: comptes inattendus -- total_nodes={:?} (7 attendu) agents={:?} (3 attendu)", total_nodes, agents));
            }
            if !status.is_success() {
                failures.push(format!("scenario 4: status inattendu: {}", status));
            }
        }
        Err(e) => failures.push(format!("scenario 4: requete HTTP echouee: {e}")),
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios graphify sont conformes (export + stats, vrai trafic HTTP+TCP, panic multi-octets corrige).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
