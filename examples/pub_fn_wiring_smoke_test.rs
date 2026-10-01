/// examples/pub_fn_wiring_smoke_test.rs -- verification live du cablage
/// des trouvailles de l'audit "pub fn jamais appelees nulle part"
/// (2026-10-01, sur demande explicite "regle tout pour qu'on puisse
/// essayer cstl"). Quatre clusters, tous reels (vraie logique DB/metier
/// deja existante, simplement jamais relies a un point d'entree):
///
/// 1. Arbitrage -- peer review + escalade au conseil. `peer_review_async`
///    et `escalate_to_council_async` (arbitrage.rs) existaient depuis
///    longtemps mais `handler.rs` n'avait aucune action wire pour les
///    declencher. Consequence concrete, prouvee ici plutot qu'affirmee:
///    avant ce cablage, `finalize_case` echouait TOUJOURS avec
///    QuorumNotReached des qu'un conseil exige >=1 revue par les pairs
///    (`RestrictedCouncil::quorum_size()`), puisque rien n'appelait jamais
///    `save_peer_review`. Cable: actions `peer_review`/`escalate_to_council`
///    sur `purpose=arbitrage_channel`.
/// 2. Registre de dictionnaires WAI -- `wai_registry` est construit et une
///    version y est enregistree au demarrage (`server/mod.rs`) mais rien
///    ne le relisait jamais. Cable: `GET /wai/dictionaries`,
///    `/wai/dictionaries/latest`, `/wai/stats`.
/// 3. Requetes Graphify -- `filter_nodes_by_type`/`filter_edges_by_type`/
///    `search_nodes`/`traverse_graph` existaient depuis la creation du
///    module mais seuls `/export`/`/stats` avaient une route. Cable:
///    `GET /graphify/filter`, `/graphify/search`, `/graphify/traverse`.
/// 4. Calibration EWMA -- `sigma_calibrator` est deja partage sur
///    `ServerContext` et alimente par un pont reel (kb_verify ->
///    observe_verdict, deja cable avant cette session) mais n'avait aucune
///    route d'inspection. Cable: `GET /calibration/agents`,
///    `/calibration/agents/:agent_name`.
///
/// Limite honnete assumee et VERIFIEE ci-dessous plutot que contournee:
/// alimenter reellement `sigma_calibrator` exige une confirmation Wikidata
/// externe (reseau indisponible dans ce bac a sable) -- les endpoints de
/// calibration sont donc verifies sur leur FORME (liste vide valide,
/// 404 propre pour un agent inconnu), pas sur une donnee observee. Documente
/// ici, pas simule.
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

fn make_test_server(tcp_port: u16) -> CstlNativeServer {
    let mut server = CstlNativeServer::with_data_path(tcp_port, ":memory:");
    let mut registry = AgentRegistry::new();
    registry.register(AgentCard {
        name: "pubfn_smoke_router".to_string(),
        version: "5.0.0".to_string(),
        capabilities: vec!["communication".to_string()],
        trust_score: 0.9,
        public_key: None,
    });
    server.agent_registry = Arc::new(RwLock::new(registry));
    // quorum_size() = ceil(2/3 * 1) = 1 -- un seul peer review suffit a
    // satisfaire le quorum de finalize_case_async.
    server.restricted_council = Arc::new(RestrictedCouncil::single_member("olivier"));
    server
}

fn arbitrage_channel_payload(action: &str, case_id: Option<&str>, extra: &str) -> String {
    let case_field = case_id.map(|c| format!(", case_id={c}")).unwrap_or_default();
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=PubFnSmokeTest, produced_by=PubFnSmokeTest]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=pubfn_smoke_opener, receiver=server, action={action}{case_field}{extra}]\n\
         ---END---\n"
    )
}

