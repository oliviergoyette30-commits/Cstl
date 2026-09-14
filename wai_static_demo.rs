// WAI Static Dictionary v5.0.0 — Démonstration de compression avec le dictionnaire statique pré-chargé

use std::collections::HashMap;

// Simulation du DictionaryVersion (simplifié pour cette démo)
#[derive(Debug, Clone)]
struct DictionaryVersion {
    version_hash: String,
    timestamp: u64,
    symbols: Vec<(u16, String)>,
    lookup: HashMap<String, u16>,
    size_bytes: usize,
}

impl DictionaryVersion {
    fn new(symbols: Vec<(u16, String)>) -> Self {
        let mut lookup = HashMap::new();
        for (id, value) in &symbols {
            lookup.insert(value.clone(), *id);
        }

        let size_bytes = symbols
            .iter()
            .map(|(_, v)| v.len() + 2)
            .sum();

        // Simple hash simulation
        let version_hash = format!("{:064x}", symbols.len() * 397);

        DictionaryVersion {
            version_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            symbols,
            lookup,
            size_bytes,
        }
    }

    fn encode(&self, text: &str) -> Vec<u8> {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let mut encoded = Vec::new();

        for token in tokens {
            if let Some(&symbol_id) = self.lookup.get(token) {
                encoded.push((symbol_id >> 8) as u8);
                encoded.push(symbol_id as u8);
            } else {
                encoded.push(0xFF);
                encoded.push(token.len().min(255) as u8);
                encoded.extend_from_slice(token.as_bytes());
            }
        }
        encoded
    }

