//! src/compression/master.rs — Master Compresseur CSTL : compose
//! `structural` (Couche 1, 3 flux) avec DEUX dictionnaires reseau stable
//! pre-remplis + un troisieme mecanisme sans table (2026-09-29, revise le
//! meme jour -- reponse a "il faut qu'il y ait DEUX dictionnaires reseau
//! stable pre-remplis pour qu'il n'y ait plus d'overhead... juste des
//! delta et des index") :
//! - `stable` (opcodes, vocabulaire FERME de 33+10+4 symboles) ->
//!   `stable_dictionary` : pre-rempli, versionne, zero octet de table.
//! - `text` (octets UTF-8 des noms/valeurs litterales, statistiques de
//!   lettres partageables mais vocabulaire OUVERT) -> `text_dictionary` :
//!   meme mecanisme, corpus d'entrainement different, avec repli
//!   automatique en mode brut si un contenu hors-norme n'est pas couvert.
//! - `variable` (longueurs/compteurs/index, PROUVABLEMENT propres a CE
//!   message -- aucun dictionnaire pre-entraine n'est possible ici, voir
//!   `variable_delta.rs`) -> delta zigzag + varint, SANS TABLE. Remplace
//!   l'ancien `order1_rans::encode` adaptatif, qui serialisait sa propre
//!   table de frequences par message -- de l'overhead pur sur un message
//!   court, le cas CSTL typique, pour un flux qui ne peut de toute facon
//!   pas etre partage entre messages.
//! - `ids` (identifiants "prefixe+chiffres" de `relations.id`/
//!   `uncertainty.identifier`, voir `structural::try_parse_id`) -> DEJA
//!   delta-encode PAR PREFIXE dans `structural.rs` lui-meme -- transmis
//!   tel quel ici, aucune couche d'entropy coding supplementaire (meme
//!   raison que `variable` : reessayer rANS dessus reintroduirait le
//!   plancher fixe d'etat qu'on vient d'eliminer, pour un flux deja petit
//!   par construction).
//!
//! Portee assumee, a repeter ici (voir `structural.rs` pour le detail) :
//! couvre `defines`/`relations`/`uncertainty` d'un `CstlPayload`, pas le
//! payload complet. PAS branche dans `server/handler.rs`.
//!
//! Format de sortie (mode DICTIONARIES, 0x00) : `[varint len(stable)]
//! [stable][varint len(text)][text][varint len(ids)][ids][variable (delta
//! zigzag) jusqu'a la fin]`.
//!
//! ## Mode hybride (2026-10-02)
//!
//! Reponse directe a la question d'Olivier -- "pour le rendement
//! vitesse/poids, quelle est la meilleure solution" a ete mesuree
//! (`examples/benchmark_master_vs_gzip_sizes.rs`) : les dictionnaires
//! ci-dessus gagnent sous ~15-20 relations (message CSTL typique), gzip
//! gagne au-dessus, et le seuil reel depend de la taille ET du taux de
//! repetition ensemble, pas de la taille seule -- un seuil fixe se
//! tromperait dans certains cas deja mesures (voir benchmark). Plutot que
//! deviner un seuil, `master_compress` essaie maintenant LES DEUX
//! candidats reels et garde le plus petit, avec un octet de mode en tete
//! pour que `master_decompress` sache lequel inverser -- meme patron que
//! `MODE_PRETRAINED`/`MODE_RAW_FALLBACK` deja utilise dans
//! `text_dictionary.rs`/`stable_dictionary.rs`, applique ici au niveau du
//! Master Compresseur au complet plutot qu'a un seul sous-flux.
//!
//! Candidat GZIP (mode 0x01) : les 4 flux structurels BRUTS (stable/text/
//! ids/variable, AUCUN dictionnaire applique) concatenes avec prefixes de
//! longueur, puis gzippes en un seul bloc (`flate2`, niveau par defaut).
//! Choisi plutot qu'un LZ77 maison: `flate2` est deja une dependance de ce
//! depot (declaree pour cet usage precis, voir Cargo.toml), deterministe,
//! et mesure -- pas besoin de reimplementer un match-finder pour verifier
//! l'idee.
//!
//! Cout assume : `master_compress` fait maintenant le travail des DEUX
//! candidats a chaque appel (plus lent qu'avant dans le cas ou le mode
//! dictionnaires gagnait deja), en echange d'un resultat toujours au moins
//! aussi bon que le meilleur des deux, mesure plutot que suppose -- choix
//! deliberement du cote "toujours optimal" plutot que "toujours rapide"
//! (voir discussion : ce compresseur sert la couche stockage/audit,
//! non temps-reel, PAS branche dans `server/handler.rs` -- le cout CPU
//! supplementaire importe moins ici qu'au chemin live).

