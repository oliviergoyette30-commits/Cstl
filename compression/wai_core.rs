use crate::wai_dictionary::{WAI_SYMBOLS, WAI_REVERSE, WAI_VERSION_HASH};
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum WaiCoreError {
    InvalidMagic,
    InvalidVersion,
    InvalidFlags,
    DictionaryMismatch(String),
    VarintDecodeError(String),
    BitUnpackError(String),
    EscapeDecodeError(String),
    ZigzagError(String),
    DeltaError(String),
    SizeMismatch,
    InsufficientData,
}

impl fmt::Display for WaiCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WaiCoreError::InvalidMagic => write!(f, "Invalid WAI magic bytes"),
            WaiCoreError::InvalidVersion => write!(f, "Unsupported WAI version"),
            WaiCoreError::InvalidFlags => write!(f, "Invalid WAI flags"),
            WaiCoreError::DictionaryMismatch(e) => write!(f, "Dictionary mismatch: {}", e),
            WaiCoreError::VarintDecodeError(e) => write!(f, "Varint decode error: {}", e),
            WaiCoreError::BitUnpackError(e) => write!(f, "Bit unpack error: {}", e),
            WaiCoreError::EscapeDecodeError(e) => write!(f, "Escape decode error: {}", e),
            WaiCoreError::ZigzagError(e) => write!(f, "ZigZag error: {}", e),
            WaiCoreError::DeltaError(e) => write!(f, "Delta error: {}", e),
            WaiCoreError::SizeMismatch => write!(f, "Size mismatch"),
            WaiCoreError::InsufficientData => write!(f, "Insufficient data"),
        }
    }
}

impl Error for WaiCoreError {}

const WAI_MAGIC: &[u8; 3] = b"WAI";
const WAI_VERSION: u8 = 0x01;

pub struct WaiEncoder {
    delta_enabled: bool,
    zigzag_enabled: bool,
}

impl WaiEncoder {
    pub fn new() -> Self {
        WaiEncoder {
            delta_enabled: true,
            zigzag_enabled: true,
        }
    }

    pub fn encode(&self, payload: &str) -> Result<Vec<u8>, WaiCoreError> {
        let symbols = self.tokenize_to_symbols(payload)?;
        let bit_packed = self.pack_symbols(&symbols)?;
        let mut output = Vec::new();

        output.extend_from_slice(WAI_MAGIC);
        output.push(WAI_VERSION);

        let mut flags = 0u8;
        if self.delta_enabled { flags |= 0x01; }
        if self.zigzag_enabled { flags |= 0x02; }
        output.push(flags);

        output.extend_from_slice(WAI_VERSION_HASH.as_bytes());

        let symbol_count = encode_varint(symbols.len() as u32);
        output.extend_from_slice(&symbol_count);

        output.extend_from_slice(&bit_packed);

        Ok(output)
    }

    fn tokenize_to_symbols(&self, payload: &str) -> Result<Vec<u16>, WaiCoreError> {
        let mut symbols = Vec::new();
        let mut current_token = String::new();

        for ch in payload.chars() {
            if ch.is_whitespace() || ch == '"' || ch == ':' || ch == '{' || ch == '}' || ch == '[' || ch == ']' || ch == ',' {
                if !current_token.is_empty() {
                    if let Some(&id) = WAI_SYMBOLS.get(&current_token) {
                        symbols.push(id);
                    } else {
                        symbols.push(0x0000);
                        for byte in current_token.as_bytes() {
                            symbols.push(*byte as u16);
                        }
                    }
                    current_token.clear();
                }
            } else {
                current_token.push(ch);
            }
        }

        if !current_token.is_empty() {
            if let Some(&id) = WAI_SYMBOLS.get(&current_token) {
                symbols.push(id);
            } else {
                symbols.push(0x0000);
                for byte in current_token.as_bytes() {
                    symbols.push(*byte as u16);
                }
            }
        }

        Ok(symbols)
    }

