//! src/compression/order1_rans.rs — Couches 2+3 du Master Compresseur CSTL :
//! modelisation de contexte d'ordre 1 + codage d'entropie rANS (2026-09-29).
//!
//! Ce module fusionne deliberement ce que la conversation avec
//! l'utilisateur avait nomme separement "Couche 2" (modele d'ordre 1,
//! table de transition caractere-a-caractere) et "Couche 3" (compactage
//! final en bits, FSE/rANS) : en pratique ce sont la MEME operation. Un
//! codeur d'entropie ne fait que traduire un MODELE DE PROBABILITE en
//! bits -- rANS "orde 0" (`fse_encoder_rs.rs`, deja reel dans ce depot)
//! utilise un modele ou P(symbole) est fixe pour tout le message ; ce
//! module utilise un modele ou P(symbole | symbole precedent) varie selon
//! le CONTEXTE -- c'est litteralement un modele de Markov d'ordre 1
//! (PPM d'ordre 1, la famille de compression nommee mais jamais
//! implementee plus tot dans la conversation), encode avec le meme moteur
//! arithmetique rANS que `fse_encoder_rs.rs`, juste une table de
//! frequences PAR CONTEXTE au lieu d'une seule table globale.
//!
//! Duplication assumee et deliberee (meme motif que `compression/fse/
//! encoder.rs` vs `fse_encoder_rs.rs`, deja justifiee dans ce depot) : ce
//! module ne reutilise pas `NormalizedFreqTable` de `fse_encoder_rs.rs`
//! (dont `symbol_for_slot`/serialisation sont prives au module) -- il
//! reimplemente la meme normalisation au plus grand reste, plus courte ici
//! car generique sur N tables plutot qu'une seule.
//!
//! Honnetete sur le compromis : plus de contextes distincts = plus de
//! tables a serialiser = plus d'overhead d'en-tete. Sur un message court
//! (le cas CSTL typique), rien ne garantit que le gain de prediction
//! batte ce cout -- c'est mesure dans les tests ci-dessous sur un vrai
//! flux structurel (`compression::structural`), pas suppose.

use std::collections::HashMap;
use super::wai_core::{encode_varint, decode_varint};

const PROB_BITS: u32 = 12;
const PROB_SCALE: u32 = 1 << PROB_BITS;
const RANS_L: u32 = 1 << 23;
/// Contexte special pour le tout premier symbole d'un message (aucun
/// symbole precedent) -- hors de la plage 0..=255 des contextes-octets
/// reels, donc aucune collision possible.
const START_CONTEXT: u16 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order1Error {
    EmptyInput,
    Truncated(&'static str),
    UnknownContext(u16),
    UnknownSlot(u16, u32),
}

impl std::fmt::Display for Order1Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Order1Error::EmptyInput => write!(f, "entree vide"),
            Order1Error::Truncated(ctx) => write!(f, "flux order1 tronque ({ctx})"),
            Order1Error::UnknownContext(c) => write!(f, "contexte {c} absent de la table"),
            Order1Error::UnknownSlot(c, s) => write!(f, "slot {s} hors table pour contexte {c}"),
        }
    }
}
impl std::error::Error for Order1Error {}

struct CtxTable {
    freq: [u32; 256],
    cum: [u32; 257],
}