fn register_arbiter_payload(key: &SigningKey, arbiter_id: &str) -> String {
    let pubkey_hex = hex::encode(key.verifying_key().to_bytes());
    let draft = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=PubFnSmokeTest, produced_by=PubFnSmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=pubfn_smoke_opener, receiver=server, arbiter_id={arbiter_id}, authority_level=senior, stake_amount=100]\n\
         ---END---\n"
    );
    let parsed = parse_payload(&draft).expect("brouillon arbiter_register doit parser");
    let sig = key.sign(&signing_bytes(&parsed));
    let sig_hex = hex::encode(sig.to_bytes());
    format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=PubFnSmokeTest, produced_by=PubFnSmokeTest, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=pubfn_smoke_opener, receiver=server, arbiter_id={arbiter_id}, authority_level=senior, stake_amount=100, signature={sig_hex}]\n\
         ---END---\n"
    )
}

#[tokio::main]
async fn main() {
    let tcp_port: u16 = 15220;
    let rest_port: u16 = 15221;

    let server = make_test_server(tcp_port);
    let adn_store_for_rest = server.adn_store.clone();
    let chain_for_rest = server.chain.clone();
    let deontic_for_rest = server.deontic.clone();
    let wai_registry_for_rest = server.wai_registry.clone();
    let sigma_calibrator_for_rest = server.sigma_calibrator.clone();

    tokio::spawn(async move {
        server.start().await.expect("TCP server start");
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
        .expect("REST API start");
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut failures = Vec::new();
    let http = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", rest_port);

    // ============================================================
    // Cluster 1: arbitrage peer_review + escalate_to_council
    // ============================================================
    println!("[1/4] Arbitrage: ouverture + assignation + ruling + PEER REVIEW + finalize (quorum reellement atteint)...");

    let arbiter_key = SigningKey::generate(&mut OsRng);
    let arbiter_id = "pubfn_smoke_arbiter";
    let resp_arbiter = send(tcp_port, &register_arbiter_payload(&arbiter_key, arbiter_id)).await;
    if extract_field(&resp_arbiter, "INTENT_PAYLOAD", "purpose").as_deref() != Some("arbiter_register_ack") {
        failures.push(format!("1: arbiter_register (arbitre principal) aurait du reussir, recu: {resp_arbiter}"));
    }

    // Deuxieme arbitre: c'est LUI qui va signer la revue par les pairs
    // (un arbitre ne revoit pas son propre ruling dans un systeme serieux,
    // meme si verify_ruling_signatures ne l'interdirait pas techniquement
    // ici -- choix de test volontairement realiste).
    let reviewer_key = SigningKey::generate(&mut OsRng);
    let reviewer_id = "pubfn_smoke_reviewer";
    let resp_reviewer = send(tcp_port, &register_arbiter_payload(&reviewer_key, reviewer_id)).await;
    if extract_field(&resp_reviewer, "INTENT_PAYLOAD", "purpose").as_deref() != Some("arbiter_register_ack") {
        failures.push(format!("1: arbiter_register (reviewer) aurait du reussir, recu: {resp_reviewer}"));
    }

    let resp_open = send(tcp_port, &arbitrage_channel_payload(
        "open_case", None, ", escalation_source=pubfn_smoke_test, contradiction_type=logical_break, description=Test+pub+fn+wiring",
    )).await;
    let case_id = extract_field(&resp_open, "INTENT_PAYLOAD", "case_id");
    println!("      case_id={:?}", case_id);

    let Some(cid) = case_id else {
        failures.push("1: open_case aurait du retourner un case_id -- reste du cluster 1 saute".to_string());
        println!();
        for f in &failures { println!("   - {f}"); }
        std::process::exit(1);
    };

    let resp_assign = send(tcp_port, &arbitrage_channel_payload("assign_arbiters", Some(&cid), ", arbiter_count=2")).await;
    let assigned = extract_field(&resp_assign, "INTENT_PAYLOAD", "assigned_arbiters").unwrap_or_default();
    println!("      assigned_arbiters={:?}", assigned);
    if !assigned.split(';').any(|a| a == arbiter_id) {
        failures.push(format!("1: l'arbitre principal ({arbiter_id}) aurait du etre assigne, recu {assigned}"));
    }

    let ruling_id = "ruling_pubfn_smoke_1";
    let decision = "accept_assertion_A";
    let justification = "pubfn_smoke_test_justification";
    let ruling_signing_payload = format!("{ruling_id}||{decision}||{justification}");
    let ruling_sig = arbiter_key.sign(ruling_signing_payload.as_bytes());
    let ruling_sig_hex = hex::encode(ruling_sig.to_bytes());

    let submit_ruling_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=PubFnSmokeTest, produced_by=PubFnSmokeTest, signature={ruling_sig_hex}]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=pubfn_smoke_opener, receiver=server, action=submit_ruling, case_id={cid}, ruling_id={ruling_id}, arbiter_id={arbiter_id}, decision={decision}, justification={justification}]\n\
         ---END---\n"
    );
    let resp_ruling = send(tcp_port, &submit_ruling_payload).await;
    if extract_field(&resp_ruling, "INTENT_PAYLOAD", "purpose").as_deref() != Some("ruling_submitted") {
        failures.push(format!("1: submit_ruling aurait du reussir, recu: {resp_ruling}"));
    }

    // AVANT tout peer review: finalize_case DOIT echouer (quorum=1 non
    // atteint, 0 revue par les pairs) -- confirme la trouvaille de l'audit
    // (le gap etait reel, pas suppose).
    let resp_finalize_too_early = send(tcp_port, &arbitrage_channel_payload("finalize_case", Some(&cid), "")).await;
    let purpose_too_early = extract_field(&resp_finalize_too_early, "INTENT_PAYLOAD", "purpose");
    println!("      finalize_case AVANT peer_review -> purpose={:?} (doit etre arbitrage_rejected)", purpose_too_early);
    if purpose_too_early.as_deref() != Some("arbitrage_rejected") {
        failures.push(format!("1: finalize_case avant tout peer review aurait du echouer (quorum non atteint), recu purpose={:?}", purpose_too_early));
    }

    // Peer review reel: le reviewer signe "{reviewing_arbiter_id}||{ruling_id}"
    // en brut (voir peer_review_async), pas via signing_bytes/CstlPayload.
    let review_signing_payload = format!("{reviewer_id}||{ruling_id}");
    let review_sig = reviewer_key.sign(review_signing_payload.as_bytes());
    let review_sig_hex = hex::encode(review_sig.to_bytes());
    let peer_review_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=PubFnSmokeTest, produced_by=PubFnSmokeTest, signature={review_sig_hex}]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=pubfn_smoke_opener, receiver=server, action=peer_review, ruling_id={ruling_id}, arbiter_id={reviewer_id}]\n\
         ---END---\n"
    );
    let resp_peer_review = send(tcp_port, &peer_review_payload).await;
    let purpose_peer_review = extract_field(&resp_peer_review, "INTENT_PAYLOAD", "purpose");
    println!("      peer_review -> purpose={:?}", purpose_peer_review);
    if purpose_peer_review.as_deref() != Some("peer_review_added") {
        failures.push(format!("1: peer_review (signe reellement par un second arbitre enregistre) aurait du reussir, recu purpose={:?} ({resp_peer_review})", purpose_peer_review));
    }

    // escalate_to_council: verifie seulement que l'action existe et
    // persiste reellement (statut EscalatedToCouncil en DB) -- n'empeche
    // pas la finalisation normale juste apres (pas de garde croisee dans
    // escalate_to_council_async lui-meme, voir arbitrage.rs).
    let resp_escalate = send(tcp_port, &arbitrage_channel_payload("escalate_to_council", Some(&cid), "")).await;
    let purpose_escalate = extract_field(&resp_escalate, "INTENT_PAYLOAD", "purpose");
    println!("      escalate_to_council -> purpose={:?}", purpose_escalate);
    if purpose_escalate.as_deref() != Some("case_escalated") {
        failures.push(format!("1: escalate_to_council aurait du reussir, recu purpose={:?} ({resp_escalate})", purpose_escalate));
    }

    // finalize_case APRES le peer review: quorum=1 maintenant atteint ->
    // doit reussir cette fois.
    let resp_finalize = send(tcp_port, &arbitrage_channel_payload("finalize_case", Some(&cid), "")).await;
    let purpose_finalize = extract_field(&resp_finalize, "INTENT_PAYLOAD", "purpose");
    println!("      finalize_case APRES peer_review -> purpose={:?}", purpose_finalize);
    if purpose_finalize.as_deref() != Some("case_finalized") {
        failures.push(format!("1: finalize_case apres peer_review (quorum=1 atteint) aurait du reussir, recu purpose={:?} ({resp_finalize})", purpose_finalize));
    }

    // ============================================================
    // Cluster 2: registre de dictionnaires WAI
    // ============================================================
    println!("[2/4] Registre WAI: /wai/dictionaries, /wai/dictionaries/latest, /wai/stats...");

    match http.get(format!("{base}/wai/dictionaries")).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let versions = body.as_array().map(|a| a.len()).unwrap_or(0);
            println!("      /wai/dictionaries -> status={} versions={}", status, versions);
            if !status.is_success() || versions == 0 {
                failures.push(format!("2: /wai/dictionaries devait repondre 2xx avec >=1 version enregistree au demarrage, recu status={status} versions={versions}"));
            }
        }
        Err(e) => failures.push(format!("2: /wai/dictionaries requete echouee: {e}")),
    }

    match http.get(format!("{base}/wai/dictionaries/latest")).send().await {
        Ok(resp) => {
            let status = resp.status();
            println!("      /wai/dictionaries/latest -> status={}", status);
            if !status.is_success() {
                failures.push(format!("2: /wai/dictionaries/latest devait repondre 2xx (une version existe), recu status={status}"));
            }
        }
        Err(e) => failures.push(format!("2: /wai/dictionaries/latest requete echouee: {e}")),
    }

    match http.get(format!("{base}/wai/stats")).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let total_versions = body["total_versions"].as_u64();
            println!("      /wai/stats -> status={} total_versions={:?}", status, total_versions);
            if !status.is_success() || total_versions != Some(1) {
                failures.push(format!("2: /wai/stats devait rapporter total_versions=1, recu status={status} total_versions={:?}", total_versions));
            }
        }
        Err(e) => failures.push(format!("2: /wai/stats requete echouee: {e}")),
    }

    // ============================================================
    // Cluster 3: requetes Graphify (filter/search/traverse)
    // ============================================================
    println!("[3/4] Requetes Graphify: /graphify/filter, /graphify/search, /graphify/traverse...");
    // Trouvaille en construisant ce test: `ctx.chain` (la HashChain que
    // Graphify exporte) n'est alimentee QUE par le chemin normal de
    // stockage ADN (`handler.rs:983`, `chain.lock().await.append(&payload)`)
    // -- tous les messages `purpose=arbitrage_channel`/`arbiter_register`
    // du cluster 1 court-circuitent AVANT ce point (`continue`), donc ne
    // touchent jamais la chaine d'audit. Un payload "normal" (purpose=inform)
    // est donc necessaire ici pour peupler le graphe avant de tester les
    // routes de requete -- sans ca, /graphify/filter|search|traverse
    // repondent 200 avec 0 noeud (comportement correct, pas un bug de
    // cablage, confirme en le decouvrant: la chaine etait reellement vide).
    let normal_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=PubFnSmokeTest, produced_by=PubFnSmokeTest]\n\
         INTENT_PAYLOAD [purpose=inform, sender={arbiter_id}, receiver=server, subject=pubfn_smoke_graphify_seed]\n\
         ---END---\n"
    );
    let resp_normal = send(tcp_port, &normal_payload).await;
    println!("      payload normal envoye pour peupler la chaine d'audit (purpose={:?})", extract_field(&resp_normal, "INTENT_PAYLOAD", "purpose"));

    match http.get(format!("{base}/graphify/filter?node_type=agent")).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let node_count = body["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("      /graphify/filter?node_type=agent -> status={} nodes={}", status, node_count);
            if !status.is_success() || node_count == 0 {
                failures.push(format!("3: /graphify/filter?node_type=agent devait retourner >=1 noeud agent, recu status={status} nodes={node_count}"));
            }
        }
        Err(e) => failures.push(format!("3: /graphify/filter requete echouee: {e}")),
    }

    match http.get(format!("{base}/graphify/search?q=pubfn_smoke")).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let node_count = body["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("      /graphify/search?q=pubfn_smoke -> status={} nodes={}", status, node_count);
            if !status.is_success() || node_count == 0 {
                failures.push(format!("3: /graphify/search?q=pubfn_smoke devait trouver >=1 noeud (agents nommes pubfn_smoke_*), recu status={status} nodes={node_count}"));
            }
        }
        Err(e) => failures.push(format!("3: /graphify/search requete echouee: {e}")),
    }

    match http.get(format!("{base}/graphify/search")).send().await {
        Ok(resp) => {
            let status = resp.status();
            println!("      /graphify/search (sans q) -> status={} (doit etre 400)", status);
            if status.as_u16() != 400 {
                failures.push(format!("3: /graphify/search sans parametre 'q' devait repondre 400, recu status={status}"));
            }
        }
        Err(e) => failures.push(format!("3: /graphify/search (sans q) requete echouee: {e}")),
    }

    match http.get(format!("{base}/graphify/traverse?start={arbiter_id}&depth=2")).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let node_count = body["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
            println!("      /graphify/traverse?start={}&depth=2 -> status={} nodes={}", arbiter_id, status, node_count);
            if !status.is_success() || node_count == 0 {
                failures.push(format!("3: /graphify/traverse depuis l'arbitre principal devait retourner >=1 noeud, recu status={status} nodes={node_count}"));
            }
        }
        Err(e) => failures.push(format!("3: /graphify/traverse requete echouee: {e}")),
    }

    // ============================================================
    // Cluster 4: calibration EWMA (verification de forme -- voir limite
    // honnete documentee en tete de fichier)
    // ============================================================
    println!("[4/4] Calibration EWMA: /calibration/agents, /calibration/agents/:agent_name (forme seulement, voir limite honnete)...");

    match http.get(format!("{base}/calibration/agents")).send().await {
        Ok(resp) => {
            let status = resp.status();
            println!("      /calibration/agents -> status={} (liste, possiblement vide -- aucun verdict Wikidata observable hors-ligne)", status);
            if !status.is_success() {
                failures.push(format!("4: /calibration/agents devait repondre 2xx meme avec une liste vide, recu status={status}"));
            }
        }
        Err(e) => failures.push(format!("4: /calibration/agents requete echouee: {e}")),
    }

    match http.get(format!("{base}/calibration/agents/unknown_agent_never_seen")).send().await {
        Ok(resp) => {
            let status = resp.status();
            println!("      /calibration/agents/unknown_agent_never_seen -> status={} (doit etre 404)", status);
            if status.as_u16() != 404 {
                failures.push(format!("4: /calibration/agents/<inconnu> devait repondre 404, recu status={status}"));
            }
        }
        Err(e) => failures.push(format!("4: /calibration/agents/<inconnu> requete echouee: {e}")),
    }

    println!();
    if failures.is_empty() {
        println!("✅ Les 4 clusters de l'audit \"pub fn jamais appelees\" sont reellement cables et verifies live (arbitrage peer_review/escalate_to_council avec quorum reellement teste avant/apres, registre WAI, requetes Graphify, calibration EWMA).");
    } else {
        println!("❌ {} echec(s):", failures.len());
        for f in &failures {
            println!("   - {}", f);
        }
        std::process::exit(1);
    }
}
