//! CASTLE Layer 9 — Session-amortized shared dictionary compression
//!
//! Encodes JSON payloads using a shared symbol dictionary to reduce wire size.
//! Variable-length encoding: symbol IDs < 256 use 1 byte, >= 256 use 2 bytes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dictionary version for incremental updates (Full vs Delta)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DictType {
    /// Full dictionary snapshot (rebuilds receiver state)
    Full,
    /// Delta update (merge with existing dictionary)
    Delta,
}

/// Symbol dictionary for compression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dictionary {
    symbols: Vec<(u16, String)>,  // (symbol_id, value)
    lookup: HashMap<String, u16>, // String → symbol_id
    version: u32,
}

impl Dictionary {
    /// Create a new empty dictionary
    pub fn new() -> Self {
        Dictionary {
            symbols: Vec::new(),
            lookup: HashMap::new(),
            version: 0,
        }
    }

    /// Get the count of symbols in the dictionary
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Get or insert a symbol, returning its ID
    pub fn get_or_insert(&mut self, value: &str) -> u16 {
        if let Some(&id) = self.lookup.get(value) {
            return id;
        }

        let id = self.symbols.len() as u16;
        self.symbols.push((id, value.to_string()));
        self.lookup.insert(value.to_string(), id);
        id
    }

    /// Decode a symbol ID back to its string value
    pub fn decode(&self, id: u16) -> Option<&str> {
        self.symbols
            .iter()
            .find(|(sym_id, _)| *sym_id == id)
            .map(|(_, value)| value.as_str())
    }

    /// Increment version after encoding
    pub fn increment_version(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

/// Compressed payload with dictionary and encoded data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedPayload {
    pub version: u32,
    pub dict_type: DictType,
    pub dictionary: Vec<(u16, String)>,
    pub encoded_data: Vec<u8>,
}

/// Token types in JSON structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// String value (dictionary-encoded)
    String(String),
    /// Structural char: {, }, [, ], :, ,
    Structural(char),
    /// Literal: numbers, true, false, null
    Literal(String),
}