use std::collections::HashMap;
use std::io::Write;
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use std::io::Read;
use std::sync::OnceLock;
use zstd::bulk::{Compressor as ZstdCompressor, Decompressor as ZstdDecompressor};
use zstd::dict::{DecoderDictionary, EncoderDictionary};
use super::wai_core::{encode_varint, decode_varint};
use super::structural::{encode_structural, decode_structural, StructuralStreams, StructuralError};
use super::stable_dictionary::{encode_stable, decode_stable, StableDictError};
use super::text_dictionary::{encode_text, decode_text, TextDictError};
use super::variable_delta::{encode_variable, decode_variable, VariableDeltaError};

const MODE_DICTIONARIES: u8 = 0x00;
const MODE_GZIP_STRUCTURAL: u8 = 0x01;
const MODE_ZSTD_DICT: u8 = 0x02;

/// Dictionnaire zstd NATIF (zdict/COVER), entraine une fois hors-ligne par
/// `examples/train_zstd_dictionary.rs` sur un corpus synthetique large de
/// flux structurels CSTL, et EMBARQUE dans le binaire -- jamais transmis
/// sur le fil, memes octets des deux cotes (compresseur et decompresseur)
/// par construction (meme binaire). Mesure (voir
/// `examples/benchmark_zstd_dictionary.rs`, 2026-10-02) : bat le mode
/// MODE_DICTIONARIES (dictionnaires semantiques maison) de 23 a 47% sur
/// la grille de tailles testee, y compris hors distribution
/// d'entrainement. A re-generer (pas a la main -- re-executer l'exemple)
/// si le vocabulaire CSTL change significativement.
const ZSTD_DICT_BYTES: &[u8] = include_bytes!("zstd_cstl_dict.bin");

fn zstd_encoder_dict() -> &'static EncoderDictionary<'static> {
    static DICT: OnceLock<EncoderDictionary<'static>> = OnceLock::new();
    DICT.get_or_init(|| EncoderDictionary::copy(ZSTD_DICT_BYTES, 3))
}

fn zstd_decoder_dict() -> &'static DecoderDictionary<'static> {
    static DICT: OnceLock<DecoderDictionary<'static>> = OnceLock::new();
    DICT.get_or_init(|| DecoderDictionary::copy(ZSTD_DICT_BYTES))
}

#[derive(Debug)]
pub enum MasterError {
    VariableDelta(VariableDeltaError),
    Structural(StructuralError),
    StableDict(StableDictError),
    TextDict(TextDictError),
    Truncated(&'static str),
    UnsupportedMode(u8),
    Gzip(String),
}

impl std::fmt::Display for MasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MasterError::VariableDelta(e) => write!(f, "flux variable (delta): {e}"),
            MasterError::Structural(e) => write!(f, "couche 1 (structural): {e}"),
            MasterError::StableDict(e) => write!(f, "flux stable (dictionnaire opcodes): {e}"),
            MasterError::TextDict(e) => write!(f, "flux texte (dictionnaire noms/valeurs): {e}"),
            MasterError::Truncated(ctx) => write!(f, "master: flux tronque ({ctx})"),
            MasterError::UnsupportedMode(m) => write!(f, "master: octet de mode inconnu ({m})"),
            MasterError::Gzip(e) => write!(f, "master: gzip ({e})"),
        }
    }
}
impl std::error::Error for MasterError {}

type Triple = (Vec<HashMap<String, String>>, Vec<HashMap<String, String>>, Vec<HashMap<String, String>>);

