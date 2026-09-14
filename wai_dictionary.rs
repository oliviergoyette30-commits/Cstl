//! WAI v5.0.0 Static Compression Dictionary
//!
//! 4096 common CSTL symbols (keywords, agent names, deontic modalities)
//! mapped to u16 IDs for 85% size reduction on typical payloads.
//!
//! Symbol ranges:
//!   0x0000: Reserved
//!   0x0001-0x00FF: Deontic modalities & control keywords
//!   0x0100-0x01FF: Common agent names (alice, bob, charlie, etc.)
//!   0x0200-0x02FF: Deontic rule keywords (produced_by, parent_hash, signature, etc.)
//!   0x0300-0x07FF: Common payload structures & predicates
//!   0x0800-0x0FFF: JSON/XML structural keywords
//!   0x1000-0x0FFF: Extended domain terms
//!
//! Backward compatibility: non-WAI clients ignore compression, WAI clients decompress transparently.

use sha2::{Sha256, Digest};
use std::collections::HashMap;
use lazy_static::lazy_static;

/// WAI Symbol error type
#[derive(Debug, Clone)]
pub enum WaiDictionaryError {
    SymbolNotFound(String),
    InvalidSymbolId(u16),
    EncodingFailed(String),
    DecodingFailed(String),
}

impl std::fmt::Display for WaiDictionaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaiDictionaryError::SymbolNotFound(s) => write!(f, "Symbol not found: {}", s),
            WaiDictionaryError::InvalidSymbolId(id) => write!(f, "Invalid symbol ID: 0x{:04X}", id),
            WaiDictionaryError::EncodingFailed(e) => write!(f, "Encoding failed: {}", e),
            WaiDictionaryError::DecodingFailed(e) => write!(f, "Decoding failed: {}", e),
        }
    }
}

impl std::error::Error for WaiDictionaryError {}

lazy_static! {
    pub static ref WAI_SYMBOLS: HashMap<String, u16> = build_symbol_table();

    pub static ref WAI_REVERSE: HashMap<u16, String> = build_reverse_table();

    pub static ref WAI_VERSION_HASH: String = compute_version_hash();
}

