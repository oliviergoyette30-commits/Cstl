//! ANS Table Construction
//! État: [Ns, 2Ns) où chaque plage représente un symbole
//! Normalisation de fréquences pour mapping déterministe

use std::collections::BTreeMap;

pub const ANS_RANGE_START: u32 = 65536;  // Ns = 2^16
pub const ANS_RANGE_END: u32 = 131072;   // 2Ns = 2^17

#[derive(Clone)]
pub struct ANSTable {
    /// Fréquence normalisée pour chaque symbole (0-255)
    pub freq: Vec<u32>,

    /// Fréquences cumulées: cumul_freq[s] = somme freq[0..s-1]
    pub cumul_freq: Vec<u32>,

    /// Table de décodage: pour état X ∈ [0, Ns), quel symbole?
    pub decode_table: Vec<u8>,

    /// Symboles actifs (fréquence > 0)
    pub symbols: Vec<u8>,

    /// Total fréquence (devrait être = ANS_RANGE_START)
    pub total_freq: u32,
}

impl ANSTable {
    /// Construire ANS table depuis fréquences brutes (256 symboles)
    pub fn build_from_frequencies(raw_freqs: &[u32; 256]) -> Self {
        // Étape 1: Normaliser en s'assurant que la somme = ANS_RANGE_START
        let total_freq: u32 = raw_freqs.iter().sum();
        if total_freq == 0 {
            panic!("No symbols with non-zero frequency");
        }

        let mut norm_freqs = vec![0u32; 256];
        let mut active_symbols = Vec::new();

        for (sym, &raw_freq) in raw_freqs.iter().enumerate() {
            if raw_freq > 0 {
                // Normaliser en s'assurant que la somme = ANS_RANGE_START
                let norm = std::cmp::max(1u32,
                    ((raw_freq as u64 * ANS_RANGE_START as u64) / total_freq as u64) as u32
                );
                norm_freqs[sym] = norm;
                active_symbols.push(sym as u8);
            }
        }

        // Vérifier que la somme des fréquences normalisées ≈ ANS_RANGE_START
        let sum: u32 = norm_freqs.iter().sum();
        if sum != ANS_RANGE_START {
            // Ajuster le dernier symbole pour que la somme soit exactement ANS_RANGE_START
            if let Some(&last_sym) = active_symbols.last() {
                let diff = ANS_RANGE_START as i32 - sum as i32;
                let adjusted = (norm_freqs[last_sym as usize] as i32 + diff) as u32;
                norm_freqs[last_sym as usize] = adjusted;
            }
        }

        // Étape 2: Construire fréquences cumulées
        let mut cumul_freq = vec![0u32; 256];
        let mut cumul = 0u32;
        for sym in 0..256 {
            cumul_freq[sym] = cumul;
            cumul += norm_freqs[sym];
        }
        let total = cumul;

        // Étape 3: Construire decode_table
        // Pour chaque X ∈ [0, Ns), quel symbole?
        let mut decode_table = vec![0u8; ANS_RANGE_START as usize];
        for sym in 0..256 {
            if norm_freqs[sym] == 0 {
                continue;
            }
            let start = cumul_freq[sym];
            let end = start + norm_freqs[sym];
            for x in start..std::cmp::min(end, ANS_RANGE_START) {
                decode_table[x as usize] = sym as u8;
            }
        }

        Self {
            freq: norm_freqs,
            cumul_freq,
            decode_table,
            symbols: active_symbols,
            total_freq: total,
        }
    }