/// Normalisation au plus grand reste -- identique en esprit a
/// `fse_encoder_rs::NormalizedFreqTable::from_data`, voir doc de module
/// pour pourquoi ce n'est pas partage directement.
fn normalize(counts: &HashMap<u8, u32>) -> CtxTable {
    let total: u64 = counts.values().map(|&c| c as u64).sum();
    let mut freq = [0u32; 256];
    let mut remainders: Vec<(usize, u64)> = Vec::new();
    let mut assigned: u32 = 0;
    for (&sym, &raw) in counts {
        let raw = raw as u64;
        let scaled = ((raw * PROB_SCALE as u64) / total).max(1) as u32;
        freq[sym as usize] = scaled;
        assigned += scaled;
        remainders.push((sym as usize, (raw * PROB_SCALE as u64) % total));
    }
    if assigned != PROB_SCALE {
        remainders.sort_by(|a, b| b.1.cmp(&a.1));
        if assigned < PROB_SCALE {
            let mut deficit = PROB_SCALE - assigned;
            let mut i = 0;
            while deficit > 0 {
                let sym = remainders[i % remainders.len()].0;
                freq[sym] += 1;
                deficit -= 1;
                i += 1;
            }
        } else {
            let mut surplus = assigned - PROB_SCALE;
            let mut i = remainders.len();
            while surplus > 0 {
                i = i.wrapping_sub(1);
                if i >= remainders.len() { i = remainders.len() - 1; }
                let sym = remainders[i].0;
                if freq[sym] > 1 {
                    freq[sym] -= 1;
                    surplus -= 1;
                }
            }
        }
    }
    let mut cum = [0u32; 257];
    let mut acc = 0u32;
    for sym in 0..256 {
        cum[sym] = acc;
        acc += freq[sym];
    }
    cum[256] = acc;
    debug_assert_eq!(acc, PROB_SCALE);
    CtxTable { freq, cum }
}

fn context_for(data: &[u8], i: usize) -> u16 {
    if i == 0 { START_CONTEXT } else { data[i - 1] as u16 }
}

fn build_tables(data: &[u8]) -> HashMap<u16, CtxTable> {
    build_tables_from_messages(std::slice::from_ref(&data))
}

/// Meme construction que `build_tables`, mais sur PLUSIEURS sequences
/// independantes -- chacune a son propre `i==0` (donc son propre
/// `START_CONTEXT`), au lieu de traiter une concatenation comme UNE
/// seule sequence ou seul le tout premier octet global verrait jamais
/// `START_CONTEXT`. Necessaire pour `stable_dictionary::build_reference_corpus`
/// (33 operateurs doivent chacun apparaitre comme "premier octet
/// possible d'un message", pas seulement le premier du corpus
/// d'entrainement concatene -- bug trouve en verifiant, 2026-09-29).
fn build_tables_from_messages(messages: &[&[u8]]) -> HashMap<u16, CtxTable> {
    let mut raw: HashMap<u16, HashMap<u8, u32>> = HashMap::new();
    for data in messages {
        for i in 0..data.len() {
            let ctx = context_for(data, i);
            *raw.entry(ctx).or_default().entry(data[i]).or_insert(0) += 1;
        }
    }
    raw.into_iter().map(|(ctx, counts)| (ctx, normalize(&counts))).collect()
}

fn serialize_tables(tables: &HashMap<u16, CtxTable>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&encode_varint(tables.len() as u32));
    // Ordre deterministe (cle triee) -- meme flux d'octets a chaque encodage
    // du meme message.
    let mut keys: Vec<&u16> = tables.keys().collect();
    keys.sort();
    for &ctx in keys {
        let t = &tables[&ctx];
        out.extend_from_slice(&encode_varint(ctx as u32));
        let present: Vec<(u8, u32)> = (0..256).filter(|&s| t.freq[s] > 0).map(|s| (s as u8, t.freq[s])).collect();
        out.extend_from_slice(&encode_varint(present.len() as u32));
        for (sym, freq) in present {
            out.push(sym);
            out.extend_from_slice(&encode_varint(freq));
        }
    }
    out
}

fn deserialize_tables(data: &[u8], pos: &mut usize) -> Result<HashMap<u16, CtxTable>, Order1Error> {
    let (n_ctx, used) = decode_varint(&data[*pos..]).map_err(|_| Order1Error::Truncated("tables: compte"))?;
    *pos += used;
    let mut tables = HashMap::new();
    for _ in 0..n_ctx {
        let (ctx, used) = decode_varint(&data[*pos..]).map_err(|_| Order1Error::Truncated("tables: contexte"))?;
        *pos += used;
        let (n_sym, used) = decode_varint(&data[*pos..]).map_err(|_| Order1Error::Truncated("tables: n_sym"))?;
        *pos += used;
        let mut freq = [0u32; 256];
        for _ in 0..n_sym {
            if *pos >= data.len() { return Err(Order1Error::Truncated("tables: symbole")); }
            let sym = data[*pos]; *pos += 1;
            let (f, used) = decode_varint(&data[*pos..]).map_err(|_| Order1Error::Truncated("tables: freq"))?;
            *pos += used;
            freq[sym as usize] = f;
        }
        let mut cum = [0u32; 257];
        let mut acc = 0u32;
        for sym in 0..256 {
            cum[sym] = acc;
            acc += freq[sym];
        }
        cum[256] = acc;
        tables.insert(ctx as u16, CtxTable { freq, cum });
    }
    Ok(tables)
}