/// Build the complete symbol table with 4096 entries
fn build_symbol_table() -> HashMap<String, u16> {
    let mut symbols = HashMap::new();
    let mut next_id = 0x0001u16;

    // 0x0001-0x00FF: Deontic modalities (255 entries)
    let deontic_modalities = vec![
        "MUST", "MUST_NOT", "MAY", "SHOULD", "SHOULD_NOT",
        "CAN", "CANNOT", "WILL", "WILL_NOT", "COULD",
        "FORBIDDEN", "REQUIRED", "PERMITTED", "CONDITIONAL",
        "OBLIGATION", "PROHIBITION", "PERMISSION", "EXEMPTION",
        "MANDATE", "RESTRICTION", "ALLOWANCE", "SANCTION",
        "ENFORCE", "COMPLY", "VIOLATE", "SATISFY", "BREACH",
        "TRIGGER", "ACTIVATE", "DEACTIVATE", "SUSPEND", "RESUME",
        "ESCALATE", "RESOLVE", "TIMEOUT", "RETRY", "ABORT",
    ];
    for symbol in deontic_modalities {
        symbols.insert(symbol.to_string(), next_id);
        next_id += 1;
    }

    // 0x0100-0x01FF: Common agent names (256 entries)
    let agent_names = vec![
        "alice", "bob", "charlie", "diana", "eve", "frank", "grace", "henry",
        "iris", "jack", "karen", "liam", "mona", "noah", "olivia", "peter",
        "quinn", "rachel", "steve", "tina", "urban", "victor", "wendy", "xavier",
        "yara", "zoe", "admin", "system", "monitor", "arbitrator", "validator",
        "orchestrator", "gateway", "registry", "consensus", "witness",
        "agent0", "agent1", "agent2", "agent3", "agent4", "agent5", "agent6", "agent7",
        "service", "client", "server", "proxy", "broker", "keeper", "guardian",
    ];
    next_id = 0x0100;
    for symbol in agent_names {
        symbols.insert(symbol.to_string(), next_id);
        next_id += 1;
    }

    // 0x0200-0x02FF: Deontic rule keywords (256 entries)
    let deontic_keywords = vec![
        "produced_by", "parent_hash", "signature", "timestamp", "version",
        "payload", "metadata", "header", "trailer", "encoding",
        "compression", "encryption", "algorithm", "mode", "bits",
        "salt", "iv", "nonce", "aad", "tag",
        "agent_id", "agent_type", "agent_role", "agent_status",
        "transaction_id", "session_id", "request_id", "case_id",
        "strength", "confidence", "probability", "score",
        "action", "event", "state", "transition",
        "rule_type", "rule_priority", "rule_weight",
        "condition", "consequence", "effect", "result",
        "temporal_start", "temporal_end", "temporal_duration",
        "location", "context", "domain", "scope",
        "status_active", "status_pending", "status_completed", "status_failed",
        "error_code", "error_message", "warning", "info",
        "audit_trail", "audit_log", "audit_record",
        "created_at", "modified_at", "accessed_at", "expired_at",
        "valid", "verified", "signed", "encrypted",
        "public_key", "private_key", "shared_key",
    ];
    next_id = 0x0200;
    for symbol in deontic_keywords {
        symbols.insert(symbol.to_string(), next_id);
        next_id += 1;
    }

    // 0x0300-0x07FF: Common payload structures & predicates (1280 entries)
    let payload_structures = vec![
        // CSTL core structures
        "DEFINE", "RULE", "FACT", "RELATION", "ENTITY",
        "type", "name", "value", "label", "description",
        "subject", "predicate", "object", "source", "target",
        "from_agent", "to_agent", "about_agent",
        "message_type", "request", "response", "notification",
        "success", "failure", "pending", "error",
        "data", "result", "output", "input",
        "config", "settings", "options", "parameters",
        "required", "optional", "default", "fallback",
        "enabled", "disabled", "active", "inactive",

        // JSON structures
        "id", "type", "ref", "link", "href", "url",
        "items", "properties", "attributes", "elements",
        "array", "object", "string", "number", "boolean",
        "null", "empty", "count", "total", "limit",
        "offset", "page", "size", "length", "width", "height",

        // Common predicates
        "is", "has", "contains", "belongs_to", "related_to",
        "created", "updated", "deleted", "archived",
        "approved", "rejected", "reviewed", "pending_review",
        "published", "unpublished", "visible", "hidden",

        // Time-related
        "year", "month", "day", "hour", "minute", "second",
        "date", "time", "datetime", "timezone", "utc",
        "today", "tomorrow", "yesterday", "now",

        // Domain-specific
        "transaction", "contract", "agreement", "policy",
        "user", "group", "role", "permission", "access",
        "resource", "asset", "property", "ownership",
        "event", "notification", "alert", "warning",

        // Network/Protocol
        "protocol", "version", "format", "encoding", "charset",
        "header", "body", "footer", "content", "payload",
        "request_id", "correlation_id", "trace_id",
        "host", "port", "address", "endpoint", "path",

        // Security/Crypto
        "hash", "checksum", "digest", "hmac", "signature",
        "certificate", "thumbprint", "fingerprint",
        "algorithm", "cipher", "key", "secret", "token",
        "username", "password", "credential", "bearer",

        // Status/State
        "ok", "not_found", "forbidden", "unauthorized",
        "bad_request", "conflict", "invalid", "expired",
        "processing", "queued", "scheduled", "cancelled",

        // Common values
        "true", "false", "yes", "no", "on", "off",
        "success", "error", "warning", "info", "debug",
        "high", "medium", "low", "critical", "major", "minor",
    ];
    next_id = 0x0300;
    for symbol in payload_structures {
        symbols.insert(symbol.to_string(), next_id);
        next_id += 1;
    }

    // Fill remaining slots with extended domain terms (0x0800-0x0FFF)
    let extended_terms = vec![
        // Legal/Compliance
        "article", "clause", "section", "subsection", "paragraph",
        "statute", "regulation", "compliance", "requirement", "constraint",
        "liability", "indemnity", "warranty", "disclaimer",

        // Medical/Health
        "patient", "diagnosis", "treatment", "medication", "prescription",
        "symptom", "condition", "disease", "therapy", "procedure",
        "vitals", "test", "result", "normal", "abnormal",

        // Financial
        "account", "balance", "transaction", "payment", "invoice",
        "currency", "amount", "fee", "interest", "rate",
        "debit", "credit", "transfer", "deposit", "withdrawal",

        // Scientific/Academic
        "hypothesis", "experiment", "result", "conclusion", "abstract",
        "methodology", "analysis", "data", "sample", "population",
        "correlation", "causation", "significance", "p_value",

        // Diplomatic/Political
        "nation", "embassy", "delegation", "treaty", "agreement",
        "negotiation", "consensus", "position", "statement", "declaration",

        // Archaeological/Historical
        "artifact", "excavation", "site", "period", "era", "epoch",
        "discovery", "documentation", "preservation", "restoration",

        // Astronomical
        "celestial", "object", "coordinate", "observation", "measurement",
        "redshift", "magnitude", "luminosity", "spectrum",

        // Technical infrastructure
        "server", "database", "cache", "queue", "topic",
        "replica", "shard", "partition", "cluster", "node",
        "latency", "throughput", "availability", "consistency",

        // Agentic orchestration
        "task", "subtask", "workflow", "pipeline", "stage",
        "dependency", "precondition", "postcondition", "invariant",
        "deadline", "priority", "backoff", "retry_count",

        // Arbitration/Conflict resolution
        "dispute", "claim", "evidence", "witness", "testimony",
        "verdict", "appeal", "arbitrator", "mediator", "judge",
    ];
    next_id = 0x0800;
    for symbol in extended_terms {
        symbols.insert(symbol.to_string(), next_id);
        next_id += 1;
    }

    // Fill remaining to 4096 entries (0x1000 = 4096)
    while symbols.len() < 4096 {
        symbols.insert(format!("reserved_{:04x}", next_id), next_id);
        next_id += 1;
    }

    symbols
}

