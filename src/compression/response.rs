//! src/compression/response.rs — compression GENERIQUE pour des listes de
//! blocs CSTL arbitraires (2026-10-01), distincte du Master Compresseur
//! (`master.rs`) qui est SPECIFIQUEMENT scope a `defines`/`relations`/
//! `uncertainty` (schema connu, champs nommes).
//!
//! Pourquoi un module separe plutot qu'une extension de `master.rs`: les
//! reponses du serveur (`server/handler.rs`) emettent des types de blocs qui
//! n'ont jamais fait partie du scope du Master Compresseur -- META,
//! INTENT_PAYLOAD, VERIFICATION, CONSISTENCY, SEMANTIC_WARNING, GOVERNANCE,
//! SIGMA_CALIBRATION, EXECUTION_TRACE, AUDIT, etc. -- avec des champs eux
//! aussi variables d'un type de bloc a l'autre. Forcer ce besoin dans le
//! schema `defines`/`relations`/`uncertainty` existant aurait ete un abus de
//! portee; ce module traite plutot un bloc comme une paire (nom, liste
//! ordonnee de (cle, valeur)) totalement generique -- moins specialise que
//! `structural.rs` (pas de fast-path ID, pas de flux `ids` dedie), mais
//! applicable a N'IMPORTE QUEL type de bloc CSTL, present ou futur.
//!
//! Reutilise les DEUX memes briques deja eprouvees que `master.rs`:
//! `text_dictionary` (dictionnaire pre-entraine, vocabulaire ouvert, avec
//! repli automatique en mode brut) pour toutes les chaines (noms de bloc,
//! cles, valeurs -- un seul corpus, contrairement a master.rs qui separe
//! `stable`/`text`: ici il n'y a pas de vocabulaire d'opcodes FERME a part
//! du vocabulaire de valeurs, donc un seul flux texte suffit) et
//! `variable_delta` (delta zigzag + varint, sans table) pour la structure
//! (comptes, index). Zero nouvelle primitive de compression inventee ici.
//!
//! Format de sortie: `[varint len(text_encoded)][text_encoded]
//! [structure_compressed (delta zigzag varint) jusqu'a la fin]`.

use super::text_dictionary::{decode_text, encode_text, TextDictError};
use super::variable_delta::{decode_variable, encode_variable, VariableDeltaError};
use super::wai_core::{decode_varint, encode_varint};

pub type ResponseBlock = (String, Vec<(String, String)>);

#[derive(Debug)]
pub enum ResponseCompressionError {
    VariableDelta(VariableDeltaError),
    TextDict(TextDictError),
    Truncated(&'static str),
    BadStringIndex(u32),
}

impl std::fmt::Display for ResponseCompressionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseCompressionError::VariableDelta(e) => write!(f, "flux structure (delta): {e}"),
            ResponseCompressionError::TextDict(e) => write!(f, "flux texte: {e}"),
            ResponseCompressionError::Truncated(ctx) => write!(f, "response: flux tronque ({ctx})"),
            ResponseCompressionError::BadStringIndex(idx) => write!(f, "response: index de chaine invalide ({idx})"),
        }
    }
}
impl std::error::Error for ResponseCompressionError {}

/// Table de chaines locale a CE message -- meme patron que
/// `structural::StringTable`, duplique intentionnellement ici plutot que
/// de rendre l'original `pub(crate)`: c'est une primitive triviale
/// (intern/serialize/deserialize, ~20 lignes) et ce module a un scope
/// different (blocs generiques, pas defines/relations/uncertainty) --
/// partager le type ferait une dependance artificielle entre deux schemas
/// qui n'ont rien en commun au-dela de "c'est une table de chaines".
struct StringTable {
    strings: Vec<String>,
    index_of: std::collections::HashMap<String, u32>,
}

