/// examples/benchmark_audit.rs -- grille d'audit empirique a 10 points
/// (2026-10-01, sur demande explicite d'Olivier) contre le serveur CSTL
/// DEJA EN COURS D'EXECUTION (port TCP 5050 / REST 8000, PID lance plus
/// tot dans la session, base ":memory:"-like fraiche sur disque
/// `cstl_adn.db`). Ne demarre PAS son propre serveur -- mesure le vrai
/// processus live, avec le vrai cout TCP/parse/validate/pipeline complet,
/// pas un micro-benchmark in-process qui sauterait tout ca.
///
/// Discipline de ce fichier: chaque nombre rapporte est mesure, jamais
/// invente. Quand une mesure n'a pas de sens (ex: endpoint REST qui
/// n'existe pas encore, DB verrouillee), c'est rapporte comme echec/absent,
/// pas remplace par un chiffre plausible.
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TCP_PORT: u16 = 5050;
const REST_PORT: u16 = 8000;
const DB_PATH: &str = "cstl_adn.db";

async fn send(payload: &str) -> (String, Duration) {
    let t0 = Instant::now();
    let mut stream = TcpStream::connect(("127.0.0.1", TCP_PORT)).await.expect("connect");
    stream.write_all(payload.as_bytes()).await.expect("send");
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
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
    let elapsed = t0.elapsed();
    (String::from_utf8_lossy(&buf).to_string(), elapsed)
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

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted_ms.len() as f64 - 1.0)).round() as usize;
    sorted_ms[idx.min(sorted_ms.len() - 1)]
}

fn stats_line(label: &str, mut samples_ms: Vec<f64>) -> String {
    samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = samples_ms.len();
    let min = samples_ms.first().copied().unwrap_or(0.0);
    let max = samples_ms.last().copied().unwrap_or(0.0);
    let mean = samples_ms.iter().sum::<f64>() / n.max(1) as f64;
    let p50 = percentile(&samples_ms, 50.0);
    let p95 = percentile(&samples_ms, 95.0);
    let p99 = percentile(&samples_ms, 99.0);
    format!(
        "{label}: n={n} min={min:.2}ms p50={p50:.2}ms mean={mean:.2}ms p95={p95:.2}ms p99={p99:.2}ms max={max:.2}ms"
    )
}