    fn pack_symbols(&self, symbols: &[u16]) -> Result<Vec<u8>, WaiCoreError> {
        let mut result = Vec::new();
        let mut bit_buffer: u64 = 0;
        let mut bit_count = 0;

        for &symbol in symbols {
            if symbol > 0x0FFF {
                return Err(WaiCoreError::BitUnpackError(
                    format!("Symbol 0x{:04X} exceeds 12-bit limit", symbol)
                ));
            }

            bit_buffer = (bit_buffer << 12) | (symbol as u64);
            bit_count += 12;

            if bit_count >= 48 {
                let bits_to_keep = bit_count % 8;
                let bytes_to_emit = bit_count / 8;

                for i in 0..bytes_to_emit {
                    let shift = bits_to_keep + (bytes_to_emit - 1 - i) * 8;
                    result.push((bit_buffer >> shift) as u8);
                }

                bit_buffer &= (1u64 << bits_to_keep) - 1;
                bit_count = bits_to_keep;
            }
        }

        if bit_count > 0 {
            let bytes_to_emit = bit_count / 8;
            let bits_remaining = bit_count % 8;

            for i in 0..bytes_to_emit {
                let shift = bits_remaining + (bytes_to_emit - 1 - i) * 8;
                result.push((bit_buffer >> shift) as u8);
            }

            if bits_remaining > 0 {
                result.push((bit_buffer << (8 - bits_remaining)) as u8);
            }
        }

        Ok(result)
    }
}

pub struct WaiDecoder;

impl WaiDecoder {
    pub fn decode(data: &[u8]) -> Result<String, WaiCoreError> {
        if data.len() < 70 {
            return Err(WaiCoreError::InsufficientData);
        }

        if &data[0..3] != WAI_MAGIC {
            return Err(WaiCoreError::InvalidMagic);
        }

        let version = data[3];
        if version != WAI_VERSION {
            return Err(WaiCoreError::InvalidVersion);
        }

        let flags = data[4];
        if flags > 0x07 {
            return Err(WaiCoreError::InvalidFlags);
        }

        let dict_hash_bytes = &data[5..69];
        let dict_hash = std::str::from_utf8(dict_hash_bytes)
            .map_err(|_| WaiCoreError::DictionaryMismatch("Invalid UTF-8 hash".into()))?;

        if dict_hash != *WAI_VERSION_HASH {
            return Err(WaiCoreError::DictionaryMismatch(
                format!("Expected {}, got {}", *WAI_VERSION_HASH, dict_hash)
            ));
        }

        if data.len() < 70 {
            return Err(WaiCoreError::InsufficientData);
        }

        let (symbol_count, offset) = decode_varint(&data[69..])?;

        let bit_packed = &data[69 + offset..];
        let symbols = unpack_symbols(bit_packed, symbol_count as usize)?;

        let mut result = String::new();
        for symbol in symbols {
            if symbol == 0x0000 {
                continue;
            } else if let Some(text) = WAI_REVERSE.get(&symbol) {
                if !result.is_empty() && !result.ends_with(' ') {
                    result.push(' ');
                }
                result.push_str(text);
            }
        }

        Ok(result)
    }
}

pub fn encode_varint(mut value: u32) -> Vec<u8> {
    let mut result = Vec::new();
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            result.push(byte);
            break;
        } else {
            result.push(byte | 0x80);
        }
    }
    result
}

pub fn decode_varint(bytes: &[u8]) -> Result<(u32, usize), WaiCoreError> {
    let mut result = 0u32;
    let mut shift = 0;
    let mut pos = 0;

    loop {
        if pos >= bytes.len() {
            return Err(WaiCoreError::VarintDecodeError("Unexpected end of data".into()));
        }

        let byte = bytes[pos];
        pos += 1;

        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;

        if shift >= 32 {
            return Err(WaiCoreError::VarintDecodeError("Varint overflow".into()));
        }
    }

    Ok((result, pos))
}

pub fn zigzag_encode(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)) as u32
}