impl StringTable {
    fn new() -> Self {
        StringTable { strings: Vec::new(), index_of: std::collections::HashMap::new() }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.index_of.get(s) {
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index_of.insert(s.to_string(), idx);
        idx
    }

    /// (lengths, text) -- lengths = compte + longueur de chaque chaine
    /// (varints bruts), text = octets UTF-8 concatenes sans separateur.
    fn serialize(&self) -> (Vec<u8>, Vec<u8>) {
        let mut lengths = Vec::new();
        let mut text = Vec::new();
        lengths.extend_from_slice(&encode_varint(self.strings.len() as u32));
        for s in &self.strings {
            let bytes = s.as_bytes();
            lengths.extend_from_slice(&encode_varint(bytes.len() as u32));
            text.extend_from_slice(bytes);
        }
        (lengths, text)
    }
}

fn read_varint_at(buf: &[u8], pos: &mut usize, ctx: &'static str) -> Result<u32, ResponseCompressionError> {
    let (v, used) = decode_varint(&buf[*pos..]).map_err(|_| ResponseCompressionError::Truncated(ctx))?;
    *pos += used;
    Ok(v)
}

/// Compresse une liste ordonnee de blocs (nom, [(cle, valeur), ...]).
/// Jamais d'echec: contrairement a `master_compress` (qui peut echouer sur
/// le flux `variable` dans des cas extremes, voir `variable_delta.rs`),
/// ce module n'a pas cette limite -- `encode_variable` ne manipule que des
/// varints d'indices locaux, toujours petits. Retourne `Vec::new()` pour
/// une liste vide.
pub fn compress_response_blocks(blocks: &[ResponseBlock]) -> Vec<u8> {
    if blocks.is_empty() {
        return Vec::new();
    }

    let mut table = StringTable::new();
    let mut structure_raw = Vec::new();
    structure_raw.extend_from_slice(&encode_varint(blocks.len() as u32));
    for (name, fields) in blocks {
        let name_idx = table.intern(name);
        structure_raw.extend_from_slice(&encode_varint(name_idx));
        structure_raw.extend_from_slice(&encode_varint(fields.len() as u32));
        for (k, v) in fields {
            let k_idx = table.intern(k);
            let v_idx = table.intern(v);
            structure_raw.extend_from_slice(&encode_varint(k_idx));
            structure_raw.extend_from_slice(&encode_varint(v_idx));
        }
    }

    let (lengths_raw, text_raw) = table.serialize();
    // Concatenation de deux sequences varint deja valides = toujours une
    // sequence varint valide (l'encodage varint est auto-delimitant) --
    // meme technique que `structural.rs` pour composer son flux `variable`.
    let mut combined_raw = lengths_raw;
    combined_raw.extend_from_slice(&structure_raw);

    let structure_compressed = encode_variable(&combined_raw)
        .expect("encode_variable ne peut echouer que sur un flux deja tronque, jamais ici");
    let text_encoded = encode_text(&text_raw);

    let mut out = Vec::new();
    out.extend_from_slice(&encode_varint(text_encoded.len() as u32));
    out.extend_from_slice(&text_encoded);
    out.extend_from_slice(&structure_compressed);
    out
}

/// Inverse de `compress_response_blocks`. `Ok(Vec::new())` pour une entree
/// vide (symetrique avec le cas `blocks.is_empty()` de la compression).
pub fn decompress_response_blocks(bytes: &[u8]) -> Result<Vec<ResponseBlock>, ResponseCompressionError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut pos = 0usize;
    let text_len = read_varint_at(bytes, &mut pos, "longueur flux texte")? as usize;
    if pos + text_len > bytes.len() {
        return Err(ResponseCompressionError::Truncated("flux texte"));
    }
    let text_encoded = &bytes[pos..pos + text_len];
    pos += text_len;
    let text_raw = decode_text(text_encoded).map_err(ResponseCompressionError::TextDict)?;

    let structure_compressed = &bytes[pos..];
    let combined_raw = decode_variable(structure_compressed).map_err(ResponseCompressionError::VariableDelta)?;

    let mut cpos = 0usize;
    let string_count = read_varint_at(&combined_raw, &mut cpos, "table de chaines: compte")? as usize;
    let mut strings: Vec<String> = Vec::with_capacity(string_count);
    let mut tpos = 0usize;
    for _ in 0..string_count {
        let len = read_varint_at(&combined_raw, &mut cpos, "table de chaines: longueur")? as usize;
        if tpos + len > text_raw.len() {
            return Err(ResponseCompressionError::Truncated("table de chaines: octets texte"));
        }
        strings.push(String::from_utf8_lossy(&text_raw[tpos..tpos + len]).into_owned());
        tpos += len;
    }