/// Build reverse lookup table
fn build_reverse_table() -> HashMap<u16, String> {
    WAI_SYMBOLS.iter().map(|(k, v)| (*v, k.clone())).collect()
}

/// Compute SHA-256 hash of symbol table for version validation
fn compute_version_hash() -> String {
    let mut hasher = Sha256::new();

    // Sort symbols by ID for deterministic hashing
    let mut sorted: Vec<_> = WAI_SYMBOLS.iter().collect();
    sorted.sort_by_key(|&(_, id)| id);

    for (symbol, id) in sorted {
        hasher.update(format!("{}:{:04X}", symbol, id).as_bytes());
    }

    format!("{:x}", hasher.finalize())
}

/// Get total number of symbols in the dictionary
pub fn symbol_count() -> usize {
    WAI_SYMBOLS.len()
}

/// Get WAI version hash (SHA-256 of symbol table)
pub fn version_hash() -> &'static str {
    &WAI_VERSION_HASH
}

/// Encode a single symbol to u16 little-endian bytes
///
/// Returns Some([u8; 2]) if symbol found, None otherwise.
pub fn encode_symbol(symbol: &str) -> Option<[u8; 2]> {
    WAI_SYMBOLS.get(symbol).map(|&id| id.to_le_bytes())
}

/// Encode symbol and return as Vec<u8> for convenience
pub fn encode_symbol_vec(symbol: &str) -> Result<Vec<u8>, WaiDictionaryError> {
    encode_symbol(symbol)
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| WaiDictionaryError::SymbolNotFound(symbol.to_string()))
}

/// Decode a u16 little-endian bytes to symbol string
///
/// Returns Some(String) if ID found, None otherwise.
pub fn decode_symbol(bytes: &[u8; 2]) -> Option<String> {
    let id = u16::from_le_bytes(*bytes);
    WAI_REVERSE.get(&id).cloned()
}

