/// examples/benchmark_unified_size_speed.rs -- repond a la question
/// d'Olivier (2026-10-02): "pour le rendement vitesse ET poids, quelle est
/// la meilleure solution ?"
///
/// Les deux benchmarks precedents ne sont PAS comparables entre eux:
/// benchmark_wire_formats.rs mesure Protobuf a UNE seule taille (20
/// concepts) ; benchmark_master_vs_gzip_sizes.rs mesure le Master
/// Compressor et gzip sur 10 tailles mais SANS Protobuf. Repondre "quelle
/// est la meilleure solution" en combinant les deux serait extrapoler entre
/// des mesures non comparables -- ce script mesure donc les TROIS
/// (Master Compressor, gzip-sur-texte-CSTL, Protobuf brut+gzip) sur LA
/// MEME grille de tailles/redondance, en une seule passe.
use cstl_parser::compression::master::master_compress;
use flate2::write::GzEncoder;
use flate2::Compression;
use prost::Message;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

#[derive(Clone, PartialEq, prost::Message)]
struct ProtoDefine {
    #[prost(string, tag = "1")] name: String,
    #[prost(string, tag = "2")] entity_type: String,
    #[prost(string, tag = "3")] id: String,
}
#[derive(Clone, PartialEq, prost::Message)]
struct ProtoRelation {
    #[prost(string, tag = "1")] r#type: String,
    #[prost(string, tag = "2")] subject: String,
    #[prost(string, tag = "3")] object: String,
    #[prost(string, tag = "4")] modality: String,
    #[prost(string, tag = "5")] id: String,
}
#[derive(Clone, PartialEq, prost::Message)]
struct ProtoBundle {
    #[prost(message, repeated, tag = "1")] defines: Vec<ProtoDefine>,
    #[prost(message, repeated, tag = "2")] relations: Vec<ProtoRelation>,
}

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

fn build(n_relations: usize, entity_pool: usize) -> (Vec<HashMap<String, String>>, Vec<HashMap<String, String>>, String) {
    let ops = ["PERFORM", "ARR.ACCESS", "MAINTAIN", "TRANSMIT_FAITHFUL", "COMMAND", "ENTAILS", "BEFORE", "OPPOSES"];
    let modalities = ["MUST", "MUST_NOT", "MAY", "SHOULD"];
    let types = ["Party", "Obligation", "Asset", "Document", "Event"];
    let mut defines = Vec::new();
    for i in 0..entity_pool {
        defines.push(m(&[("name", &format!("Entity_{i}")), ("entity_type", types[i % types.len()]), ("id", &format!("e{i:04}"))]));
    }
    let mut relations = Vec::new();
    for i in 0..n_relations {
        relations.push(m(&[
            ("type", ops[i % ops.len()]),
            ("subject", &format!("Entity_{}", i % entity_pool)),
            ("object", &format!("Entity_{}", (i + 3) % entity_pool)),
            ("modality", modalities[i % modalities.len()]),
            ("id", &format!("r{i:04}")),
        ]));
    }
    let mut text = String::new();
    for def in &defines {
        text.push_str(&format!("DEFINE {} AS {} [id={}]\n", def["name"], def["entity_type"], def["id"]));
    }
    text.push_str("CONSTRAINTS [\n");
    for rel in &relations {
        text.push_str(&format!("  ({}) {} {} {} [id={}]\n", rel["modality"], rel["subject"], rel["type"], rel["object"], rel["id"]));
    }
    text.push_str("]\n---END---\n");
    (defines, relations, text)
}

fn bench(label: &str, n_relations: usize, entity_pool: usize, iters: u32) {
    let (defines, relations, text) = build(n_relations, entity_pool);
    let uncertainty: Vec<HashMap<String, String>> = vec![];

    // Master Compressor
    let t0 = Instant::now();
    let mut master_out = Vec::new();
    for _ in 0..iters { master_out = master_compress(&defines, &relations, &uncertainty).unwrap(); }
    let master_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // gzip sur le texte CSTL
    let t0 = Instant::now();
    let mut gz_out = Vec::new();
    for _ in 0..iters { gz_out = gzip_bytes(text.as_bytes()); }
    let gzip_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // Protobuf brut
    let bundle = ProtoBundle {
        defines: defines.iter().map(|d| ProtoDefine { name: d["name"].clone(), entity_type: d["entity_type"].clone(), id: d["id"].clone() }).collect(),
        relations: relations.iter().map(|r| ProtoRelation { r#type: r["type"].clone(), subject: r["subject"].clone(), object: r["object"].clone(), modality: r["modality"].clone(), id: r["id"].clone() }).collect(),
    };
    let t0 = Instant::now();
    let mut proto_out = Vec::new();
    for _ in 0..iters { proto_out = bundle.encode_to_vec(); }
    let proto_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // Protobuf + gzip
    let t0 = Instant::now();
    let mut proto_gz_out = Vec::new();
    for _ in 0..iters { proto_gz_out = gzip_bytes(&proto_out); }
    let proto_gzip_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // Score combine simple: taille(o) * temps(ms) -- plus bas = meilleur compromis conjoint
    let score = |size: usize, time: f64| size as f64 * time.max(0.0001);
    let candidates = [
        ("Master Compressor", master_out.len(), master_time),
        ("gzip (texte CSTL)", gz_out.len(), gzip_time),
        ("Protobuf brut", proto_out.len(), proto_time),
        ("Protobuf + gzip", proto_gz_out.len(), proto_time + proto_gzip_time),
    ];
    let best = candidates.iter().min_by(|a, b| score(a.1, a.2).partial_cmp(&score(b.1, b.2)).unwrap()).unwrap();

    println!("{label}");
    println!("  texte CSTL brut: {}o", text.len());
    for (name, size, time) in &candidates {
        let mark = if *name == best.0 { " <= meilleur score taille*temps" } else { "" };
        println!("  {name:22} {size:>6}o  {time:>8.4}ms  score={:.2}{mark}", score(*size, *time));
    }
    println!();
}

fn main() {
    println!("=== Comparaison unifiee: Master Compressor vs gzip vs Protobuf (brut et +gzip) -- MEME grille de tailles ===");
    println!("    score = taille(octets) x temps(ms) -- plus bas = meilleur compromis conjoint taille/vitesse, pas un classement absolu sur un seul axe\n");

    bench("1 relation, pool=1 (message unique)", 1, 1, 2000);
    bench("3 relations, pool=3 (petit message typique)", 3, 3, 2000);
    bench("10 relations, pool=10 (aucune repetition)", 10, 10, 1000);
    bench("20 relations, pool=20 (realiste varie)", 20, 20, 500);
    bench("20 relations, pool=4 (forte repetition, cas du README)", 20, 4, 500);
    bench("50 relations, pool=50 (grand message)", 50, 50, 200);
    bench("200 relations, pool=200 (tres grand, varie)", 200, 200, 50);
    bench("200 relations, pool=10 (tres grand, tres repetitif)", 200, 10, 50);
}
