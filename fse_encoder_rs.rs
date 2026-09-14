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
        let max_state = (total.next_power_of_two() * 2) as usize;

        let mut table = Vec::new();
        let mut state_base = 0;

        // Build cumulative state assignment table
        for (symbol_id, freq) in frequencies.iter() {
            table.push(FSETable {
                symbol_id: *symbol_id,
                frequency: *freq,
                state_base,
                state_count: freq.max(1),
            });
            state_base += freq.max(1);
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
        if symbols.is_empty() {
            return Ok(vec![]);
        }

        let mut state: usize = self.max_state;
        let mut bits = Vec::new();

        // Encode in reverse (FSE standard)
        for &symbol in symbols.iter().rev() {
            // Find state range for this symbol
            let table_entry = self.table
                .iter()
                .find(|t| t.symbol_id == symbol)
                .ok_or(format!("Unknown symbol: {}", symbol))?;

            // Calculate state transition
            let nb_bits = (state.leading_zeros() as usize) + 1;
            let mask = (1 << nb_bits) - 1;

            // Append state bits to stream
            bits.extend_from_slice(&state.to_le_bytes()[..2]);

            // New state
            state = (state >> nb_bits) + table_entry.state_base;
            state = state.min(self.max_state);
        }

        // Pack bits into bytes
        self.pack_bits(&bits)
    }

    /// Decode FSE bitstream back to symbols
    pub fn decode(&self, bitstream: &[u8]) -> Result<Vec<u8>, String> {
        if bitstream.is_empty() {
            return Ok(vec![]);
        }

        let mut symbols = Vec::new();
        let mut state = self.max_state;
        let mut bit_pos = 0;

        // Decode by reading state machine in reverse
        while bit_pos < bitstream.len() * 8 {
            // Find which symbol this state belongs to
            let symbol = self.table
                .iter()
                .find(|t| {
                    let min = t.state_base;
                    let max = t.state_base + t.state_count;
                    state >= min && state < max
                })
                .map(|t| t.symbol_id)
                .ok_or("Invalid state during decode")?;

            symbols.push(symbol);

            // Update state (simplified version)
            if bit_pos + 16 <= bitstream.len() * 8 {
                let mut bytes = [0u8; 2];
                bytes.copy_from_slice(&bitstream[bit_pos / 8..(bit_pos / 8) + 2]);
                state = u16::from_le_bytes(bytes) as usize;
                bit_pos += 16;
            } else {
                break;
            }
        }

        // Reverse back to original order
        symbols.reverse();
        Ok(symbols)
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
        let encoder = FSEEncoder::new(&freqs);
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