pub fn zigzag_decode(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

pub fn delta_encode(values: &[u32]) -> Vec<u32> {
    if values.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(values.len());
    result.push(values[0]);

    for i in 1..values.len() {
        result.push(values[i].wrapping_sub(values[i - 1]));
    }

    result
}

pub fn delta_decode(deltas: &[u32]) -> Vec<u32> {
    if deltas.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(deltas.len());
    result.push(deltas[0]);

    for i in 1..deltas.len() {
        result.push(result[i - 1].wrapping_add(deltas[i]));
    }

    result
}

fn unpack_symbols(data: &[u8], expected_count: usize) -> Result<Vec<u16>, WaiCoreError> {
    if expected_count == 0 {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    let mut bit_buffer: u64 = 0;
    let mut bit_count = 0;
    let mut byte_pos = 0;

    while result.len() < expected_count {
        if byte_pos >= data.len() && bit_count < 12 {
            return Err(WaiCoreError::BitUnpackError("Incomplete symbol data".into()));
        }

        if bit_count < 12 && byte_pos < data.len() {
            bit_buffer = (bit_buffer << 8) | (data[byte_pos] as u64);
            bit_count += 8;
            byte_pos += 1;
        }

        if bit_count >= 12 {
            let shift = bit_count - 12;
            let symbol = ((bit_buffer >> shift) & 0xFFF) as u16;
            result.push(symbol);
            bit_buffer &= (1u64 << shift) - 1;
            bit_count -= 12;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_roundtrip_small() {
        for v in &[0u32, 1, 127, 128] {
            let encoded = encode_varint(*v);
            let (decoded, _) = decode_varint(&encoded).unwrap();
            assert_eq!(*v, decoded);
        }
    }

    #[test]
    fn test_varint_roundtrip_large() {
        for v in &[16383u32, 16384, 2097151, 2097152, u32::MAX] {
            let encoded = encode_varint(*v);
            let (decoded, _) = decode_varint(&encoded).unwrap();
            assert_eq!(*v, decoded);
        }
    }

    #[test]
    fn test_zigzag_roundtrip() {
        for v in &[-128i32, -1, 0, 1, 127] {
            let encoded = zigzag_encode(*v);
            let decoded = zigzag_decode(encoded);
            assert_eq!(*v, decoded);
        }
    }

    #[test]
    fn test_zigzag_examples() {
        assert_eq!(zigzag_encode(0), 0);
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(1), 2);
        assert_eq!(zigzag_encode(-2), 3);
    }

    #[test]
    fn test_delta_encode_decode() {
        let values = vec![100u32, 105, 108, 110, 115];
        let deltas = delta_encode(&values);
        assert_eq!(deltas, vec![100, 5, 3, 2, 5]);

        let recovered = delta_decode(&deltas);
        assert_eq!(values, recovered);
    }

    #[test]
    fn test_delta_empty() {
        assert_eq!(delta_encode(&[] as &[u32]), Vec::<u32>::new());
        assert_eq!(delta_decode(&[] as &[u32]), Vec::<u32>::new());
    }

    #[test]
    fn test_delta_single() {
        assert_eq!(delta_encode(&[42]), vec![42]);
        assert_eq!(delta_decode(&[42]), vec![42]);
    }

    #[test]
    fn test_symbol_exceed_limit() {
        let encoder = WaiEncoder::new();
        let symbols = vec![0x1000u16];
        assert!(encoder.pack_symbols(&symbols).is_err());
    }

    #[test]
    fn test_decode_invalid_version() {
        let mut data = vec![b'W', b'A', b'I', 0xFF];
        data.resize(71, 0);
        assert!(matches!(WaiDecoder::decode(&data), Err(WaiCoreError::InvalidVersion)));
    }

    #[test]
    fn test_decode_short_data() {
        let data = b"WAI";
        assert!(matches!(WaiDecoder::decode(data), Err(WaiCoreError::InsufficientData)));
    }
}
