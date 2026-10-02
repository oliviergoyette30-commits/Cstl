/// examples/train_zstd_dictionary.rs -- genere le dictionnaire zstd EMBARQUE
/// (src/compression/zstd_cstl_dict.bin) a partir d'un corpus synthetique
/// large et varie de flux structurels CSTL. Ce fichier binaire est ensuite
/// embarque via `include_bytes!` dans master.rs -- memes octets des deux
/// cotes (compresseur ET decompresseur), jamais transmis sur le fil
/// (reponse a la question d'Olivier : "peut-on pre-entrainer le
/// dictionnaire et le mettre dans le reseau" -- oui, exactement le meme
/// principe que `stable_dictionary.rs`/`text_dictionary.rs`, mais pour le
/// mecanisme de dictionnaire NATIF de zstd plutot qu'un dictionnaire
/// semantique maison).
///
/// A re-executer seulement si on veut RE-entrainer le dictionnaire sur un
/// corpus different (plus representatif du vrai trafic CSTL, par
/// exemple). Le binaire genere est commis au depot, pas regenere a chaque
/// build.
use cstl_parser::compression::structural::encode_structural;
use cstl_parser::compression::wai_core::encode_varint;
use std::collections::HashMap;
use std::fs;

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn flat_streams(defines: &[HashMap<String, String>], relations: &[HashMap<String, String>]) -> Vec<u8> {
    let streams = encode_structural(defines, relations, &[]);
    let mut flat = Vec::new();
    flat.extend_from_slice(&encode_varint(streams.stable.len() as u32));
    flat.extend_from_slice(&streams.stable);
    flat.extend_from_slice(&encode_varint(streams.text.len() as u32));
    flat.extend_from_slice(&streams.text);
    flat.extend_from_slice(&encode_varint(streams.ids.len() as u32));
    flat.extend_from_slice(&streams.ids);
    flat.extend_from_slice(&encode_varint(streams.variable.len() as u32));
    flat.extend_from_slice(&streams.variable);
    flat
}

fn build(n_relations: usize, entity_pool: usize, seed: usize) -> (Vec<HashMap<String, String>>, Vec<HashMap<String, String>>) {
    let ops = ["PERFORM", "ARR.ACCESS", "MAINTAIN", "TRANSMIT_FAITHFUL", "COMMAND", "ENTAILS", "BEFORE", "OPPOSES"];
    let modalities = ["MUST", "MUST_NOT", "MAY", "SHOULD"];
    let types = ["Party", "Obligation", "Asset", "Document", "Event"];
    let names = ["Tenant", "Landlord", "Rent", "SecurityDeposit", "LeaseAgreement", "Property", "PaymentSchedule", "Inspector", "Buyer", "Seller", "Contract", "Invoice", "Vendor", "Client", "Auditor", "Regulator"];
    let mut defines = Vec::new();
    for i in 0..entity_pool {
        let base_name = names[(i + seed) % names.len()];
        defines.push(m(&[("name", &format!("{base_name}_{seed}_{i}")), ("entity_type", types[(i + seed) % types.len()]), ("id", &format!("e{seed:03}{i:03}"))]));
    }
    let mut relations = Vec::new();
    for i in 0..n_relations {
        relations.push(m(&[
            ("type", ops[(i + seed) % ops.len()]),
            ("subject", &format!("{}_{seed}_{}", names[(i + seed) % names.len()], i % entity_pool.max(1))),
            ("object", &format!("{}_{seed}_{}", names[(i + seed + 3) % names.len()], (i + 3) % entity_pool.max(1))),
            ("modality", modalities[(i + seed) % modalities.len()]),
            ("id", &format!("r{seed:03}{i:03}")),
        ]));
    }
    (defines, relations)
}

fn main() {
    // Distribution large : petits messages (ou le dictionnaire aide le
    // plus, voir benchmark_zstd_dictionary.rs) SURREPRESENTES, mais toutes
    // les tailles couvertes jusqu'a "grand message" pour que le
    // dictionnaire reste utile meme la (il est combine au niveau flux,
    // pas juste prefixe).
    let mut samples: Vec<Vec<u8>> = Vec::new();
    let mut seed = 0usize;
    for n_rel in [1, 2, 3, 4, 5, 6, 8, 10, 15, 20, 30, 50] {
        for pool in [1, 2, 3, 5, 8, 12, 20] {
            if pool > n_rel + 5 { continue; } // eviter des pools absurdement plus grands que le besoin reel
            for _rep in 0..3 {
                let (defines, relations) = build(n_rel, pool, seed);
                samples.push(flat_streams(&defines, &relations));
                seed += 1;
            }
        }
    }

    println!("Corpus d'entrainement: {} echantillons (seeds 0..{seed})", samples.len());

    let dict_bytes = zstd::dict::from_samples(&samples, 8192).expect("entrainement dictionnaire zstd");
    println!("Dictionnaire genere: {} octets", dict_bytes.len());

    let out_path = "src/compression/zstd_cstl_dict.bin";
    fs::write(out_path, &dict_bytes).expect("ecriture dictionnaire");
    println!("Ecrit dans {out_path}");
}
