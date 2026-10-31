/// examples/conversation_context_smoke_test.rs -- verification live du
/// branchement de `conversation_id`/`turn` (handler.rs, 2026-10-31).
///
/// Contexte: Olivier a propose que deux agents IA restent "intriques" a
/// travers une conversation -- que le contexte des tours precedents reste
/// disponible sans tout retransmettre a chaque message. En creusant,
/// `get_primer()`/`load_context()` (Couche 5, adn_store.rs) existaient deja
/// pour ca, mais etaient du code mort: `handler.rs` appelait toujours
/// `put(..., None, None)` pour conversation_id/turn (confirme par grep,
/// zero occurrence), et `get_primer()` exige `committed=1`, jamais vrai
/// pour un payload traite par le chemin AI-to-AI live (aucune logique de
/// quorum humain n'appelle `commit()` automatiquement) -- donc meme branche,
/// `get_primer()` n'aurait jamais rien trouve.
///
/// Fix: `conversation_id` (opt-in, dans INTENT_PAYLOAD) fait assigner un
/// `turn` par le SERVEUR (compte des tours deja stockes pour cet id), et un
/// nouveau bloc reponse CONVERSATION_CONTEXT liste les tours precedents via
/// `AdnStore::get_conversation_context` (meme requete que get_primer(),
/// SANS le filtre committed=1).
///
/// Ce test verifie en live, contre un vrai serveur TCP:
///   1. 1er tour d'une conversation -> turn=0, aucun contexte precedent.
///   2. 2e tour -> turn=1, le tour 0 est bien retrouve (hash correct).
///   3. 3e tour -> turn=2, LES DEUX tours precedents sont retrouves, dans
///      l'ordre (plus recent d'abord).
///   4. Une conversation DIFFERENTE est isolee -- turn=0, ne voit rien de
///      la premiere conversation meme si elle a ete creee apres.
///   5. Un payload SANS conversation_id ne produit AUCUN bloc
///      CONVERSATION_CONTEXT (opt-in, pas de bruit sur le trafic normal).
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::server::CstlNativeServer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;

async fn send(port: u16, payload: &str) -> String {
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

fn extract_field(response: &str, block: &str, key: &str) -> Option<String> {
    for line in response.lines() {
        if line.starts_with(block) {
            let inner = line.split_once('[')?.1.trim_end_matches(']').trim_end_matches("]\n");
            for part in inner.split(',') {
                let part = part.trim();
                if let Some((k, v)) = part.split_once('=') {
                    if k.trim() == key {
                        return Some(v.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

fn payload(sender: &str, receiver: &str, conv: Option<&str>, obj: &str) -> String {
    let conv_field = conv.map(|c| format!(", conversation_id={c}")).unwrap_or_default();
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=ContextSmokeTest, produced_by=ContextSmokeTest]\n\
         INTENT_PAYLOAD [purpose=inform, sender={sender}, receiver={receiver}{conv_field}]\n\
         RELATION [type=STATE, subject=x, object={obj}]\n\
         ---END---\n"
    )
}

fn make_test_server(port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server
}

#[tokio::main]
async fn main() {
    let port: u16 = 15201;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- 1. 1er tour: turn=0, pas de contexte ----
    println!("[1] 1er tour de conv001...");
    let r1 = send(port, &payload("alice", "bob", Some("conv001"), "y1")).await;
    let turn1 = extract_field(&r1, "CONVERSATION_CONTEXT", "turn");
    let included1 = extract_field(&r1, "CONVERSATION_CONTEXT", "prior_turns_included");
    let hash1 = extract_field(&r1, "AUDIT", "hash").unwrap_or_default();
    println!("     turn={:?}, prior_turns_included={:?}", turn1, included1);
    if turn1.as_deref() != Some("0") || included1.as_deref() != Some("0") {
        failures.push(format!("1er tour: attendu turn=0/prior_turns_included=0, obtenu turn={:?} included={:?}", turn1, included1));
    }

    // ---- 2. 2e tour: turn=1, retrouve le tour 0 ----
    println!("[2] 2e tour de conv001...");
    let r2 = send(port, &payload("bob", "alice", Some("conv001"), "y2")).await;
    let turn2 = extract_field(&r2, "CONVERSATION_CONTEXT", "turn");
    let included2 = extract_field(&r2, "CONVERSATION_CONTEXT", "prior_turns_included");
    let hashes2 = extract_field(&r2, "CONVERSATION_CONTEXT", "prior_hashes");
    println!("     turn={:?}, prior_turns_included={:?}, prior_hashes={:?}", turn2, included2, hashes2);
    if turn2.as_deref() != Some("1") || included2.as_deref() != Some("1") {
        failures.push(format!("2e tour: attendu turn=1/prior_turns_included=1, obtenu turn={:?} included={:?}", turn2, included2));
    }
    if hashes2.as_deref() != Some(hash1.as_str()) {
        failures.push(format!("2e tour: prior_hashes devrait etre le hash du tour 0 ({}), obtenu {:?}", hash1, hashes2));
    }

    // ---- 3. 3e tour: turn=2, retrouve LES DEUX tours precedents, ordre DESC ----
    println!("[3] 3e tour de conv001...");
    let r3 = send(port, &payload("alice", "bob", Some("conv001"), "y3")).await;
    let turn3 = extract_field(&r3, "CONVERSATION_CONTEXT", "turn");
    let included3 = extract_field(&r3, "CONVERSATION_CONTEXT", "prior_turns_included");
    let prior_turns3 = extract_field(&r3, "CONVERSATION_CONTEXT", "prior_turns");
    println!("     turn={:?}, prior_turns_included={:?}, prior_turns={:?}", turn3, included3, prior_turns3);
    if turn3.as_deref() != Some("2") || included3.as_deref() != Some("2") {
        failures.push(format!("3e tour: attendu turn=2/prior_turns_included=2, obtenu turn={:?} included={:?}", turn3, included3));
    }
    if prior_turns3.as_deref() != Some("1;0") {
        failures.push(format!("3e tour: prior_turns devrait etre '1;0' (plus recent d'abord), obtenu {:?}", prior_turns3));
    }

    // ---- 4. Conversation DIFFERENTE, isolee ----
    println!("[4] 1er tour d'une conversation DIFFERENTE (conv002)...");
    let r4 = send(port, &payload("charlie", "dave", Some("conv002"), "z1")).await;
    let turn4 = extract_field(&r4, "CONVERSATION_CONTEXT", "turn");
    let included4 = extract_field(&r4, "CONVERSATION_CONTEXT", "prior_turns_included");
    println!("     turn={:?}, prior_turns_included={:?}", turn4, included4);
    if turn4.as_deref() != Some("0") || included4.as_deref() != Some("0") {
        failures.push(format!("conv002 devrait etre isolee (turn=0, included=0), obtenu turn={:?} included={:?}", turn4, included4));
    }

    // ---- 5. Sans conversation_id: aucun bloc ----
    println!("[5] Payload sans conversation_id...");
    let r5 = send(port, &payload("alice", "server", None, "w1")).await;
    if r5.contains("CONVERSATION_CONTEXT") {
        failures.push("Un payload sans conversation_id ne devrait PAS produire CONVERSATION_CONTEXT".to_string());
    } else {
        println!("     absent, comme attendu");
    }

    // ---- Verdict ----
    if failures.is_empty() {
        println!("\n✅ TOUS LES SCENARIOS PASSENT (CONVERSATION_CONTEXT)");
    } else {
        println!("\n❌ {} ECHEC(S):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
