/// examples/priority_escalation_smoke_test.rs -- verification EN DIRECT (vrai
/// TCP, vrai serveur, pas un test unitaire isole) du champ de grammaire
/// `INTENT_PAYLOAD [priority=critical|high|normal|low]` (CSTL_SPEC_v5_0.md
/// §6, ligne ~231 et exemple d'usage ligne ~859).
///
/// Contexte du gap (2026-09-06): `priority` etait specifie dans la grammaire
/// officielle depuis la v5.0 mais `grep -rn '"priority"' src/` ne retournait
/// RIEN -- zero implementation, ni validation ni comportement. Ce test
/// verifie que ce n'est plus le cas, ET que l'ajout est bien additif (aucune
/// regression sur le trafic qui ne declare jamais ce champ).
///
/// Portee assumee du comportement cable (voir handler.rs STEP 3-priority
/// pour le detail complet): SEULE la valeur `critical` declenche un effet
/// reel (escalade Telegram, meme canal fire-and-forget que les alertes de
/// gouvernance) -- `high`/`normal`/`low` ne font qu'apparaitre dans une
/// ligne PRIORITY purement informative de la reponse. Ce test ne peut pas
/// verifier la RECEPTION du message Telegram (pas de bot configure dans ce
/// sandbox -- TelegramNotifier::from_env() retourne None sans les variables
/// d'environnement requises) ; il verifie donc le signal mesurable cote
/// protocole TCP (`escalated=true` dans la reponse) qui accompagne
/// deterministiquement le declenchement de l'escalade dans le code
/// (handler.rs: la ligne PRIORITY et le `tokio::spawn` du message Telegram
/// sont ecrits dans la MEME branche `Some("critical")`).
///
/// Scenarios:
///   1. Payload SANS `priority` -> aucune ligne PRIORITY dans la reponse
///      (regression -- comportement identique a avant ce fix).
///   2. `priority=critical` -> `PRIORITY [value=critical, escalated=true, ...]`.
///   3. `priority=normal` / `low` / `high` -> `PRIORITY [value=X, escalated=false]`,
///      jamais `escalated=true`.
///   4. `priority=urgent` (hors enum) -> rejete avec `E311` (validation_error),
///      pas de routage/traitement normal.
use cstl_parser::agent_discovery::{AgentCard, AgentRegistry};
use cstl_parser::server::CstlNativeServer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

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

fn payload_with_priority(priority: Option<&str>) -> String {
    let priority_field = match priority {
        Some(p) => format!(", priority={p}"),
        None => String::new(),
    };
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=smoke_test, sender=agent_1, receiver=server{priority_field}]\n\
         ---END---\n"
    )
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
    server.agent_registry = Arc::new(Mutex::new(registry));
    server
}

#[tokio::main]
async fn main() {
    let port: u16 = 15191;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: pas de priority -- regression ----
    println!("[1/6] payload sans priority -> aucune ligne PRIORITY (regression)...");
    let resp1 = send(port, &payload_with_priority(None)).await;
    if resp1.contains("PRIORITY [") {
        failures.push(format!("1) ligne PRIORITY presente a tort sans champ priority -- reponse: {resp1}"));
    } else {
        println!("    OK: aucune ligne PRIORITY, comportement inchange");
    }

    // ---- Scenario 2: priority=critical -- escalade reelle ----
    println!("[2/6] priority=critical -> PRIORITY [escalated=true] attendu...");
    let resp2 = send(port, &payload_with_priority(Some("critical"))).await;
    match extract_field(&resp2, "PRIORITY", "escalated") {
        Some(v) if v == "true" => println!("    OK: escalated=true"),
        other => failures.push(format!("2) attendu escalated=true, obtenu {other:?} -- reponse: {resp2}")),
    }
    match extract_field(&resp2, "PRIORITY", "value") {
        Some(v) if v == "critical" => println!("    OK: value=critical"),
        other => failures.push(format!("2) attendu value=critical, obtenu {other:?}")),
    }

    // ---- Scenario 3: normal/low/high -- pas d'escalade ----
    for p in ["normal", "low", "high"] {
        println!("[3/6] priority={p} -> PRIORITY [escalated=false], pas d'escalade...");
        let resp = send(port, &payload_with_priority(Some(p))).await;
        match extract_field(&resp, "PRIORITY", "escalated") {
            Some(v) if v == "false" => println!("    OK: escalated=false pour {p}"),
            other => failures.push(format!("3) priority={p}: attendu escalated=false, obtenu {other:?} -- reponse: {resp}")),
        }
    }

    // ---- Scenario 4: valeur hors enum -- rejet E311 ----
    println!("[4/6] priority=urgent (hors enum) -> rejete avec E311...");
    let resp4 = send(port, &payload_with_priority(Some("urgent"))).await;
    if !resp4.contains("E311") {
        failures.push(format!("4) E311 absent pour priority=urgent -- reponse: {resp4}"));
    } else if !resp4.contains("validation_error") {
        failures.push(format!("4) purpose=validation_error absent -- reponse: {resp4}"));
    } else {
        println!("    OK: rejete avec E311 (validation_error)");
    }
    if resp4.contains("PRIORITY [") {
        failures.push(format!("4) ligne PRIORITY presente a tort sur un payload rejete -- reponse: {resp4}"));
    }

    // ---- Scenario 5: valeur hors enum insensible a un autre cas invalide ----
    println!("[5/6] priority=CRITICAL (majuscules, hors enum litteral) -> rejete avec E311...");
    let resp5 = send(port, &payload_with_priority(Some("CRITICAL"))).await;
    if !resp5.contains("E311") {
        failures.push(format!("5) E311 absent pour priority=CRITICAL -- reponse: {resp5}"));
    } else {
        println!("    OK: rejete avec E311 (l'enum de la grammaire est en minuscules)");
    }

    // ---- Scenario 6: priority=critical ne bloque pas le routage normal ----
    println!("[6/6] priority=critical -> le payload est quand meme route/traite normalement (pas juste l'escalade)...");
    let resp6 = send(port, &payload_with_priority(Some("critical"))).await;
    if !resp6.contains("AUDIT [hash=") {
        failures.push(format!("6) bloc AUDIT absent -- le payload critical n'a pas ete traite normalement: {resp6}"));
    } else {
        println!("    OK: AUDIT present, traitement normal preserve en plus de l'escalade");
    }

    if failures.is_empty() {
        println!("\n✅ TOUS LES SCENARIOS PRIORITY PASSENT");
    } else {
        println!("\n❌ {} ECHEC(S):", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
}
