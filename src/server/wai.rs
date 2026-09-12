//! WAI — Networked Dictionary with Static Versioning (Layer 10)
//!
//! Extends CASTLE compression with a distributed, immutable dictionary registry.
//! Each dictionary version is identified by a content hash and timestamp.
//! Agents reference dictionary versions by hash, enabling cross-agent compression.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sha2::{Sha256, Digest};

/// A versioned dictionary snapshot (immutable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryVersion {
    /// SHA-256 hash of serialized symbols (version ID)
    pub version_hash: String,
    /// Timestamp when this version was created (Unix seconds)
    pub timestamp: u64,
    /// Symbol entries: (symbol_id, value)
    pub symbols: Vec<(u16, String)>,
    /// Reverse lookup: String → symbol_id
    #[serde(skip)]
    pub lookup: HashMap<String, u16>,
    /// Size in bytes (for accounting)
    pub size_bytes: usize,
}

impl DictionaryVersion {
    /// Create a new versioned dictionary from symbols
    pub fn new(symbols: Vec<(u16, String)>) -> Self {
        // Build reverse lookup
        let mut lookup = HashMap::new();
        for (id, value) in &symbols {
            lookup.insert(value.clone(), *id);
        }

        // Calculate size (rough estimate)
        let size_bytes = symbols
            .iter()
            .map(|(_, v)| v.len() + 2) // string + u16 id
            .sum();

        // Compute version hash from sorted symbols
        let serialized = serde_json::to_string(&symbols).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let version_hash = format!("{:x}", hasher.finalize());

        DictionaryVersion {
            version_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            symbols,
            lookup,
            size_bytes,
        }
    }

    /// Decode a symbol ID to its string value
    pub fn decode(&self, id: u16) -> Option<&str> {
        self.symbols
            .iter()
            .find(|(sym_id, _)| *sym_id == id)
            .map(|(_, value)| value.as_str())
    }

    /// Check if a symbol exists in this version
    pub fn contains(&self, value: &str) -> bool {
        self.lookup.contains_key(value)
    }
}

/// Registry of all dictionary versions (immutable, networked)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryRegistry {
    /// All versions indexed by version_hash
    versions: HashMap<String, DictionaryVersion>,
    /// Latest version hash
    latest_version: Option<String>,
    /// Metadata: registry ID (node identifier)
    registry_id: String,
}

impl DictionaryRegistry {
    /// Create a new empty registry
    pub fn new(registry_id: String) -> Self {
        DictionaryRegistry {
            versions: HashMap::new(),
            latest_version: None,
            registry_id,
        }
    }

    /// Register a new dictionary version
    /// Returns the version hash for referencing
    pub fn register_version(&mut self, dict: DictionaryVersion) -> String {
        let hash = dict.version_hash.clone();
        self.versions.insert(hash.clone(), dict);
        self.latest_version = Some(hash.clone());
        hash
    }

    /// Retrieve a specific dictionary version by hash
    pub fn get_version(&self, version_hash: &str) -> Option<&DictionaryVersion> {
        self.versions.get(version_hash)
    }

    /// Get the latest dictionary version
    pub fn get_latest(&self) -> Option<&DictionaryVersion> {
        self.latest_version
            .as_ref()
            .and_then(|hash| self.versions.get(hash))
    }

    /// List all registered versions (summary)
    pub fn list_versions(&self) -> Vec<VersionSummary> {
        self.versions
            .values()
            .map(|v| VersionSummary {
                version_hash: v.version_hash.clone(),
                timestamp: v.timestamp,
                symbol_count: v.symbols.len(),
                size_bytes: v.size_bytes,
            })
            .collect()
    }

    /// Get registry statistics
    pub fn stats(&self) -> RegistryStats {
        let total_size: usize = self.versions.values().map(|v| v.size_bytes).sum();
        RegistryStats {
            registry_id: self.registry_id.clone(),
            total_versions: self.versions.len(),
            total_symbols: self.versions.values().map(|v| v.symbols.len()).sum(),
            total_size_bytes: total_size,
            latest_version: self.latest_version.clone(),
        }
    }
}

/// Summary of a dictionary version (lightweight)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionSummary {
    pub version_hash: String,
    pub timestamp: u64,
    pub symbol_count: usize,
    pub size_bytes: usize,
}

/// Registry statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStats {
    pub registry_id: String,
    pub total_versions: usize,
    pub total_symbols: usize,
    pub total_size_bytes: usize,
    pub latest_version: Option<String>,
}

/// Payload encoded with WAI dictionary reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaiEncodedPayload {
    /// Reference to dictionary version by hash
    pub dict_version_hash: String,
    /// The registry ID that hosts this dictionary
    pub registry_id: String,
    /// Encoded data using referenced dictionary
    pub encoded_data: Vec<u8>,
    /// Fallback: inline symbols for unknown dict versions
    pub fallback_symbols: Option<Vec<(u16, String)>>,
}

