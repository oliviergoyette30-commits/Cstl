/// examples/uncertainty_and_audit_query_smoke_test.rs -- verification live
/// de deux corrections du 2026-10-30, toutes deux trouvees en repondant a
/// des questions posees (via Olivier) par GPT pendant une session de test
/// en relais contre le vrai serveur CSTL:
///
/// 1. UNCERTAINTY_AUDIT (handler.rs) -- avant ce fix, `payload.uncertainty`
///    etait persiste (`put_master_compressed`) mais jamais relu pour
///    produire un effet observable dans la reponse: un payload avec
///    `ESTIMATED` et le meme payload avec `INFERRED` produisaient une
///    reponse strictement identique, et `target_relation` n'etait verifie
///    contre rien nulle part (confirme par grep). Ce test verifie trois
///    choses en live:
///      a. ESTIMATED et INFERRED produisent des `weighted_confidence`
///         DIFFERENTS dans le nouveau bloc UNCERTAINTY_AUDIT (0.60 vs 0.40
///         -- voir le bareme documente dans handler.rs juste avant ce bloc).
///      b. Un `target_relation` qui ne correspond a aucun `id` de RELATION
///         presente dans le meme payload est compte dans `dangling`.
///      c. Un payload SANS bloc UNCERTAINTY ne produit PAS de ligne
///         UNCERTAINTY_AUDIT (pas de bruit sur le trafic normal).
///
/// 2. `/audit/query` (REST API, port separe du TCP) -- avant ce fix,
///    retournait toujours `results: [], total_count: 0` codes en dur, quels
///    que soient les parametres. Ce test envoie un payload reel par TCP,
///    recupere son hash AUDIT, puis verifie en live via `curl`-equivalent
///    (reqwest) que `/audit/query?hash=<hash>` retrouve CE message precis
///    (sender/receiver/purpose corrects) -- ce que `/audit/{case_id}`
///    (routee vers `arbitrage_cases`, une table differente) ne pouvait
///    jamais faire pour un message ordinaire.
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

fn uncertainty_payload(sender: &str, status: &str, target: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=UncertaintySmokeTest, produced_by=UncertaintySmokeTest]\n\
         INTENT_PAYLOAD [purpose=uncertainty_smoke, sender={sender}, receiver=server]\n\
         RELATION [id=r001, type=STATE, subject=x, object=y]\n\
         UNCERTAINTY [\n\
         \u{20}u001 {status} [target_relation={target}]\n\
         ]\n\
         ---END---\n"
    )
}

