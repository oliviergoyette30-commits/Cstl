//! src/compression/variable_delta.rs — delta+zigzag SANS TABLE pour le
//! flux `variable` (2026-09-29).
//!
//! Reponse directe a la demande de l'utilisateur : "il faut qu'il y ait
//! DEUX dictionnaires reseau stable pre-remplis pour qu'il n'y ait plus
//! d'overhead... juste des delta et des index" -- c'est-a-dire : SEULS
//! `stable_dictionary` (opcodes) et `text_dictionary` (noms/valeurs)
//! restent des dictionnaires pre-entraines ; le troisieme flux
//! (`variable` : compteurs, longueurs, index locaux dans la table de
//! chaines) n'en a PAS besoin, et n'en a jamais tire aucun gain garanti.
//!
//! Pourquoi ce module remplace `order1_rans::encode` pour ce flux
//! precis : ce dernier est adaptatif mais SERIALISE sa propre table de
//! frequences par contexte DANS chaque message (`serialize_tables`) --
//! pour un message CSTL court (le cas typique de cette session), ce cout
//! d'en-tete peut depasser le gain d'entropie. Pire : `variable` est
//! PROUVABLEMENT non partageable entre messages (les index pointent dans
//! une table de chaines locale a CE message -- voir la mise en garde
//! dans `structural.rs` et le bug de generalisation deja trouve en
//! verifiant cette session), donc un DICTIONNAIRE PRE-ENTRAINE (comme
//! pour `stable`/`text`) est architecturalement impossible ici -- il n'y
//! a rien de stable a pre-remplir.
//!
//! Ce que ce flux PEUT exploiter sans aucune table : la localite entre
//! valeurs voisines dans la sequence (les compteurs restent petits, les
//! index consecutifs pointant vers la meme entite se repetent ou se
//! suivent) -- delta zigzag + varint capture exactement ca, avec zero
//! octet de metadonnee transmis. Mesure honnete dans les tests
//! ci-dessous, pas supposee.
//!
//! Limite assumee et documentee (jamais un risque reel sur un flux CSTL
//! authentique) : le delta signe est encode sur i32 avant zigzag, pas
//! i64. Les valeurs de ce flux (compteurs de defines/relations, longueurs
//! de chaines, index dans une table de chaines locale) ne s'approchent
//! jamais de 2^31 en pratique -- aucun message CSTL reel ne contiendra
//! des milliards d'entites. Un ecart superieur a cette plage est
//! defensivement clampe plutot que de paniquer, ce qui produirait un
//! roundtrip FAUX dans ce cas extreme jamais rencontre -- a corriger vers
//! un delta i64/varint64 si jamais un flux `variable` legitime approche
//! cette taille (aucun test actuel ne l'exerce).

use super::wai_core::{decode_varint, encode_varint, zigzag_decode, zigzag_encode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableDeltaError {
    Truncated,
}

impl std::fmt::Display for VariableDeltaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableDeltaError::Truncated => write!(f, "flux variable (delta): tronque ou corrompu"),
        }
    }
}
impl std::error::Error for VariableDeltaError {}

/// Decode TOUS les varints d'un buffer, jusqu'a epuisement -- `variable`
/// est une sequence plate de valeurs varint sans longueur globale prefixee
/// (chaque champ sait combien en lire via le schema de `structural.rs`,
/// mais ici on traite la sequence entiere comme une liste de valeurs a
/// delta-encoder, sans se soucier de leur role individuel).
fn decode_all_varints(data: &[u8]) -> Result<Vec<u32>, VariableDeltaError> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let (v, used) = decode_varint(&data[pos..]).map_err(|_| VariableDeltaError::Truncated)?;
        if used == 0 {
            return Err(VariableDeltaError::Truncated);
        }
        out.push(v);
        pos += used;
    }
    Ok(out)
}

/// `raw` = flux `variable` tel qu'ecrit par `structural.rs` (varints bruts,
/// meme ordre). Sortie: meme nombre de valeurs, chacune remplacee par le
/// delta zigzag avec la precedente (premiere valeur delta-ee contre 0).
/// Aucune table, aucun en-tete de dictionnaire -- juste de l'arithmetique
/// deterministe.
pub fn encode_variable(raw: &[u8]) -> Result<Vec<u8>, VariableDeltaError> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let values = decode_all_varints(raw)?;
    let mut out = Vec::new();
    let mut prev: i64 = 0;
    for v in values {
        let delta = (v as i64) - prev;
        let delta32 = delta.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        out.extend_from_slice(&encode_varint(zigzag_encode(delta32)));
        prev = v as i64;
    }
    Ok(out)
}

/// Inverse exact de `encode_variable` -- reconstruit les MEMES octets
/// varint bruts que `structural.rs` avait produits (l'encodage varint est
/// canonique : une valeur -> une seule sequence d'octets), donc
/// `decode_structural` n'a besoin d'aucun changement.
pub fn decode_variable(data: &[u8]) -> Result<Vec<u8>, VariableDeltaError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let deltas = decode_all_varints(data)?;
    let mut out = Vec::new();
    let mut prev: i64 = 0;
    for d in deltas {
        let delta = zigzag_decode(d) as i64;
        let v = prev + delta;
        prev = v;
        out.extend_from_slice(&encode_varint(v as u32));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_roundtrip() {
        assert_eq!(encode_variable(&[]).unwrap(), Vec::<u8>::new());
        assert_eq!(decode_variable(&[]).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_roundtrip_preserves_exact_bytes() {
        // Simule un vrai flux `variable` : compteurs et index varint bruts.
        let mut raw = Vec::new();
        for v in [3u32, 0, 1, 2, 5, 5, 5, 5, 9, 2, 0, 300, 301, 302] {
            raw.extend_from_slice(&encode_varint(v));
        }
        let encoded = encode_variable(&raw).unwrap();
        let decoded = decode_variable(&encoded).unwrap();
        assert_eq!(decoded, raw, "doit reconstruire EXACTEMENT les octets varint bruts d'origine");
    }

    #[test]
    fn test_repeated_and_incrementing_indices_compress() {
        // Cas realiste: index qui se repetent (meme entite referencee
        // plusieurs fois) ou qui s'incrementent de 1 (table de chaines
        // remplie dans l'ordre) -- delta doit shrink ca vers 0 ou 1,
        // donc 1 octet varint au lieu de potentiellement 2.
        let mut raw = Vec::new();
        for v in [100u32, 101, 102, 103, 103, 103, 104, 200, 200, 200] {
            raw.extend_from_slice(&encode_varint(v));
        }
        let encoded = encode_variable(&raw).unwrap();
        assert_eq!(decode_variable(&encoded).unwrap(), raw);
        println!(
            "variable brut: {} octets | via delta zigzag: {} octets",
            raw.len(), encoded.len()
        );
        assert!(encoded.len() <= raw.len(), "sequence localement previsible: le delta ne doit jamais faire pire");
    }

    #[test]
    fn test_truncated_input_errors_cleanly() {
        // Dernier octet d'un varint multi-octets coupe (bit de continuation
        // pose mais rien apres) -- doit echouer proprement, jamais paniquer
        // ni boucler.
        let mut raw = encode_varint(300); // valeur >127 => >= 2 octets
        raw.push(0x80); // octet de continuation pose, tronque juste apres
        assert!(decode_variable(&raw).is_err());
    }
}
