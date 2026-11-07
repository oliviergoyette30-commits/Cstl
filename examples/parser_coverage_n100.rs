/// examples/parser_coverage_n100.rs -- reponse directe a la demande d'Olivier
/// (2026-11-03) "regle tout jusqu'a ce que ce soit regle", item #3 de la
/// liste identifiee dans ANALYSE_COMPLETE_FRANCHE_CSTL_2026-10-11.md /
/// AUDIT_COMPLET_CSTL_2026-09-26.md (LANG-1, PUB-1) : "refaire la premiere
/// vraie experience de la these a N>=100, encodeur contraint au catalogue
/// officiel, valide par le vrai parser".
///
/// FAIT : genere 100 payloads varies, construits a partir du catalogue
/// OFFICIEL D'OPERATEURS (`semantic::official_operators_fn()`, pas d'operateur
/// invente -- correctif direct de LANG-1), les fait tous passer par
/// `parser::parse_payload`, le vrai moteur Rust, et rapporte le taux de
/// succes structurel ainsi que la couverture reelle du catalogue.
///
/// NE FAIT PAS : ceci n'est PAS un test de fidelite semantique (kappa).
/// Items generes par template deterministe, pas encodes par un LLM depuis
/// un texte source -- donc ca mesure "le parser accepte-t-il, a l'echelle,
/// des payloads structurellement valides", pas "un LLM choisit-il les bons
/// operateurs depuis un texte en langage naturel". Le volet kappa (juges
/// externes, conversations videes) reste l'instrument KAPPA_V2.
use cstl_parser::server::parser::parse_payload;
use cstl_parser::semantic::official_operators as official_operators_fn;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Serialize;
use std::collections::HashSet;
use std::io::Write;

fn gzip_size(data: &[u8]) -> usize {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap().len()
}

#[derive(Serialize)]
struct JsonRel { subject: String, operator: String, object: String }
#[derive(Serialize)]
struct JsonDefine { name: String, entity_type: String }
#[derive(Serialize)]
struct JsonItem {
    sender: String,
    receiver: String,
    purpose: String,
    defines: Vec<JsonDefine>,
    relations: Vec<JsonRel>,
}

const DOMAINS: &[(&str, &[&str], &[&str])] = &[
    ("legal", &["Party", "Obligation", "Document", "Clause"], &["contract_alpha", "nda_beta", "clause_gamma", "waiver_delta"]),
    ("medical", &["Patient", "Treatment", "Symptom", "Protocol"], &["recovery_plan", "dosage_schedule", "diagnosis_note", "consent_form"]),
    ("business", &["Company", "Deal", "Report", "Stakeholder"], &["quarterly_report", "merger_case", "budget_plan", "vendor_contract"]),
    ("technical", &["System", "Component", "Process", "Config"], &["filtration_stage", "deploy_pipeline", "auth_module", "cache_layer"]),
];

