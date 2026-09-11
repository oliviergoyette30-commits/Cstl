# Rust Implementation Sketch - Dictionary Encoder/Decoder

## 1. Core Data Structures

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dictionary {
    symbols: Vec<(u16, String)>,  // (symbol_id, value)
    lookup: HashMap<String, u16>,  // String → symbol_id
    version: u32,
}

pub struct EncodedPayload {
    pub version: u32,
    pub dict_type: DictType,  // Full | Delta
    pub dictionary: Vec<(u16, String)>,
    pub encoded_data: Vec<u8>,
}

pub enum DictType { Full, Delta }
```

## 2. Encoding Pipeline

```rust
pub fn encode_json_with_dict(
    json_input: &str,
    dict: &mut Dictionary,
    prev_dict_version: u32,
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
            Token::Structural(ch) => output.push(*ch as u8),
            Token::Literal(val) => output.extend(val.as_bytes()),
        }
    }
    
    (output, new_count)
}

fn encode_symbol_id(id: u16) -> Vec<u8> {
    if id < 256 {
        vec![id as u8]  // 1 byte
    } else {
        vec![(id >> 8) as u8, (id & 0xFF) as u8]  // 2 bytes
    }
}
```

## 3. Decoding Pipeline

```rust
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
                prev_dict.symbols.push((*id, value.clone()));
            }
        }
    }
    
    // Step 2: Decode data section
    let decoded = decode_data_section(&payload.encoded_data, prev_dict)?;
    prev_dict.version = payload.version;
    
    Ok(decoded)
}

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

fn decode_symbol_id(bytes: &[u8]) -> (u16, usize) {
    if bytes[0] < 128 {
        (bytes[0] as u16, 1)
    } else {
        let id = ((bytes[0] as u16) << 8) | (bytes[1] as u16);
        (id, 2)
    }
}
```

## 4. Parser Integration

```rust
pub struct CastleParser {
    dictionary: Dictionary,
    compression_enabled: bool,
}

impl CastleParser {
    pub fn new(compression_enabled: bool) -> Self {
        CastleParser {
            dictionary: Dictionary::new(),
            compression_enabled,
        }
    }

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

    pub fn receive_and_decode(&mut self, payload: &EncodedPayload) -> Result<String, ParseError> {
        decode_encoded_payload(payload, &mut self.dictionary)
            .map_err(|e| ParseError::DecodeError(format!("{:?}", e)))
    }
}
```

## 5. Performance Optimizations

- HashMap lookup (encoding): O(1) average
- Vec iteration (decoding): O(n) per message, acceptable for <500 symbols
- For large dicts (1000+ symbols): use BTreeMap for better cache locality
- Parallel encoding: use rayon::prelude for large payloads

## 6. Integration Checklist

- [ ] Add Dictionary struct to lib.rs
- [ ] Implement encode_json_with_dict() and decode_encoded_payload()
- [ ] Add variable-length encoding/decoding
- [ ] Modify CastleParser to accept compression_enabled flag
- [ ] Add serde serialization/deserialization
- [ ] Write encode/decode roundtrip tests
- [ ] Benchmark compression ratio on real payloads
- [ ] Measure latency impact (target: <2ms per message)
- [ ] Add feature flag: --features=compression
- [ ] Document dictionary refresh strategy (every 500 messages?)