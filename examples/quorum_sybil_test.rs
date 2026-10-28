/// examples/quorum_sybil_test.rs -- verification live d'une trouvaille de
/// l'audit du 2026-10-28 (session Olivier) : le quorum BFT
/// (src/server/quorum_wire.rs, "membre = agent enregistre avec public_key",
/// voir le commentaire de tete du module) n'a AUCUNE protection Sybil.
/// L'enregistrement d'un nouveau nom d'agent (purpose=agent_register) ne
/// prouve que la possession d'UNE paire de cles Ed25519 fraichement
/// generee -- aucune preuve de travail, aucun cout, aucune identite
/// humaine distincte exigee. Un seul acteur peut donc generer N paires de
/// cles, enregistrer N agents, et devenir lui-meme la totalite (ou la
/// supermajorite) du "quorum" dont member_count/threshold dependent --
/// defaisant la garantie BFT (2/3 des membres) que ce module revendique.
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

fn make_server(port: u16) -> CstlNativeServer {
    // Registre VIDE de depart (comme un serveur fraichement deploye, avant
    // que quiconque de legitime ne se soit encore enregistre avec une cle)
    // -- le pire cas, mais aussi le plus realiste pour une nouvelle
    // instance: alice/bob bootstrap (main.rs) ont public_key=None, donc ne
    // comptent de toute facon PAS comme membres du quorum (voir le
    // commentaire de tete de quorum_wire.rs).
    let mut server = CstlNativeServer::with_data_path(port, ":memory:");
    server.agent_registry = Arc::new(RwLock::new(AgentRegistry::new()));
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("Olivier"));
    server
}

/// Enregistre un agent avec une cle Ed25519 fraichement generee et renvoie
/// sa cle privee (pour signer son futur vote).
async fn register_sybil(port: u16, name: &str) -> SigningKey {
    let mut csprng = OsRng;
    let sk = SigningKey::generate(&mut csprng);
    let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
    let unsigned = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=Sybil, produced_by=Sybil, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=voting]\n\
         ---END---\n"
    );
    let parsed = parse_payload(&unsigned).expect("parse");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    let signed = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=Sybil, produced_by=Sybil, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, name={name}, capabilities=voting, signature={sig_hex}]\n\
         ---END---\n"
    );
    let resp = send(port, &signed).await;
    assert_eq!(extract_field(&resp, "INTENT_PAYLOAD", "purpose"), Some("agent_register_ack".to_string()), "echec enregistrement de {name}");
    sk
}

/// `body_no_bracket` = ligne INTENT_PAYLOAD SANS le ']' final (ex:
/// "INTENT_PAYLOAD [purpose=quorum_vote, sender=sybil_1, ..., decision=yea").
fn signed_payload(sk: &SigningKey, pubkey_hex: &str, body_no_bracket: &str) -> String {
    let unsigned = format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=Sybil, produced_by=Sybil, public_key={pubkey_hex}]\n{body_no_bracket}]\n---END---\n"
    );
    let parsed = parse_payload(&unsigned).expect("parse (unsigned draft)");
    let sig = sk.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=Sybil, produced_by=Sybil, public_key={pubkey_hex}]\n{body_no_bracket}, signature={sig_hex}]\n---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15097;
    let server = make_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    println!("=== UN SEUL ACTEUR enregistre 3 agents sybils (3 cles Ed25519 fraiches, zero cout) ===");
    let sk1 = register_sybil(port, "sybil_1").await;
    let sk2 = register_sybil(port, "sybil_2").await;
    let sk3 = register_sybil(port, "sybil_3").await;
    println!("sybil_1, sybil_2, sybil_3 enregistres -- aucun humain distinct requis, aucune preuve autre que la possession d'une cle fraichement generee.");

    println!("\n=== sybil_1 propose ===");
    let pk1_hex = hex::encode(sk1.verifying_key().to_bytes());
    let propose = signed_payload(&sk1, &pk1_hex, "INTENT_PAYLOAD [purpose=quorum_propose, sender=sybil_1, receiver=server, proposal_id=p1, description=attaque_sybil");
    let resp = send(port, &propose).await;
    println!("reponse propose: {}", resp.lines().find(|l| l.starts_with("INTENT_PAYLOAD")).unwrap_or(""));
    let threshold = extract_field(&resp, "INTENT_PAYLOAD", "threshold");
    let member_count = extract_field(&resp, "INTENT_PAYLOAD", "member_count");
    println!("member_count={:?} threshold={:?} -- ce sont les 3 sybils du MEME attaquant, personne d'autre", member_count, threshold);

    println!("\n=== sybil_1 et sybil_2 votent yea (threshold=2 attendu avec 3 membres sybils) ===");
    let vote1 = signed_payload(&sk1, &pk1_hex, "INTENT_PAYLOAD [purpose=quorum_vote, sender=sybil_1, receiver=server, proposal_id=p1, decision=yea");
    let r1 = send(port, &vote1).await;
    println!("vote 1: {}", r1.lines().find(|l| l.starts_with("INTENT_PAYLOAD")).unwrap_or(""));

    let pk2_hex = hex::encode(sk2.verifying_key().to_bytes());
    let vote2 = signed_payload(&sk2, &pk2_hex, "INTENT_PAYLOAD [purpose=quorum_vote, sender=sybil_2, receiver=server, proposal_id=p1, decision=yea");
    let r2 = send(port, &vote2).await;
    println!("vote 2: {}", r2.lines().find(|l| l.starts_with("INTENT_PAYLOAD")).unwrap_or(""));

    let consensus_reached = extract_field(&r2, "INTENT_PAYLOAD", "consensus_reached");
    let final_decision = extract_field(&r2, "INTENT_PAYLOAD", "final_decision");
    println!("\nconsensus_reached={:?} final_decision={:?}", consensus_reached, final_decision);

    if consensus_reached.as_deref() == Some("true") && final_decision.as_deref() == Some("yea") {
        println!("\n!!! CONFIRME: consensus BFT 'atteint' avec sybil_3 qui n'a jamais vote -- 2 des 3 'membres' suffisent, et les 3 appartiennent au MEME attaquant. Le seuil 2/3 ne protege de rien quand l'identite est gratuite.");
    } else {
        println!("\nPas reproduit sur cette run.");
    }
}
