use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum FseEncoderError {
    AlphabetSizeExceeded,
    InvalidStateTransition,
    TableGenerationFailed(String),
    EncodingFailed(String),
    DecodingFailed(String),
    InvalidStateRange,
}

impl fmt::Display for FseEncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FseEncoderError::AlphabetSizeExceeded => write!(f, "Alphabet size exceeded"),
            FseEncoderError::InvalidStateTransition => write!(f, "Invalid state transition"),
            FseEncoderError::TableGenerationFailed(e) => write!(f, "Table generation failed: {}", e),
            FseEncoderError::EncodingFailed(e) => write!(f, "Encoding failed: {}", e),
            FseEncoderError::DecodingFailed(e) => write!(f, "Decoding failed: {}", e),
            FseEncoderError::InvalidStateRange => write!(f, "Invalid state range"),
        }
    }
}

impl std::error::Error for FseEncoderError {}

#[derive(Clone)]
pub struct TansTable {
    state_size: u16,
    table: Vec<TansSymbol>,
}

#[derive(Clone, Debug)]
struct TansSymbol {
    next_state: u16,
    bits: u8,
    value: u8,
}

pub struct PretrainedTans {
    frequency_table: HashMap<u8, u32>,
    state_range: (u16, u16),
    cumulative: Vec<u32>,
}

impl PretrainedTans {
    pub fn new() -> Self {
        let mut freq = HashMap::new();
        freq.insert(b'M', 187u32);
        freq.insert(b'U', 156u32);
        freq.insert(b'A', 203u32);
        freq.insert(b'T', 198u32);
        freq.insert(b'D', 167u32);
        freq.insert(b'E', 201u32);
        freq.insert(b'R', 172u32);
        freq.insert(b'O', 168u32);
        freq.insert(b'I', 145u32);
        freq.insert(b'S', 189u32);

        PretrainedTans {
            frequency_table: freq,
            state_range: (256, 512),
            cumulative: Vec::new(),
        }
    }

    pub fn compute_cumulative_frequencies(&mut self) -> Result<(), FseEncoderError> {
        let mut sorted_syms: Vec<(u8, u32)> = self.frequency_table.iter()
            .map(|(&k, &v)| (k, v))
            .collect();
        sorted_syms.sort_by_key(|x| x.0);

        self.cumulative.clear();
        let mut cum = 0u32;
        for (_, freq) in &sorted_syms {
            self.cumulative.push(cum);
            cum += freq;
        }

        Ok(())
    }

    pub fn generate_state_table(&self) -> Result<TansTable, FseEncoderError> {
        if self.cumulative.is_empty() {
            return Err(FseEncoderError::TableGenerationFailed(
                "Cumulative frequencies not computed".into()
            ));
        }

        let state_size = self.state_range.1 - self.state_range.0;
        let mut table = vec![
            TansSymbol {
                next_state: 0,
                bits: 0,
                value: 0,
            };
            state_size as usize
        ];

        let mut sym_idx = 0;
        let mut sorted_syms: Vec<(u8, u32)> = self.frequency_table.iter()
            .map(|(&k, &v)| (k, v))
            .collect();
        sorted_syms.sort_by_key(|x| x.0);

        for state_idx in 0..(state_size as usize) {
            if sym_idx >= sorted_syms.len() {
                sym_idx = 0;
            }

            let (_sym, _freq) = sorted_syms[sym_idx];
            let bits = if state_size > 256 { 1 } else { 0 };
            let next_state = ((state_idx + 1) % (state_size as usize)) as u16 + self.state_range.0;

            table[state_idx] = TansSymbol {
                next_state,
                bits,
                value: sorted_syms[sym_idx].0,
            };

            sym_idx += 1;
        }

        Ok(TansTable {
            state_size,
            table,
        })
    }
}

pub struct SharedSessionState {
    dynamic_slots: HashMap<u16, Vec<u8>>,
    slot_count: u16,
}

impl SharedSessionState {
    pub fn new() -> Self {
        SharedSessionState {
            dynamic_slots: HashMap::new(),
            slot_count: 0,
        }
    }

    pub fn inject_dynamic(&mut self, slot_id: u16, data: Vec<u8>) -> Result<(), FseEncoderError> {
        if slot_id >= 256 {
            return Err(FseEncoderError::InvalidStateRange);
        }
        self.dynamic_slots.insert(slot_id, data);
        self.slot_count += 1;
        Ok(())
    }

    pub fn retrieve_dynamic(&self, slot_id: u16) -> Option<&Vec<u8>> {
        self.dynamic_slots.get(&slot_id)
    }

    pub fn is_full(&self) -> bool {
        self.slot_count >= 256
    }
}

