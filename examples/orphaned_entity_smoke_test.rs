/// examples/orphaned_entity_smoke_test.rs -- verification live du fix W608
/// (2026-09-23), meme patron que examples/epistemic_antonym_smoke_test.rs.
///
/// Trouvaille de fond (cstl_legal_test.py, item legal_003): le CSTL genere
/// pour "This agreement terminates automatically if either party is
/// acquired by a competitor" definissait Agreement AS Contract [id=e004]
/// mais construisait la relation de terminaison sur e001 (Party) au lieu
/// de e004 -- Agreement restait orphelin, aucune RELATION ne le touchait.
/// Ce smoke test reproduit ce scenario EXACT contre le vrai parser/serveur
/// TCP, en utilisant le format REELLEMENT supporte cote serveur (RELATION
/// [type=..., subject=..., object=...], forme singuliere plate) -- pas la
/// forme `RELATIONS [ (x) OP y ]` documentee au spec §9/§10, verifiee
/// separement (2026-09-23) comme jamais parsee par le moteur reel
/// (payload.relations reste a 0, silencieusement, aucun avertissement).
///
/// Scenarios verifies en direct:
/// 1. Toutes les entites DEFINE referencees par au moins une RELATION --
///    aucun W608.
/// 2. Une entite (Agreement) definie mais jamais utilisee (sujet mal cable
///    sur une autre entite a la place) -- W608 present, nomme l'entite.
/// 3. Reference par NOM plutot que par ID (legal_001 utilise ce style) --
///    toujours reconnue comme referencee, aucun faux positif W608.
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

fn payload(defines: &str, relations: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=orphaned_entity_smoke, sender=agent_x, receiver=server]\n\
         {defines}\
         {relations}\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let port: u16 = 15182;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: toutes les entites referencees (par id) ----
    println!("[1/3] Toutes les entites referencees par id (aucun W608 attendu)...");
    let resp1 = send(port, &payload(
        "DEFINE Party AS Entity [id=e001]\n\
         DEFINE Competitor AS Entity [id=e002]\n",
        "RELATION [type=ACQUIRED_BY, subject=e001, object=e002]\n",
    )).await;
    let has_w608_1 = resp1.contains("W608");
    println!("      W608_present={}", has_w608_1);
    if has_w608_1 {
        failures.push(format!("scenario 1: toutes referencees, aucun W608 attendu, reponse: {}", resp1));
    }

    // ---- Scenario 2: reproduction exacte de legal_003 -- Agreement orphelin ----
    println!("[2/3] Agreement (e004) defini mais jamais reference (sujet mal cable sur e001)...");
    let resp2 = send(port, &payload(
        "DEFINE Party AS Entity [id=e001]\n\
         DEFINE Competitor AS Entity [id=e002]\n\
         DEFINE Acquisition AS Event [id=e003]\n\
         DEFINE Agreement AS Contract [id=e004]\n",
        "RELATION [type=TERMINATES_IF, subject=e001, object=e003]\n\
         RELATION [type=REQUIRES, subject=e003, object=e002]\n",
    )).await;
    let has_w608_2 = resp2.contains("W608") && resp2.contains("e004") && resp2.contains("Agreement");
    println!("      W608_present_et_nomme_Agreement={}", has_w608_2);
    if !has_w608_2 {
        failures.push(format!("scenario 2: Agreement (e004) orphelin doit produire W608 le nommant, reponse: {}", resp2));
    }

    // ---- Scenario 3: reference par NOM plutot que par id (style legal_001) ----
    println!("[3/3] Reference par nom (Tenant/Rent), pas par id (aucun W608 attendu)...");
    let resp3 = send(port, &payload(
        "DEFINE Tenant AS Party [id=e001]\n\
         DEFINE Rent AS Obligation [id=e002]\n",
        "RELATION [type=MUST, subject=Tenant, object=Rent]\n",
    )).await;
    let has_w608_3 = resp3.contains("W608");
    println!("      W608_present={}", has_w608_3);
    if has_w608_3 {
        failures.push(format!("scenario 3: reference par nom doit compter comme referencee, aucun W608 attendu, reponse: {}", resp3));
    }

    println!("\n{}", "=".repeat(70));
    if failures.is_empty() {
        println!("TOUS LES SCENARIOS PASSENT (3/3) -- fix W608 verifie en direct contre le vrai serveur TCP.");
    } else {
        println!("ECHECS ({}/3):", failures.len());
        for f in &failures {
            println!("  - {}", f);
        }
        std::process::exit(1);
    }
}
