//! examples/full_compression_sizes.rs -- tailles de la chaine de
//! compression CSTL COMPLETE, sur de vrais messages (2026-11-21).
//!
//! Appele par `scripts/compression_benchmark.py`, qui lui passe un fichier
//! JSON `[{"name": ..., "wire": <message CSTL complet>}, ...]` et lit la
//! sortie JSON. Pour chaque message, mesure (en octets, roundtrip verifie) :
//!   - `blocks` : `compress_response_blocks` sur TOUS les blocs (dictionnaire
//!     texte pre-entraine + delta zigzag varint), plus 1 octet d'en-tete ;
//!   - `master` : `master_compress` (hybride : dictionnaires reseau fixes,
//!     gzip structurel, zstd + dictionnaire embarque, le plus petit des
//!     trois) sur les RELATION, plus `compress_response_blocks` sur les
//!     autres blocs, plus les prefixes de longueur et 1 octet d'en-tete ;
//!   - `wire_compressed` : ce que le serveur enverrait vraiment avec
//!     `compress_response=true` (`maybe_compress_response`, base64 compris).
//!
//! Usage : cargo run --release --example full_compression_sizes -- messages.json
use cstl_parser::compression::{
    compress_response_blocks, decompress_response_blocks, master_compress, master_decompress, ResponseBlock,
};
use cstl_parser::compression::wai_core::encode_varint;
use cstl_parser::server::parser::parse_payload;
use cstl_parser::server::response_compression::maybe_compress_response;
use std::collections::HashMap;

fn split_top_level(content: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    for ch in content.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

/// Blocs (nom, [(cle, valeur brute)]) du corps, et le texte rendu a
/// l'identique pour prouver que rien n'est perdu.
fn blocks_of(wire: &str) -> Option<Vec<ResponseBlock>> {
    let mut blocks = Vec::new();
    for line in wire.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("#!CSTL") || line == "---END---" {
            continue;
        }
        let (start, end) = (line.find('[')?, line.rfind(']')?);
        let name = line[..start].trim().to_string();
        let mut fields = Vec::new();
        for part in split_top_level(&line[start + 1..end]) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = part.split_once('=')?;
            fields.push((k.trim().to_string(), v.trim().to_string()));
        }
        blocks.push((name, fields));
    }
    Some(blocks)
}

fn render(blocks: &[ResponseBlock]) -> String {
    blocks
        .iter()
        .map(|(n, f)| format!("{n} [{}]\n", f.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ")))
        .collect()
}

fn body_of(wire: &str) -> String {
    wire.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("#!CSTL") && *l != "---END---")
        .map(|l| format!("{l}\n"))
        .collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("chemin du fichier de messages");
    let msgs: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut out = Vec::new();
    for m in msgs {
        let name = m["name"].as_str().unwrap().to_string();
        let wire = m["wire"].as_str().unwrap().to_string();
        let blocks = blocks_of(&wire).expect("forme de bloc inattendue");
        // Le rendu des blocs doit redonner le corps a l'identique (aux
        // espaces de bord de ligne pres) : sinon la mesure ne vaudrait rien.
        let lossless_text = render(&blocks) == body_of(&wire);

        let all = compress_response_blocks(&blocks);
        let blocks_ok = decompress_response_blocks(&all).map(|b| b == blocks).unwrap_or(false);

        let payload = parse_payload(&wire).expect("message CSTL valide");
        let rest: Vec<ResponseBlock> = blocks.iter().filter(|(n, _)| n != "RELATION").cloned().collect();
        let (master_size, master_ok) = if payload.relations.is_empty() {
            (all.len(), blocks_ok)
        } else {
            let none: Vec<HashMap<String, String>> = Vec::new();
            let rel = master_compress(&payload.defines, &payload.relations, &none).expect("master_compress");
            let back = master_decompress(&rel).expect("master_decompress");
            let rest_c = compress_response_blocks(&rest);
            let ok = back.1 == payload.relations && decompress_response_blocks(&rest_c).map(|b| b == rest).unwrap_or(false);
            let framing = encode_varint(rel.len() as u32).len();
            (rel.len() + rest_c.len() + framing, ok)
        };
        let wire_compressed = maybe_compress_response(&wire, true).len();
        out.push(serde_json::json!({
            "name": name, "cstl": wire.len(), "lossless_text": lossless_text,
            "blocks": all.len() + 1, "blocks_ok": blocks_ok,
            "master": master_size + 1, "master_ok": master_ok,
            "wire_compressed": wire_compressed,
        }));
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}
