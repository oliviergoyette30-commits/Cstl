/// examples/trust_score_poisoning_test.rs -- verification live du fix
/// 2026-10-28 (F5) : `receiver=` dans l'accuse de reception du serveur doit
/// maintenant refleter le VRAI agent route par `AgentRegistry::route()`
/// (selection trust-weighted), pas l'expediteur original echo.
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::audit::signing_bytes;
use cstl_parser::server::parser::parse_payload;
use cstl_parser::server::CstlNativeServer;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
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

fn make_realistic_server(port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "alice".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string(), "arbitration".to_string()],
        trust_score: 0.95,
        public_key: None,
    });
    registry.register(AgentCard {
        name: "bob".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string(), "fact_checking".to_string()],
        trust_score: 0.85,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("Olivier"));
    server
}

fn ping_payload(sender: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=inform, sender={sender}, receiver=server]\n\
         RELATION [type=born_in, subject=x, object=y]\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15098;
    let server = make_realistic_server(port);
    let registry_handle = server.agent_registry.clone();
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    println!("=== AVANT: receiver doit etre alice (trust_score=0.95, max initial) ===");
    let before = send(port, &ping_payload("victim_checker1")).await;
    let routed_before = extract_field(&before, "INTENT_PAYLOAD", "receiver");
    println!("receiver: {:?}", routed_before);
    assert_eq!(routed_before.as_deref(), Some("alice"), "avant toute attaque, alice (0.95) doit gagner le routage");

    println!("\n=== mallory s'enregistre avec trust_score=999999 ===");
    let mut csprng = OsRng;
    let sk = SigningKey::generate(&mut csprng);
    let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
    let unsigned = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender=mallory, receiver=server, name=mallory, capabilities=communication, trust_score=999999]\n\
         ---END---\n"
    );
    let parsed = parse_payload(&unsigned).expect("parse");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    let register_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender=mallory, receiver=server, name=mallory, capabilities=communication, trust_score=999999, signature={sig_hex}]\n\
         ---END---\n"
    );
    let reg_response = send(port, &register_payload).await;
    assert_eq!(extract_field(&reg_response, "INTENT_PAYLOAD", "purpose"), Some("agent_register_ack".to_string()));

    println!("\n=== preuve DIRECTE du clamp: lecture du registre en memoire, pas d'inference via route() ===");
    let stored_trust_score = {
        let reg = registry_handle.read().await;
        reg.agents.iter().find(|a| a.name == "mallory").map(|a| a.trust_score)
    };
    println!("trust_score stocke pour mallory: {:?} (envoye: 999999)", stored_trust_score);
    assert_eq!(stored_trust_score, Some(1.0), "doit etre clampe a 1.0, jamais 999999");

    println!("\n=== receiver suit toujours reellement route() (F5): mallory gagne LEGITIMEMENT a 1.0 > alice 0.95 ===");
    let after = send(port, &ping_payload("victim_checker2")).await;
    let routed_after = extract_field(&after, "INTENT_PAYLOAD", "receiver");
    println!("receiver: {:?}", routed_after);
    assert_eq!(routed_after.as_deref(), Some("mallory"), "1.0 > 0.95 -- mallory gagne, mais borne a l'echelle [0,1], pas un chiffre arbitraire");

    println!("\n=== OK: F5 cable (receiver suit reellement route()) ET le clamp [0,1] empeche un auto-declare arbitraire de sortir de l'echelle. mallory peut au mieux atteindre 1.0, jamais 999999. ===");
}