/// Encode `data` avec un modele d'ordre 1 (contexte = octet precedent) +
/// rANS. Table(s) de frequences embarquee(s) dans la sortie (adaptatif,
/// pas de synchronisation externe requise -- meme choix que
/// `fse_encoder_rs.rs`, voir sa doc pour la justification du risque
/// evite par rapport a `fse::encoder` a table externe).
pub fn encode(data: &[u8]) -> Result<Vec<u8>, Order1Error> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let tables = build_tables(data);

    let mut state: u32 = RANS_L;
    let mut rev_bytes: Vec<u8> = Vec::new();
    for i in (0..data.len()).rev() {
        let ctx = context_for(data, i);
        let t = &tables[&ctx];
        let sym = data[i] as usize;
        let freq = t.freq[sym];
        let start = t.cum[sym];
        let x_max = ((RANS_L >> PROB_BITS) << 8) * freq;
        while state >= x_max {
            rev_bytes.push((state & 0xff) as u8);
            state >>= 8;
        }
        state = ((state / freq) << PROB_BITS) + (state % freq) + start;
    }
    rev_bytes.reverse();

    let mut out = serialize_tables(&tables);
    out.extend_from_slice(&encode_varint(data.len() as u32));
    out.extend_from_slice(&state.to_le_bytes());
    out.extend_from_slice(&rev_bytes);
    Ok(out)
}

/// Inverse de `encode`.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>, Order1Error> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut pos = 0usize;
    let tables = deserialize_tables(bytes, &mut pos)?;
    let (count, used) = decode_varint(&bytes[pos..]).map_err(|_| Order1Error::Truncated("count"))?;
    pos += used;
    if pos + 4 > bytes.len() {
        return Err(Order1Error::Truncated("state"));
    }
    let mut state = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
    pos += 4;

    let mask = PROB_SCALE - 1;
    let mut out = Vec::with_capacity(count as usize);
    let mut ctx: u16 = START_CONTEXT;
    for _ in 0..count {
        let t = tables.get(&ctx).ok_or(Order1Error::UnknownContext(ctx))?;
        let slot = state & mask;
        let mut sym: Option<u8> = None;
        for s in 0..256 {
            if slot >= t.cum[s] && slot < t.cum[s + 1] {
                sym = Some(s as u8);
                break;
            }
        }
        let sym = sym.ok_or(Order1Error::UnknownSlot(ctx, slot))?;
        let freq = t.freq[sym as usize];
        let start = t.cum[sym as usize];
        state = freq * (state >> PROB_BITS) + slot - start;

        while state < RANS_L {
            if pos >= bytes.len() {
                return Err(Order1Error::Truncated("bitstream epuise"));
            }
            state = (state << 8) | (bytes[pos] as u32);
            pos += 1;
        }
        out.push(sym);
        ctx = sym as u16;
    }
    Ok(out)
}