/// Candidat DICTIONARIES (sans l'octet de mode -- ajoute par l'appelant).
/// C'est l'ancien corps de `master_compress`, inchange.
fn compress_dictionaries(streams: &StructuralStreams) -> Result<Vec<u8>, MasterError> {
    let stable_encoded = encode_stable(&streams.stable);
    let text_encoded = encode_text(&streams.text);
    let variable_encoded = encode_variable(&streams.variable).map_err(MasterError::VariableDelta)?;

    let mut out = Vec::new();
    out.extend_from_slice(&encode_varint(stable_encoded.len() as u32));
    out.extend_from_slice(&stable_encoded);
    out.extend_from_slice(&encode_varint(text_encoded.len() as u32));
    out.extend_from_slice(&text_encoded);
    out.extend_from_slice(&encode_varint(streams.ids.len() as u32));
    out.extend_from_slice(&streams.ids);
    out.extend_from_slice(&variable_encoded);
    Ok(out)
}

fn decompress_dictionaries(bytes: &[u8]) -> Result<StructuralStreams, MasterError> {
    let mut pos = 0usize;
    let (stable_len, used) = decode_varint(bytes).map_err(|_| MasterError::Truncated("longueur flux stable"))?;
    pos += used;
    let stable_len = stable_len as usize;
    if pos + stable_len > bytes.len() { return Err(MasterError::Truncated("flux stable")); }
    let stable_encoded = &bytes[pos..pos + stable_len];
    pos += stable_len;

    let (text_len, used) = decode_varint(&bytes[pos..]).map_err(|_| MasterError::Truncated("longueur flux texte"))?;
    pos += used;
    let text_len = text_len as usize;
    if pos + text_len > bytes.len() { return Err(MasterError::Truncated("flux texte")); }
    let text_encoded = &bytes[pos..pos + text_len];
    pos += text_len;

    let (ids_len, used) = decode_varint(&bytes[pos..]).map_err(|_| MasterError::Truncated("longueur flux ids"))?;
    pos += used;
    let ids_len = ids_len as usize;
    if pos + ids_len > bytes.len() { return Err(MasterError::Truncated("flux ids")); }
    let ids = bytes[pos..pos + ids_len].to_vec();
    pos += ids_len;

    let variable_encoded = &bytes[pos..];

    let stable = decode_stable(stable_encoded).map_err(MasterError::StableDict)?;
    let text = decode_text(text_encoded).map_err(MasterError::TextDict)?;
    let variable = decode_variable(variable_encoded).map_err(MasterError::VariableDelta)?;

    Ok(StructuralStreams { stable, text, ids, variable })
}

/// Concatene les 4 flux structurels BRUTS (aucun dictionnaire applique)
/// avec prefixes de longueur -- partage par les candidats GZIP et ZSTD
/// (meme representation d'entree, seul l'entropy coder final differe).
fn flatten_streams(streams: &StructuralStreams) -> Vec<u8> {
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

fn unflatten_streams(flat: &[u8], ctx_prefix: &'static str) -> Result<StructuralStreams, MasterError> {
    let mut pos = 0usize;
    let (stable_len, used) = decode_varint(&flat[pos..]).map_err(|_| MasterError::Truncated(ctx_prefix))?;
    pos += used;
    let stable_len = stable_len as usize;
    if pos + stable_len > flat.len() { return Err(MasterError::Truncated(ctx_prefix)); }
    let stable = flat[pos..pos + stable_len].to_vec();
    pos += stable_len;

    let (text_len, used) = decode_varint(&flat[pos..]).map_err(|_| MasterError::Truncated(ctx_prefix))?;
    pos += used;
    let text_len = text_len as usize;
    if pos + text_len > flat.len() { return Err(MasterError::Truncated(ctx_prefix)); }
    let text = flat[pos..pos + text_len].to_vec();
    pos += text_len;

    let (ids_len, used) = decode_varint(&flat[pos..]).map_err(|_| MasterError::Truncated(ctx_prefix))?;
    pos += used;
    let ids_len = ids_len as usize;
    if pos + ids_len > flat.len() { return Err(MasterError::Truncated(ctx_prefix)); }
    let ids = flat[pos..pos + ids_len].to_vec();
    pos += ids_len;

    let (variable_len, used) = decode_varint(&flat[pos..]).map_err(|_| MasterError::Truncated(ctx_prefix))?;
    pos += used;
    let variable_len = variable_len as usize;
    if pos + variable_len > flat.len() { return Err(MasterError::Truncated(ctx_prefix)); }
    let variable = flat[pos..pos + variable_len].to_vec();

    Ok(StructuralStreams { stable, text, ids, variable })
}

/// Candidat GZIP (sans l'octet de mode) : les 4 flux BRUTS concatenes puis
/// gzippes en un seul bloc -- gagne quand le contenu est plus grand/varie
/// que ce que les dictionnaires peuvent exploiter (voir doc de module).
fn compress_gzip_structural(streams: &StructuralStreams) -> Result<Vec<u8>, MasterError> {
    let flat = flatten_streams(streams);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&flat).map_err(|e| MasterError::Gzip(e.to_string()))?;
    encoder.finish().map_err(|e| MasterError::Gzip(e.to_string()))
}