impl WaiEncodedPayload {
    /// Create a WAI payload referencing a dictionary version
    pub fn new(
        dict_version_hash: String,
        registry_id: String,
        encoded_data: Vec<u8>,
    ) -> Self {
        WaiEncodedPayload {
            dict_version_hash,
            registry_id,
            encoded_data,
            fallback_symbols: None,
        }
    }

    /// Set fallback symbols for robustness
    pub fn with_fallback(mut self, symbols: Vec<(u16, String)>) -> Self {
        self.fallback_symbols = Some(symbols);
        self
    }
}

/// Decoding result with dictionary metadata
#[derive(Debug, Clone)]
pub struct DecodedWaiPayload {
    pub data: String,
    pub dict_version_hash: String,
    pub fallback_used: bool,
}

/// Decode WAI payload using registry
pub fn decode_wai_payload(
    payload: &WaiEncodedPayload,
    registry: &DictionaryRegistry,
) -> Result<DecodedWaiPayload, String> {
    // Try to find the referenced dictionary version
    if let Some(dict) = registry.get_version(&payload.dict_version_hash) {
        // Decode using the network dictionary
        let decoded = decode_with_dict(&payload.encoded_data, dict)?;
        return Ok(DecodedWaiPayload {
            data: decoded,
            dict_version_hash: payload.dict_version_hash.clone(),
            fallback_used: false,
        });
    }

    // Fallback: use inline symbols if provided
    if let Some(fallback_symbols) = &payload.fallback_symbols {
        let mut lookup = HashMap::new();
        for (id, value) in fallback_symbols {
            lookup.insert(*id, value.clone());
        }

        let decoded = decode_with_symbols(&payload.encoded_data, &lookup)?;
        return Ok(DecodedWaiPayload {
            data: decoded,
            dict_version_hash: payload.dict_version_hash.clone(),
            fallback_used: true,
        });
    }

    Err(format!(
        "Dictionary version {} not found and no fallback provided",
        payload.dict_version_hash
    ))
}

/// Helper: decode data using a dictionary version
fn decode_with_dict(data: &[u8], dict: &DictionaryVersion) -> Result<String, String> {
    let mut output = String::new();
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];

        // Simple marker detection: high byte = symbol ID, low byte = literal
        if byte >= 128 {
            // Multi-byte symbol ID
            if i + 1 < data.len() {
                let id = ((byte as u16) << 8) | (data[i + 1] as u16);
                if let Some(s) = dict.decode(id) {
                    output.push_str(s);
                }
                i += 2;
            } else {
                i += 1;
            }
        } else if byte < 32 && byte != b' ' {
            // Structural byte (preserved)
            output.push(byte as char);
            i += 1;
        } else {
            // Literal ASCII
            output.push(byte as char);
            i += 1;
        }
    }

    Ok(output)
}

/// Helper: decode data using inline symbol map
fn decode_with_symbols(data: &[u8], lookup: &HashMap<u16, String>) -> Result<String, String> {
    let mut output = String::new();
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];

        if byte >= 128 && i + 1 < data.len() {
            let id = ((byte as u16) << 8) | (data[i + 1] as u16);
            if let Some(s) = lookup.get(&id) {
                output.push_str(s);
            }
            i += 2;
        } else if byte < 32 && byte != b' ' {
            output.push(byte as char);
            i += 1;
        } else {
            output.push(byte as char);
            i += 1;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_version_hash() {
        let symbols = vec![(0, "hello".to_string()), (1, "world".to_string())];
        let dict1 = DictionaryVersion::new(symbols.clone());
        let dict2 = DictionaryVersion::new(symbols);

        // Same symbols should produce same hash
        assert_eq!(dict1.version_hash, dict2.version_hash);
    }

    #[test]
    fn test_registry_versions() {
        let mut registry = DictionaryRegistry::new("node1".to_string());

        let dict1 = DictionaryVersion::new(vec![(0, "msg".to_string())]);
        let hash1 = registry.register_version(dict1);

        let dict2 = DictionaryVersion::new(vec![(0, "msg".to_string()), (1, "data".to_string())]);
        let hash2 = registry.register_version(dict2);

        let stats = registry.stats();
        assert_eq!(stats.total_versions, 2);
        assert_eq!(registry.latest_version, Some(hash2.clone()));
        assert!(registry.get_version(&hash1).is_some());
        assert!(registry.get_version(&hash2).is_some());
    }

    #[test]
    fn test_wai_payload_with_fallback() {
        let payload = WaiEncodedPayload::new(
            "abc123".to_string(),
            "node1".to_string(),
            vec![1, 2, 3],
        )
        .with_fallback(vec![(0, "test".to_string())]);

        assert!(payload.fallback_symbols.is_some());
    }
}