fn plain_payload(sender: &str) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=UncertaintySmokeTest, produced_by=UncertaintySmokeTest]\n\
         INTENT_PAYLOAD [purpose=uncertainty_smoke_plain, sender={sender}, receiver=server]\n\
         RELATION [id=r001, type=STATE, subject=x, object=y]\n\
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
    let tcp_port: u16 = 15191;
    let rest_port: u16 = 18091;

    let server = make_test_server(tcp_port);
    let adn_store_for_rest = server.adn_store.clone();
    let chain_for_rest = server.chain.clone();
    let deontic_for_rest = server.deontic.clone();
    let wai_registry_for_rest = server.wai_registry.clone();
    let sigma_calibrator_for_rest = server.sigma_calibrator.clone();

    tokio::spawn(async move {
        server.start().await.expect("tcp server start");
    });
    tokio::spawn(async move {
        cstl_parser::server::rest_api::start_rest_api(
            adn_store_for_rest,
            chain_for_rest,
            deontic_for_rest,
            wai_registry_for_rest,
            sigma_calibrator_for_rest,
            "127.0.0.1",
            rest_port,
        )
        .await
        .expect("rest api start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let mut failures = Vec::new();

    // ---- 1a. MEASURED vs INFERRED -> weighted_confidence different ----
    println!("[1a] MEASURED vs INFERRED...");
    let resp_measured = send(tcp_port, &uncertainty_payload("agent_m", "MEASURED", "r001")).await;
    let resp_inferred = send(tcp_port, &uncertainty_payload("agent_i", "INFERRED", "r001")).await;
    let wc_measured = extract_field(&resp_measured, "UNCERTAINTY_AUDIT", "weighted_confidence");
    let wc_inferred = extract_field(&resp_inferred, "UNCERTAINTY_AUDIT", "weighted_confidence");
    println!("     MEASURED -> {:?}, INFERRED -> {:?}", wc_measured, wc_inferred);
    if wc_measured.as_deref() != Some("1.00") {
        failures.push(format!("MEASURED devrait donner weighted_confidence=1.00, obtenu {:?}", wc_measured));
    }
    if wc_inferred.as_deref() != Some("0.40") {
        failures.push(format!("INFERRED devrait donner weighted_confidence=0.40, obtenu {:?}", wc_inferred));
    }
    if wc_measured == wc_inferred {
        failures.push("MEASURED et INFERRED ne devraient PAS produire la meme valeur".to_string());
    }

    // ---- 1b. target_relation dangling ----
    println!("[1b] target_relation pointant vers une relation inexistante...");
    let resp_dangling = send(tcp_port, &uncertainty_payload("agent_d", "ESTIMATED", "r999_n_existe_pas")).await;
    let dangling = extract_field(&resp_dangling, "UNCERTAINTY_AUDIT", "dangling");
    println!("     dangling -> {:?}", dangling);
    if dangling.as_deref() != Some("1") {
        failures.push(format!("target_relation inexistant devrait donner dangling=1, obtenu {:?}", dangling));
    }

    // ---- 1c. Pas de bloc UNCERTAINTY -> pas de ligne UNCERTAINTY_AUDIT ----
    println!("[1c] Payload sans UNCERTAINTY...");
    let resp_plain = send(tcp_port, &plain_payload("agent_p")).await;
    if resp_plain.contains("UNCERTAINTY_AUDIT") {
        failures.push("Un payload sans bloc UNCERTAINTY ne devrait PAS produire UNCERTAINTY_AUDIT".to_string());
    } else {
        println!("     absent, comme attendu");
    }

    // ---- 1d. Dedup par target_relation (2026-10-30, trouve en testant en
    // direct avec Gemini): empiler 5 entrees MEASURED sur la meme relation
    // ne doit PLUS tirer la moyenne vers le haut au-dela de ce qu'une seule
    // entree MEASURED donnerait -- le poids retenu par cible est le MIN du
    // groupe, pas une moyenne qui recompte les doublons. ----
    println!("[1d] 5x MEASURED empiles sur la meme relation + 1 UNKNOWN + 1 dangling...");
    let stacked_payload = "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=UncertaintySmokeTest, produced_by=UncertaintySmokeTest]\n\
         INTENT_PAYLOAD [purpose=uncertainty_smoke_stack, sender=agent_s, receiver=server]\n\
         RELATION [id=r004, type=ARR.ACCESS, subject=target_node, object=target_node]\n\
         UNCERTAINTY [\n\
         \u{20}u1 UNKNOWN [target_relation=r004]\n\
         \u{20}u2 INFERRED [target_relation=r999]\n\
         \u{20}u3 MEASURED [target_relation=r004]\n\
         \u{20}u4 MEASURED [target_relation=r004]\n\
         \u{20}u5 MEASURED [target_relation=r004]\n\
         \u{20}u6 MEASURED [target_relation=r004]\n\
         \u{20}u7 MEASURED [target_relation=r004]\n\
         ]\n\
         ---END---\n";
    let resp_stacked = send(tcp_port, stacked_payload).await;
    let annotated_stacked = extract_field(&resp_stacked, "UNCERTAINTY_AUDIT", "annotated");
    let unique_stacked = extract_field(&resp_stacked, "UNCERTAINTY_AUDIT", "unique_targets");
    let wc_stacked = extract_field(&resp_stacked, "UNCERTAINTY_AUDIT", "weighted_confidence");
    println!(
        "     annotated={:?}, unique_targets={:?}, weighted_confidence={:?}",
        annotated_stacked, unique_stacked, wc_stacked
    );
    if annotated_stacked.as_deref() != Some("7") {
        failures.push(format!("annotated devrait rester 7 (compte brut), obtenu {:?}", annotated_stacked));
    }
    if unique_stacked.as_deref() != Some("2") {
        failures.push(format!("unique_targets devrait etre 2 (r004, r999), obtenu {:?}", unique_stacked));
    }
    // Groupe r004: min(UNKNOWN=0.1, MEASURED=1.0 x5) = 0.1. Groupe r999 (dangling): INFERRED=0.4.
    // (0.1 + 0.4) / 2 = 0.25 -- PAS 0.79, qui serait le resultat (faux) d'une moyenne sans dedup.
    if wc_stacked.as_deref() != Some("0.25") {
        failures.push(format!(
            "weighted_confidence devrait etre 0.25 (dedup par min, pas 0.79 sans dedup), obtenu {:?}",
            wc_stacked
        ));
    }

    // ---- 2. /audit/query?hash=<hash> retrouve le bon message ----
    println!("[2] /audit/query?hash=<hash> sur le message MEASURED ci-dessus...");
    let hash_field = extract_field(&resp_measured, "AUDIT", "hash").unwrap_or_default();
    println!("     hash envoye par le serveur: {}", hash_field);
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/audit/query?hash={}", rest_port, hash_field);
    match client.post(&url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            println!("     status: {}, corps brut: {}", status, text);
            let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            let total = body.get("total_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let found_sender = body
                .get("results")
                .and_then(|r| r.get(0))
                .and_then(|e| e.get("sender"))
                .and_then(|s| s.as_str());
            if total != 1 {
                failures.push(format!("/audit/query?hash=... devrait retourner total_count=1, obtenu {}", total));
            }
            if found_sender != Some("agent_m") {
                failures.push(format!("/audit/query?hash=... devrait retrouver sender=agent_m, obtenu {:?}", found_sender));
            }
        }
        Err(e) => failures.push(format!("Requete /audit/query a echoue: {}", e)),
    }

    // ---- Verdict ----
    if failures.is_empty() {
        println!("\n✅ TOUS LES SCENARIOS PASSENT (UNCERTAINTY_AUDIT + /audit/query reel)");
    } else {
        println!("\n❌ {} ECHEC(S):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
