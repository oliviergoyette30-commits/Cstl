/// examples/benchmark_zstd_dictionary.rs -- repond a la question d'Olivier
/// (2026-10-02): "si j'utilise zstd ca serait mieux, et il y a deja un
/// dictionnaire je crois". Deux dictionnaires DIFFERENTS sont en jeu ici,
/// et il faut les mesurer separement plutot que les confondre :
///
/// 1. Le dictionnaire CSTL fait main (`stable_dictionary.rs` /
///    `text_dictionary.rs`) -- entraine sur le VOCABULAIRE SEMANTIQUE CSTL
///    (opcodes, mots courants des DEFINE/CONSTRAINTS). C'est le candidat
///    MODE_DICTIONARIES deja dans master.rs.
/// 2. Le mecanisme de dictionnaire NATIF de zstd (`zstd::dict::from_samples`,
///    algo COVER/zdict de la lib C reference) -- entraine sur des OCTETS
///    BRUTS (peu importe leur sens), conçu precisement pour le probleme
///    qu'on a deja identifie : les petits messages n'ont pas assez de
///    redondance INTERNE pour que LZ77 trouve des correspondances.
///
/// Est-ce que (2) ajoute quelque chose au-dessus de (1) + gzip/zstd sans
/// dictionnaire ? Mesure directe, pas une supposition.
use cstl_parser::compression::master::master_compress;
use cstl_parser::compression::structural::encode_structural;
use cstl_parser::compression::wai_core::encode_varint;
use std::collections::HashMap;
use zstd::dict::{DecoderDictionary, EncoderDictionary};
use zstd::bulk::{Compressor, Decompressor};

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Meme flux plat que le candidat GZIP/zstd de master.rs (4 flux
/// structurels avec prefixes de longueur, sans dictionnaire applique).
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
    let mut defines = Vec::new();
    for i in 0..entity_pool {
        defines.push(m(&[("name", &format!("Entity_{}_{}", seed, i)), ("entity_type", types[(i + seed) % types.len()]), ("id", &format!("e{seed:02}{i:03}"))]));
    }
    let mut relations = Vec::new();
    for i in 0..n_relations {
        relations.push(m(&[
            ("type", ops[(i + seed) % ops.len()]),
            ("subject", &format!("Entity_{}_{}", seed, i % entity_pool)),
            ("object", &format!("Entity_{}_{}", seed, (i + 3) % entity_pool)),
            ("modality", modalities[(i + seed) % modalities.len()]),
            ("id", &format!("r{seed:02}{i:03}")),
        ]));
    }
    (defines, relations)
}

fn bench(label: &str, n_relations: usize, entity_pool: usize, enc_dict: &EncoderDictionary<'static>, dec_dict: &DecoderDictionary<'static>) {
    // Message de TEST -- seed=999, jamais vu dans le corpus d'entrainement
    // (seeds 0..40 ci-dessous), pour ne pas tricher avec un dictionnaire
    // qui "connait" deja la reponse.
    let (defines, relations) = build(n_relations, entity_pool, 999);
    let flat = flat_streams(&defines, &relations);

    let master = master_compress(&defines, &relations, &[]).unwrap();

    let zstd_plain = zstd::encode_all(&flat[..], 3).unwrap();

    let mut compressor = Compressor::with_prepared_dictionary(enc_dict).unwrap();
    let zstd_dict = compressor.compress(&flat).unwrap();

    // Roundtrip pour prouver que le dictionnaire n'est pas juste plus
    // petit mais produit aussi un decompresseur fonctionnel.
    let mut decompressor = Decompressor::with_prepared_dictionary(dec_dict).unwrap();
    let roundtrip = decompressor.decompress(&zstd_dict, flat.len() + 64).unwrap();
    assert_eq!(roundtrip, flat, "roundtrip zstd+dictionnaire casse pour {label}");

    println!(
        "{label}\n  flux brut: {}o\n  master_compress (dict CSTL maison vs gzip, meilleur des 2): {}o\n  zstd(3) sans dictionnaire:                       {}o\n  zstd(3) AVEC dictionnaire entraine (zdict):      {}o\n",
        flat.len(), master.len(), zstd_plain.len(), zstd_dict.len(),
    );
}

fn main() {
    println!("=== zstd AVEC dictionnaire entraine vs master_compress (dict CSTL maison) vs zstd sans dictionnaire ===");
    println!("    Dictionnaire zstd entraine sur 40 messages CSTL varies (seeds 0..40), teste sur un message JAMAIS vu (seed=999)\n");

    // Corpus d'entrainement : messages varies en taille/pool, DIFFERENTS
    // du message de test (seed 999) -- sinon le dictionnaire "connaitrait"
    // la reponse et on mesurerait du sur-apprentissage, pas une capacite
    // reelle.
    let mut samples: Vec<Vec<u8>> = Vec::new();
    for seed in 0..40usize {
        let n_rel = 3 + (seed % 8);
        let pool = 3 + (seed % 5);
        let (defines, relations) = build(n_rel, pool, seed);
        samples.push(flat_streams(&defines, &relations));
    }

    let dict_bytes = zstd::dict::from_samples(&samples, 4096).expect("entrainement dictionnaire zstd");
    println!("Dictionnaire zstd entraine: {} octets (sur {} echantillons, max demande 4096o)\n", dict_bytes.len(), samples.len());

    let enc_dict = EncoderDictionary::copy(&dict_bytes, 3);
    let dec_dict = DecoderDictionary::copy(&dict_bytes);

    bench("3 relations, pool=3 (petit message typique)", 3, 3, &enc_dict, &dec_dict);
    bench("20 relations, pool=20 (realiste varie)", 20, 20, &enc_dict, &dec_dict);
    bench("50 relations, pool=5 (grand, tres repetitif)", 50, 5, &enc_dict, &dec_dict);
    bench("5 relations, pool=5 (tres petit, hors distribution d'entrainement)", 5, 5, &enc_dict, &dec_dict);
}