fn build_item(seed: usize) -> (String, Vec<String>, Vec<String>, JsonItem, String) {
    let (domain_name, entity_types, subjects) = DOMAINS[seed % DOMAINS.len()];
    let n_entities = 3 + (seed % 4);
    let n_relations = 3 + (seed % 6);

    let mut defines = Vec::new();
    let mut define_lines = String::new();
    for i in 0..n_entities {
        let ename = format!("{domain_name}_e{seed:03}_{i}");
        let etype = entity_types[(i + seed) % entity_types.len()];
        define_lines.push_str(&format!("DEFINE {ename} AS {etype} [id=e{seed:03}{i:02}]
"));
        defines.push(ename);
    }

    let mut relation_lines = String::new();
    let mut constraint_lines = String::new();
    let mut operators_used = Vec::new();
    for i in 0..n_relations {
        let op = official_operators_fn()[(seed * 7 + i * 3) % official_operators_fn().len()];
        let subj = &defines[i % defines.len()];
        let obj = &defines[(i + 1) % defines.len()];
        operators_used.push(op.to_string());
        if i % 4 == 3 {
            let modality = ["MUST", "MAY", "MUST_NOT"][i % 3];
            constraint_lines.push_str(&format!(
                "({modality}) {subj} {op} {obj} [id=c{seed:03}{i:02}]
"
            ));
        } else {
            relation_lines.push_str(&format!(
                "({subj}) {op} {obj} [id=r{seed:03}{i:02}, tau=past]
"
            ));
        }
    }

    let subject_topic = subjects[seed % subjects.len()];
    let payload = format!(
        "#!CSTL v5.0.0 MODE=A
         META [encoder=ParserCoverageN100, produced_by=ParserCoverageN100]
         INTENT_PAYLOAD [purpose=inform, sender=coverage_agent_{seed}, receiver=server, subject={subject_topic}]
         {define_lines}         RELATIONS [
{relation_lines}]
         CONSTRAINTS [
{constraint_lines}]
         ---END---
"
    );

    let json_defines: Vec<JsonDefine> = defines.iter().enumerate()
        .map(|(i, n)| JsonDefine { name: n.clone(), entity_type: entity_types[(i + seed) % entity_types.len()].to_string() })
        .collect();
    let json_relations: Vec<JsonRel> = (0..n_relations)
        .map(|i| JsonRel {
            subject: defines[i % defines.len()].clone(),
            operator: operators_used[i].clone(),
            object: defines[(i + 1) % defines.len()].clone(),
        })
        .collect();
    let json_item = JsonItem {
        sender: format!("coverage_agent_{seed}"),
        receiver: "server".to_string(),
        purpose: "inform".to_string(),
        defines: json_defines,
        relations: json_relations,
    };

    let natural_text = {
        let mut s = format!("Message de coverage_agent_{seed} a server, a titre informatif, sujet {subject_topic}. ");
        for i in 0..n_entities {
            s.push_str(&format!("{} est un(e) {}. ", defines[i], entity_types[(i + seed) % entity_types.len()]));
        }
        for i in 0..n_relations {
            s.push_str(&format!("{} {} {}. ", defines[i % defines.len()], operators_used[i], defines[(i + 1) % defines.len()]));
        }
        s
    };

    (payload, operators_used, defines, json_item, natural_text)
}

fn main() {
    const N: usize = 100;
    let mut n_parsed_ok = 0usize;
    let mut n_parse_failed = 0usize;
    let mut failures: Vec<(usize, String)> = Vec::new();
    let mut operators_seen: HashSet<String> = HashSet::new();
    let mut total_relations_extracted = 0usize;
    let mut total_defines_extracted = 0usize;

    let mut total_cstl_bytes = 0usize;
    let mut total_json_bytes = 0usize;
    let mut total_natural_bytes = 0usize;
    let mut total_cstl_gz = 0usize;
    let mut total_json_gz = 0usize;
    let mut total_natural_gz = 0usize;

    for seed in 0..N {
        let (cstl_payload, ops_used, defines, json_item, natural_text) = build_item(seed);
        for op in &ops_used {
            operators_seen.insert(op.clone());
        }

        match parse_payload(&cstl_payload) {
            Ok(parsed) => {
                n_parsed_ok += 1;
                total_defines_extracted += parsed.defines.len();
                total_relations_extracted += parsed.relations.len();
                if parsed.defines.len() != defines.len() {
                    n_parse_failed += 1;
                    n_parsed_ok -= 1;
                    failures.push((seed, format!(
                        "DEFINE perdus en silence: {} generes, {} extraits",
                        defines.len(), parsed.defines.len()
                    )));
                }
            }
            Err(e) => {
                n_parse_failed += 1;
                failures.push((seed, format!("{e:?}")));
            }
        }

        total_cstl_bytes += cstl_payload.len();
        total_cstl_gz += gzip_size(cstl_payload.as_bytes());
        let json_bytes = serde_json::to_vec(&json_item).unwrap();
        total_json_bytes += json_bytes.len();
        total_json_gz += gzip_size(&json_bytes);
        total_natural_bytes += natural_text.len();
        total_natural_gz += gzip_size(natural_text.as_bytes());
    }

    println!("=== Couverture parser reel a N={N}, vocabulaire contraint au catalogue officiel ===
");
    println!("Payloads structurellement valides (parse_payload + DEFINE count exact): {n_parsed_ok}/{N}");
    println!("Payloads en echec ou avec perte silencieuse: {n_parse_failed}/{N}");
    if !failures.is_empty() {
        println!("
Detail des echecs:");
        for (seed, reason) in &failures {
            println!("  seed={seed}: {reason}");
        }
    }
    println!("
Operateurs officiels exerces: {}/{} ({})",
        operators_seen.len(), official_operators_fn().len(),
        if operators_seen.len() == official_operators_fn().len() { "catalogue complet couvert" } else { "catalogue partiel" }
    );
    let not_covered: Vec<&str> = official_operators_fn().iter().filter(|op| !operators_seen.contains(**op)).copied().collect();
    if !not_covered.is_empty() {
        println!("Operateurs jamais exerces sur ce run: {}", not_covered.join(", "));
    }
    println!("
Total DEFINE extraits par le vrai parser: {total_defines_extracted}");
    println!("Total RELATIONS/CONSTRAINTS extraits par le vrai parser: {total_relations_extracted}");

    println!("
--- Tailles agregees sur les {N} items (CSTL vs JSON vs texte naturel, meme contenu) ---");
    println!("  CSTL          : {total_cstl_bytes:>8}o brut, {total_cstl_gz:>7}o gzip");
    println!("  JSON          : {total_json_bytes:>8}o brut, {total_json_gz:>7}o gzip");
    println!("  Texte naturel : {total_natural_bytes:>8}o brut, {total_natural_gz:>7}o gzip");
    println!("
  CSTL vs JSON (brut)   : {:+.1}%", 100.0 * (total_cstl_bytes as f64 / total_json_bytes as f64 - 1.0));
    println!("  CSTL vs JSON (gzip)   : {:+.1}%", 100.0 * (total_cstl_gz as f64 / total_json_gz as f64 - 1.0));
    println!("  CSTL vs naturel (brut): {:+.1}%", 100.0 * (total_cstl_bytes as f64 / total_natural_bytes as f64 - 1.0));

    if n_parse_failed > 0 {
        std::process::exit(1);
    }
}
