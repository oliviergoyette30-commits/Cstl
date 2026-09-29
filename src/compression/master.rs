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
//! Format de sortie : `[varint len(stable)][stable][varint len(text)]
//! [text][varint len(ids)][ids][variable (delta zigzag) jusqu'a la fin]`.

use std::collections::HashMap;
use super::wai_core::{encode_varint, decode_varint};
use super::structural::{encode_structural, decode_structural, StructuralStreams, StructuralError};
use super::stable_dictionary::{encode_stable, decode_stable, StableDictError};
use super::text_dictionary::{encode_text, decode_text, TextDictError};
use super::variable_delta::{encode_variable, decode_variable, VariableDeltaError};

#[derive(Debug)]
pub enum MasterError {
    VariableDelta(VariableDeltaError),
    Structural(StructuralError),
    StableDict(StableDictError),
    TextDict(TextDictError),
    Truncated(&'static str),
}

impl std::fmt::Display for MasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MasterError::VariableDelta(e) => write!(f, "flux variable (delta): {e}"),
            MasterError::Structural(e) => write!(f, "couche 1 (structural): {e}"),
            MasterError::StableDict(e) => write!(f, "flux stable (dictionnaire opcodes): {e}"),
            MasterError::TextDict(e) => write!(f, "flux texte (dictionnaire noms/valeurs): {e}"),
            MasterError::Truncated(ctx) => write!(f, "master: flux tronque ({ctx})"),
        }
    }
}
impl std::error::Error for MasterError {}

type Triple = (Vec<HashMap<String, String>>, Vec<HashMap<String, String>>, Vec<HashMap<String, String>>);

/// Couche 1 (3 flux) puis, sur CHAQUE flux, le dictionnaire adapte a sa
/// nature statistique -- voir doc de module. Roundtrip garanti via
/// `master_decompress`, voir tests.
pub fn master_compress(
    defines: &[HashMap<String, String>],
    relations: &[HashMap<String, String>],
    uncertainty: &[HashMap<String, String>],
) -> Result<Vec<u8>, MasterError> {
    let StructuralStreams { stable, text, ids, variable } = encode_structural(defines, relations, uncertainty);

    let stable_encoded = encode_stable(&stable);
    let text_encoded = encode_text(&text);
    let variable_encoded = encode_variable(&variable).map_err(MasterError::VariableDelta)?;

    let mut out = Vec::new();
    out.extend_from_slice(&encode_varint(stable_encoded.len() as u32));
    out.extend_from_slice(&stable_encoded);
    out.extend_from_slice(&encode_varint(text_encoded.len() as u32));
    out.extend_from_slice(&text_encoded);
    out.extend_from_slice(&encode_varint(ids.len() as u32));
    out.extend_from_slice(&ids);
    out.extend_from_slice(&variable_encoded);
    Ok(out)
}

pub fn master_decompress(bytes: &[u8]) -> Result<Triple, MasterError> {
    if bytes.is_empty() {
        return decode_structural(&StructuralStreams::default()).map_err(MasterError::Structural);
    }
    let mut pos = 0usize;
    let (stable_len, used) = decode_varint(bytes).map_err(|_| MasterError::Truncated("longueur flux stable"))?;
    pos += used;
    let stable_len = stable_len as usize;
    if pos + stable_len > bytes.len() {
        return Err(MasterError::Truncated("flux stable"));
    }
    let stable_encoded = &bytes[pos..pos + stable_len];
    pos += stable_len;

    let (text_len, used) = decode_varint(&bytes[pos..]).map_err(|_| MasterError::Truncated("longueur flux texte"))?;
    pos += used;
    let text_len = text_len as usize;
    if pos + text_len > bytes.len() {
        return Err(MasterError::Truncated("flux texte"));
    }
    let text_encoded = &bytes[pos..pos + text_len];
    pos += text_len;

    let (ids_len, used) = decode_varint(&bytes[pos..]).map_err(|_| MasterError::Truncated("longueur flux ids"))?;
    pos += used;
    let ids_len = ids_len as usize;
    if pos + ids_len > bytes.len() {
        return Err(MasterError::Truncated("flux ids"));
    }
    let ids = bytes[pos..pos + ids_len].to_vec();
    pos += ids_len;

    let variable_encoded = &bytes[pos..];

    let stable = decode_stable(stable_encoded).map_err(MasterError::StableDict)?;
    let text = decode_text(text_encoded).map_err(MasterError::TextDict)?;
    let variable = decode_variable(variable_encoded).map_err(MasterError::VariableDelta)?;

    decode_structural(&StructuralStreams { stable, text, ids, variable }).map_err(MasterError::Structural)
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