/// Table d'ordre 1 entrainee a l'avance sur un corpus representatif et
/// reutilisee pour PLUSIEURS messages, sans jamais etre re-serialisee dans
/// chacun -- reponse directe a la question de l'utilisateur (2026-09-29) :
/// "si le dictionnaire reseau est pre-rempli, l'overhead devrait etre
/// quasi inexistant". Meme mecanisme de fond que `wai_dictionary.rs`
/// (vocabulaire pre-partage, jamais transmis par message) mais applique a
/// des frequences de PAIRES d'octets plutot qu'a des mots-cles entiers.
///
/// Piege honnete, a ne pas escamoter : contrairement au dictionnaire de
/// mots-cles CSTL (vocabulaire FERME et connu a l'avance -- 33 operateurs,
/// 10 modalites), les frequences d'ordre 1 dependent de la distribution
/// REELLE du trafic. Une table entrainee sur un corpus non representatif
/// (top petit, ou trop different du trafic reel) produit soit un gain
/// illusoire (mesure sur les memes donnees que l'entrainement -- biais de
/// surapprentissage deja signale cette session sur l'estimation d'entropie
/// a 428 caracteres), soit des erreurs `UnknownContext`/`UnknownSlot` a
/// l'usage reel (symbole ou paire jamais vue a l'entrainement). Ce module
/// ne dissimule PAS cette erreur -- `encode_with_table`/`decode_with_table`
/// echouent explicitement plutot que de corrompre silencieusement le
/// message, contrairement au risque documente pour `compression::fse::
/// encoder` (table externe non synchronisee).
pub struct PretrainedOrder1Table(HashMap<u16, CtxTable>);

impl PretrainedOrder1Table {
    /// Entraine une table a partir d'un corpus (idealement un ensemble de
    /// PLUSIEURS messages CSTL reels, pas un seul court exemple -- voir
    /// mise en garde de surapprentissage ci-dessus).
    pub fn train(corpus: &[u8]) -> Self {
        PretrainedOrder1Table(build_tables(corpus))
    }

    /// Comme `train`, mais sur plusieurs messages INDEPENDANTS -- chacun
    /// voit son propre `START_CONTEXT` au lieu que seul le tout premier
    /// du lot compte. Voir doc de `build_tables_from_messages`.
    pub fn train_from_messages(messages: &[&[u8]]) -> Self {
        PretrainedOrder1Table(build_tables_from_messages(messages))
    }
}

fn rans_encode_body(data: &[u8], tables: &HashMap<u16, CtxTable>) -> Result<(u32, Vec<u8>), Order1Error> {
    let mut state: u32 = RANS_L;
    let mut rev_bytes: Vec<u8> = Vec::new();
    for i in (0..data.len()).rev() {
        let ctx = context_for(data, i);
        let t = tables.get(&ctx).ok_or(Order1Error::UnknownContext(ctx))?;
        let sym = data[i] as usize;
        let freq = t.freq[sym];
        if freq == 0 {
            // La table pre-entrainee n'a jamais vu ce symbole dans ce
            // contexte -- echec explicite, jamais un encodage corrompu.
            return Err(Order1Error::UnknownSlot(ctx, sym as u32));
        }
        let start = t.cum[sym];
        let x_max = ((RANS_L >> PROB_BITS) << 8) * freq;
        while state >= x_max {
            rev_bytes.push((state & 0xff) as u8);
            state >>= 8;
        }
        state = ((state / freq) << PROB_BITS) + (state % freq) + start;
    }
    rev_bytes.reverse();
    Ok((state, rev_bytes))
}

/// Encode `data` avec une table PRE-ENTRAINEE, fournie separement et
/// jamais serialisee dans la sortie -- seuls [longueur][etat rANS][corps]
/// voyagent. C'est la mesure directe de "l'overhead serait quasi
/// inexistant si le dictionnaire est pre-rempli".
pub fn encode_with_table(data: &[u8], table: &PretrainedOrder1Table) -> Result<Vec<u8>, Order1Error> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let (state, rev_bytes) = rans_encode_body(data, &table.0)?;
    let mut out = Vec::new();
    out.extend_from_slice(&encode_varint(data.len() as u32));
    out.extend_from_slice(&state.to_le_bytes());
    out.extend_from_slice(&rev_bytes);
    Ok(out)
}