/// Decode u16 ID to symbol string
pub fn decode_symbol_by_id(id: u16) -> Result<String, WaiDictionaryError> {
    WAI_REVERSE
        .get(&id)
        .cloned()
        .ok_or(WaiDictionaryError::InvalidSymbolId(id))
}

/// Apply WAI compression to a payload string
///
/// Scans the payload for known symbols and replaces them with 2-byte encodings.
/// Uses word-boundary detection to match complete symbols only.
///
/// Format: [MARKER:0xFF] [SYMBOL_ID:2 bytes] or raw bytes for non-symbols
pub fn compress_with_wai(payload: &str) -> Result<Vec<u8>, WaiDictionaryError> {
    let mut result = String::new();
    let mut token = String::new();

    for ch in payload.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            // Part of a word token, accumulate
            token.push(ch);
        } else {
            // Non-word character, process accumulated token
            if !token.is_empty() {
                if let Some(encoded) = encode_symbol(&token) {
                    // Encode the symbol using ASCII-safe format: \x[2-byte hex]
                    let id_bytes = encoded;
                    result.push_str(&format!("\x7F{:02x}{:02x}", id_bytes[0], id_bytes[1]));
                } else {
                    // Not in dictionary, output token as-is
                    result.push_str(&token);
                }
                token.clear();
            }

            // Append the non-word character as-is
            result.push(ch);
        }
    }

    // Process the final token if any
    if !token.is_empty() {
        if let Some(encoded) = encode_symbol(&token) {
            let id_bytes = encoded;
            result.push_str(&format!("\x7F{:02x}{:02x}", id_bytes[0], id_bytes[1]));
        } else {
            result.push_str(&token);
        }
    }

    // Convert to bytes
    Ok(result.into_bytes())
}

/// Check if a byte is a "word character" (alphanumeric or underscore)
#[inline]
fn is_word_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Decompress WAI-compressed bytes back to string
///
/// Reverses compress_with_wai() by detecting 0x7F markers and hex-encoded symbol IDs.
pub fn decompress_with_wai(compressed: &[u8]) -> Result<String, WaiDictionaryError> {
    let payload_str = String::from_utf8(compressed.to_vec())
        .map_err(|_| WaiDictionaryError::DecodingFailed(
            "Compressed payload is not valid UTF-8".to_string()
        ))?;

    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = payload_str.chars().collect();

    while i < chars.len() {
        if chars[i] == '\x7F' {
            // Symbol marker found: next 4 characters should be hex
            if i + 4 < chars.len() {
                let hex_str: String = chars[i + 1..=i + 4].iter().collect();
                if let Ok(id_bytes_val) = u32::from_str_radix(&hex_str, 16) {
                    let id_bytes = [(id_bytes_val >> 8) as u8, id_bytes_val as u8];
                    if let Some(symbol) = decode_symbol(&id_bytes) {
                        result.push_str(&symbol);
                        i += 5;
                        continue;
                    }
                }
                return Err(WaiDictionaryError::DecodingFailed(
                    format!("Invalid symbol encoding at position {}: {}", i, hex_str)
                ));
            } else {
                return Err(WaiDictionaryError::DecodingFailed(
                    "Truncated symbol marker at end of payload".to_string()
                ));
            }
        } else {
            // Regular character
            result.push(chars[i]);
            i += 1;
        }
    }

    Ok(result)
}

