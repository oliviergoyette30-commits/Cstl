// src/compression/fse/encoder.rs
// FSE (Finite State Entropy) Encoder -- rANS avec table de frequences
// FOURNIE PAR L'APPELANT (pas embarquee dans le flux), a la difference de
// `compression::fse_encoder_rs::FseEncoder` (table adaptative, embarquee).
//
// 2026-09-27 -- REECRITURE. Etat trouve avant reecriture (verifie en
// lisant le fichier, pas suppose) : `encode()` etait explicitement
// commente "v5.1 Week 1: Simplified stub implementation... Real FSE
// bit-level encoding deferred to Week 2" -- la "Week 2" n'a jamais eu
// lieu. Le "stub" ecrivait juste `[longueur, ...symboles bruts]`, zero
// compression (expansion de 1 octet par l'en-tete de longueur). Le test
// `test_fse_roundtrip` ne verifiait meme pas l'egalite decode(encode(x))
// == x -- il checkait seulement que la sortie n'est pas vide et que
// `max_state == 16`. Un encodeur qui ne compresse jamais rien passait ce
// test aussi bien qu'un vrai.
//
// Ce fichier implemente maintenant un vrai rANS byte-oriented (meme
// moteur mathematique que `fse_encoder_rs.rs`, voir ses commentaires pour
// le detail de l'algorithme et le choix rANS-plutot-que-tANS). Difference
// deliberee avec l'autre module : ICI la table de frequences est fournie
// par l'appelant a la construction (`FSEEncoder::new(&frequencies)`) et
// n'est PAS reserialisee dans le flux compresse -- plus compact (utile
// quand la meme table sert a beaucoup de petits messages, ex. un
// dictionnaire de session partage), mais implique une contrainte reelle a
// documenter : encodeur et decodeur DOIVENT utiliser exactement la meme
// table, sous peine de decoder du bruit sans erreur detectable dans le cas
// general. `fse_encoder_rs::FseEncoder` (table adaptative embarquee) reste
// le choix par defaut recommande tant qu'aucune synchronisation de table
// fiable n'est cablee ailleurs dans le projet.

use std::collections::HashMap;

const PROB_BITS: u32 = 12;
const PROB_SCALE: u32 = 1 << PROB_BITS;
const RANS_L: u32 = 1 << 23;

#[derive(Clone, Debug)]
pub struct FSETable {
    symbol_id: u8,
    frequency: usize,
    state_base: usize,
    state_count: usize,
}

pub struct FSEEncoder {
    table: Vec<FSETable>,
    max_state: usize,
    /// Frequences normalisees a somme PROB_SCALE, indexees par octet.
    norm_freq: [u32; 256],
    norm_cum: [u32; 257],
    bitstream: Vec<u8>,
    bit_pos: usize,
}

impl FSEEncoder {
    /// Create FSE encoder from symbol frequencies. Les frequences fournies
    /// n'ont pas besoin de sommer a une puissance de 2 -- normalisees en
    /// interne (meme methode du plus grand reste que `fse_encoder_rs.rs`).
    pub fn new(frequencies: &HashMap<u8, usize>) -> Self {
        let total: usize = frequencies.values().sum();
        let max_state = total.next_power_of_two().max(1);

        let mut table = Vec::new();
        let mut state_base = 0;
        for (&symbol_id, &freq_val) in frequencies.iter() {
            table.push(FSETable {
                symbol_id,
                frequency: freq_val,
                state_base,
                state_count: freq_val.max(1),
            });
            state_base += freq_val.max(1);
        }

        let (norm_freq, norm_cum) = Self::normalize(frequencies);

        FSEEncoder {
            table,
            max_state,
            norm_freq,
            norm_cum,
            bitstream: Vec::new(),
            bit_pos: 0,
        }
    }

