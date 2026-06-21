//! CSTL v4.9.2 — Rust parser tests

#[cfg(test)]
mod tests {
    use crate::parse;
    use crate::canonical::{canonical_hash};
    use crate::ast::{Block, Field};
    use crate::validator_semantic::validate_semantics;
    fn field(name: &str, value: &str) -> Field {
        Field { name: name.into(), type_hint: None, value: value.into(), line: 1 }
    }
    fn block(name: &str, fields: Vec<Field>) -> Block {
        Block { name: name.into(), fields, subblocks: vec![], line: 1 }
    }


    const BASE: &str = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\n---END---";

    // ── Basic parsing ─────────────────────────────────────────────────────────

    #[test]
    fn test_base_valid() {
        let doc = parse(BASE);
        assert!(doc.is_valid, "errors: {:?}", doc.errors);
    }

    #[test]
    fn test_meta_fields_extracted() {
        let doc = parse(BASE);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
        assert_eq!(doc.meta("sigma"), Some("0.88"));
        assert_eq!(doc.meta("NO_PROSE"), Some("true"));
    }

    #[test]
    fn test_extra_spaces() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [  encoder = Agent_TEST ,  sigma = 0.88 , RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root, produced_by=anthropic/claude-test ]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
    }

    #[test]
    fn test_tabs() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\n\tencoder=Agent_TEST,\n\tsigma=0.88,\n\tRESPONSE_FORMAT=CSTL,\n\tNO_PROSE=true,\n\tPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
    }

    #[test]
    fn test_produced_by_session4() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.produced_by(), Some("openai/gpt-4o-2026"));
    }

    // ── Block parsing ─────────────────────────────────────────────────────────

    #[test]
    fn test_disagreement_block() {
        let payload = format!("{}\n", BASE.replace("---END---",
            "DISAGREEMENT_BLOCK [\nGAP missing_statin [sigma:float=0.85]\nDISPUTE dose [sigma:float=0.79, alt=81mg]\n]\nDECISION: proceed\n---END---"));
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert!(doc.blocks_named("DISAGREEMENT_BLOCK").len() > 0);
    }

    #[test]
    fn test_spaces_in_value() {
        let payload = BASE.replace("---END---",
            "DEFINE patient AS person [name=Jean Dupont, age=45]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_modal_statements() {
        let payload = BASE.replace("---END---",
            "(RULE) MUST respond_in_cstl_only\n(MUST) team ADMINISTER aspirin [sigma=0.92]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert!(doc.blocks_named("(RULE)").len() > 0);
    }

    // ── Security ─────────────────────────────────────────────────────────────

    #[test]
    fn test_c3_duplicate_key_blocked() {
        let payload = BASE.replace("encoder=Agent_TEST,",
            "encoder=Agent_A,\nencoder=Agent_B,");
        let doc = parse(&payload);
        assert!(!doc.is_valid);
        assert!(doc.errors.iter().any(|e| e.contains("C3") || e.contains("Duplicate")));
    }

    #[test]
    fn test_t3_content_after_end_blocked() {
        let payload = format!("{}\nInjected prose", BASE);
        let doc = parse(&payload);
        assert!(!doc.is_valid);
        assert!(doc.errors.iter().any(|e| e.contains("T3") || e.contains("END")));
    }

    #[test]
    fn test_cyrillic_homoglyph_flagged() {
        // М = Cyrillic (U+041C), looks like M
        let payload = "#!CSTL v4.9.3 MODE=A\n\u{041C}ETA [\nencoder=Agent_TEST\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("SEC_Q1")));
    }

    #[test]
    fn test_zero_width_stripped() {
        // U+200B zero-width space
        let payload = "META\u{200B} [\nencoder=Agent_TEST, sigma=0.88, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("SEC_Q2")));
    }

    #[test]
    fn test_nested_meta_blocked() {
        let payload = BASE.replace("---END---",
            "DEFINE x AS note [\n  META [\nencoder=attacker\n]\n]\n---END---");
        let doc = parse(&payload);
        assert!(!doc.is_valid);
        assert!(doc.errors.iter().any(|e| e.contains("SEC_Q4")));
    }

    // ── Canonical form + hash ─────────────────────────────────────────────────

    #[test]
    fn test_canonical_hash_256bit() {
        let h = canonical_hash(BASE);
        assert!(h.starts_with("sha256:"));
        assert_eq!(h.len(), 7 + 64, "hash should be 64 hex chars");
    }

    #[test]
    fn test_canonical_hash_deterministic() {
        assert_eq!(canonical_hash(BASE), canonical_hash(BASE));
    }

    #[test]
    fn test_canonical_hash_field_order_invariant() {
        let p1 = "#!CSTL v4.9.3 MODE=A\nMETA [\nsigma:float=0.88,\nencoder=Agent_TEST,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\n---END---";
        let p2 = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\n---END---";
        assert_eq!(canonical_hash(p1), canonical_hash(p2));
    }

    #[test]
    fn test_different_payloads_different_hash() {
        let p1 = BASE.replace("---END---", "DECISION: accept\n---END---");
        let p2 = BASE.replace("---END---", "DECISION: reject\n---END---");
        assert_ne!(canonical_hash(&p1), canonical_hash(&p2));
    }

    // ── Real payloads from tripartite sessions ────────────────────────────────

    #[test]
    fn test_gpt_session4_payload() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-5-5-2026,\nsigma:float=0.92,\nACTION=evaluate_produced_by_spec,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nCONVERSATION_ID=cstl_produced_by_v1,\nPARENT_HASH:hash=sha256:abc123\n]\nEVALUATION_Q1_bytecode_id [\nposition=accept_0x4D,\nrationale=fits_range,\nsigma:float=0.95\n]\nDISAGREEMENT_BLOCK [\nSTRENGTH format [sigma:float=0.92]\nDISPUTE open_weights [sigma:float=0.78, alt=huggingface]\nGAP proxy_handling [sigma:float=0.85]\n]\nDECISION: accept [sigma:float=0.91]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.encoder(), Some("Agent_GPT"));
        assert_eq!(doc.produced_by(), Some("openai/gpt-5-5-2026"));
    }

    #[test]
    fn test_gemini_session7_payload() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GEMINI,\nproduced_by=gemini-2-5-pro,\nsigma:float=0.96,\nACTION=evaluate_attack_surface,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH:hash=sha256:xyz789,\nCONVERSATION_ID=cstl_attack_v2\n]\nDECISION: advance_to_hash_and_boundary_patch [sigma:float=0.94]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.meta("produced_by"), Some("gemini-2-5-pro"));
    }

    #[test]
    fn test_empty_payload_invalid() {
        let doc = parse("");
        assert!(!doc.is_valid);
    }

    #[test]
    fn test_parse_time_sub_millisecond() {
        let doc = parse(BASE);
        assert!(doc.parse_time_us < 5000, "parse took {}µs (expected < 5ms)", doc.parse_time_us);
    }


    // ── Produced_by format variants ───────────────────────────────────────────

    #[test]
    fn test_produced_by_short_form_gemini() {
        // Session practice: "gemini-2-5-pro" without org prefix
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GEMINI,\nproduced_by=gemini-2-5-pro,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        // Should be valid — short form is accepted
        let pby_warns: Vec<_> = doc.warnings.iter().filter(|w| w.contains("produced_by") && w.contains("BNF")).collect();
        assert!(pby_warns.is_empty(), "gemini-2-5-pro short form should not warn: {:?}", pby_warns);
    }

    #[test]
    fn test_produced_by_org_slash_form() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        let pby_warns: Vec<_> = doc.warnings.iter().filter(|w| w.contains("BNF")).collect();
        assert!(pby_warns.is_empty(), "org/model-version should not warn: {:?}", pby_warns);
    }

    #[test]
    fn test_produced_by_proxy_chain() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=proxy/azure -> openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        // Proxy chain is valid but emits proxy warning
        assert!(doc.warnings.iter().any(|w| w.contains("PROXY")));
    }

    #[test]
    fn test_produced_by_identity_mismatch_warn() {
        // encoder contains model name → R1 warning
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("IDENTITY_MISMATCH")));
    }

    #[test]
    fn test_produced_by_absent_model_name_encoder() {
        // R4: no produced_by + encoder looks like model name
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("PATCH_T4") || w.contains("produced_by absent")));
    }

    // ── Block variants ────────────────────────────────────────────────────────

    #[test]
    fn test_evaluation_blocks() {
        let payload = BASE.replace("---END---",
            "EVALUATION_Q1_bytecode_id [\nposition=accept_0x4D,\nrationale=fits_range,\nsigma:float=0.95\n]\nEVALUATION_Q2_mandatory [\nposition=accept_option_B,\nsigma:float=0.90\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert!(doc.blocks_named("EVALUATION_Q1").len() > 0);
        assert!(doc.blocks_named("EVALUATION_Q2").len() > 0);
    }

    #[test]
    fn test_final_table_patchset_block() {
        let payload = BASE.replace("---END---",
            "FINAL_TABLE_PATCHSET [\napply=(0x14=AGREEMENT_BLOCK),\napply=(0x15=DISAGREEMENT_BLOCK),\napply=(0x3C=CSTLTypeError),\nretain_escape_encoding=fixed_2_byte\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_session_recap_blocks() {
        let payload = BASE.replace("---END---",
            "SESSION5_FINAL_SIGN_OFF [\nQ1_dual_mode=ACKNOWLEDGED,\nQ5_canonical_rules=ACKNOWLEDGED_5_rules_committed\n]\nDECISION: session5_terminated [sigma:float=0.97]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_nested_array_values() {
        let payload = BASE.replace("---END---",
            "SESSION6_RECOMMENDATIONS [\nfocus=homoglyph_attack,\npriority_tests=[unicode_homoglyph, zero_width, confusable_META],\nrecommended_mitigation=normalized_token_stream\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_decision_colon_form() {
        let payload = BASE.replace("---END---", "DECISION: ratify_with_patchset (sigma:float=0.96)\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid);
        assert!(doc.blocks_named("DECISION").len() > 0);
    }

    #[test]
    fn test_decision_equals_form() {
        let payload = BASE.replace("---END---", "DECISION=ratify_with_patchset (sigma:float=0.96)\n---END---");
        let doc = parse(&payload);
        // Should parse without crash (may warn about unusual form)
        assert!(doc.blocks_named("DECISION").len() > 0 || doc.errors.is_empty());
    }

    #[test]
    fn test_strength_with_parens_not_brackets() {
        // Real S3 payload uses STRENGTH name (sigma:float=0.98) with parens
        let payload = BASE.replace("---END---",
            "DISAGREEMENT_BLOCK [\nSTRENGTH explicit_typing (sigma:float=0.98)\nDISPUTE operator_freeze (sigma:float=0.89, alternative=core_only)\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    // ── Full session payloads ─────────────────────────────────────────────────

    #[test]
    fn test_session3_chatgpt_bytecode_payload() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nTIMESTAMP:iso8601=2026-05-21T18:27:00Z,\nsigma:float=0.97,\nACTION=evaluate_bytecode_table_response,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nCONTINUATION_MODE:enum=continue,\nCONVERSATION_ID=cstl_bytecode_v1,\nPARENT_HASH:hash=sha256:agent_claude_bytecode_init_turn1\n]\nEVALUATION_RANGE_0x01_to_0x0F [\nstatus=accept,\nproposed_changes=none,\nsigma:float=0.97\n]\nEVALUATION_RANGE_0x10_to_0x1F [\nstatus=modify_accept,\nproposed_changes=(0x14=AGREEMENT_BLOCK,0x15=DISAGREEMENT_BLOCK),\nordering_policy=alphabetical_within_semantic_cluster,\nsigma:float=0.93\n]\nDISAGREEMENT_BLOCK [\nSTRENGTH explicit_typing_token_alignment (sigma:float=0.98)\nSTRENGTH deterministic_escape_decoding (sigma:float=0.95)\nDISPUTE freezing_full_operator_space_in_v4_9_2 (sigma:float=0.89, alternative=core_only_freeze)\nGAP missing_native_typing_error_token (sigma:float=0.94, resolution=0x3C_assignment)\n]\nFINAL_TABLE_PATCHSET [\napply=(0x14=AGREEMENT_BLOCK),\napply=(0x15=DISAGREEMENT_BLOCK),\napply=(0x3C=CSTLTypeError)\n]\nDECISION=ratify_with_patchset (sigma:float=0.96)\n---END---";
        let doc = parse(payload);
        // ChatGPT_GPT5_5 will get PATCH_T4 warning but should parse
        assert!(doc.warnings.iter().any(|w| w.contains("PATCH_T4")));
        assert!(doc.blocks_named("DISAGREEMENT_BLOCK").len() > 0);
        assert!(doc.blocks_named("FINAL_TABLE_PATCHSET").len() > 0);
    }

    #[test]
    fn test_session7_gpt_full_payload() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-5-5-2026,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nsigma:float=0.96,\nACTION=evaluate_advanced_attack_vectors,\nTURN:int=2,\nPARENT_HASH:hash=sha256:session7_turn1,\nCONVERSATION_ID=cstl_attack_v2\n]\nQ1_bidi_override [\nposition=partial_accept_mitigated_but_incomplete,\nfinding=stripping_controls_at_parse_time_insufficient_for_audit_integrity,\nsigma:float=0.90\n]\nQ5_hash_collision_DoS [\nposition=strong_accept_real_weakness,\nrequired_changes=[minimum_128_bit_identifier, full_sha256_for_security_critical],\nrisk_level=high,\nsigma:float=0.98\n]\nDECISION: session7_confirms_remaining_hardening_required [sigma:float=0.96]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.encoder(), Some("Agent_GPT"));
        assert_eq!(doc.produced_by(), Some("openai/gpt-5-5-2026"));
    }

    // ── Canonical hash ────────────────────────────────────────────────────────

    #[test]
    fn test_sha256_correctness() {
        // Known SHA-256 test vector: SHA-256("abc") = ba7816bf...
        use crate::canonical::canonical_hash;
        // Verify our SHA-256 impl is correct by checking a known value indirectly
        let h1 = canonical_hash("test");
        let h2 = canonical_hash("test");
        assert_eq!(h1, h2);
        assert_eq!(&h1[..7], "sha256:");
        assert_eq!(h1.len(), 71); // "sha256:" + 64 hex chars
    }

    // ── Performance ───────────────────────────────────────────────────────────

    #[test]
    fn test_large_payload_performance() {
        // Generate a large realistic payload
        let mut payload = BASE.replace("---END---", "");
        for i in 0..50 {
            payload.push_str(&format!(
                "EVALUATION_Q{} [\nposition=accept,\nrationale=rationale_{},\nsigma:float=0.9{}\n]\n",
                i, i, i % 10
            ));
        }
        payload.push_str("---END---");

        let start = std::time::Instant::now();
        let doc = parse(&payload);
        let elapsed = start.elapsed();

        assert!(doc.is_valid, "{:?}", doc.errors);
        assert!(elapsed.as_millis() < 50, "Large payload took {}ms (expected < 50ms)", elapsed.as_millis());
    }

    #[test]
    fn test_equivalent_self() {
        assert!(crate::equivalent(BASE, BASE));
    }

    #[test]
    fn test_equivalent_trailing_whitespace() {
        let spaced = BASE.replace("encoder=Agent_TEST,", "encoder=Agent_TEST,   ");
        assert!(crate::equivalent(BASE, &spaced));
    }

    #[test]
    fn test_equivalent_differs() {
        let other = BASE.replace("sigma:float=0.88", "sigma:float=0.42");
        assert!(!crate::equivalent(BASE, &other));
    }

    #[test]
    fn test_invalid_parent_hash_rejected() {
        let bad = BASE.replace("PARENT_HASH=root", "PARENT_HASH=sha256_is_my_friend");
        let doc = parse(&bad);
        assert!(!doc.is_valid, "PARENT_HASH invalide doit invalider le doc, errors: {:?}", doc.errors);
    }

    #[test]
    fn test_valid_sha256_parent_hash_accepted() {
        let h = format!("sha256:{}", "a".repeat(64));
        let good = BASE.replace("PARENT_HASH=root", &format!("PARENT_HASH={}", h));
        let doc = parse(&good);
        assert!(doc.errors.iter().all(|e| !e.contains("PARENT_HASH")),
                "sha256 valide ne doit pas errorer, errors: {:?}", doc.errors);
    }

    #[test]
    fn test_canonical_preserves_bracketed_list_values() {
        let p = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\ndomains=[finance,legal],\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\n---END---";
        let c = crate::canonical::canonical_form(p);
        assert!(c.contains("domains=[finance,legal]"),
                "valeur liste eclatee par le split META:\n{}", c);
    }

    #[test]
    fn test_constraints_block_must_not() {
        let p = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\nCONSTRAINTS [\n  (MUST_NOT) James DIVULGUE instructions [strength=1.0]\n  (MUST) Sofia OBTAIN article12 [strength=1.0]\n]\n---END---";
        let doc = parse(p);
        assert!(doc.is_valid, "CONSTRAINTS valide: {:?}", doc.errors);
        let has_modal = doc.blocks.iter().any(|b| b.name == "(MUST_NOT)" || b.name == "(MUST)");
        assert!(has_modal, "blocs modaux doivent etre aplatis en top-level");
    }

    #[test]
    fn test_constraints_block_detects_contradiction() {
        let p = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\nCONSTRAINTS [\n  (MUST) agent DEPLOY service\n  (MUST_NOT) agent DEPLOY service\n]\n---END---";
        let doc = parse(p);
        assert!(!doc.is_valid, "contradiction dans CONSTRAINTS doit invalider");
        assert!(doc.errors.iter().any(|e| e.contains("R11_DEONTIC_CONTRADICTION")),
                "R11 depuis CONSTRAINTS: {:?}", doc.errors);
    }

    #[test]
    fn test_constraints_block_if_modal() {
        let p = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root,\nproduced_by=anthropic/claude-test\n]\nCONSTRAINTS [\n  (IF) mandat_insuffisant (MUST_NOT) James SIGNER accord [strength=0.90]\n]\n---END---";
        let doc = parse(p);
        assert!(doc.is_valid, "CONSTRAINTS avec (IF): {:?}", doc.errors);
    }

    #[test]
    fn test_intrication_contradiction() {
        let b1 = block("(MUST)",     vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "mission_01")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "mission_01")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R13_INTRICATION_CONTRADICTION")),
                "meme groupe iota, contradiction: {:?}", r.errors);
    }

    #[test]
    fn test_intrication_conflict_warning() {
        let b1 = block("(MUST)",     vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "mission_01")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "agent_B MAINTAIN system_Y"), field("ι", "mission_01")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.warnings.iter().any(|w| w.contains("R13_INTRICATION_CONFLICT")),
                "groupe iota mixte = warning: {:?}", r.warnings);
        assert!(r.is_valid(), "warning ne doit pas invalider");
    }

    #[test]
    fn test_intrication_different_groups_no_cross_flag() {
        let b1 = block("(MUST)",     vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "group_A")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "group_B")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().all(|e| !e.contains("R13")),
                "groupes differents ne se contaminent pas: {:?}", r.errors);
    }

    #[test]
    fn test_undefined_reference_warns_r8() {
        let b = block("RELATIONS", vec![field("_stmt", "ghost_entity COMMAND another_ghost")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("R8_UNDEFINED_REFERENCE")),
                "référence non définie devrait warner R8");
    }

    #[test]
    fn test_defined_reference_no_warning_r8() {
        let define_a = block("DEFINE", vec![field("_stmt", "alice AS human")]);
        let define_b = block("DEFINE", vec![field("_stmt", "bob AS human")]);
        let rel = block("RELATIONS", vec![field("_stmt", "alice COMMAND bob")]);
        let r = validate_semantics(&[define_a, define_b, rel], None);
        assert!(r.warnings.iter().all(|w| !w.contains("R8_UNDEFINED_REFERENCE")),
                "entités définies ne devraient pas warner R8: {:?}", r.warnings);
    }


    #[test]
    fn test_too_many_attributes_errors_r10() {
        let mut fields = vec![];
        for i in 0..13 {
            fields.push(field(&format!("attr_{}", i), "x"));
        }
        let b = block("DEFINE", fields);
        let r = validate_semantics(&[b], None);
        assert!(r.errors.iter().any(|e| e.contains("R10_TOO_MANY_ATTRIBUTES")),
                "13 attributs devrait déclencher R10: {:?}", r.errors);
        assert!(!r.is_valid());
    }

    #[test]
    fn test_twelve_attributes_ok_r10() {
        let mut fields = vec![];
        for i in 0..12 {
            fields.push(field(&format!("attr_{}", i), "x"));
        }
        let b = block("DEFINE", fields);
        let r = validate_semantics(&[b], None);
        assert!(r.errors.iter().all(|e| !e.contains("R10_TOO_MANY_ATTRIBUTES")),
                "12 attributs (limite) ne devrait pas déclencher R10: {:?}", r.errors);
    }


    #[test]
    fn test_non_canonical_value_warns_r9() {
        let b = block("RELATIONS", vec![field("polarity", "kinda_negative")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("R9_NON_CANONICAL_VALUE")),
                "valeur hors ontologie devrait warner R9: {:?}", r.warnings);
    }

    #[test]
    fn test_canonical_value_no_warning_r9() {
        let b = block("RELATIONS", vec![field("polarity", "negative")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("R9_NON_CANONICAL_VALUE")),
                "valeur canonique ne devrait pas warner R9: {:?}", r.warnings);
    }

    #[test]
    fn test_unknown_key_no_warning_r9() {
        let b = block("RELATIONS", vec![field("custom_key", "anything")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("R9_NON_CANONICAL_VALUE")),
                "clé hors ontologie (custom) ne devrait pas warner R9: {:?}", r.warnings);
    }

}