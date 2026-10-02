/// examples/benchmark_zstd_lz4_gzip.rs -- repond a la question d'Olivier
/// (2026-10-02): "zstd est meilleur que gzip et lz4 le plus rapide, c'est
/// bien ca ?" Mesure directe plutot que confirmer de memoire -- gzip
/// (deflate), zstd et lz4 sur les MEMES flux structurels CSTL que
/// `benchmark_master_vs_gzip_sizes.rs`, pour que la reponse soit ancree
/// dans le contexte reel (pas un micro-benchmark generique deja fait mille
/// fois sur le web).
use cstl_parser::compression::structural::encode_structural;
use flate2::write::GzEncoder;
use flate2::Compression as GzCompression;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn build(n_relations: usize, entity_pool: usize) -> Vec<u8> {
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
    let streams = encode_structural(&defines, &relations, &[]);
    // Meme flux plat que le candidat GZIP du Master Compressor hybride --
    // comparaison directe sur le MEME contenu, pas du texte generique.
    let mut flat = Vec::new();
    flat.extend_from_slice(&streams.stable);
    flat.extend_from_slice(&streams.text);
    flat.extend_from_slice(&streams.ids);
    flat.extend_from_slice(&streams.variable);
    flat
}

fn bench(label: &str, n_relations: usize, entity_pool: usize, iters: u32) {
    let flat = build(n_relations, entity_pool);

    // gzip (deflate, niveau par defaut -- meme reglage que master.rs)
    let t0 = Instant::now();
    let mut gz_out = Vec::new();
    for _ in 0..iters {
        let mut enc = GzEncoder::new(Vec::new(), GzCompression::default());
        enc.write_all(&flat).unwrap();
        gz_out = enc.finish().unwrap();
    }
    let gz_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // zstd, niveau 3 (defaut recommande, equivalent "usage general")
    let t0 = Instant::now();
    let mut zstd_out = Vec::new();
    for _ in 0..iters {
        zstd_out = zstd::encode_all(&flat[..], 3).unwrap();
    }
    let zstd_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    // zstd, niveau 19 (compression maximale, cout CPU assume)
    let t0 = Instant::now();
    let mut zstd19_out = Vec::new();
    for _ in 0..(iters.min(200)) {
        zstd19_out = zstd::encode_all(&flat[..], 19).unwrap();
    }
    let zstd19_time = t0.elapsed().as_secs_f64() * 1000.0 / (iters.min(200)) as f64;

    // lz4 (frame format, compatible inter-implementations)
    let t0 = Instant::now();
    let mut lz4_out = Vec::new();
    for _ in 0..iters {
        lz4_out = lz4_flex::compress_prepend_size(&flat);
    }
    let lz4_time = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    println!(
        "{label}\n  flux brut: {}o\n  gzip(def)  {:>6}o {:>8.4}ms\n  zstd(3)    {:>6}o {:>8.4}ms\n  zstd(19)   {:>6}o {:>8.4}ms\n  lz4        {:>6}o {:>8.4}ms\n",
        flat.len(),
        gz_out.len(), gz_time,
        zstd_out.len(), zstd_time,
        zstd19_out.len(), zstd19_time,
        lz4_out.len(), lz4_time,
    );
}

fn main() {
    println!("=== gzip vs zstd vs lz4 sur les flux structurels CSTL (meme contenu que benchmark_master_vs_gzip_sizes.rs) ===\n");
    bench("3 relations, pool=3 (petit message typique)", 3, 3, 3000);
    bench("20 relations, pool=20 (realiste varie)", 20, 20, 1000);
    bench("50 relations, pool=5 (grand, tres repetitif)", 50, 5, 500);
    bench("200 relations, pool=200 (tres grand, varie)", 200, 200, 100);
    bench("200 relations, pool=10 (tres grand, tres repetitif)", 200, 10, 100);
}
