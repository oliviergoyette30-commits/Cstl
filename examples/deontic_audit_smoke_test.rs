/// examples/deontic_audit_smoke_test.rs -- verification live de l'audit
/// deontique (Couche 8, 2026-09-04), en reponse a la trouvaille "Deontic
/// Modality Audit" (intitule sans code correspondant dans
/// docs/ARCHITECTURE.md avant ce fix).
///
/// Trouvaille (creusee en cherchant a construire l'audit): le seul check
/// deontique reellement appele sur le chemin TCP (server/validator.rs::
/// validate_deontic_constraints) verifiait si le champ `type` d'UNE SEULE
/// RELATION contenait a la fois les sous-chaines "MUST" et "MUST_NOT" --
/// double bug: (1) le format wire n'encode jamais MUST/MUST_NOT dans `type`
/// (predicat KB ou operateur SDL uniquement), et (2) "MUST_NOT".contains
/// ("MUST") est vrai en Rust, donc un simple RELATION[type=MUST_NOT,...]
/// isole se faisait rejeter a tort. Le VRAI moteur (SDL Axiome D,
/// semantic.rs::check_axiom_d, E107) existait deja, teste, mais n'etait
/// JAMAIS appele en direct.
///
/// Corrige en deux volets:
///   A. Intra-payload (bloquant, E107): une RELATION porte desormais un
///      champ optionnel `modality=MUST|MUST_NOT|REQUIRE|FORBID` --
///      validate_deontic_constraints construit de vraies AstRelation et
///      appelle le vrai Axiome D.
///   B. Historique (informatif, jamais de rejet): les relations avec
///      modality sont persistees (adn_relations.modality, migration
///      idempotente) et verifiees contre TOUT l'historique
///      (execution_lab::check_deontic_consistency_with_history), sur le
///      modele de check_consistency_with_history deja existant pour les
///      faits -- une contradiction qui s'etale sur PLUSIEURS payloads
///      (invisible au check bloquant intra-payload) apparait dans un
///      nouveau bloc de reponse DEONTIC_AUDIT.
///
/// Scenarios verifies:
/// 1. RELATION[modality=MUST_NOT] isolee -> traitee normalement (regression
///    directe du faux positif corrige).
/// 2. MUST et MUST_NOT sur le MEME (subject, object) dans le MEME payload
///    -> rejete (E107, validation_error) -- vraie contradiction detectee.
/// 3. MUST_NOT etabli par un PREMIER payload, MUST sur la MEME (subject,
///    object) dans un DEUXIEME payload distinct -> accepte (jamais rejete,
///    design assume) MAIS le bloc DEONTIC_AUDIT [consistent=false,
///    violations=1] apparait dans la reponse -- la contradiction
///    HISTORIQUE, invisible au check intra-payload, est bien detectee.
/// 4. Meme modalite repetee (MUST puis MUST_NOT sur un objet DIFFERENT) ->
///    aucun bloc DEONTIC_AUDIT (pas de faux positif sur l'historique).
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

fn deontic_payload(sender: &str, subject: &str, object: &str, modalities: &[&str]) -> String {
    let relations: String = modalities.iter().map(|m| {
        format!("RELATION [type=PERFORM, subject={subject}, object={object}, modality={m}]\n")
    }).collect();
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=SmokeTest, produced_by=SmokeTest]\n\
         INTENT_PAYLOAD [purpose=deontic_audit_smoke, sender={sender}, receiver=server]\n\
         {relations}\
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
    let port: u16 = 15180;
    let server = make_test_server(port);
    tokio::spawn(async move {
        server.start().await.expect("server start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();

    // ---- Scenario 1: MUST_NOT isole -- regression du faux positif ----
    println!("[1/4] RELATION[modality=MUST_NOT] isolee (doit passer normalement)...");
    let resp1 = send(port, &deontic_payload("agent_1", "agent_x", "delete_prod_db", &["MUST_NOT"])).await;
    let status1 = extract_field(&resp1, "META", "status");
    println!("      status={:?}", status1);
    if status1.as_deref() != Some("processed") {
        failures.push(format!("scenario 1: MUST_NOT isole doit etre traite normalement, reponse: {}", resp1));
    }

    // ---- Scenario 2: MUST + MUST_NOT meme (subject,object), MEME payload ----
    println!("[2/4] MUST et MUST_NOT sur le meme (subject,object), meme payload (doit etre rejete E107)...");
    let resp2 = send(port, &deontic_payload("agent_2", "agent_y", "backup_db", &["MUST", "MUST_NOT"])).await;
    let purpose2 = extract_field(&resp2, "INTENT_PAYLOAD", "purpose");
    let has_e107 = resp2.contains("E107");
    println!("      purpose={:?} E107_present={}", purpose2, has_e107);
    if purpose2.as_deref() != Some("validation_error") || !has_e107 {
        failures.push(format!("scenario 2: contradiction intra-payload doit etre rejetee avec E107, reponse: {}", resp2));
    }

    // ---- Scenario 3: MUST_NOT (payload A) puis MUST (payload B distinct) ----
    println!("[3/4] MUST_NOT etabli par un payload, MUST sur le meme (subject,object) dans un AUTRE payload...");
    let resp3a = send(port, &deontic_payload("agent_3a", "agent_z", "wipe_logs", &["MUST_NOT"])).await;
    let status3a = extract_field(&resp3a, "META", "status");
    if status3a.as_deref() != Some("processed") {
        failures.push(format!("scenario 3a: le premier payload (MUST_NOT seul) doit passer, reponse: {}", resp3a));
    }
    let resp3b = send(port, &deontic_payload("agent_3b", "agent_z", "wipe_logs", &["MUST"])).await;
    let status3b = extract_field(&resp3b, "META", "status");
    let audit_consistent3b = extract_field(&resp3b, "DEONTIC_AUDIT", "consistent");
    let audit_violations3b = extract_field(&resp3b, "DEONTIC_AUDIT", "violations");
    println!("      status={:?} DEONTIC_AUDIT consistent={:?} violations={:?}", status3b, audit_consistent3b, audit_violations3b);
    if status3b.as_deref() != Some("processed") {
        failures.push(format!("scenario 3b: une contradiction HISTORIQUE ne doit jamais etre rejetee (design assume), reponse: {}", resp3b));
    }
    if audit_consistent3b.as_deref() != Some("false") || audit_violations3b.as_deref() != Some("1") {
        failures.push(format!("scenario 3b: le bloc DEONTIC_AUDIT doit signaler 1 violation contre l'historique, reponse: {}", resp3b));
    }

    // ---- Scenario 4: aucun conflit -- pas de bloc DEONTIC_AUDIT ----
    println!("[4/4] Nouvelle modalite sur un objet DIFFERENT (aucun conflit attendu)...");
    let resp4 = send(port, &deontic_payload("agent_4", "agent_z", "rotate_keys", &["MUST"])).await;
    let has_audit_block4 = resp4.contains("DEONTIC_AUDIT");
    println!("      DEONTIC_AUDIT_present={}", has_audit_block4);
    if has_audit_block4 {
        failures.push(format!("scenario 4: aucun conflit reel -- le bloc DEONTIC_AUDIT ne doit pas apparaitre, reponse: {}", resp4));
    }

    println!();
    if failures.is_empty() {
        println!("✅ Tous les scenarios de l'audit deontique sont conformes.");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