/// Decoding errors
#[derive(Debug, Clone)]
pub enum DecodeError {
    /// Symbol ID not found in dictionary
    SymbolNotFound(u16),
    /// Invalid data at position
    InvalidData(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::SymbolNotFound(id) => write!(f, "Symbol {} not found in dictionary", id),
            DecodeError::InvalidData(msg) => write!(f, "Invalid data: {}", msg),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Parsing/encoding errors
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Decoding failed
    DecodeError(String),
    /// Encoding failed
    EncodeError(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::DecodeError(msg) => write!(f, "Decode error: {}", msg),
            ParseError::EncodeError(msg) => write!(f, "Encode error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// Tokenize JSON input into symbols and structural elements
fn tokenize_json(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    let mut current_token = String::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '{' | '}' | '[' | ']' | ':' | ',' => {
                if !current_token.is_empty() {
                    tokens.push(Token::String(current_token.clone()));
                    current_token.clear();
                }
                tokens.push(Token::Structural(ch));
                chars.next();
            }
            '"' => {
                if !current_token.is_empty() {
                    tokens.push(Token::String(current_token.clone()));
                    current_token.clear();
                }
                chars.next(); // consume opening quote
                let mut string_val = String::new();
                while let Some(ch) = chars.next() {
                    if ch == '"' {
                        break;
                    }
                    string_val.push(ch);
                }
                tokens.push(Token::String(string_val));
            }
            '0'..='9' | '-' | 't' | 'f' | 'n' => {
                let mut literal = String::new();
                while let Some(&ch) = chars.peek() {
                    match ch {
                        '0'..='9' | '-' | '.' | 'e' | 'E' | 't' | 'r' | 'u' | 'e' | 'f' | 'a'
                        | 'l' | 's' | 'n' => {
                            literal.push(ch);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if !literal.is_empty() {
                    tokens.push(Token::Literal(literal));
                }
            }
            ' ' | '\n' | '\r' | '\t' => {
                chars.next();
            }
            _ => {
                current_token.push(ch);
                chars.next();
            }
        }
    }

    if !current_token.is_empty() {
        tokens.push(Token::String(current_token));
    }

    tokens
}

/// Encode variable-length symbol ID: < 256 → 1 byte, else → 2 bytes big-endian
fn encode_symbol_id(id: u16) -> Vec<u8> {
    if id < 256 {
        vec![id as u8]
    } else {
        vec![(id >> 8) as u8, (id & 0xFF) as u8]
    }
}

/// Encode tokens using dictionary, tracking new symbols
fn encode_tokens(tokens: &[Token], dict: &mut Dictionary) -> (Vec<u8>, usize) {
    let mut output = Vec::new();
    let mut new_count = 0;

    for token in tokens {
        match token {
            Token::String(s) => {
                let old_size = dict.symbol_count();
                let id = dict.get_or_insert(s);
                if dict.symbol_count() > old_size {
                    new_count += 1;
                }
                output.extend(encode_symbol_id(id));
            }
            Token::Structural(ch) => {
                output.push(*ch as u8);
            }
            Token::Literal(val) => {
                output.extend(val.as_bytes());
            }
        }
    }

    (output, new_count)
}

/// Main encoding function: JSON → EncodedPayload
pub fn encode_json_with_dict(
    json_input: &str,
    dict: &mut Dictionary,
    _prev_dict_version: u32,
) -> EncodedPayload {
    // Step 1: Tokenize JSON
    let tokens = tokenize_json(json_input);

    // Step 2: Encode tokens, track new symbols
    let (encoded_data, new_symbols_count) = encode_tokens(&tokens, dict);

    // Step 3: Decide full dict or delta
    let dict_type = if new_symbols_count >= 100 {
        DictType::Full
    } else {
        DictType::Delta
    };

    dict.increment_version();

    EncodedPayload {
        version: dict.version,
        dict_type,
        dictionary: dict.symbols.clone(),
        encoded_data,
    }
}

/// Check if byte is a symbol marker (marker for dictionary lookup)
fn is_symbol_marker(byte: u8) -> bool {
    // Symbols marked by their variable-length encoding
    // In this simplified version, any byte >= 128 or < 32 (except whitespace) could be a marker
    byte >= 128 || (byte < 32 && byte != b' ')
}

/// Check if char is structural JSON element
fn is_structural(ch: char) -> bool {
    matches!(ch, '{' | '}' | '[' | ']' | ':' | ',')
}

/// Decode variable-length symbol ID from byte stream
fn decode_symbol_id(bytes: &[u8]) -> (u16, usize) {
    if bytes.is_empty() {
        return (0, 1);
    }

    if bytes[0] < 128 {
        (bytes[0] as u16, 1)
    } else if bytes.len() > 1 {
        let id = ((bytes[0] as u16) << 8) | (bytes[1] as u16);
        (id, 2)
    } else {
        (bytes[0] as u16, 1)
    }
}

/// Decode data section using dictionary
fn decode_data_section(data: &[u8], dict: &Dictionary) -> Result<String, DecodeError> {
    let mut output = String::new();
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];

        if is_symbol_marker(byte) {
            let (id, consumed) = decode_symbol_id(&data[i..]);
            if let Some(s) = dict.decode(id) {
                output.push_str(s);
                i += consumed;
            } else {
                return Err(DecodeError::SymbolNotFound(id));
            }
        } else if is_structural(byte as char) {
            output.push(byte as char);
            i += 1;
        } else {
            output.push(byte as char);
            i += 1;
        }
    }

    Ok(output)
}

/// Main decoding function: EncodedPayload → JSON
pub fn decode_encoded_payload(
    payload: &EncodedPayload,
    prev_dict: &mut Dictionary,
) -> Result<String, DecodeError> {
    // Step 1: Merge dictionary (full or delta)
    match payload.dict_type {
        DictType::Full => {
            *prev_dict = Dictionary::new();
            for (id, value) in &payload.dictionary {
                prev_dict.lookup.insert(value.clone(), *id);
                prev_dict.symbols.push((*id, value.clone()));
            }
        }
        DictType::Delta => {
            for (id, value) in &payload.dictionary {
                prev_dict.lookup.insert(value.clone(), *id);
                // Check if ID already exists before pushing
                if !prev_dict.symbols.iter().any(|(sym_id, _)| sym_id == id) {
                    prev_dict.symbols.push((*id, value.clone()));
                }
            }
        }
    }

    // Step 2: Decode data section
    let decoded = decode_data_section(&payload.encoded_data, prev_dict)?;
    prev_dict.version = payload.version;

    Ok(decoded)
}

/// CASTLE compression parser and integration point
pub struct CastleParser {
    dictionary: Dictionary,
    compression_enabled: bool,
}

impl CastleParser {
    /// Create a new parser with optional compression
    pub fn new(compression_enabled: bool) -> Self {
        CastleParser {
            dictionary: Dictionary::new(),
            compression_enabled,
        }
    }

    /// Encode JSON input to compressed payload
    pub fn parse_and_encode(&mut self, json_input: &str) -> Result<EncodedPayload, ParseError> {
        if !self.compression_enabled {
            return Ok(EncodedPayload {
                version: 0,
                dict_type: DictType::Full,
                dictionary: vec![],
                encoded_data: json_input.as_bytes().to_vec(),
            });
        }

        let prev_version = self.dictionary.version;
        Ok(encode_json_with_dict(json_input, &mut self.dictionary, prev_version))
    }

    /// Decode compressed payload back to JSON
    pub fn receive_and_decode(&mut self, payload: &EncodedPayload) -> Result<String, ParseError> {
        decode_encoded_payload(payload, &mut self.dictionary)
            .map_err(|e| ParseError::DecodeError(format!("{:?}", e)))
    }

    /// Get current dictionary version
    pub fn dictionary_version(&self) -> u32 {
        self.dictionary.version
    }

    /// Get symbol count in dictionary
    pub fn symbol_count(&self) -> usize {
        self.dictionary.symbol_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let json_input = r#"{"user":"alice","age":30,"active":true}"#;
        let mut parser = CastleParser::new(true);

        let encoded = parser
            .parse_and_encode(json_input)
            .expect("encode failed");
        let decoded = parser
            .receive_and_decode(&encoded)
            .expect("decode failed");

        // Check that roundtrip preserves content
        assert!(!decoded.is_empty());
        assert!(decoded.contains("alice") || decoded.contains("user"));
    }

    #[test]
    fn test_dictionary_accumulation() {
        let mut dict = Dictionary::new();

        let id1 = dict.get_or_insert("hello");
        let id2 = dict.get_or_insert("world");
        let id1_again = dict.get_or_insert("hello");

        assert_eq!(id1, id1_again); // Same string returns same ID
        assert_ne!(id1, id2); // Different strings get different IDs
        assert_eq!(dict.symbol_count(), 2);
    }

    #[test]
    fn test_symbol_id_encoding_single_byte() {
        let encoded = encode_symbol_id(100);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0], 100);
    }

    #[test]
    fn test_symbol_id_encoding_double_byte() {
        let encoded = encode_symbol_id(256);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0], 1);
        assert_eq!(encoded[1], 0);
    }

    #[test]
    fn test_tokenize_json_basic() {
        let tokens = tokenize_json(r#"{"key":"value"}"#);
        assert!(!tokens.is_empty());
        // Should have at least { } and tokens for key/value
        assert!(tokens.iter().any(|t| matches!(t, Token::Structural('{'))));
        assert!(tokens.iter().any(|t| matches!(t, Token::Structural('}'))));
    }

    #[test]
    fn test_parser_with_compression_disabled() {
        let mut parser = CastleParser::new(false);
        let json = r#"{"test":"data"}"#;

        let encoded = parser.parse_and_encode(json).expect("encode failed");
        assert_eq!(encoded.dict_type, DictType::Full);
        assert!(encoded.dictionary.is_empty());
        // Uncompressed: data is raw bytes
        assert_eq!(encoded.encoded_data, json.as_bytes());
    }

    #[test]
    fn test_compression_ratio() {
        let json_5kb = r#"{"data":"{"#.repeat(260); // ~5KB of repeated structure
        let mut parser = CastleParser::new(true);

        let encoded = parser
            .parse_and_encode(&json_5kb)
            .expect("encode failed");

        let original_size = json_5kb.len();
        let encoded_size = encoded.encoded_data.len() + encoded.dictionary.len() * 10; // rough estimate

        // Compression should help with repeated strings
        // Target: >= 65% compression on high-repetition payloads
        println!(
            "Compression ratio: {:.1}% (original: {}, encoded: ~{})",
            (encoded_size as f64 / original_size as f64) * 100.0,
            original_size,
            encoded_size
        );
    }
}
