/// examples/benchmark_master_vs_gzip_sizes.rs -- repond a la question
/// d'Olivier (2026-10-02): "est-ce que CSTL est parfait comme ca, ou le
/// gain d'une fenetre glissante est meilleur que la perte de vitesse ?"
///
/// Au lieu de deviner, on mesure le Master Compressor REEL (celui du
/// depot, pas une simulation) contre gzip (deflate+LZ77, flate2, deja
/// une dependance du projet) sur PLUSIEURS tailles de payload CSTL
/// realistes -- pas seulement le cas 20x20 artificiellement repetitif
/// deja publie dans le README (benchmark_wire_formats.rs), qui surestime
/// la redondance d'un message CSTL typique en production.
///
/// gzip = deflate = LZ77 + Huffman. Un vrai LZ77 integre au Master
/// Compressor n'existe pas encore dans ce depot -- gzip sert donc de
/// PROXY MESURABLE pour "qu'est-ce qu'un etage de match-finding
/// apporterait", pas une mesure directe du code qui n'existe pas. Le
/// temps de gzip N'EST PAS le temps qu'aurait un LZ77 integre
/// directement dans Rust sans passer par flate2 -- lecture a faire avec
/// cette limite en tete, signalee ici plutot que cachee.
use cstl_parser::compression::master::master_compress;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

/// Construit un payload CSTL de N relations, avec un taux de reutilisation
/// d'entites CONTROLE par `entity_pool` -- plus le pool est petit relatif a
/// N, plus il y a de redondance (cas favorable a LZ77/gzip) ; plus il est
/// grand, moins il y a de redondance (cas defavorable, plus proche d'un
/// message CSTL reel avec des entites variees, peu repetees).
fn build_payload(n_relations: usize, entity_pool: usize) -> (Vec<HashMap<String, String>>, Vec<HashMap<String, String>>, String) {
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

fn bench_one(label: &str, n_relations: usize, entity_pool: usize, iters: u32) {
    let (defines, relations, text) = build_payload(n_relations, entity_pool);
    let uncertainty: Vec<HashMap<String, String>> = vec![];

    // Master Compressor -- moyenne sur N iterations pour un temps stable
    let mut master_out = Vec::new();
    let t0 = Instant::now();
    for _ in 0..iters {
        master_out = master_compress(&defines, &relations, &uncertainty).expect("master_compress");
    }
    let master_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // gzip sur le texte CSTL equivalent (proxy LZ77+entropy mesurable)
    let mut gz_out = Vec::new();
    let t0 = Instant::now();
    for _ in 0..iters {
        gz_out = gzip_bytes(text.as_bytes());
    }
    let gzip_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    let redundancy_ratio = n_relations as f64 / entity_pool as f64;
    println!(
        "{label:36} texte={:>6}o  master={:>5}o ({:>5.1}%)  gzip={:>5}o ({:>5.1}%)  | temps master={:.4}ms gzip={:.4}ms (gzip/master={:.1}x)  | repetition(n_rel/pool)={:.1}",
        text.len(),
        master_out.len(), 100.0 * master_out.len() as f64 / text.len() as f64,
        gz_out.len(), 100.0 * gz_out.len() as f64 / text.len() as f64,
        master_time, gzip_time, gzip_time / master_time,
        redundancy_ratio,
    );
}

fn main() {
    println!("=== Master Compressor (reel) vs gzip (proxy LZ77) -- plusieurs tailles et niveaux de redondance ===");
    println!("    gzip/master <1.0x = gzip plus RAPIDE malgre le match-finding (CSTL moins cher que prevu) ; >1.0x = gzip plus LENT (cout du match-finding visible)\n");

    // Message CSTL UNIQUE, realiste (1 relation, pas de redondance intra-message -- le cas le plus courant en production, un message = un echange)
    bench_one("1 relation, pool=1 (message unique)", 1, 1, 2000);
    bench_one("3 relations, pool=3 (petit message typique)", 3, 3, 2000);
    bench_one("10 relations, pool=10 (aucune repetition)", 10, 10, 1000);
    bench_one("10 relations, pool=3 (repetition moderee)", 10, 3, 1000);
    bench_one("20 relations, pool=20 (aucune repetition, realiste varie)", 20, 20, 500);
    bench_one("20 relations, pool=4 (forte repetition, cas du README)", 20, 4, 500);
    bench_one("50 relations, pool=50 (grand message, varie)", 50, 50, 200);
    bench_one("50 relations, pool=5 (grand message, tres repetitif)", 50, 5, 200);
    bench_one("200 relations, pool=200 (tres grand, varie)", 200, 200, 50);
    bench_one("200 relations, pool=10 (tres grand, tres repetitif)", 200, 10, 50);
}