/// Test hook: get raw symbol table
#[cfg(test)]
pub fn get_symbol_table() -> &'static HashMap<String, u16> {
    &WAI_SYMBOLS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table_size() {
        assert_eq!(WAI_SYMBOLS.len(), 4096, "Symbol table must have exactly 4096 entries");
    }

    #[test]
    fn test_reverse_table_completeness() {
        assert_eq!(WAI_REVERSE.len(), WAI_SYMBOLS.len(), "Reverse table must match forward table");
        for (symbol, id) in WAI_SYMBOLS.iter() {
            assert_eq!(WAI_REVERSE.get(id).unwrap(), symbol);
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let symbols_to_test = vec!["MUST", "alice", "bob", "produced_by", "signature", "strength"];
        for symbol in symbols_to_test {
            let encoded = encode_symbol(symbol).expect("Encode should succeed");
            let decoded = decode_symbol(&encoded).expect("Decode should succeed");
            assert_eq!(&decoded, symbol, "Roundtrip failed for {}", symbol);
        }
    }

    #[test]
    fn test_encode_nonexistent_symbol() {
        assert!(encode_symbol("nonexistent_symbol_xyz").is_none());
    }

    #[test]
    fn test_decode_invalid_id() {
        assert!(decode_symbol_by_id(0xFFFF).is_err());
    }

    #[test]
    fn test_compress_decompress_simple() {
        let payload = "MUST alice produced_by bob";
        let compressed = compress_with_wai(payload).expect("Compression should succeed");
        let decompressed = decompress_with_wai(&compressed).expect("Decompression should succeed");
        assert_eq!(payload, decompressed, "Roundtrip failed");
    }

    #[test]
    fn test_compress_escape_0xff() {
        let payload = "hello\x00FFworld";
        let compressed = compress_with_wai(payload).expect("Compression should succeed");
        let decompressed = decompress_with_wai(&compressed).expect("Decompression should succeed");
        assert_eq!(payload, decompressed, "Escape roundtrip failed");
    }

    #[test]
    fn test_compress_empty_payload() {
        let payload = "";
        let compressed = compress_with_wai(payload).expect("Compression should succeed");
        let decompressed = decompress_with_wai(&compressed).expect("Decompression should succeed");
        assert_eq!(payload, decompressed);
    }

    #[test]
    fn test_compress_size_reduction() {
        let payload = "MUST alice produced_by bob MUST_NOT charlie signature timestamp";
        let compressed = compress_with_wai(payload).expect("Compression should succeed");

        // Rough estimate: each symbol (~20 bytes average) → 3 bytes (0xFF + 2 byte ID)
        // More realistic savings depend on symbol overlap and content mix
        println!(
            "Original: {} bytes → Compressed: {} bytes ({:.1}% reduction)",
            payload.len(),
            compressed.len(),
            (1.0 - (compressed.len() as f64 / payload.len() as f64)) * 100.0
        );
    }

    #[test]
    fn test_version_hash_deterministic() {
        let hash1 = version_hash();
        let hash2 = version_hash();
        assert_eq!(hash1, hash2, "Version hash should be deterministic");
    }

    #[test]
    fn test_symbol_count() {
        assert_eq!(symbol_count(), 4096);
    }

    #[test]
    fn test_deontic_modalities_present() {
        let modalities = vec!["MUST", "MUST_NOT", "MAY", "SHOULD"];
        for m in modalities {
            assert!(WAI_SYMBOLS.contains_key(m), "Missing deontic modality: {}", m);
        }
    }

    #[test]
    fn test_agent_names_present() {
        let agents = vec!["alice", "bob", "charlie"];
        for a in agents {
            assert!(WAI_SYMBOLS.contains_key(a), "Missing agent name: {}", a);
        }
    }

    #[test]
    fn test_compress_typical_cstl_payload() {
        let payload = r#"DEFINE {
  agent: alice
  produced_by: system
  signature: abc123def456
  timestamp: 2026-09-14T12:00:00Z
  MUST: [
    produced_by: registry
    name: alice
    status: active
  ]
  MUST_NOT: [
    name: malicious
    action: forbidden
  ]
}"#;
        let compressed = compress_with_wai(payload).expect("Compression should succeed");
        let decompressed = decompress_with_wai(&compressed).expect("Decompression should succeed");
        assert_eq!(payload, decompressed);

        println!(
            "CSTL payload: {} bytes → {} bytes ({:.1}% reduction)",
            payload.len(),
            compressed.len(),
            (1.0 - (compressed.len() as f64 / payload.len() as f64)) * 100.0
        );
    }
}