fn decompress_gzip_structural(bytes: &[u8]) -> Result<StructuralStreams, MasterError> {
    let mut flat = Vec::new();
    GzDecoder::new(bytes).read_to_end(&mut flat).map_err(|e| MasterError::Gzip(e.to_string()))?;
    unflatten_streams(&flat, "gzip: flux tronque")
}

/// Candidat ZSTD+DICTIONNAIRE (sans l'octet de mode) : les 4 flux BRUTS
/// concatenes, compresses avec zstd niveau 3 EN UTILISANT le dictionnaire
/// natif embarque (`ZSTD_DICT_BYTES`, voir doc plus haut). La longueur du
/// flux decompresse est prefixee en varint -- necessaire pour
/// `Decompressor::decompress`, qui a besoin d'une borne de capacite
/// (contrairement a gzip, qui lit jusqu'a EOF).
fn compress_zstd_dict_structural(streams: &StructuralStreams) -> Result<Vec<u8>, MasterError> {
    let flat = flatten_streams(streams);
    let mut compressor = ZstdCompressor::with_prepared_dictionary(zstd_encoder_dict())
        .map_err(|e| MasterError::Gzip(format!("zstd compressor: {e}")))?;
    let body = compressor.compress(&flat).map_err(|e| MasterError::Gzip(format!("zstd compress: {e}")))?;

    let mut out = Vec::with_capacity(body.len() + 5);
    out.extend_from_slice(&encode_varint(flat.len() as u32));
    out.extend_from_slice(&body);
    Ok(out)
}

fn decompress_zstd_dict_structural(bytes: &[u8]) -> Result<StructuralStreams, MasterError> {
    let (flat_len, used) = decode_varint(bytes).map_err(|_| MasterError::Truncated("zstd: longueur flux decompresse"))?;
    let body = &bytes[used..];
    let mut decompressor = ZstdDecompressor::with_prepared_dictionary(zstd_decoder_dict())
        .map_err(|e| MasterError::Gzip(format!("zstd decompressor: {e}")))?;
    let flat = decompressor
        .decompress(body, flat_len as usize)
        .map_err(|e| MasterError::Gzip(format!("zstd decompress: {e}")))?;
    unflatten_streams(&flat, "zstd: flux tronque")
}