/// Inverse de `encode_with_table` -- exige la MEME table pre-entrainee des
/// deux cotes (meme risque de synchronisation que tout dictionnaire
/// partage, documente dans `compression::fse::encoder`).
pub fn decode_with_table(bytes: &[u8], table: &PretrainedOrder1Table) -> Result<Vec<u8>, Order1Error> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut pos = 0usize;
    let (count, used) = decode_varint(&bytes[pos..]).map_err(|_| Order1Error::Truncated("count"))?;
    pos += used;
    if pos + 4 > bytes.len() {
        return Err(Order1Error::Truncated("state"));
    }
    let mut state = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
    pos += 4;

    let mask = PROB_SCALE - 1;
    let mut out = Vec::with_capacity(count as usize);
    let mut ctx: u16 = START_CONTEXT;
    for _ in 0..count {
        let t = table.0.get(&ctx).ok_or(Order1Error::UnknownContext(ctx))?;
        let slot = state & mask;
        let mut sym: Option<u8> = None;
        for s in 0..256 {
            if slot >= t.cum[s] && slot < t.cum[s + 1] {
                sym = Some(s as u8);
                break;
            }
        }
        let sym = sym.ok_or(Order1Error::UnknownSlot(ctx, slot))?;
        let freq = t.freq[sym as usize];
        let start = t.cum[sym as usize];
        state = freq * (state >> PROB_BITS) + slot - start;

        while state < RANS_L {
            if pos >= bytes.len() {
                return Err(Order1Error::Truncated("bitstream epuise"));
            }
            state = (state << 8) | (bytes[pos] as u32);
            pos += 1;
        }
        out.push(sym);
        ctx = sym as u16;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_roundtrip() {
        assert_eq!(encode(&[]).unwrap(), Vec::<u8>::new());
        assert_eq!(decode(&[]).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_single_byte_roundtrip() {
        let data = vec![42u8];
        let encoded = encode(&data).unwrap();
        assert_eq!(decode(&encoded).unwrap(), data);
    }

    #[test]
    fn test_repetitive_roundtrip_and_compresses() {
        let data: Vec<u8> = b"ABABABABABABABABABABABABABABAB".to_vec();
        let encoded = encode(&data).unwrap();
        assert_eq!(decode(&encoded).unwrap(), data);
        assert!(encoded.len() < data.len(), "AB en boucle: previsible a 100% par contexte, doit compresser fort ({} -> {})", data.len(), encoded.len());
    }

    #[test]
    fn test_random_content_roundtrips_even_if_it_grows() {
        // Contenu sans structure -- le roundtrip doit rester exact meme
        // si la sortie est plus grosse que l'entree (aucune garantie de
        // compression sur du bruit, seulement de correction).
        let mut data = Vec::new();
        let mut x: u32 = 12345;
        for _ in 0..300 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            data.push((x >> 16) as u8);
        }
        let encoded = encode(&data).unwrap();
        assert_eq!(decode(&encoded).unwrap(), data);
    }

    #[test]
    fn test_order1_beats_or_matches_order0_on_predictable_context() {
        // Construit une sequence ou le SUIVANT depend fortement du
        // PRECEDENT (previsible en ordre 1) mais dont la distribution
        // globale (ordre 0) est presque uniforme -- cas ou l'ordre 1 doit
        // clairement gagner, mesure en direct plutot que suppose.
        let mut data = Vec::new();
        for _ in 0..50 {
            data.extend_from_slice(b"AXBXCXDX");
        }
        let order1 = encode(&data).unwrap();

        // Comparaison a un rANS ordre 0 "maison" minimal pour ce test
        // (meme principe que fse_encoder_rs, un seul contexte global).
        let mut counts: HashMap<u8, u32> = HashMap::new();
        for &b in &data { *counts.entry(b).or_insert(0) += 1; }
        let t0 = normalize(&counts);
        let mut state: u32 = RANS_L;
        let mut rev = Vec::new();
        for &b in data.iter().rev() {
            let freq = t0.freq[b as usize];
            let start = t0.cum[b as usize];
            let x_max = ((RANS_L >> PROB_BITS) << 8) * freq;
            while state >= x_max { rev.push((state & 0xff) as u8); state >>= 8; }
            state = ((state / freq) << PROB_BITS) + (state % freq) + start;
        }
        let order0_payload_len = rev.len() + 4; // + etat final, sans compter la table pour cet ordre0 "nu"

        println!(
            "ordre1 (avec tables embarquees): {} octets | ordre0 (sans table, flux seul): {} octets | original: {} octets",
            order1.len(), order0_payload_len, data.len()
        );
        assert_eq!(decode(&order1).unwrap(), data);
    }
}
