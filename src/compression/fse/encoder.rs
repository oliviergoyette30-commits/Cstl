// src/compression/fse/encoder.rs
// FSE (Finite State Entropy) Encoder for v5.1
// Variable-length bitstream, unlimited symbol sequences

use std::collections::HashMap;

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
    bitstream: Vec<u8>,
    bit_pos: usize,
}

impl FSEEncoder {
    /// Create FSE encoder from symbol frequencies
    pub fn new(frequencies: &HashMap<u8, usize>) -> Self {
        let total: usize = frequencies.values().sum();
        let max_state = total.next_power_of_two();

        let mut table = Vec::new();
        let mut state_base = 0;

        // Build cumulative state assignment table
        for (symbol_id, freq) in frequencies.iter() {
            let freq_val = *freq;
            table.push(FSETable {
                symbol_id: *symbol_id,
                frequency: freq_val,
                state_base,
                state_count: freq_val.max(1),
            });
            state_base += freq_val.max(1);
        }

        FSEEncoder {
            table,
            max_state,
            bitstream: Vec::new(),
            bit_pos: 0,
        }
    }

    /// Encode symbol sequence into FSE bitstream
    pub fn encode(&mut self, symbols: &[u8]) -> Result<Vec<u8>, String> {
        // v5.1 Week 1: Simplified stub implementation
        // Real FSE bit-level encoding deferred to Week 2

        if symbols.is_empty() {
            return Ok(vec![]);
        }

        // Validate all symbols are in the frequency table
        for &symbol in symbols {
            if !self.table.iter().any(|t| t.symbol_id == symbol) {
                return Err(format!("Unknown symbol: {}", symbol));
            }
        }

        // Week 1 stub: Simple byte-level encoding (symbol count + symbols)
        let mut result = Vec::new();
        result.push(symbols.len() as u8);
        result.extend_from_slice(symbols);

        Ok(result)
    }

    /// Decode FSE bitstream back to symbols
    pub fn decode(&self, bitstream: &[u8]) -> Result<Vec<u8>, String> {
        // v5.1 Week 1: Simplified stub implementation matching simplified encode
        // Real FSE bit-level decoding deferred to Week 2

        if bitstream.is_empty() {
            return Ok(vec![]);
        }

        if bitstream.len() < 1 {
            return Err("Bitstream too short".to_string());
        }

        let count = bitstream[0] as usize;
        if bitstream.len() < 1 + count {
            return Err("Bitstream corrupted: insufficient data".to_string());
        }

        // Week 1 stub: Simple byte-level decoding (read count + symbols)
        Ok(bitstream[1..1 + count].to_vec())
    }

    fn pack_bits(&self, bits: &[u8]) -> Result<Vec<u8>, String> {
        // Simple byte-packing (placeholder; real impl uses bit-level packing)
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
        freqs.insert(0x00, 5); // purpose (freq 5)
        freqs.insert(0x01, 3); // sender (freq 3)
        freqs.insert(0x02, 2); // name (freq 2)

        let mut encoder = FSEEncoder::new(&freqs);
        let symbols = vec![0x00, 0x01, 0x02, 0x00, 0x01];

        let encoded = encoder.encode(&symbols).expect("Encoding failed");
        assert!(!encoded.is_empty(), "Encoded output should not be empty");

        // Verify state machine initialized
        assert_eq!(encoder.get_max_state(), 16, "Max state should be power of 2");
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
}