pub struct FseEncoder {
    pretrained_tans: PretrainedTans,
    session_state: SharedSessionState,
    encoding_buffer: Vec<u8>,
}

impl FseEncoder {
    pub fn new() -> Self {
        FseEncoder {
            pretrained_tans: PretrainedTans::new(),
            session_state: SharedSessionState::new(),
            encoding_buffer: Vec::new(),
        }
    }

    pub fn initialize(&mut self) -> Result<(), FseEncoderError> {
        self.pretrained_tans.compute_cumulative_frequencies()?;
        Ok(())
    }

    pub fn inject_session_amortization(&mut self, key: Vec<u8>) -> Result<(), FseEncoderError> {
        let slot_id = self.session_state.slot_count;
        self.session_state.inject_dynamic(slot_id, key)?;
        Ok(())
    }

    pub fn encode(&mut self, data: &[u8]) -> Result<Vec<u8>, FseEncoderError> {
        self.encoding_buffer.clear();

        self.encoding_buffer.extend_from_slice(&[0x46, 0xFE, 0x00]);

        for byte in data {
            if self.pretrained_tans.frequency_table.contains_key(byte) {
                self.encoding_buffer.push(*byte);
            } else {
                self.encoding_buffer.push(0x00);
                self.encoding_buffer.push(*byte);
            }
        }

        Ok(self.encoding_buffer.clone())
    }

    pub fn decode(&self, data: &[u8]) -> Result<Vec<u8>, FseEncoderError> {
        if data.len() < 3 || data[0] != 0x46 || data[1] != 0xFE {
            return Err(FseEncoderError::DecodingFailed("Invalid FSE magic".into()));
        }

        let mut result = Vec::new();
        let mut i = 3;

        while i < data.len() {
            if data[i] == 0x00 && i + 1 < data.len() {
                result.push(data[i + 1]);
                i += 2;
            } else {
                result.push(data[i]);
                i += 1;
            }
        }

        Ok(result)
    }

    pub fn get_state_size(&self) -> (u16, u16) {
        self.pretrained_tans.state_range
    }

    pub fn session_state_full(&self) -> bool {
        self.session_state.is_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pretrained_tans_creation() {
        let tans = PretrainedTans::new();
        assert!(!tans.frequency_table.is_empty());
        assert_eq!(tans.state_range, (256, 512));
    }

    #[test]
    fn test_pretrained_tans_cumulative() {
        let mut tans = PretrainedTans::new();
        let result = tans.compute_cumulative_frequencies();
        assert!(result.is_ok());
        assert!(!tans.cumulative.is_empty());
    }

    #[test]
    fn test_tans_table_generation() {
        let mut tans = PretrainedTans::new();
        tans.compute_cumulative_frequencies().unwrap();
        let table_result = tans.generate_state_table();
        assert!(table_result.is_ok());
        let table = table_result.unwrap();
        assert!(table.state_size > 0);
        assert!(!table.table.is_empty());
    }

    #[test]
    fn test_shared_session_state_inject() {
        let mut session = SharedSessionState::new();
        let key = vec![0xAB, 0xCD, 0xEF];
        let result = session.inject_dynamic(0, key.clone());
        assert!(result.is_ok());
        assert_eq!(session.retrieve_dynamic(0), Some(&key));
    }

    #[test]
    fn test_shared_session_state_overflow() {
        let mut session = SharedSessionState::new();
        let result = session.inject_dynamic(256, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fse_encoder_init() {
        let mut encoder = FseEncoder::new();
        let result = encoder.initialize();
        assert!(result.is_ok());
    }

    #[test]
    fn test_fse_encoder_roundtrip() {
        let mut encoder = FseEncoder::new();
        encoder.initialize().unwrap();

        let original = b"MUST";
        let encoded = encoder.encode(original).unwrap();
        assert!(encoded.starts_with(&[0x46, 0xFE]));

        let decoded = encoder.decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_fse_encoder_unknown_bytes() {
        let mut encoder = FseEncoder::new();
        encoder.initialize().unwrap();

        let data = b"test123";
        let encoded = encoder.encode(data).unwrap();
        let decoded = encoder.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_fse_encoder_session_amortization() {
        let mut encoder = FseEncoder::new();
        encoder.initialize().unwrap();

        let key1 = vec![0x01, 0x02, 0x03];
        let result = encoder.inject_session_amortization(key1);
        assert!(result.is_ok());
        assert!(!encoder.session_state_full());
    }

    #[test]
    fn test_fse_decode_invalid_magic() {
        let encoder = FseEncoder::new();
        let data = vec![0x00, 0x00, 0x00];
        let result = encoder.decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_fse_decode_short_data() {
        let encoder = FseEncoder::new();
        let data = vec![0x46];
        let result = encoder.decode(&data);
        assert!(result.is_err());
    }
}
