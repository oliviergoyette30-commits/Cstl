//! CASTLE Layer 9 — Session-amortized shared dictionary compression
//!
//! Encodes text payloads using a shared symbol dictionary to reduce wire size.
//!
//! **2026-10-01 correctness rewrite.** This module existed since v5.0.0 with
//! 7 passing unit tests but was never wired into the live pipeline (dead
//! code — `pub mod castle;` only). Wiring it up (see `castle_wire.rs`)
//! required an honest look first, and it surfaced three real bugs the loose
//! "contains()" assertions in the original tests never caught:
//!
//! 1. **Ambiguous byte stream.** The old `encode_symbol_id`/`is_symbol_marker`
//!    scheme tried to tell symbol-ID bytes apart from structural/literal
//!    bytes using a heuristic (`byte >= 128 || byte < 32`) with NO reserved
//!    marker range -- a single-byte symbol ID is any value 0..=255 and
//!    routinely collides with the ASCII range used by structural chars and
//!    literals. Decoding could silently emit the wrong bytes. Fixed here
//!    with a self-describing tag-prefixed format (see `encode_tokens`/
//!    `decode_data_section`): every token is `[tag][payload]`, no ambiguity
//!    possible regardless of byte values.
//! 2. **Whitespace and quote marks silently dropped** by `tokenize_json`
//!    (whitespace was consumed and never re-emitted; quote characters were
//!    stripped from quoted spans). Harmless for JSON formatted exactly the
//!    way the tokenizer expects, but CSTL block text is not JSON -- it has
//!    meaningful whitespace and quoted values (`errors="...,..."`) -- so
//!    this would have corrupted real traffic. Fixed by folding whitespace
//!    into the current token and keeping the quote characters themselves as
//!    part of the captured string.
//! 3. **"Delta" was not actually a delta**: `encode_json_with_dict` cloned
//!    the ENTIRE dictionary into every `EncodedPayload.dictionary`,
//!    `DictType::Delta` or not -- the one thing "session-amortized" is
//!    supposed to avoid. Fixed: `Delta` now carries only the symbols newly
//!    inserted during that one call; `Full` still carries everything (first
//!    message / large-vocabulary bursts).
//!
//! None of this is a performance optimization pass — it's what makes a
//! byte-exact roundtrip even possible, which is the precondition for the
//! safety gate in `castle_wire.rs` to ever let compression through at all.

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
                // 2026-10-01 fix: the original code consumed the opening and
                // closing quote chars without ever re-emitting them, so a
                // decode could never reproduce a quoted field exactly (CSTL
                // values are routinely quoted, e.g. `errors="E304: ..., ..."`
                // to protect an embedded comma -- see
                // parser::split_top_level_commas). Both quote characters are
                // now kept as part of the captured token, so the token's
                // content is exactly the source bytes it spans, no matter
                // how it gets chunked.
                if !current_token.is_empty() {
                    tokens.push(Token::String(current_token.clone()));
                    current_token.clear();
                }
                let mut string_val = String::new();
                string_val.push(ch); // opening quote
                chars.next();
                for c in chars.by_ref() {
                    string_val.push(c);
                    if c == '"' {
                        break;
                    }
                }
                tokens.push(Token::String(string_val));
            }
            '0'..='9' | '-' | 't' | 'f' | 'n' => {
                // 2026-10-01 fix: this branch is reached on EVERY peeked
                // char, not just at token boundaries -- a wildcard-started
                // token like "status" hits 's' (wildcard, pushed into
                // current_token) then 't' (which matches this branch's
                // trigger set) *mid-word*. The original code started a new
                // `literal` buffer and pushed it to `tokens` immediately,
                // while the "s" sitting in `current_token` only got flushed
                // later -- reordering the output ("tatus" before "s").
                // Flushing here first keeps byte order correct; it still
                // mid-word-splits occasionally (e.g. "status" -> "s" +
                // "tatus"), which only affects compression efficiency, not
                // correctness, since decode just concatenates in order.
                if !current_token.is_empty() {
                    tokens.push(Token::String(current_token.clone()));
                    current_token.clear();
                }
                let mut literal = String::new();
                while let Some(&ch) = chars.peek() {
                    match ch {
                        '0'..='9' | '-' | '.' | 'e' | 'E' | 't' | 'r' | 'u' | 'f' | 'a'
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
                // 2026-10-01 fix: previously consumed and discarded -- fine
                // for insignificant JSON whitespace between tokens, wrong
                // for CSTL block text where whitespace inside a field value
                // is meaningful content (e.g. "produced_by=Server, status=
                // ..." -- the space after the comma is part of the real
                // bytes a decode must reproduce). Folded into the current
                // token instead of being dropped.
                current_token.push(ch);
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

/// Tag bytes for the self-describing token stream (2026-10-01 rewrite).
/// The old format tried to distinguish symbol IDs from structural/literal
/// bytes by VALUE HEURISTIC (`byte >= 128 || byte < 32`) with no reserved
/// range -- a single-byte symbol ID can legitimately be any value 0..=255,
/// so it routinely collided with ASCII structural/literal bytes and decoded
/// wrong. Every token is now `[tag][payload]`, so decoding never has to
/// guess what a byte means.
const TAG_SYMBOL: u8 = 0;
const TAG_STRUCTURAL: u8 = 1;
const TAG_LITERAL: u8 = 2;

/// Encode tokens using the dictionary. Returns the encoded byte stream and
/// the list of symbols NEWLY inserted during this call (id, value) -- the
/// caller decides whether to ship the full dictionary or just this delta.
fn encode_tokens(tokens: &[Token], dict: &mut Dictionary) -> (Vec<u8>, Vec<(u16, String)>) {
    let mut output = Vec::new();
    let mut new_symbols = Vec::new();

    for token in tokens {
        match token {
            Token::String(s) => {
                let old_size = dict.symbol_count();
                let id = dict.get_or_insert(s);
                if dict.symbol_count() > old_size {
                    new_symbols.push((id, s.clone()));
                }
                output.push(TAG_SYMBOL);
                output.extend_from_slice(&id.to_be_bytes());
            }
            Token::Structural(ch) => {
                // CSTL/JSON structural chars ({}[]:,) are all single-byte
                // ASCII, so the `as u8` cast is exact -- never used for
                // anything outside that fixed set (see tokenize_json).
                output.push(TAG_STRUCTURAL);
                output.push(*ch as u8);
            }
            Token::Literal(val) => {
                let bytes = val.as_bytes();
                output.push(TAG_LITERAL);
                output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                output.extend_from_slice(bytes);
            }
        }
    }

    (output, new_symbols)
}

/// Main encoding function: text → EncodedPayload. The name (and the
/// `json_input` parameter name below) predates this rewrite -- despite the
/// "JSON" framing the tokenizer is tolerant of arbitrary text (CSTL block
/// text included), see the module doc comment.
pub fn encode_json_with_dict(
    json_input: &str,
    dict: &mut Dictionary,
    _prev_dict_version: u32,
) -> EncodedPayload {
    // Step 1: Tokenize
    let tokens = tokenize_json(json_input);

    // Step 2: Encode tokens, collect newly-inserted symbols
    let (encoded_data, new_symbols) = encode_tokens(&tokens, dict);

    // Step 3: Decide full dict or delta. A large burst of new vocabulary
    // (>=100 new symbols in one call) ships the whole dictionary so a
    // receiver that somehow desynced can resync in one shot; otherwise
    // (the common case) only the genuinely new entries travel -- see the
    // module doc comment for why the old code's "delta" didn't actually do
    // this.
    let dict_type = if new_symbols.len() >= 100 {
        DictType::Full
    } else {
        DictType::Delta
    };

    dict.increment_version();

    EncodedPayload {
        version: dict.version,
        dict_type,
        dictionary: match dict_type {
            DictType::Full => dict.symbols.clone(),
            DictType::Delta => new_symbols,
        },
        encoded_data,
    }
}

/// Decode the tag-prefixed data section using the dictionary.
fn decode_data_section(data: &[u8], dict: &Dictionary) -> Result<String, DecodeError> {
    let mut output = String::new();
    let mut i = 0;

    while i < data.len() {
        let tag = data[i];
        i += 1;
        match tag {
            TAG_SYMBOL => {
                if i + 2 > data.len() {
                    return Err(DecodeError::InvalidData("truncated symbol id".to_string()));
                }
                let id = u16::from_be_bytes([data[i], data[i + 1]]);
                i += 2;
                match dict.decode(id) {
                    Some(s) => output.push_str(s),
                    None => return Err(DecodeError::SymbolNotFound(id)),
                }
            }
            TAG_STRUCTURAL => {
                if i >= data.len() {
                    return Err(DecodeError::InvalidData("truncated structural token".to_string()));
                }
                output.push(data[i] as char);
                i += 1;
            }
            TAG_LITERAL => {
                if i + 4 > data.len() {
                    return Err(DecodeError::InvalidData("truncated literal length".to_string()));
                }
                let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
                i += 4;
                if i + len > data.len() {
                    return Err(DecodeError::InvalidData("truncated literal body".to_string()));
                }
                let s = std::str::from_utf8(&data[i..i + len])
                    .map_err(|e| DecodeError::InvalidData(format!("invalid utf-8 in literal: {e}")))?;
                output.push_str(s);
                i += len;
            }
            other => {
                return Err(DecodeError::InvalidData(format!("unknown token tag byte {other}")));
            }
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
#[derive(Clone)]
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

    /// Snapshot the current dictionary state (cheap clone -- used by
    /// `castle_wire.rs` to trial-encode a message without committing the
    /// live dictionary until the byte-exact roundtrip gate has passed; see
    /// that module for why a rejected trial must never advance the
    /// connection's real dictionary).
    pub fn dictionary_snapshot(&self) -> Dictionary {
        self.dictionary.clone()
    }

    /// Replace the live dictionary wholesale (commits a trial encode after
    /// its gate has passed).
    pub fn commit_dictionary(&mut self, dict: Dictionary) {
        self.dictionary = dict;
    }
}

/// Compact binary wire serialization of an `EncodedPayload` -- deliberately
/// NOT `serde_json` (a `Vec<u8>` serialized as a JSON array of numbers would
/// bloat the very thing this module exists to shrink). Format:
///   [1 byte dict_type: 0=Full, 1=Delta]
///   [4 bytes version, big-endian]
///   [4 bytes dictionary entry count, big-endian]
///   repeated: [2 bytes id][4 bytes value byte-length][value bytes]
///   [4 bytes encoded_data byte-length][encoded_data bytes]
pub fn serialize_encoded_payload(payload: &EncodedPayload) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(match payload.dict_type {
        DictType::Full => 0u8,
        DictType::Delta => 1u8,
    });
    out.extend_from_slice(&payload.version.to_be_bytes());
    out.extend_from_slice(&(payload.dictionary.len() as u32).to_be_bytes());
    for (id, value) in &payload.dictionary {
        out.extend_from_slice(&id.to_be_bytes());
        let vb = value.as_bytes();
        out.extend_from_slice(&(vb.len() as u32).to_be_bytes());
        out.extend_from_slice(vb);
    }
    out.extend_from_slice(&(payload.encoded_data.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload.encoded_data);
    out
}

/// Inverse of `serialize_encoded_payload`. Never panics on truncated or
/// malformed input -- every length-prefixed read is bounds-checked and
/// returns `DecodeError::InvalidData` instead, same discipline as
/// `decode_data_section` above.
pub fn deserialize_encoded_payload(bytes: &[u8]) -> Result<EncodedPayload, DecodeError> {
    fn need(bytes: &[u8], i: usize, n: usize) -> Result<(), DecodeError> {
        if i + n > bytes.len() {
            Err(DecodeError::InvalidData("truncated CASTLE wire payload".to_string()))
        } else {
            Ok(())
        }
    }

    let mut i = 0;
    need(bytes, i, 1)?;
    let dict_type = match bytes[i] {
        0 => DictType::Full,
        1 => DictType::Delta,
        other => {
            return Err(DecodeError::InvalidData(format!("unknown dict_type tag {other}")));
        }
    };
    i += 1;

    need(bytes, i, 4)?;
    let version = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
    i += 4;

    need(bytes, i, 4)?;
    let dict_len = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    i += 4;

    let mut dictionary = Vec::with_capacity(dict_len.min(1_000_000));
    for _ in 0..dict_len {
        need(bytes, i, 2)?;
        let id = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
        i += 2;

        need(bytes, i, 4)?;
        let vlen = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        i += 4;

        need(bytes, i, vlen)?;
        let value = std::str::from_utf8(&bytes[i..i + vlen])
            .map_err(|e| DecodeError::InvalidData(format!("invalid utf-8 in dict value: {e}")))?
            .to_string();
        i += vlen;

        dictionary.push((id, value));
    }

    need(bytes, i, 4)?;
    let data_len = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    i += 4;
    need(bytes, i, data_len)?;
    let encoded_data = bytes[i..i + data_len].to_vec();

    Ok(EncodedPayload {
        version,
        dict_type,
        dictionary,
        encoded_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        // 2026-10-01: tightened from a loose `contains()` check to exact
        // equality, now that the tag-prefixed format makes it possible to
        // actually guarantee this.
        let json_input = r#"{"user":"alice","age":30,"active":true}"#;
        let mut parser = CastleParser::new(true);

        let encoded = parser
            .parse_and_encode(json_input)
            .expect("encode failed");
        let decoded = parser
            .receive_and_decode(&encoded)
            .expect("decode failed");

        assert_eq!(decoded, json_input);
    }

    #[test]
    fn test_roundtrip_exact_on_cstl_block_text_with_whitespace_and_quotes() {
        // The old tokenizer dropped whitespace and quote marks -- both are
        // meaningful bytes in real CSTL block text (not JSON). This is the
        // shape of text castle_wire.rs actually compresses.
        let cstl_body = "META [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=acknowledgement, errors=\"E304: Missing sender, E305: Missing receiver\"]\n";
        let mut parser = CastleParser::new(true);

        let encoded = parser.parse_and_encode(cstl_body).expect("encode failed");
        let decoded = parser.receive_and_decode(&encoded).expect("decode failed");

        assert_eq!(decoded, cstl_body);
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
    fn test_delta_carries_only_newly_inserted_symbols() {
        // 2026-10-01 fix: DictType::Delta used to clone the WHOLE dictionary
        // into every EncodedPayload regardless -- defeating the point of a
        // delta. This asserts the real contract: a second message that
        // repeats the first message's vocabulary adds nothing new.
        let mut parser = CastleParser::new(true);

        let first = parser
            .parse_and_encode(r#"{"user":"alice"}"#)
            .expect("encode failed");
        assert_eq!(first.dict_type, DictType::Delta);
        assert!(!first.dictionary.is_empty(), "first message must introduce symbols");

        let second = parser
            .parse_and_encode(r#"{"user":"alice"}"#)
            .expect("encode failed");
        assert_eq!(second.dict_type, DictType::Delta);
        assert!(
            second.dictionary.is_empty(),
            "repeating known vocabulary must not resend any dictionary entries, got {:?}",
            second.dictionary
        );

        // And it must still decode correctly against the accumulated dictionary.
        let decoded = parser.receive_and_decode(&second).expect("decode failed");
        assert_eq!(decoded, r#"{"user":"alice"}"#);
    }

    #[test]
    fn test_serialize_deserialize_encoded_payload_roundtrips() {
        let mut dict = Dictionary::new();
        let encoded = encode_json_with_dict(
            r#"{"user":"alice","note":"hello, world"}"#,
            &mut dict,
            0,
        );

        let wire_bytes = serialize_encoded_payload(&encoded);
        let restored = deserialize_encoded_payload(&wire_bytes).expect("deserialize failed");

        assert_eq!(restored.version, encoded.version);
        assert_eq!(restored.dict_type, encoded.dict_type);
        assert_eq!(restored.dictionary, encoded.dictionary);
        assert_eq!(restored.encoded_data, encoded.encoded_data);
    }

    #[test]
    fn test_deserialize_encoded_payload_truncated_input_never_panics() {
        // Same fuzz-style discipline as compression/response.rs's truncated-
        // input test: a malformed/truncated wire payload must error, not panic.
        let valid = {
            let mut dict = Dictionary::new();
            let encoded = encode_json_with_dict(r#"{"a":"b"}"#, &mut dict, 0);
            serialize_encoded_payload(&encoded)
        };

        for cut in 0..valid.len() {
            let _ = deserialize_encoded_payload(&valid[..cut]);
        }
        let _ = deserialize_encoded_payload(&[]);
        let _ = deserialize_encoded_payload(&[0xFF; 3]);
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