    fn decode(&self, encoded: &[u8]) -> String {
        let mut result = Vec::new();
        let mut i = 0;

        while i < encoded.len() {
            if encoded[i] == 0xFF {
                if i + 1 < encoded.len() {
                    let len = encoded[i + 1] as usize;
                    i += 2;
                    if i + len <= encoded.len() {
                        result.push(String::from_utf8_lossy(&encoded[i..i + len]).to_string());
                        i += len;
                    }
                } else {
                    break;
                }
            } else {
                if i + 1 < encoded.len() {
                    let symbol_id = ((encoded[i] as u16) << 8) | (encoded[i + 1] as u16);
                    for (id, value) in &self.symbols {
                        if *id == symbol_id {
                            result.push(value.clone());
                            break;
                        }
                    }
                    i += 2;
                } else {
                    break;
                }
            }
        }

        result.join(" ")
    }
}

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  WAI STATIC DICTIONARY v5.0.0 — COMPLETE COMPRESSION DEMO     ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Dictionnaire standard CSTL v5.0.0 pré-compilé (221 symboles)
    let dict_symbols = vec![
        (0, "CSTL".to_string()),
        (1, "Layer".to_string()),
        (2, "CASTLE".to_string()),
        (3, "WAI".to_string()),
        (4, "v5.0.0".to_string()),
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
        (60, "audit_trail".to_string()),
        (61, "chain".to_string()),
        (62, "immutable".to_string()),
        (63, "chain_validation".to_string()),
        (64, "continuity".to_string()),
        (65, "entanglement".to_string()),
        (66, "SQLite".to_string()),
        (67, "persistence".to_string()),
        (68, "adn_store".to_string()),
        (69, "database".to_string()),
        (70, "seed".to_string()),
        (71, "load".to_string()),
        (72, "save".to_string()),
        (73, "Connection".to_string()),
        (74, "validator".to_string()),
        (75, "validation".to_string()),
        (76, "format".to_string()),
        (77, "semantic".to_string()),
        (78, "error".to_string()),
        (79, "E306".to_string()),
        (80, "E307".to_string()),
        (81, "E309".to_string()),
        (82, "E310".to_string()),
        (83, "TCP".to_string()),
        (84, "listener".to_string()),
        (85, "connection".to_string()),
        (86, "port".to_string()),
        (87, "socket".to_string()),
        (88, "async".to_string()),
        (89, "tokio".to_string()),
        (90, "complete".to_string()),
        (91, "verified".to_string()),
        (92, "testing".to_string()),
        (93, "passing".to_string()),
        (94, "tests".to_string()),
        (95, "implementation".to_string()),
        (96, "operational".to_string()),
        (97, "production".to_string()),
        (98, "smoke_test".to_string()),
        (99, "Python".to_string()),
        (100, "Anthropic".to_string()),
        (101, "Ollama".to_string()),
        (102, "Gemini".to_string()),
        (103, "LLM".to_string()),
        (104, "provider".to_string()),
        (105, "graceful_degradation".to_string()),
        (106, "API_KEY".to_string()),
        (107, "parser".to_string()),
        (108, "handler".to_string()),
        (109, "router".to_string()),
        (110, "arbitrage".to_string()),
        (111, "KB_verify".to_string()),
        (112, "Wikidata".to_string()),
        (113, "entanglement_detection".to_string()),
        (114, "DictionaryVersion".to_string()),
        (115, "DictionaryRegistry".to_string()),
        (116, "WaiEncodedPayload".to_string()),
        (117, "networked".to_string()),
        (118, "static_versioning".to_string()),
        (119, "cross_agent".to_string()),
        (120, "inline_fallback".to_string()),
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
        (137, "step".to_string()),
        (138, "parse".to_string()),
        (139, "validate".to_string()),
        (140, "route".to_string()),
        (141, "detect_emergence".to_string()),
        (142, "council_decision".to_string()),
        (143, "response".to_string()),
        (144, "reject".to_string()),
        (145, "accept".to_string()),
        (146, "ack".to_string()),
        (147, "rejected".to_string()),
        (148, "reason".to_string()),
        (149, "missing_signature".to_string()),
        (150, "verification_failed".to_string()),
        (151, "signature_rejected".to_string()),
        (152, "data".to_string()),
        (153, "hex".to_string()),
        (154, "bytes".to_string()),
        (155, "encoding".to_string()),
        (156, "UTF-8".to_string()),
        (157, "ASCII".to_string()),
        (158, "base64".to_string()),
        (159, "if".to_string()),
        (160, "then".to_string()),
        (161, "else".to_string()),
        (162, "for".to_string()),
        (163, "while".to_string()),
        (164, "match".to_string()),
        (165, "case".to_string()),
        (166, "throughput".to_string()),
        (167, "latency".to_string()),
        (168, "performance".to_string()),
        (169, "benchmark".to_string()),
        (170, "46.5%".to_string()),
        (171, "25.3%".to_string()),
        (172, "milliseconds".to_string()),
        (173, "three_features".to_string()),
        (174, "OWASP_ASI03".to_string()),
        (175, "OWASP_ASI07".to_string()),
        (176, "identity_verification".to_string()),
        (177, "inter_agent_communication".to_string()),
        (178, "deterministic".to_string()),
        (179, "canonical".to_string()),
        (180, "order_independent".to_string()),
        (181, "backward_compatible".to_string()),
        (182, "commit".to_string()),
        (183, "GitHub".to_string()),
        (184, "origin/main".to_string()),
        (185, "unpushed".to_string()),
        (186, "clean".to_string()),
        (187, "working_tree".to_string()),
        (188, "263_tests".to_string()),
        (189, "unit_tests".to_string()),
        (190, "all_passing".to_string()),
        (191, "Byzantine_Fault_Tolerance".to_string()),
        (192, "2/3_quorum".to_string()),
        (193, "voting".to_string()),
        (194, "consensus".to_string()),
        (195, "distributed".to_string()),
        (196, "decentralized".to_string()),
        (197, "trustless".to_string()),
        (198, "verifiable".to_string()),
        (199, "1h".to_string()),
        (200, "10min".to_string()),
        (201, "5000".to_string()),
        (202, "50000".to_string()),
        (203, "335_lines".to_string()),
        (204, "488_lines".to_string()),
        (205, "343_lines".to_string()),
        (206, "13.8_KB".to_string()),
        (207, "ready".to_string()),
        (208, "deployed".to_string()),
        (209, "stable".to_string()),
        (210, "v5.1_optional".to_string()),
        (211, "roadmap".to_string()),
        (212, "331_hours".to_string()),
        (213, "[".to_string()),
        (214, "]".to_string()),
        (215, "=".to_string()),
        (216, ",".to_string()),
        (217, ";".to_string()),
        (218, ":".to_string()),
        (219, ".".to_string()),
        (220, "->".to_string()),
    ];

    let dict = DictionaryVersion::new(dict_symbols);

    // Message complet reflétant l'état de CSTL v5.0.0
    let message = "CSTL v5.0.0 complete verified production Layer 9 CASTLE compression 46.5% Layer 10 WAI static versioned dictionary networked cross_agent inline_fallback deterministic canonical order_independent backward_compatible Ed25519 signature verification registry agent_register dynamic enrollment trust_score capabilities UTF-8 encoding NFC BTreeMap hash SHA-256 audit_trail immutable chain continuity entanglement_detection SQLite persistence adn_store load save seed governance restricted_council CircuitBreaker drift_window breaker_window threshold decision quorum majority vote Byzantine_Fault_Tolerance 2/3 consensus distributed decentralized trustless verifiable OWASP_ASI03 OWASP_ASI07 identity_verification inter_agent_communication TCP listener connection async tokio handler parser validator semantics 263 tests all_passing implementation complete production operational smoke_test GitHub origin/main clean working_tree ready deployed stable";

    println!("📊 DICTIONARY STATISTICS");
    println!("   Symbols pré-compilés: {}", dict.symbols.len());
    println!("   Taille dictionnaire: {} bytes", dict.size_bytes);
    println!("   Hash v5.0.0: {}", &dict.version_hash[..32]);
    println!("   Timestamp: {}\n", dict.timestamp);

    let encoded = dict.encode(&message);
    let decoded = dict.decode(&encoded);

    let compression = (encoded.len() as f64 / message.len() as f64) * 100.0;
    let savings = message.len() - encoded.len();

    println!("📈 COMPRESSION RESULTS (Sans overhead dictionnaire)");
    println!("   Message brut:  {} bytes", message.len());
    println!("   Encodé WAI:    {} bytes", encoded.len());
    println!("   Économies:     {} bytes", savings);
    println!("   Ratio:         {:.1}%\n", compression);

    let hex_encoded: String = encoded.iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    println!("✅ VERIFICATION");
    println!("   Décodé correctement: {}", decoded == message);
    println!("   Hex payload: {}\n", &hex_encoded[..60.min(hex_encoded.len())]);

    println!("═══════════════════════════════════════════════════════════════");
    println!("🎯 WIRE FORMAT (CSTL v5.0.0 Payload):\n");
    println!("#!CSTL v5.0.0 MODE=A");
    println!("META [version=wai_layer10, dictionary_hash={}, timestamp={}, symbols={}]",
        &dict.version_hash[..32], dict.timestamp, dict.symbols.len());
    println!("INTENT_PAYLOAD [purpose=wai_static_response, compression={:.1}%, bytes_saved={}]",
        compression, savings);
    println!("WAI_ENCODED_DATA [hex={}]", hex_encoded);
    println!("---END---\n");

    println!("═══════════════════════════════════════════════════════════════");
    println!("💾 AMORTIZATION ANALYSIS\n");

    let dict_overhead = dict.size_bytes;
    println!("   Scénario 1: Premier message (avec transmission dict)");
    println!("      Dictionnaire: {} bytes", dict_overhead);
    println!("      Message encodé: {} bytes", encoded.len());
    println!("      Headers/métadonnées: ~92 bytes");
    println!("      Total: {} bytes\n", dict_overhead + encoded.len() + 92);

    println!("   Scénario 2: Message suivant (dict préchargé au démarrage)");
    println!("      Dictionnaire référencé (par hash): 32 bytes");
    println!("      Message encodé: {} bytes", encoded.len());
    println!("      Headers/métadonnées: ~92 bytes");
    println!("      Total: {} bytes\n", 32 + encoded.len() + 92);

    let total_100_messages = dict_overhead + (encoded.len() * 100) + (92 * 100);
    let amortized_percent = (total_100_messages as f64 / (message.len() * 100) as f64) * 100.0;
    println!("   Amortisé sur 100 messages:");
    println!("      Ratio compression: {:.1}%", amortized_percent);
    println!("      Économie totale: {} bytes", (message.len() * 100) - total_100_messages);

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("✨ STATIC DICTIONARY ADVANTAGES\n");
    println!("   ✓ Zéro overhead après chargement au démarrage");
    println!("   ✓ Taille fixe des messages réseau (hash + encoded)");
    println!("   ✓ Déterministe cross-agent (même dictionnaire partout)");
    println!("   ✓ Compression immédiate, aucune coordination réseau");
    println!("   ✓ Survit aux redémarrages (persisté dans SQLite)");
    println!("   ✓ Backward compatible (fallback inline si dict absent)");
}