    let lookup = |idx: u32| -> Result<String, ResponseCompressionError> {
        strings.get(idx as usize).cloned().ok_or(ResponseCompressionError::BadStringIndex(idx))
    };

    let num_blocks = read_varint_at(&combined_raw, &mut cpos, "compte de blocs")? as usize;
    let mut blocks = Vec::with_capacity(num_blocks);
    for _ in 0..num_blocks {
        let name_idx = read_varint_at(&combined_raw, &mut cpos, "index nom de bloc")?;
        let name = lookup(name_idx)?;
        let num_fields = read_varint_at(&combined_raw, &mut cpos, "compte de champs")? as usize;
        let mut fields = Vec::with_capacity(num_fields);
        for _ in 0..num_fields {
            let k_idx = read_varint_at(&combined_raw, &mut cpos, "index cle")?;
            let v_idx = read_varint_at(&combined_raw, &mut cpos, "index valeur")?;
            fields.push((lookup(k_idx)?, lookup(v_idx)?));
        }
        blocks.push((name, fields));
    }

    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, fields: &[(&str, &str)]) -> ResponseBlock {
        (name.to_string(), fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn test_empty_blocks_roundtrip() {
        let compressed = compress_response_blocks(&[]);
        assert!(compressed.is_empty());
        let decompressed = decompress_response_blocks(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_single_block_roundtrip() {
        let blocks = vec![b("AUDIT", &[("hash", "sha256:abc"), ("parent_hash", "root"), ("seq", "0")])];
        let compressed = compress_response_blocks(&blocks);
        let decompressed = decompress_response_blocks(&compressed).unwrap();
        assert_eq!(decompressed, blocks);
    }

    #[test]
    fn test_multiple_blocks_shared_vocabulary_roundtrip() {
        // Reponse realiste: plusieurs blocs, vocabulaire de cles partage
        // (status apparait 2x, devrait n'etre interne qu'une fois).
        let blocks = vec![
            b("META", &[("encoder", "CstlNativeServer"), ("produced_by", "Server"), ("status", "processed")]),
            b("INTENT_PAYLOAD", &[("purpose", "acknowledgement"), ("sender", "server"), ("receiver", "alice")]),
            b("RELATION", &[("type", "received"), ("subject", "test"), ("status", "valid")]),
            b("GOVERNANCE", &[("sender", "alice"), ("circuit", "closed"), ("breaker_trips", "0")]),
            b("AUDIT", &[("hash", "sha256:03d86bf1"), ("parent_hash", "root"), ("seq", "0")]),
        ];
        let compressed = compress_response_blocks(&blocks);
        let decompressed = decompress_response_blocks(&compressed).unwrap();
        assert_eq!(decompressed, blocks);
    }

    #[test]
    fn test_block_with_no_fields_roundtrips() {
        let blocks = vec![b("EMPTY_BLOCK", &[])];
        let compressed = compress_response_blocks(&blocks);
        let decompressed = decompress_response_blocks(&compressed).unwrap();
        assert_eq!(decompressed, blocks);
    }

    #[test]
    fn test_decompress_truncated_input_errors_not_panics() {
        let blocks = vec![b("AUDIT", &[("hash", "sha256:abc")])];
        let compressed = compress_response_blocks(&blocks);
        for cut in 1..compressed.len() {
            // Ne doit jamais paniquer, seulement retourner Err ou (rarement,
            // sur une coupe qui tombe pile sur une frontiere) un resultat
            // incomplet mais valide structurellement -- la propriete testee
            // ici est l'absence de panique, pas un Err garanti a CHAQUE coupe.
            let _ = decompress_response_blocks(&compressed[..cut]);
        }
    }

    #[test]
    fn test_decompress_empty_bytes_is_empty_blocks() {
        assert_eq!(decompress_response_blocks(&[]).unwrap(), Vec::<ResponseBlock>::new());
    }
}