    /// Construire depuis fréquences de symboles spécifiques (par défaut CSTL)
    pub fn build_cstl_default() -> Self {
        let mut freqs = [0u32; 256];

        // Métastructure CSTL (tokens 0x01-0x20): 50% de la distribution
        // Zipfienne: 0x01 >> 0x10 >> 0x03
        let meta_freqs = vec![
            (0x01, 300),  // encoder, très fréquent
            (0x02, 200),  // timestamp
            (0x03, 150),  // metadata
            (0x04, 100),  // intent_payload
            (0x05, 100),  // extra
            (0x10, 200),  // relation
            (0x11, 150),  // capabilities
            (0x12, 100),  // trust_score
            (0x20, 80),   // queue
        ];

        for (sym, freq) in meta_freqs {
            freqs[sym as usize] = freq;
        }

        // Sémantique (0x21-0x84): 35% distribution, bimodale
        let sem_freqs = vec![
            (0x21, 200),  // define
            (0x22, 180),  // relation
            (0x30, 100),  // modality
            (0x31, 90),   // value
            (0x40, 150),  // purpose
            (0x50, 100),  // signature
        ];

        for (sym, freq) in sem_freqs {
            freqs[sym as usize] = freq;
        }

        // Données brutes (0x85+): 15% distribution, uniforme
        for sym in 0x85..0xFF {
            freqs[sym as usize] = 20;
        }

        Self::build_from_frequencies(&freqs)
    }

    /// Trouver plage [L, H) pour un symbole
    pub fn get_symbol_range(&self, symbol: u8) -> (u32, u32) {
        let start = self.cumul_freq[symbol as usize];
        let end = start + self.freq[symbol as usize];
        (start, end)
    }

    /// Traduire état X (quelconque) vers état normalisé dans [Ns, 2Ns)
    pub fn normalize_state(&self, state: u64, symbol: u8) -> u64 {
        let freq = self.freq[symbol as usize];
        if freq == 0 {
            return state;
        }

        // Réduire état tant qu'il dépasse la plage de sécurité
        let max_state_allowed = (u64::MAX / freq as u64) * freq as u64;
        let mut result = state;
        while result > max_state_allowed {
            result >>= 8;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ans_table_construction() {
        let mut freqs = [0u32; 256];
        freqs[0x01] = 100;
        freqs[0x02] = 50;
        freqs[0x03] = 30;

        let table = ANSTable::build_from_frequencies(&freqs);

        // Vérifier symboles actifs
        assert!(table.symbols.contains(&0x01));
        assert!(table.symbols.contains(&0x02));
        assert!(table.symbols.contains(&0x03));
        assert_eq!(table.freq[0x00], 0);  // Inactif

        // Vérifier que la somme des fréquences = ANS_RANGE_START
        let sum: u32 = table.freq.iter().sum();
        assert_eq!(sum, ANS_RANGE_START);
    }

    #[test]
    fn test_symbol_range_non_overlapping() {
        let mut freqs = [0u32; 256];
        for i in 0..3 {
            freqs[i] = 100;
        }

        let table = ANSTable::build_from_frequencies(&freqs);
        let (l0, h0) = table.get_symbol_range(0);
        let (l1, h1) = table.get_symbol_range(1);
        let (l2, h2) = table.get_symbol_range(2);

        // Plages non-chevauchantes
        assert!(h0 <= l1);
        assert!(h1 <= l2);
    }

    #[test]
    fn test_cstl_default_table() {
        let table = ANSTable::build_cstl_default();

        // Vérifier que les symboles CSTL importants sont présents
        assert!(table.symbols.contains(&0x01));  // encoder
        assert!(table.symbols.contains(&0x21));  // define
        assert!(table.symbols.contains(&0x40));  // purpose

        // Vérifier que la somme des fréquences = ANS_RANGE_START
        let sum: u32 = table.freq.iter().sum();
        assert_eq!(sum, ANS_RANGE_START);
    }

    #[test]
    fn test_decode_table_covers_range() {
        let table = ANSTable::build_cstl_default();

        // Chaque position dans [0, Ns) doit avoir un symbole
        for x in 0..ANS_RANGE_START {
            let sym = table.decode_table[x as usize];
            assert!(table.freq[sym as usize] > 0, "Invalid symbol at index {}", x);
        }
    }
}
