//! ANS Decoder - FSE with variable-length bitstream
//! Reconstructs full state from bitstream + final state bytes

use super::ans_table::{ANSTable, ANS_RANGE_START};

pub struct ANSDecoder {
    table: ANSTable,
    state: u64,
    bitstream: Vec<u8>,
    bitstream_idx: usize,  // Index into bitstream for byte restoration
}

impl ANSDecoder {
    pub fn new(table: ANSTable, compressed: &[u8]) -> Result<Self, String> {
        if compressed.len() < 14 {
            return Err("Compressed data too short (minimum 14 bytes)".to_string());
        }

        if &compressed[0..4] != b"ANS\x01" {
            return Err("Invalid ANS marker".to_string());
        }

        // Parse variable-length bitstream
        let bitstream_len = compressed[4] as usize | ((compressed[5] as usize) << 8);
        let bitstream_end = 6 + bitstream_len;

        if bitstream_end + 8 > compressed.len() {
            return Err("Truncated compressed data".to_string());
        }

        // Extract bitstream
        let bitstream = compressed[6..bitstream_end].to_vec();

        // Load final state from last 8 bytes (little-endian)
        let mut state = 0u64;
        for i in 0..8 {
            state |= (compressed[bitstream_end + i] as u64) << (i * 8);
        }

        // The state stored is the final state after encoding.
        // Bytes are extracted bytes that will be restored DURING decoding as needed.
        // Restore bytes from the beginning of bitstream until state >= ANS_RANGE_START,
        // but do it incrementally to avoid overflow.
        let mut bitstream_idx = 0;
        while state < ANS_RANGE_START as u64 && bitstream_idx < bitstream.len() {
            let byte = bitstream[bitstream_idx] as u64;
            state = (state << 8) | byte;
            bitstream_idx += 1;
        }

        if state < ANS_RANGE_START as u64 {
            return Err(format!("State {} below minimum {} after restoring bitstream", state, ANS_RANGE_START));
        }

        Ok(Self {
            table,
            state,
            bitstream,
            bitstream_idx,  // Start reading from where we left off
        })
    }

    /// Décoder un symbole
    pub fn decode_symbol(&mut self) -> Result<u8, String> {
        if self.state < ANS_RANGE_START as u64 {
            return Err(format!("Invalid state: {}", self.state));
        }

        // Lookup symbole
        let lookup_idx = (self.state % ANS_RANGE_START as u64) as usize;
        if lookup_idx >= self.table.decode_table.len() {
            return Err("Decode table index out of bounds".to_string());
        }

        let symbol = self.table.decode_table[lookup_idx];
        let (l, _h) = self.table.get_symbol_range(symbol);
        let freq = self.table.freq[symbol as usize];

        if freq == 0 {
            return Err(format!("Invalid frequency for symbol {}", symbol));
        }

        // Inverse transformation
        let q = self.state / ANS_RANGE_START as u64;
        let r = (self.state % ANS_RANGE_START as u64) as u32;

        if r < l {
            return Err(format!("Invalid remainder: {} < {}", r, l));
        }

        let new_state = q.checked_mul(freq as u64)
            .and_then(|prod| prod.checked_add((r - l) as u64));

        match new_state {
            Some(state) => {
                self.state = state;

                // Extract bytes if state drops below Ns (error in logic, shouldn't happen)
                while self.state < ANS_RANGE_START as u64 && self.bitstream_idx < self.bitstream.len() {
                    let byte = self.bitstream[self.bitstream_idx] as u64;
                    self.state = (self.state << 8) | byte;
                    self.bitstream_idx += 1;
                }

                Ok(symbol)
            }
            None => Err(format!("State overflow during decode: q={}, freq={}, r={}, l={}", q, freq, r, l))
        }
    }

    /// Décoder N symboles
    pub fn decode_sequence(&mut self, count: usize) -> Result<Vec<u8>, String> {
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(self.decode_symbol()?);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::ans_encoder::ANSEncoder;

    #[test]
    fn test_decode_after_encode() {
        let table = ANSTable::build_cstl_default();

        let mut encoder = ANSEncoder::new(table.clone());
        let original = vec![0x01, 0x02, 0x03];
        encoder.encode_sequence(&original);
        let compressed = encoder.finalize();

        let mut decoder = ANSDecoder::new(table, &compressed)
            .expect("Failed to create decoder");

        let decoded = decoder.decode_sequence(3)
            .expect("Failed to decode");

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_invalid_marker() {
        let table = ANSTable::build_cstl_default();
        let invalid = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x08, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
        let result = ANSDecoder::new(table, &invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_data() {
        let table = ANSTable::build_cstl_default();
        let truncated = vec![0x41, 0x4E, 0x53, 0x01, 0x08, 0x00];
        let result = ANSDecoder::new(table, &truncated);
        assert!(result.is_err());
    }

    #[test]
    fn test_round_trip_5_symbols() {
        let table = ANSTable::build_cstl_default();

        let mut encoder = ANSEncoder::new(table.clone());
        let original: Vec<u8> = (0..5)
            .map(|i| ((i % 10) + 0x01) as u8)
            .collect();

        encoder.encode_sequence(&original);
        let compressed = encoder.finalize();

        let mut decoder = ANSDecoder::new(table, &compressed)
            .expect("Failed to create decoder");

        let decoded = decoder.decode_sequence(original.len())
            .expect("Failed to decode");

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_wire_format_variable_bitstream() {
        // Validate wire format with variable-length bitstream
        let table = ANSTable::build_cstl_default();
        let mut encoder = ANSEncoder::new(table.clone());

        let symbols = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        encoder.encode_sequence(&symbols);
        let output = encoder.finalize();

        // Verify structure
        assert_eq!(&output[0..4], b"ANS\x01");
        let bitstream_len = output[4] as usize | ((output[5] as usize) << 8);
        let expected_len = 4 + 2 + bitstream_len + 8;
        assert_eq!(output.len(), expected_len);

        // Verify we can parse it
        let _decoder = ANSDecoder::new(table, &output)
            .expect("Should parse valid wire format");
    }

}