    fn normalize(frequencies: &HashMap<u8, usize>) -> ([u32; 256], [u32; 257]) {
        let mut freq = [0u32; 256];
        if frequencies.is_empty() {
            let mut cum = [0u32; 257];
            for i in 0..=256 {
                cum[i] = 0;
            }
            return (freq, cum);
        }
        let total: u64 = frequencies.values().map(|&v| v as u64).sum::<u64>().max(1);

        let mut remainders: Vec<(usize, u64)> = Vec::new();
        let mut assigned: u32 = 0;
        for (&sym, &raw) in frequencies.iter() {
            let raw = raw.max(1) as u64;
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
                    if i >= remainders.len() {
                        i = remainders.len() - 1;
                    }
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

        (freq, cum)
    }

    fn symbol_for_slot(&self, slot: u32) -> Result<u8, String> {
        for sym in 0..256 {
            if slot >= self.norm_cum[sym] && slot < self.norm_cum[sym + 1] {
                return Ok(sym as u8);
            }
        }
        Err(format!("slot {} hors de toute plage de la table normalisee", slot))
    }

    /// Encode symbol sequence into a real rANS bitstream (table externe,
    /// NON reserialisee -- voir doc de module). Chaque symbole de `symbols`
    /// doit avoir ete present dans la table de frequences fournie a `new`.
    pub fn encode(&mut self, symbols: &[u8]) -> Result<Vec<u8>, String> {
        if symbols.is_empty() {
            return Ok(vec![]);
        }

        for &symbol in symbols {
            if !self.table.iter().any(|t| t.symbol_id == symbol) {
                return Err(format!("Unknown symbol: {}", symbol));
            }
        }

        let mut state: u32 = RANS_L;
        let mut rev_bytes: Vec<u8> = Vec::new();

        for &b in symbols.iter().rev() {
            let sym = b as usize;
            let freq = self.norm_freq[sym];
            let start = self.norm_cum[sym];
            if freq == 0 {
                return Err(format!("symbol {} has zero normalized frequency", b));
            }
            let x_max = ((RANS_L >> PROB_BITS) << 8) * freq;
            while state >= x_max {
                rev_bytes.push((state & 0xff) as u8);
                state >>= 8;
            }
            state = ((state / freq) << PROB_BITS) + (state % freq) + start;
        }

        let mut out = Vec::new();
        out.extend_from_slice(&(symbols.len() as u32).to_le_bytes());
        out.extend_from_slice(&state.to_le_bytes());
        rev_bytes.reverse();
        out.extend_from_slice(&rev_bytes);

        self.bitstream = out.clone();
        Ok(out)
    }

    /// Decode FSE bitstream back to symbols. Exige la MEME table de
    /// frequences que celle utilisee pour `encode` (fournie a `new` sur
    /// l'instance qui decode) -- ce n'est pas verifie automatiquement,
    /// voir avertissement en tete de fichier.
    pub fn decode(&self, bitstream: &[u8]) -> Result<Vec<u8>, String> {
        if bitstream.is_empty() {
            return Ok(vec![]);
        }
        if bitstream.len() < 8 {
            return Err("Bitstream too short".to_string());
        }

        let count = u32::from_le_bytes([bitstream[0], bitstream[1], bitstream[2], bitstream[3]]) as usize;
        let mut state = u32::from_le_bytes([bitstream[4], bitstream[5], bitstream[6], bitstream[7]]);
        let mut byte_pos = 8usize;
        let mask = PROB_SCALE - 1;

        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let slot = state & mask;
            let sym = self.symbol_for_slot(slot)?;
            let freq = self.norm_freq[sym as usize];
            let start = self.norm_cum[sym as usize];
            state = freq * (state >> PROB_BITS) + slot - start;

            while state < RANS_L {
                if byte_pos >= bitstream.len() {
                    return Err("Bitstream corrupted: insufficient data".to_string());
                }
                state = (state << 8) | (bitstream[byte_pos] as u32);
                byte_pos += 1;
            }
            out.push(sym);
        }

        Ok(out)
    }

    fn pack_bits(&self, bits: &[u8]) -> Result<Vec<u8>, String> {
        // Conserve tel quel -- non utilise par le chemin rANS ci-dessus
        // (garde-fou de compat si du code externe l'appelait encore).
        Ok(bits.to_vec())
    }

    pub fn get_max_state(&self) -> usize {
        self.max_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fse_roundtrip() {
        let mut freqs = HashMap::new();
        freqs.insert(0x00, 50); // purpose
        freqs.insert(0x01, 30); // sender
        freqs.insert(0x02, 20); // name

        let mut encoder = FSEEncoder::new(&freqs);
        let symbols = vec![0x00, 0x01, 0x02, 0x00, 0x01, 0x00, 0x00, 0x01, 0x02, 0x00];

        let encoded = encoder.encode(&symbols).expect("Encoding failed");
        assert!(!encoded.is_empty(), "Encoded output should not be empty");

        let decoded = encoder.decode(&encoded).expect("Decoding failed");
        assert_eq!(decoded, symbols, "roundtrip doit restituer exactement les symboles d'origine");

        assert_eq!(encoder.get_max_state(), 128, "Max state should be power of 2 >= total freq (100)");
    }

    #[test]
    fn test_fse_empty_input() {
        let freqs = HashMap::new();
        let mut encoder = FSEEncoder::new(&freqs);
        let result = encoder.encode(&[]).expect("Should handle empty");
        assert!(result.is_empty());
    }

    #[test]
    fn test_fse_frequency_table() {
        let mut freqs = HashMap::new();
        freqs.insert(0x00, 5);
        freqs.insert(0x01, 3);

        let encoder = FSEEncoder::new(&freqs);
        assert!(encoder.table.len() >= 2, "Table should have entries");
    }

    #[test]
    fn test_fse_rejects_unknown_symbol() {
        let mut freqs = HashMap::new();
        freqs.insert(0x00, 5);
        let mut encoder = FSEEncoder::new(&freqs);
        assert!(encoder.encode(&[0x00, 0xFF]).is_err());
    }

    #[test]
    fn test_fse_compresses_skewed_distribution() {
        let mut freqs = HashMap::new();
        freqs.insert(b'A', 90);
        freqs.insert(b'B', 9);
        freqs.insert(b'C', 1);

        let mut encoder = FSEEncoder::new(&freqs);
        // Genere une sequence conforme a la distribution declaree.
        let mut symbols = Vec::new();
        symbols.extend(std::iter::repeat(b'A').take(900));
        symbols.extend(std::iter::repeat(b'B').take(90));
        symbols.extend(std::iter::repeat(b'C').take(10));

        let encoded = encoder.encode(&symbols).unwrap();
        let decoded = encoder.decode(&encoded).unwrap();
        assert_eq!(decoded, symbols);
        assert!(encoded.len() < symbols.len() / 4,
            "distribution tres skewed (90% A) doit compresser fortement: {} -> {}",
            symbols.len(), encoded.len());
    }
}
