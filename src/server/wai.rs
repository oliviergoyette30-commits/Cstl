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
    pub fn new(mut symbols: Vec<(u16, String)>) -> Self {
        // FIX 2026-11-17 : le commentaire "hash from sorted symbols" ci-dessous
        // n'etait pas respecte -- aucun tri n'etait fait, donc le meme jeu de
        // symboles fourni dans un autre ordre donnait une autre empreinte, ce
        // qui casse l'adressage par contenu entre agents. Tri canonique par
        // (id, valeur). Le dictionnaire standard v5.0.0 etant deja trie et sans
        // doublon, son empreinte (f906a301...) est inchangee (verifie par test).
        symbols.sort();

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

    /// Create the static CSTL v5.0.0 standard dictionary (221 symbols pre-compiled)
    /// This dictionary is immutable, designed to be embedded at startup and
    /// referenced by content hash across all agents.
    pub fn new_standard_cstl_v5_0_0() -> Self {
        let symbols = vec![
            // Couches
            (0, "CSTL".to_string()),
            (1, "Layer".to_string()),
            (2, "CASTLE".to_string()),
            (3, "WAI".to_string()),
            (4, "v5.0.0".to_string()),
            // Cryptographie
            (5, "Ed25519".to_string()),
            (6, "signature".to_string()),
            (7, "SHA-256".to_string()),
            (8, "hash".to_string()),
            (9, "public_key".to_string()),
            (10, "private_key".to_string()),
            (11, "verification".to_string()),
            (12, "verified".to_string()),
            (13, "invalid".to_string()),
            (14, "rotation_signature".to_string()),
            (15, "NFC".to_string()),
            (16, "BTreeMap".to_string()),
            // Registre et Agent
            (17, "registry".to_string()),
            (18, "agent".to_string()),
            (19, "AgentCard".to_string()),
            (20, "register".to_string()),
            (21, "upsert".to_string()),
            (22, "dynamic".to_string()),
            (23, "enrollment".to_string()),
            (24, "name".to_string()),
            (25, "capabilities".to_string()),
            (26, "trust_score".to_string()),
            (27, "sender".to_string()),
            (28, "receiver".to_string()),
            // Protocol
            (29, "payload".to_string()),
            (30, "message".to_string()),
            (31, "purpose".to_string()),
            (32, "agent_register".to_string()),
            (33, "communication".to_string()),
            (34, "META".to_string()),
            (35, "INTENT_PAYLOAD".to_string()),
            (36, "RELATION".to_string()),
            (37, "timestamp".to_string()),
            (38, "MODE".to_string()),
            (39, "PARENT_HASH".to_string()),
            // Compression
            (40, "compression".to_string()),
            (41, "compression_ratio".to_string()),
            (42, "dictionary".to_string()),
            (43, "symbols".to_string()),
            (44, "encoded".to_string()),
            (45, "fallback".to_string()),
            (46, "version_hash".to_string()),
            (47, "bytes".to_string()),
            (48, "token".to_string()),
            (49, "index".to_string()),
            // Gouvernance
            (50, "governance".to_string()),
            (51, "restricted_council".to_string()),
            (52, "CircuitBreaker".to_string()),
            (53, "drift_window".to_string()),
            (54, "breaker_window".to_string()),
            (55, "threshold".to_string()),
            (56, "decision".to_string()),
            (57, "quorum".to_string()),
            (58, "majority".to_string()),
            (59, "vote".to_string()),
            // Audit
            (60, "audit_trail".to_string()),
            (61, "chain".to_string()),
            (62, "immutable".to_string()),
            (63, "chain_validation".to_string()),
            (64, "continuity".to_string()),
            (65, "entanglement".to_string()),
            // Storage
            (66, "SQLite".to_string()),
            (67, "persistence".to_string()),
            (68, "adn_store".to_string()),
            (69, "database".to_string()),
            (70, "seed".to_string()),
            (71, "load".to_string()),
            (72, "save".to_string()),
            (73, "Connection".to_string()),
            // Validation
            (74, "validator".to_string()),
            (75, "validation".to_string()),
            (76, "format".to_string()),
            (77, "semantic".to_string()),
            (78, "error".to_string()),
            (79, "E306".to_string()),
            (80, "E307".to_string()),
            (81, "E309".to_string()),
            (82, "E310".to_string()),
            // Réseau/TCP
            (83, "TCP".to_string()),
            (84, "listener".to_string()),
            (85, "connection".to_string()),
            (86, "port".to_string()),
            (87, "socket".to_string()),
            (88, "async".to_string()),
            (89, "tokio".to_string()),
            // Status/Résultats
            (90, "complete".to_string()),
            (91, "verified".to_string()),
            (92, "testing".to_string()),
            (93, "passing".to_string()),
            (94, "tests".to_string()),
            (95, "implementation".to_string()),
            (96, "operational".to_string()),
            (97, "production".to_string()),
            (98, "smoke_test".to_string()),
            // LLM/Python
            (99, "Python".to_string()),
            (100, "Anthropic".to_string()),
            (101, "Ollama".to_string()),
            (102, "Gemini".to_string()),
            (103, "LLM".to_string()),
            (104, "provider".to_string()),
            (105, "graceful_degradation".to_string()),
            (106, "API_KEY".to_string()),
            // Architecture
            (107, "parser".to_string()),
            (108, "handler".to_string()),
            (109, "router".to_string()),
            (110, "arbitrage".to_string()),
            (111, "KB_verify".to_string()),
            (112, "Wikidata".to_string()),
            (113, "entanglement_detection".to_string()),
            // Réseau versionné (WAI)
            (114, "DictionaryVersion".to_string()),
            (115, "DictionaryRegistry".to_string()),
            (116, "WaiEncodedPayload".to_string()),
            (117, "networked".to_string()),
            (118, "static_versioning".to_string()),
            (119, "cross_agent".to_string()),
            (120, "inline_fallback".to_string()),
            // Opérateurs CSTL
            (121, "==".to_string()),
            (122, "!=".to_string()),
            (123, "||".to_string()),
            (124, "&&".to_string()),
            (125, ">=".to_string()),
            (126, "<=".to_string()),
            (127, ">".to_string()),
            (128, "<".to_string()),
            (129, "ASSERT".to_string()),
            (130, "REQUIRE".to_string()),
            (131, "FORBID".to_string()),
            (132, "PERMIT".to_string()),
            (133, "AUDIT".to_string()),
            (134, "DELEGATE".to_string()),
            (135, "REVOKE".to_string()),
            (136, "ESCALATE".to_string()),
            // Flux d'exécution
            (137, "step".to_string()),
            (138, "parse".to_string()),
            (139, "validate".to_string()),
            (140, "route".to_string()),
            (141, "detect_emergence".to_string()),
            (142, "council_decision".to_string()),
            (143, "response".to_string()),
            (144, "reject".to_string()),
            (145, "accept".to_string()),
            // Champs spécialisés
            (146, "ack".to_string()),
            (147, "rejected".to_string()),
            (148, "reason".to_string()),
            (149, "missing_signature".to_string()),
            (150, "verification_failed".to_string()),
            (151, "signature_rejected".to_string()),
            // Données
            (152, "data".to_string()),
            (153, "hex".to_string()),
            (154, "bytes".to_string()),
            (155, "encoding".to_string()),
            (156, "UTF-8".to_string()),
            (157, "ASCII".to_string()),
            (158, "base64".to_string()),
            // Opérateurs logiques textuels
            (159, "if".to_string()),
            (160, "then".to_string()),
            (161, "else".to_string()),
            (162, "for".to_string()),
            (163, "while".to_string()),
            (164, "match".to_string()),
            (165, "case".to_string()),
            // Termes de performance
            (166, "throughput".to_string()),
            (167, "latency".to_string()),
            (168, "performance".to_string()),
            (169, "benchmark".to_string()),
            (170, "46.5%".to_string()),
            (171, "25.3%".to_string()),
            (172, "milliseconds".to_string()),
            // Conclusions clés
            (173, "three_features".to_string()),
            (174, "OWASP_ASI03".to_string()),
            (175, "OWASP_ASI07".to_string()),
            (176, "identity_verification".to_string()),
            (177, "inter_agent_communication".to_string()),
            (178, "deterministic".to_string()),
            (179, "canonical".to_string()),
            (180, "order_independent".to_string()),
            (181, "backward_compatible".to_string()),
            // État du système
            (182, "commit".to_string()),
            (183, "GitHub".to_string()),
            (184, "origin/main".to_string()),
            (185, "unpushed".to_string()),
            (186, "clean".to_string()),
            (187, "working_tree".to_string()),
            (188, "263_tests".to_string()),
            (189, "unit_tests".to_string()),
            (190, "all_passing".to_string()),
            // Termes avancés
            (191, "Byzantine_Fault_Tolerance".to_string()),
            (192, "2/3_quorum".to_string()),
            (193, "voting".to_string()),
            (194, "consensus".to_string()),
            (195, "distributed".to_string()),
            (196, "decentralized".to_string()),
            (197, "trustless".to_string()),
            (198, "verifiable".to_string()),
            // Valeurs/Métriques
            (199, "1h".to_string()),
            (200, "10min".to_string()),
            (201, "5000".to_string()),
            (202, "50000".to_string()),
            (203, "335_lines".to_string()),
            (204, "488_lines".to_string()),
            (205, "343_lines".to_string()),
            (206, "13.8_KB".to_string()),
            // États finaux
            (207, "ready".to_string()),
            (208, "deployed".to_string()),
            (209, "stable".to_string()),
            (210, "v5.1_optional".to_string()),
            (211, "roadmap".to_string()),
            (212, "331_hours".to_string()),
            // Ponctuation/Opérateurs
            (213, "[".to_string()),
            (214, "]".to_string()),
            (215, "=".to_string()),
            (216, ",".to_string()),
            (217, ";".to_string()),
            (218, ":".to_string()),
            (219, ".".to_string()),
            (220, "->".to_string()),
        ];

        Self::new(symbols)
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
    decode_symbols_strict(data, |id| dict.decode(id))
}

/// Helper: decode data using inline symbol map
fn decode_with_symbols(data: &[u8], lookup: &HashMap<u16, String>) -> Result<String, String> {
    decode_symbols_strict(data, |id| lookup.get(&id).map(String::as_str))
}

/// Plus grand ID de symbole representable sur le fil (15 bits).
pub const MAX_WAI_SYMBOL_ID: u16 = 0x7FFF;

/// Encode un ID de symbole au format fil : 2 octets, bit de poids fort du
/// premier octet a 1 (marqueur de symbole), puis les 15 bits de l'ID.
pub fn encode_symbol_id(id: u16) -> Result<[u8; 2], String> {
    if id > MAX_WAI_SYMBOL_ID {
        return Err(format!(
            "WAI symbol ID {} exceeds maximum {} (15-bit wire format)",
            id, MAX_WAI_SYMBOL_ID
        ));
    }
    Ok([0x80 | (id >> 8) as u8, (id & 0xFF) as u8])
}

/// Decodeur commun aux deux chemins (dictionnaire reseau / symboles inline).
///
/// Format : un octet >= 128 ouvre un ID de symbole sur 2 octets ; le bit de
/// poids fort n'est QUE le marqueur, l'ID est sur les 15 bits restants
/// (`((b0 & 0x7F) << 8) | b1`). Tout octet < 128 est un litteral ASCII
/// recopie tel quel (y compris les octets structurels < 32).
///
/// FIX 2026-11-17 : l'ID etait auparavant lu sur 16 bits, marqueur compris,
/// donc toujours >= 0x8000 -- alors que le dictionnaire standard v5.0.0
/// utilise les IDs 0..=220 : aucun de ses 221 symboles n'etait decodable.
/// Aucun dictionnaire du repo n'utilisait d'ID >= 0x8000, donc masquer le
/// marqueur ne casse aucun payload existant.
///
/// FIX 2026-11-17 : les deux decodeurs precedents perdaient des donnees EN
/// SILENCE tout en retournant `Ok` -- un ID absent du dictionnaire etait
/// simplement ignore, et un octet de symbole tronque en fin de flux etait
/// soit jete (`decode_with_dict`), soit recopie comme caractere Latin-1
/// parasite (`decode_with_symbols`). Un payload corrompu ou encode avec un
/// autre dictionnaire produisait donc un texte faux sans aucun signal.
/// Ces cas renvoient maintenant une erreur explicite.
fn decode_symbols_strict<'a, F>(data: &[u8], lookup: F) -> Result<String, String>
where
    F: Fn(u16) -> Option<&'a str>,
{
    let mut output = String::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];
        if byte >= 128 {
            let Some(&low) = data.get(i + 1) else {
                return Err(format!(
                    "Truncated WAI symbol ID at byte offset {} (high byte 0x{:02X} without low byte)",
                    i, byte
                ));
            };
            let id = (((byte & 0x7F) as u16) << 8) | (low as u16);
            match lookup(id) {
                Some(s) => output.push_str(s),
                None => {
                    return Err(format!(
                        "Unknown WAI symbol ID 0x{:04X} at byte offset {}",
                        id, i
                    ))
                }
            }
            i += 2;
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
    #[test]
    fn test_decode_roundtrip_known_symbol_and_literals() {
        let dict = DictionaryVersion::new(vec![(0x0001, "STATE".to_string())]);
        let data = [b'[', 0x80, 0x01, b']', b'\n'];
        assert_eq!(decode_with_dict(&data, &dict).unwrap(), "[STATE]\n");
    }

    #[test]
    fn test_decode_unknown_symbol_is_error_not_silent_drop() {
        let dict = DictionaryVersion::new(vec![(0x0001, "STATE".to_string())]);
        let data = [b'a', 0x80, 0x02, b'b'];
        let err = decode_with_dict(&data, &dict).unwrap_err();
        assert!(err.contains("0x0002"), "{}", err);

        let lookup: HashMap<u16, String> = HashMap::new();
        assert!(decode_with_symbols(&data, &lookup).is_err());
    }

    #[test]
    fn test_decode_truncated_symbol_is_error_on_both_paths() {
        let dict = DictionaryVersion::new(vec![(0x0001, "STATE".to_string())]);
        let data = [b'a', 0x80];
        assert!(decode_with_dict(&data, &dict).unwrap_err().contains("Truncated"));

        let mut lookup = HashMap::new();
        lookup.insert(0x0001u16, "STATE".to_string());
        // Avant le fix : renvoyait Ok("a\u{80}") -- caractere parasite.
        assert!(decode_with_symbols(&data, &lookup).unwrap_err().contains("Truncated"));
    }

    #[test]
    fn test_decode_wai_payload_uses_fallback_symbols() {
        let registry = DictionaryRegistry::new("node1".to_string());
        let payload = WaiEncodedPayload::new(
            "missing".to_string(),
            "node1".to_string(),
            vec![0x80, 0x00, b'!'],
        )
        .with_fallback(vec![(0x0000, "hi".to_string())]);
        let decoded = decode_wai_payload(&payload, &registry).unwrap();
        assert_eq!(decoded.data, "hi!");
        assert!(decoded.fallback_used);
    }
    #[test]
    fn test_standard_dictionary_hash_unchanged_by_canonical_sort() {
        // Empreinte persistee en base et citee dans docs/WAI_CASTLE_ARCHITECTURE.md.
        let d = DictionaryVersion::new_standard_cstl_v5_0_0();
        assert_eq!(
            d.version_hash,
            "f906a301891329caab7a9850f20a5ad09dbaf028e3b629c77dc1846b031c1ebd"
        );
    }

    #[test]
    fn test_dictionary_hash_independent_of_symbol_order() {
        let a = DictionaryVersion::new(vec![(0, "x".to_string()), (1, "y".to_string())]);
        let b = DictionaryVersion::new(vec![(1, "y".to_string()), (0, "x".to_string())]);
        assert_eq!(a.version_hash, b.version_hash);
    }

    #[test]
    fn test_every_standard_symbol_roundtrips_through_wire_format() {
        // Avant le fix : 0/221 symboles decodables (ID lu avec le marqueur).
        let d = DictionaryVersion::new_standard_cstl_v5_0_0();
        let mut data = Vec::new();
        let mut expected = String::new();
        for (id, value) in &d.symbols {
            data.extend_from_slice(&encode_symbol_id(*id).unwrap());
            data.push(b' ');
            expected.push_str(value);
            expected.push(' ');
        }
        assert_eq!(decode_with_dict(&data, &d).unwrap(), expected);
    }

    #[test]
    fn test_encode_symbol_id_rejects_ids_beyond_15_bits() {
        assert_eq!(encode_symbol_id(MAX_WAI_SYMBOL_ID).unwrap(), [0xFF, 0xFF]);
        assert!(encode_symbol_id(MAX_WAI_SYMBOL_ID + 1).is_err());
    }
}