#[tokio::main]
async fn main() {
    println!("=== GRILLE D'AUDIT CSTL -- 10 POINTS (serveur live, port {TCP_PORT}/{REST_PORT}) ===\n");

    // Verifie que le serveur est bien joignable avant de commencer --
    // echoue fort plutot que de produire des zeros silencieux si le
    // serveur n'est pas la.
    let http = reqwest::Client::new();
    match http.get(format!("http://127.0.0.1:{REST_PORT}/health")).send().await {
        Ok(r) if r.status().is_success() => println!("[precheck] serveur REST joignable (/health 200 OK)\n"),
        Ok(r) => {
            eprintln!("[precheck] /health a repondu {} -- arret", r.status());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("[precheck] serveur REST injoignable sur :{REST_PORT}: {e} -- arret (le serveur tourne-t-il ?)");
            std::process::exit(1);
        }
    }

    // Taille DB avant toute charge de ce benchmark (point 8).
    let db_size_before = std::fs::metadata(DB_PATH).map(|m| m.len()).unwrap_or(0);

    // ============================================================
    // Point 1 + 2: latence TCP par message + debit soutenu
    // ============================================================
    println!("--- Point 1/10: Latence TCP (payload simple, N=100, sequentiel) ---");
    let n_latency = 100;
    let mut latencies_ms = Vec::with_capacity(n_latency);
    let bench_sender = "bench_latency_agent";
    for i in 0..n_latency {
        let payload = format!(
            "#!CSTL v5.0.0 MODE=A\n\
             META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
             INTENT_PAYLOAD [purpose=inform, sender={bench_sender}, receiver=server, subject=bench_point_{i}]\n\
             ---END---\n"
        );
        let (_, dt) = send(&payload).await;
        latencies_ms.push(dt.as_secs_f64() * 1000.0);
    }
    println!("{}", stats_line("latence/message", latencies_ms.clone()));
    let total_secs: f64 = latencies_ms.iter().sum::<f64>() / 1000.0;
    let throughput = n_latency as f64 / total_secs;
    println!("Point 2/10 -- debit soutenu (meme serie, connexions TCP sequentielles, 1 a la fois): {:.1} msg/s\n", throughput);

    // ============================================================
    // Point 3: taux de compression Master Compressor (production, storage)
    // ============================================================
    println!("--- Point 3/10: Taux de compression Master Compressor (reel, lu depuis master_compressed) ---");
    // 3 tailles de payload REELLES avec DEFINE/RELATION repetitifs
    // (le cas d'usage reel du Master Compressor -- texte semi-structure,
    // pas du bruit aleatoire incompressible).
    // Syntaxe wire REELLE (verifiee dans parser.rs, pas devinee): un bloc
    // DEFINE est "DEFINE <identifiant> AS <type> [attr=val,...]" (une ligne
    // par definition, pas de conteneur) ; RELATIONS est un CONTENEUR
    // multi-ligne exact -- "RELATIONS [" seul sur sa ligne, puis des lignes
    // "(sujet) OPERATEUR objet", puis "]" seul sur sa ligne. La premiere
    // version de ce point envoyait une syntaxe inventee ("DEFINE [term=...]",
    // "RELATIONS [subject=...]") que le parser ignorait silencieusement
    // (0 blocs parses a chaque fois, confirme par les logs serveur) --
    // mesurait donc le ratio de compression d'un triple VIDE, pas du
    // contenu reel. Corrige ici avant de publier le moindre chiffre.
    let sizes: Vec<(&str, usize)> = vec![("petit", 3), ("moyen", 20), ("grand", 80)];
    let mut compression_hashes = Vec::new();
    for (label, n_defines) in &sizes {
        let defines: String = (0..*n_defines)
            .map(|i| format!("DEFINE concept_{i} AS entity [definition=Une+definition+repetitive+pour+tester+la+compression+du+terme+{i}]\n"))
            .collect();
        let relations_inner: String = (0..*n_defines)
            .map(|i| format!("(concept_{i}) implies concept_{}\n", (i + 1) % n_defines))
            .collect();
        let payload = format!(
            "#!CSTL v5.0.0 MODE=A\n\
             META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
             INTENT_PAYLOAD [purpose=inform, sender=bench_compression_agent, receiver=server, subject=compression_test_{label}]\n\
             {defines}\
             RELATIONS [\n\
             {relations_inner}\
             ]\n\
             ---END---\n"
        );
        let (resp, _) = send(&payload).await;
        let hash = extract_field(&resp, "AUDIT", "hash");
        println!("      payload '{label}' ({} octets bruts) envoye, hash={:?}", payload.len(), hash);
        if let Some(h) = hash {
            compression_hashes.push((label.to_string(), h));
        }
    }
    // Laisse le temps au handler de committer le master_compressed (ecriture
    // synchrone normalement, petite marge de securite quand meme).
    tokio::time::sleep(Duration::from_millis(200)).await;
    match rusqlite::Connection::open_with_flags(DB_PATH, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => {
            for (label, hash) in &compression_hashes {
                let row: Result<(i64, i64), _> = conn.query_row(
                    "SELECT reference_payload_text_len, compressed_len FROM master_compressed WHERE hash = ?1",
                    [hash],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                );
                match row {
                    Ok((reference_len, compressed_len)) => {
                        let ratio = 1.0 - (compressed_len as f64 / reference_len.max(1) as f64);
                        println!(
                            "      {label}: reference={reference_len}o compressed={compressed_len}o ratio={:.1}% de reduction",
                            ratio * 100.0
                        );
                    }
                    Err(e) => println!("      {label}: pas de ligne master_compressed trouvee (hash={hash}): {e}"),
                }
            }
        }
        Err(e) => println!("      [echec] impossible d'ouvrir {DB_PATH} en lecture seule (verrouillee par le serveur ?): {e}"),
    }
    println!();

    // ============================================================
    // Point 4: overhead de verification de signature Ed25519
    // ============================================================
    println!("--- Point 4/10: Overhead de verification de signature Ed25519 (meme pipeline, sender non-enregistre vs enregistre+signe) ---");
    let unsigned_payload = |i: usize| format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
         INTENT_PAYLOAD [purpose=inform, sender=bench_sig_unregistered, receiver=server, subject=sig_overhead_unsigned_{i}]\n\
         ---END---\n"
    );
    let n_sig = 30;
    let mut unsigned_latencies = Vec::with_capacity(n_sig);
    for i in 0..n_sig {
        let (_, dt) = send(&unsigned_payload(i)).await;
        unsigned_latencies.push(dt.as_secs_f64() * 1000.0);
    }

    // Enregistre un agent avec une vraie cle Ed25519 (meme pattern que les
    // smoke tests deontic/pub_fn_wiring).
    let signing_key = SigningKey::generate(&mut OsRng);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let agent_name = "bench_sig_registered";
    let draft = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication]\n\
         ---END---\n"
    );
    let parsed_draft = cstl_parser::server::parser::parse_payload(&draft).expect("brouillon doit parser");
    let sig = signing_key.sign(&cstl_parser::server::audit::signing_bytes(&parsed_draft));
    let sig_hex = hex::encode(sig.to_bytes());
    let register_payload = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=agent_register, sender={agent_name}, receiver=server, name={agent_name}, capabilities=communication, signature={sig_hex}]\n\
         ---END---\n"
    );
    let (resp_reg, _) = send(&register_payload).await;
    let reg_ok = extract_field(&resp_reg, "INTENT_PAYLOAD", "purpose").as_deref() == Some("agent_register_ack");
    println!("      inscription de l'agent signe: {}", if reg_ok { "OK" } else { "ECHEC -- le reste du point 4 sera invalide" });

    let mut signed_latencies = Vec::with_capacity(n_sig);
    if reg_ok {
        for i in 0..n_sig {
            let draft = format!(
                "#!CSTL v5.0.0 MODE=A\n\
                 META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={pubkey_hex}]\n\
                 INTENT_PAYLOAD [purpose=inform, sender={agent_name}, receiver=server, subject=sig_overhead_signed_{i}]\n\
                 ---END---\n"
            );
            let parsed = cstl_parser::server::parser::parse_payload(&draft).expect("doit parser");
            let msg_sig = signing_key.sign(&cstl_parser::server::audit::signing_bytes(&parsed));
            let msg_sig_hex = hex::encode(msg_sig.to_bytes());
            let signed_payload = format!(
                "#!CSTL v5.0.0 MODE=A\n\
                 META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={pubkey_hex}]\n\
                 INTENT_PAYLOAD [purpose=inform, sender={agent_name}, receiver=server, subject=sig_overhead_signed_{i}, signature={msg_sig_hex}]\n\
                 ---END---\n"
            );
            let (_, dt) = send(&signed_payload).await;
            signed_latencies.push(dt.as_secs_f64() * 1000.0);
        }
    }
    println!("{}", stats_line("non-signe (sender inconnu, verif ignoree)", unsigned_latencies.clone()));
    if !signed_latencies.is_empty() {
        println!("{}", stats_line("signe+verifie (sender enregistre avec cle)", signed_latencies.clone()));
        let mean_u: f64 = unsigned_latencies.iter().sum::<f64>() / unsigned_latencies.len() as f64;
        let mean_s: f64 = signed_latencies.iter().sum::<f64>() / signed_latencies.len() as f64;
        println!("      delta moyen attribuable a la verification Ed25519 (+ parsing du champ signature): {:.3}ms", mean_s - mean_u);
    }
    println!();

    // ============================================================
    // Point 5: cycle d'arbitrage complet, bout en bout
    // ============================================================
    println!("--- Point 5/10: Cycle d'arbitrage complet (open -> assign -> ruling -> peer_review -> escalate -> finalize) ---");
    let t_cycle_start = Instant::now();
    let mut step_times = Vec::new();

    let arbiter_key = SigningKey::generate(&mut OsRng);
    let arbiter_id = "bench_arbiter_1";
    let t0 = Instant::now();
    let arb_pubkey_hex = hex::encode(arbiter_key.verifying_key().to_bytes());
    let arb_draft = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={arb_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=bench_opener, receiver=server, arbiter_id={arbiter_id}, authority_level=senior, stake_amount=100]\n\
         ---END---\n"
    );
    let arb_parsed = cstl_parser::server::parser::parse_payload(&arb_draft).expect("doit parser");
    let arb_sig = arbiter_key.sign(&cstl_parser::server::audit::signing_bytes(&arb_parsed));
    let arb_sig_hex = hex::encode(arb_sig.to_bytes());
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={arb_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=bench_opener, receiver=server, arbiter_id={arbiter_id}, authority_level=senior, stake_amount=100, signature={arb_sig_hex}]\n\
         ---END---\n"
    )).await;
    step_times.push(("arbiter_register", t0.elapsed()));

    let reviewer_key = SigningKey::generate(&mut OsRng);
    let reviewer_id = "bench_arbiter_2";
    let t0 = Instant::now();
    let rev_pubkey_hex = hex::encode(reviewer_key.verifying_key().to_bytes());
    let rev_draft = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={rev_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=bench_opener, receiver=server, arbiter_id={reviewer_id}, authority_level=senior, stake_amount=100]\n\
         ---END---\n"
    );
    let rev_parsed = cstl_parser::server::parser::parse_payload(&rev_draft).expect("doit parser");
    let rev_sig = reviewer_key.sign(&cstl_parser::server::audit::signing_bytes(&rev_parsed));
    let rev_sig_hex = hex::encode(rev_sig.to_bytes());
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, public_key={rev_pubkey_hex}]\n\
         INTENT_PAYLOAD [purpose=arbiter_register, sender=bench_opener, receiver=server, arbiter_id={reviewer_id}, authority_level=senior, stake_amount=100, signature={rev_sig_hex}]\n\
         ---END---\n"
    )).await;
    step_times.push(("arbiter_register (2e)", t0.elapsed()));

    let t0 = Instant::now();
    let resp_open = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=bench_opener, receiver=server, action=open_case, escalation_source=benchmark_audit, contradiction_type=logical_break, description=Benchmark+cycle]\n\
         ---END---\n"
    )).await.0;
    step_times.push(("open_case", t0.elapsed()));
    let case_id = extract_field(&resp_open, "INTENT_PAYLOAD", "case_id").unwrap_or_default();

    let t0 = Instant::now();
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=bench_opener, receiver=server, action=assign_arbiters, case_id={case_id}, arbiter_count=2]\n\
         ---END---\n"
    )).await;
    step_times.push(("assign_arbiters", t0.elapsed()));

    let ruling_id = "ruling_bench_1";
    let decision = "accept_assertion_A";
    let justification = "benchmark_justification";
    let ruling_sig = arbiter_key.sign(format!("{ruling_id}||{decision}||{justification}").as_bytes());
    let ruling_sig_hex = hex::encode(ruling_sig.to_bytes());
    let t0 = Instant::now();
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, signature={ruling_sig_hex}]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=bench_opener, receiver=server, action=submit_ruling, case_id={case_id}, ruling_id={ruling_id}, arbiter_id={arbiter_id}, decision={decision}, justification={justification}]\n\
         ---END---\n"
    )).await;
    step_times.push(("submit_ruling", t0.elapsed()));

    let review_sig = reviewer_key.sign(format!("{reviewer_id}||{ruling_id}").as_bytes());
    let review_sig_hex = hex::encode(review_sig.to_bytes());
    let t0 = Instant::now();
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit, signature={review_sig_hex}]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=bench_opener, receiver=server, action=peer_review, ruling_id={ruling_id}, arbiter_id={reviewer_id}]\n\
         ---END---\n"
    )).await;
    step_times.push(("peer_review", t0.elapsed()));

    let t0 = Instant::now();
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=bench_opener, receiver=server, action=escalate_to_council, case_id={case_id}]\n\
         ---END---\n"
    )).await;
    step_times.push(("escalate_to_council", t0.elapsed()));

    let t0 = Instant::now();
    let resp_finalize = send(&format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\n\
         INTENT_PAYLOAD [purpose=arbitrage_channel, sender=bench_opener, receiver=server, action=finalize_case, case_id={case_id}]\n\
         ---END---\n"
    )).await.0;
    step_times.push(("finalize_case", t0.elapsed()));
    let finalize_ok = extract_field(&resp_finalize, "INTENT_PAYLOAD", "purpose").as_deref() == Some("case_finalized");

    let cycle_total = t_cycle_start.elapsed();
    for (step, dt) in &step_times {
        println!("      {step}: {:.2}ms", dt.as_secs_f64() * 1000.0);
    }
    println!("      TOTAL cycle (8 round-trips TCP): {:.2}ms -- finalize_case reussi: {}", cycle_total.as_secs_f64() * 1000.0, finalize_ok);
    println!();

    // ============================================================
    // Point 6: latence des endpoints REST
    // ============================================================
    println!("--- Point 6/10: Latence endpoints REST (p50 sur N=20 chacun) ---");
    let rest_endpoints = vec![
        "/health",
        "/audit/stats",
        "/graphify/export",
        "/graphify/stats",
        "/deontic/executions",
        "/wai/stats",
        "/calibration/agents",
    ];
    for ep in &rest_endpoints {
        let mut lat = Vec::with_capacity(20);
        for _ in 0..20 {
            let t0 = Instant::now();
            let _ = http.get(format!("http://127.0.0.1:{REST_PORT}{ep}")).send().await;
            lat.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{}", stats_line(&format!("GET {ep}"), lat));
    }
    println!();

    // ============================================================
    // Point 7: overhead ADN_DELTA (NoChange rapide vs changement reel)
    // ============================================================
    println!("--- Point 7/10: Overhead ADN_DELTA (parent identique=NoChange vs parent different=diff reel) ---");
    let delta_sender = "bench_delta_agent";
    let base_subject = "bench_delta_subject_A_identique_a_chaque_fois_pour_mesurer_nochange";
    let _ = send(&format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\nINTENT_PAYLOAD [purpose=inform, sender={delta_sender}, receiver=server, subject={base_subject}]\n---END---\n"
    )).await;
    let mut nochange_lat = Vec::with_capacity(15);
    for _ in 0..15 {
        let (_, dt) = send(&format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\nINTENT_PAYLOAD [purpose=inform, sender={delta_sender}, receiver=server, subject={base_subject}]\n---END---\n"
        )).await;
        nochange_lat.push(dt.as_secs_f64() * 1000.0);
    }
    let mut change_lat = Vec::with_capacity(15);
    for i in 0..15 {
        let varied = format!("bench_delta_subject_variant_{i}_completement_different_a_chaque_envoi_pour_forcer_un_diff_reel_non_trivial_avec_beaucoup_de_texte_different");
        let (_, dt) = send(&format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\nINTENT_PAYLOAD [purpose=inform, sender={delta_sender}, receiver=server, subject={varied}]\n---END---\n"
        )).await;
        change_lat.push(dt.as_secs_f64() * 1000.0);
    }
    println!("{}", stats_line("NoChange (parent identique)", nochange_lat.clone()));
    println!("{}", stats_line("Changement reel (diff calcule)", change_lat.clone()));
    let mean_nc: f64 = nochange_lat.iter().sum::<f64>() / nochange_lat.len() as f64;
    let mean_c: f64 = change_lat.iter().sum::<f64>() / change_lat.len() as f64;
    println!("      delta moyen attribuable au calcul de diff reel: {:.3}ms\n", mean_c - mean_nc);

    // ============================================================
    // Point 8: croissance reelle de la DB
    // ============================================================
    println!("--- Point 8/10: Croissance de la base SQLite ---");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let db_size_after = std::fs::metadata(DB_PATH).map(|m| m.len()).unwrap_or(0);
    let total_messages_sent = n_latency + sizes.len() + n_sig + n_sig + step_times.len() + 1 + 15 + 15;
    let growth = db_size_after.saturating_sub(db_size_before);
    println!(
        "      taille avant: {db_size_before} octets, apres: {db_size_after} octets, croissance: {growth} octets sur ~{total_messages_sent} messages envoyes par ce benchmark -> ~{:.0} octets/message (moyenne grossiere, inclut index/overhead SQLite, pas juste les octets utiles)\n",
        growth as f64 / total_messages_sent.max(1) as f64
    );

    // ============================================================
    // Point 9: latence des requetes Graphify sur un graphe reel non-trivial
    // ============================================================
    println!("--- Point 9/10: Latence requetes Graphify (graphe reel, apres ~{total_messages_sent} messages) ---");
    let graphify_queries = vec![
        format!("/graphify/filter?node_type=agent"),
        format!("/graphify/search?q=bench"),
        format!("/graphify/traverse?start={bench_sender}&depth=2"),
    ];
    for q in &graphify_queries {
        let t0 = Instant::now();
        let resp = http.get(format!("http://127.0.0.1:{REST_PORT}{q}")).send().await;
        let dt = t0.elapsed();
        match resp {
            Ok(r) => {
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let node_count = body["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
                println!("      GET {q} -> {:.2}ms, {node_count} noeud(s)", dt.as_secs_f64() * 1000.0);
            }
            Err(e) => println!("      GET {q} -> echec: {e}"),
        }
    }
    println!();

    // ============================================================
    // Point 10: robustesse sous rafale (succes/echec, latence sous charge)
    // ============================================================
    println!("--- Point 10/10: Robustesse sous rafale (N=50, aussi vite que possible, sequentiel single-client) ---");
    let n_burst = 50;
    let mut burst_lat = Vec::with_capacity(n_burst);
    let mut ok_count = 0usize;
    let t_burst_start = Instant::now();
    for i in 0..n_burst {
        let payload = format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=BenchmarkAudit, produced_by=BenchmarkAudit]\nINTENT_PAYLOAD [purpose=inform, sender=bench_burst_agent, receiver=server, subject=burst_{i}]\n---END---\n"
        );
        let (resp, dt) = send(&payload).await;
        burst_lat.push(dt.as_secs_f64() * 1000.0);
        if resp.contains("purpose=") && !resp.contains("status=error") {
            ok_count += 1;
        }
    }
    let burst_total = t_burst_start.elapsed();
    println!("{}", stats_line("latence sous rafale", burst_lat));
    println!(
        "      succes: {ok_count}/{n_burst} ({:.1}%), debit rafale: {:.1} msg/s (1 connexion TCP a la fois, pas de parallelisme client)\n",
        100.0 * ok_count as f64 / n_burst as f64,
        n_burst as f64 / burst_total.as_secs_f64()
    );

    println!("=== FIN DE LA GRILLE -- 10/10 points mesures contre le serveur live ===");
}