/// Couche 1 (4 flux) puis le MEILLEUR des TROIS candidats mesures pour CE
/// message precis (dictionnaires semantiques maison, gzip sur flux bruts,
/// zstd+dictionnaire natif embarque) -- voir doc de module. Roundtrip
/// garanti via `master_decompress`, voir tests.
pub fn master_compress(
    defines: &[HashMap<String, String>],
    relations: &[HashMap<String, String>],
    uncertainty: &[HashMap<String, String>],
) -> Result<Vec<u8>, MasterError> {
    let streams = encode_structural(defines, relations, uncertainty);

    let dict_candidate = compress_dictionaries(&streams)?;
    let gzip_candidate = compress_gzip_structural(&streams)?;
    let zstd_dict_candidate = compress_zstd_dict_structural(&streams)?;

    let mut best_mode = MODE_DICTIONARIES;
    let mut best_body = dict_candidate;
    if gzip_candidate.len() < best_body.len() {
        best_mode = MODE_GZIP_STRUCTURAL;
        best_body = gzip_candidate;
    }
    if zstd_dict_candidate.len() < best_body.len() {
        best_mode = MODE_ZSTD_DICT;
        best_body = zstd_dict_candidate;
    }
    let (mode, body) = (best_mode, best_body);

    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(mode);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Lit l'octet de mode ecrit par `master_compress` et inverse le bon
/// candidat -- voir doc de module. `bytes` vide reste un cas special
/// (triple entierement vide, jamais produit avec un octet de mode par
/// `master_compress` lui-meme mais tolere defensivement ici).
pub fn master_decompress(bytes: &[u8]) -> Result<Triple, MasterError> {
    if bytes.is_empty() {
        return decode_structural(&StructuralStreams::default()).map_err(MasterError::Structural);
    }
    let mode = bytes[0];
    let body = &bytes[1..];
    let streams = match mode {
        MODE_DICTIONARIES => decompress_dictionaries(body)?,
        MODE_GZIP_STRUCTURAL => decompress_gzip_structural(body)?,
        MODE_ZSTD_DICT => decompress_zstd_dict_structural(body)?,
        other => return Err(MasterError::UnsupportedMode(other)),
    };
    decode_structural(&streams).map_err(MasterError::Structural)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_master_empty_roundtrip() {
        let compressed = master_compress(&[], &[], &[]).unwrap();
        let (d, r, u) = master_decompress(&compressed).unwrap();
        assert!(d.is_empty() && r.is_empty() && u.is_empty());
    }

    /// Verifie que le candidat MODE_ZSTD_DICT lui-meme fait un roundtrip
    /// correct en l'ISOLANT (sans passer par le choix a 3 candidats, qui
    /// pourrait masquer une regression si un AUTRE mode se trouve gagner
    /// pour ce message precis).
    #[test]
    fn test_zstd_dict_candidate_roundtrip_isolated() {
        let defines = vec![
            m(&[("name", "Tenant"), ("entity_type", "Party"), ("id", "e001")]),
            m(&[("name", "Landlord"), ("entity_type", "Party"), ("id", "e002")]),
        ];
        let relations = vec![
            m(&[("type", "PERFORM"), ("subject", "Tenant"), ("object", "Rent"), ("modality", "MUST"), ("id", "r001")]),
        ];
        let streams = encode_structural(&defines, &relations, &[]);

        let compressed = compress_zstd_dict_structural(&streams).unwrap();
        let decompressed = decompress_zstd_dict_structural(&compressed).unwrap();
        assert_eq!(decompressed.stable, streams.stable);
        assert_eq!(decompressed.text, streams.text);
        assert_eq!(decompressed.ids, streams.ids);
        assert_eq!(decompressed.variable, streams.variable);
    }

    /// Le test direct de la question d'Olivier : sur un message petit
    /// typique, le candidat choisi par `master_compress` doit etre
    /// MODE_ZSTD_DICT (et pas juste "un des trois qui marche") -- sinon
    /// le dictionnaire zstd embarque n'apporte rien en pratique sur le
    /// cas le plus courant.
    #[test]
    fn test_master_picks_zstd_dict_for_typical_small_message() {
        let defines = vec![
            m(&[("name", "Tenant"), ("entity_type", "Party"), ("id", "e001")]),
            m(&[("name", "Landlord"), ("entity_type", "Party"), ("id", "e002")]),
            m(&[("name", "Rent"), ("entity_type", "Obligation"), ("id", "e003")]),
        ];
        let relations = vec![
            m(&[("type", "PERFORM"), ("subject", "Tenant"), ("object", "Rent"), ("modality", "MUST"), ("id", "r001")]),
            m(&[("type", "ARR.ACCESS"), ("subject", "Tenant"), ("object", "SecurityDeposit"), ("modality", "MUST_NOT"), ("id", "r002")]),
        ];
        let compressed = master_compress(&defines, &relations, &[]).unwrap();
        assert_eq!(compressed[0], MODE_ZSTD_DICT, "le mode choisi devrait etre MODE_ZSTD_DICT pour ce message typique, octet de mode = {}", compressed[0]);

        let (d, r, u) = master_decompress(&compressed).unwrap();
        assert_eq!((d, r, u), (defines, relations, vec![]));
    }

    /// LE test qui repond a la demande de l'utilisateur : dictionnaire
    /// reseau stable pre-rempli (pour les opcodes) + dictionnaire
    /// adaptatif pour le reste, mesure sur le meme petit payload qui
    /// pesait 393 octets (173,9% du texte) avec l'ancienne approche a un
    /// seul flux + table adaptative unique.
    #[test]
    fn test_master_with_split_dictionaries_real_measured_ratio() {
        let defines = vec![
            m(&[("name", "Tenant"), ("entity_type", "Party"), ("id", "e001")]),
            m(&[("name", "Landlord"), ("entity_type", "Party"), ("id", "e002")]),
            m(&[("name", "Rent"), ("entity_type", "Obligation"), ("id", "e003")]),
        ];
        let relations = vec![
            m(&[("type", "PERFORM"), ("subject", "Tenant"), ("object", "Rent"), ("modality", "MUST"), ("id", "r001")]),
            m(&[("type", "ARR.ACCESS"), ("subject", "Tenant"), ("object", "SecurityDeposit"), ("modality", "MUST_NOT"), ("id", "r002")]),
        ];
        let uncertainty: Vec<HashMap<String, String>> = vec![];

        let compressed = master_compress(&defines, &relations, &uncertainty).unwrap();
        let (d, r, u) = master_decompress(&compressed).unwrap();
        assert_eq!((d, r, u), (defines, relations, uncertainty));

        let text_equivalent = "DEFINE Tenant AS Party [id=e001]\nDEFINE Landlord AS Party [id=e002]\nDEFINE Rent AS Obligation [id=e003]\nCONSTRAINTS [\n  (MUST) Tenant PERFORM Rent [id=r001]\n  [MUST_NOT] Tenant ARR.ACCESS SecurityDeposit [id=r002]\n]\n---END---\n";
        println!(
            "MASTER (dictionnaires separes): {} octets | ancienne approche (1 flux, 1 table adaptative): 393 octets | texte CSTL: {} octets | ratio vs texte: {:.1}% | ratio vs ancienne approche: {:.1}%",
            compressed.len(), text_equivalent.len(),
            100.0 * compressed.len() as f64 / text_equivalent.len() as f64,
            100.0 * compressed.len() as f64 / 393.0
        );
    }

    #[test]
    fn test_master_larger_corpus_measured_ratio() {
        let mut defines = Vec::new();
        let mut relations = Vec::new();
        let entities = ["Tenant", "Landlord", "Rent", "SecurityDeposit", "LeaseAgreement", "Property", "PaymentSchedule", "Inspector"];
        let types = ["Party", "Obligation", "Asset", "Document", "Event"];
        for (i, name) in entities.iter().enumerate() {
            defines.push(m(&[("name", name), ("entity_type", types[i % types.len()]), ("id", &format!("e{i:03}"))]));
        }
        let ops = ["PERFORM", "ARR.ACCESS", "MAINTAIN", "TRANSMIT_FAITHFUL", "COMMAND", "ENTAILS", "BEFORE", "OPPOSES"];
        let modalities = ["MUST", "MUST_NOT", "MAY", "SHOULD"];
        for i in 0..16usize {
            relations.push(m(&[
                ("type", ops[i % ops.len()]),
                ("subject", entities[i % entities.len()]),
                ("object", entities[(i + 3) % entities.len()]),
                ("modality", modalities[i % modalities.len()]),
                ("id", &format!("r{i:03}")),
            ]));
        }
        let uncertainty = vec![
            m(&[("identifier", "e001"), ("status", "ESTIMATED"), ("sigma", "0.3")]),
            m(&[("identifier", "r005"), ("status", "UNKNOWN")]),
        ];

        let compressed = master_compress(&defines, &relations, &uncertainty).unwrap();
        let (d, r, u) = master_decompress(&compressed).unwrap();
        assert_eq!(d, defines);
        assert_eq!(r, relations);
        assert_eq!(u, uncertainty);

        let mut text = String::new();
        for def in &defines {
            text.push_str(&format!("DEFINE {} AS {} [id={}]\n", def["name"], def["entity_type"], def["id"]));
        }
        text.push_str("CONSTRAINTS [\n");
        for rel in &relations {
            text.push_str(&format!("  ({}) {} {} {} [id={}]\n", rel["modality"], rel["subject"], rel["type"], rel["object"], rel["id"]));
        }
        text.push_str("]\n");
        text.push_str("UNCERTAINTY [\n");
        for u in &uncertainty {
            match u.get("sigma") {
                Some(s) => text.push_str(&format!("  {} {} [sigma={}]\n", u["identifier"], u["status"], s)),
                None => text.push_str(&format!("  {} {}\n", u["identifier"], u["status"])),
            }
        }
        text.push_str("]\n---END---\n");

        println!(
            "MASTER corpus plus large (dictionnaires separes): {} octets | texte CSTL equivalent: {} octets | ratio: {:.1}%",
            compressed.len(), text.len(),
            100.0 * compressed.len() as f64 / text.len() as f64
        );
    }
}



