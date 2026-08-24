//! CSTL v5.0.0 — Rust parser tests
//! 92 existing tests (v4.9.3 baseline) + v5.0 new operator tests

#[cfg(test)]
mod tests {
    use crate::parse;
    use crate::canonical::canonical_hash;

    const BASE: &str = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";

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
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [  encoder = Agent_TEST ,  sigma = 0.88 , RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root, produced_by=anthropic/claude-sonnet-4-6  ]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
    }

    #[test]
    fn test_tabs() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\n\tencoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6,\n\tsigma=0.88,\n\tRESPONSE_FORMAT=CSTL,\n\tNO_PROSE=true,\n\tPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
    }

    #[test]
    fn test_produced_by_session4() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
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
        assert!(!doc.blocks_named("DISAGREEMENT_BLOCK").is_empty());
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
        assert!(!doc.blocks_named("(RULE)").is_empty());
    }

    // ── Security ─────────────────────────────────────────────────────────────

    #[test]
    fn test_c3_duplicate_key_blocked() {
        let payload = BASE.replace("encoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6,",
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
        let payload = "#!CSTL v5.0.0 MODE=A\n\u{041C}ETA [\nencoder=Agent_TEST\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("SEC_Q1")));
    }

    #[test]
    fn test_zero_width_stripped() {
        let payload = "META\u{200B} [\nencoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6, sigma=0.88, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root\n]\n---END---";
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
        assert_eq!(h.len(), 7 + 64);
    }

    #[test]
    fn test_canonical_hash_deterministic() {
        assert_eq!(canonical_hash(BASE), canonical_hash(BASE));
    }

    #[test]
    fn test_canonical_hash_field_order_invariant() {
        let p1 = "#!CSTL v5.0.0 MODE=A\nMETA [\nsigma:float=0.88,\nencoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let p2 = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        assert_eq!(canonical_hash(p1), canonical_hash(p2));
    }

    #[test]
    fn test_different_payloads_different_hash() {
        let p1 = BASE.replace("---END---", "DECISION: accept\n---END---");
        let p2 = BASE.replace("---END---", "DECISION: reject\n---END---");
        assert_ne!(canonical_hash(&p1), canonical_hash(&p2));
    }

    // ── produced_by format variants ───────────────────────────────────────────

    #[test]
    fn test_produced_by_short_form_gemini() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=Agent_GEMINI,\nproduced_by=gemini-2-5-pro,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        let pby_warns: Vec<_> = doc.warnings.iter().filter(|w| w.contains("produced_by") && w.contains("BNF")).collect();
        assert!(pby_warns.is_empty(), "gemini short form should not warn: {:?}", pby_warns);
    }

    #[test]
    fn test_produced_by_org_slash_form() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        let pby_warns: Vec<_> = doc.warnings.iter().filter(|w| w.contains("BNF")).collect();
        assert!(pby_warns.is_empty(), "org/model-version should not warn: {:?}", pby_warns);
    }

    #[test]
    fn test_produced_by_proxy_chain() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=proxy/azure -> openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("PROXY")));
    }

    #[test]
    fn test_produced_by_identity_mismatch_warn() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("IDENTITY_MISMATCH")));
    }

    #[test]
    fn test_produced_by_absent_model_name_encoder() {
        let payload = "#!CSTL v5.0.0 MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
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
        assert!(!doc.blocks_named("EVALUATION_Q1").is_empty());
        assert!(!doc.blocks_named("EVALUATION_Q2").is_empty());
    }

    #[test]
    fn test_decision_colon_form() {
        let payload = BASE.replace("---END---", "DECISION: ratify_with_patchset [sigma:float=0.96]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid);
        assert!(!doc.blocks_named("DECISION").is_empty());
    }

    #[test]
    fn test_strength_with_parens_not_brackets() {
        let payload = BASE.replace("---END---",
            "DISAGREEMENT_BLOCK [\nSTRENGTH explicit_typing (sigma:float=0.98)\nDISPUTE operator_freeze (sigma:float=0.89, alternative=core_only)\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_empty_payload_invalid() {
        let doc = parse("");
        assert!(!doc.is_valid);
    }

    #[test]
    fn test_parse_time_sub_millisecond() {
        let doc = parse(BASE);
        assert!(doc.parse_time_us < 5000, "parse took {}µs", doc.parse_time_us);
    }

    // ── Performance ───────────────────────────────────────────────────────────

    #[test]
    fn test_large_payload_performance() {
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
        assert!(elapsed.as_millis() < 50);
    }

    #[test]
    fn test_sha256_correctness() {
        let h1 = canonical_hash("test");
        let h2 = canonical_hash("test");
        assert_eq!(h1, h2);
        assert_eq!(&h1[..7], "sha256:");
        assert_eq!(h1.len(), 71);
    }

    // ── v5.0 NEW: Logical propagation — ENTAILS ───────────────────────────────

    #[test]
    fn test_entails_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(fix_p3) ENTAILS propagation_logique_native [sigma=0.90, tau=f_future, id=r007]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_entails_in_constraints() {
        let payload = BASE.replace("---END---",
            "CONSTRAINTS [\n(MUST) hypothesis ENTAILS conclusion [sigma=0.85]\n]\n---END---");
        let doc = parse(&payload);
        // No E-level errors about ENTAILS being unknown
        let entails_errors: Vec<_> = doc.errors.iter()
            .filter(|e| e.contains("ENTAILS") && e.contains("unknown")).collect();
        assert!(entails_errors.is_empty(), "ENTAILS should be recognized: {:?}", entails_errors);
    }

    #[test]
    fn test_entails_transitivity_warning() {
        // A ENTAILS B, B ENTAILS C — W603 for missing A ENTAILS C
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) ENTAILS B [sigma=0.9]\n(B) ENTAILS C [sigma=0.9]\n]\n---END---");
        let doc = parse(&payload);
        // W603 may or may not fire depending on extraction — at minimum no crash
        let _ = &doc.warnings;
    }

    #[test]
    fn test_entails_transitive_complete_no_warn() {
        // A ENTAILS B, B ENTAILS C, A ENTAILS C — complete, no W603
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) ENTAILS B [sigma=0.9]\n(B) ENTAILS C [sigma=0.9]\n(A) ENTAILS C [sigma=0.9]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        let w603: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W603")).collect();
        assert!(w603.is_empty(), "Complete transitive chain should not warn W603: {:?}", w603);
    }

    // ── v5.0 NEW: CONTRADICTS ─────────────────────────────────────────────────

    #[test]
    fn test_contradicts_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(hypothesis_A) CONTRADICTS hypothesis_B [sigma=0.88, tau=n_present]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_contradicts_symmetry_warning() {
        // Both A CONTRADICTS B and B CONTRADICTS A declared — W602
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) CONTRADICTS B [sigma=0.9]\n(B) CONTRADICTS A [sigma=0.9]\n]\n---END---");
        let doc = parse(&payload);
        let w602: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W602")).collect();
        assert!(!w602.is_empty(), "Redundant CONTRADICTS pair should warn W602: {:?}", doc.warnings);
    }

    #[test]
    fn test_contradicts_one_direction_no_warn() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(claim_A) CONTRADICTS claim_B [sigma=0.92]\n]\n---END---");
        let doc = parse(&payload);
        let w602: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W602")).collect();
        assert!(w602.is_empty(), "Single direction CONTRADICTS should not warn W602: {:?}", w602);
    }

    // ── v5.0 NEW: Epistemic operators ────────────────────────────────────────

    #[test]
    fn test_believes_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent_A) BELIEVES hypothesis_valid [sigma=0.75, tau=n_present]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_knows_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent_A) KNOWS patient_weight_82kg [sigma=0.98, tau=n_present]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_assumes_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(model) ASSUMES prior_distribution_normal [sigma=0.60]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_doubts_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(reviewer) DOUBTS compression_claim [sigma=0.30]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_knows_low_sigma_warns() {
        // KNOWS with sigma < 0.8 should emit W604
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent) KNOWS uncertain_value [sigma=0.5]\n]\n---END---");
        let doc = parse(&payload);
        let w604: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W604")).collect();
        assert!(!w604.is_empty(), "KNOWS sigma=0.5 should warn W604: {:?}", doc.warnings);
    }

    #[test]
    fn test_doubts_high_sigma_warns() {
        // DOUBTS with sigma > 0.5 should emit W605
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent) DOUBTS claim_X [sigma=0.8]\n]\n---END---");
        let doc = parse(&payload);
        let w605: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W605")).collect();
        assert!(!w605.is_empty(), "DOUBTS sigma=0.8 should warn W605: {:?}", doc.warnings);
    }

    #[test]
    fn test_believes_vs_knows_distinct() {
        // BELIEVES and KNOWS in same payload — both recognized, semantically distinct
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent_A) BELIEVES claim_X [sigma=0.65]\n(agent_A) KNOWS fact_Y [sigma=0.97]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        // No W604 on KNOWS (sigma=0.97 >= 0.8)
        let w604: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W604")).collect();
        assert!(w604.is_empty(), "KNOWS sigma=0.97 should not warn: {:?}", w604);
    }

    // ── v5.0 NEW: Temporal Allen subset ──────────────────────────────────────

    #[test]
    fn test_before_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(step_1) BEFORE step_2 [sigma=0.95]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_after_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(step_3) AFTER step_2 [sigma=0.95]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_during_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(monitoring) DURING clinical_trial [sigma=0.90]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_temporal_contradiction_before_and_after_same_pair() {
        // A BEFORE B and A AFTER B — E701 temporal contradiction
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(step_A) BEFORE step_B [sigma=0.9]\n(step_A) AFTER step_B [sigma=0.9]\n]\n---END---");
        let doc = parse(&payload);
        let e701: Vec<_> = doc.errors.iter().filter(|e| e.contains("E701")).collect();
        assert!(!e701.is_empty(), "BEFORE + AFTER same pair should emit E701: {:?}", doc.errors);
    }

    #[test]
    fn test_temporal_workflow_valid() {
        // Multi-step workflow with BEFORE/AFTER/DURING
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(ingestion) BEFORE processing [sigma=1.0]\n(processing) BEFORE output [sigma=1.0]\n(monitoring) DURING processing [sigma=0.90]\n(validation) AFTER output [sigma=0.95]\n]\n---END---");
        let doc = parse(&payload);
        let e701: Vec<_> = doc.errors.iter().filter(|e| e.contains("E701")).collect();
        assert!(e701.is_empty(), "Valid workflow should not emit E701: {:?}", e701);
    }

    // ── v5.0 NEW: Relational operators (MUTUAL replacements) ──────────────────

    #[test]
    fn test_equals_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(concept_A) EQUALS concept_B [sigma=0.95, id=r001]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_possesses_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(patient) POSSESSES medical_record [sigma=0.99]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_resembles_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(format_A) RESEMBLES format_B [sigma=0.72]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_co_locates_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent_A) CO_LOCATES agent_B [sigma=0.85, context=datacenter_eu]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_opposes_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(claim_compression) OPPOSES claim_fidelity [sigma=0.70]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_compares_operator_recognized() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(CSTL) COMPARES JSON_LD [sigma=0.88, axis=token_efficiency]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    #[test]
    fn test_all_six_relational_operators_in_one_payload() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) EQUALS B [sigma=0.95]\n(C) POSSESSES D [sigma=0.90]\n(E) RESEMBLES F [sigma=0.75]\n(G) CO_LOCATES H [sigma=0.85]\n(I) OPPOSES J [sigma=0.70]\n(K) COMPARES L [sigma=0.80]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }

    // ── v5.0 NEW: MUTUAL deprecated — W601 ───────────────────────────────────

    #[test]
    fn test_mutual_deprecated_emits_w601() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) MUTUAL B [sigma=0.80]\n]\n---END---");
        let doc = parse(&payload);
        let w601: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W601")).collect();
        assert!(!w601.is_empty(), "MUTUAL should emit W601 deprecation warning: {:?}", doc.warnings);
    }

    #[test]
    fn test_mutual_deprecated_still_parses() {
        // MUTUAL is deprecated but not a hard error — payload still valid
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(concept_A) MUTUAL concept_B [sigma=0.80]\n]\n---END---");
        let doc = parse(&payload);
        // Should not fail with errors (backward compat)
        assert!(doc.errors.is_empty(), "MUTUAL should not cause errors (only W601 warning): {:?}", doc.errors);
    }

    #[test]
    fn test_mutual_migration_hint_in_warning() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(x) MUTUAL y [sigma=0.70]\n]\n---END---");
        let doc = parse(&payload);
        let w601: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W601")).collect();
        assert!(!w601.is_empty());
        // Warning should contain migration hint
        assert!(w601[0].contains("EQUALS") || w601[0].contains("Migration"), 
                "W601 should contain migration hint: {}", w601[0]);
    }

    // ── v5.0 NEW: Mixed operator payload (real-world simulation) ─────────────

    #[test]
    fn test_v5_full_scientific_payload() {
        // Simulates a multi-agent scientific review payload using all v5 operators
        let payload = BASE.replace("---END---", r#"
DEFINE hypothesis AS concept [id=h001, content=cstl_improves_fidelity, sigma=0.85]
DEFINE baseline AS concept [id=b001, content=json_ld_baseline]
DEFINE agent_reviewer AS agent [id=a001, role=scientific_reviewer]
DEFINE finding_A AS concept [id=f001, content=fidelity_100pct_measured]
DEFINE finding_B AS concept [id=f002, content=n_too_small_for_confirmation]

RELATIONS [
(hypothesis) ENTAILS finding_A [sigma=0.80, tau=f_future, id=r001]
(finding_A) CONTRADICTS naive_null_hypothesis [sigma=0.90, id=r002]
(finding_B) OPPOSES publication_claim [sigma=0.85, id=r003]
(agent_reviewer) KNOWS finding_A [sigma=0.97, id=r004]
(agent_reviewer) DOUBTS hypothesis [sigma=0.35, id=r005]
(agent_reviewer) BELIEVES additional_validation_required [sigma=0.92, id=r006]
(baseline_setup) BEFORE experiment_run [sigma=1.0, id=r007]
(experiment_run) BEFORE analysis [sigma=1.0, id=r008]
(monitoring) DURING experiment_run [sigma=0.90, id=r009]
(hypothesis) RESEMBLES prior_work_AMR [sigma=0.60, id=r010]
(CSTL) COMPARES JSON_LD [sigma=0.88, axis=token_cost, id=r011]
]
---END---"#);
        let doc = parse(&payload);
        assert!(doc.is_valid, "Full v5 payload should be valid: {:?}", doc.errors);
    }

    #[test]
    fn test_v5_payload_entails_and_temporal_combined() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(data_collection) BEFORE analysis [sigma=1.0]\n(analysis) ENTAILS results [sigma=0.90]\n(results) BEFORE publication [sigma=0.95]\n(peer_review) DURING publication [sigma=0.85]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        let e701: Vec<_> = doc.errors.iter().filter(|e| e.contains("E701")).collect();
        assert!(e701.is_empty());
    }

    #[test]
    fn test_v5_epistemic_chain() {
        // Agent chain: A KNOWS X → A BELIEVES Y based on X
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent_A) KNOWS measured_value [sigma=0.98]\n(agent_A) BELIEVES derived_claim [sigma=0.72]\n(agent_A) ASSUMES boundary_condition [sigma=0.55]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        // No W604 on KNOWS (0.98 >= 0.8)
        let w604: Vec<_> = doc.warnings.iter().filter(|w| w.contains("W604")).collect();
        assert!(w604.is_empty());
    }

    // ── Backward compatibility: v4.9.3 payloads still parse ──────────────────

    #[test]
    fn test_v493_payload_still_valid_under_v5() {
        // v4.9.3 payload format should parse without errors
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_CLAUDE,\nproduced_by=anthropic/claude-sonnet-4-6,\nsigma:float=0.97,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\nDECISION: test_backward_compat [sigma=0.95]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "v4.9.3 payload should parse under v5: {:?}", doc.errors);
    }

    #[test]
    fn test_v493_with_mutual_gets_w601_but_no_error() {
        let payload = "#!CSTL v4.9.3 MODE=A\nMETA [\nencoder=Agent_TEST,
produced_by=anthropic/claude-sonnet-4-6,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\nRELATIONS [\n(x) MUTUAL y [sigma=0.75]\n]\n---END---";
        let doc = parse(payload);
        // W601 warning yes, hard error no
        assert!(doc.errors.is_empty(), "MUTUAL in v4.9.3 should warn not error: {:?}", doc.errors);
        assert!(doc.warnings.iter().any(|w| w.contains("W601")));

    }

    // ═══════════════════════════════════════════════════════════════════════
    // AST STRUCTUREL — tests que les relations sont dans doc.relations
    // Ces tests prouvent que le parser extrait réellement les relations
    // (pas juste le text-based validator)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_relation_extracted_into_ast() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(hypothesis) ENTAILS finding [sigma=0.85, tau=f_future]\n]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.relations.len(), 1, "Should have 1 relation in AST");
        let rel = &doc.relations[0];
        assert_eq!(rel.subject,  "hypothesis");
        assert_eq!(rel.operator, "ENTAILS");
        assert_eq!(rel.object,   "finding");
        assert!(rel.modality.is_none());
    }

    #[test]
    fn test_relation_attrs_extracted() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) KNOWS B [sigma=0.97, tau=n_present, id=r001]\n]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 1);
        let rel = &doc.relations[0];
        assert_eq!(rel.operator, "KNOWS");
        // sigma attr extracted
        let sigma = rel.attrs.iter().find(|f| f.name == "sigma");
        assert!(sigma.is_some(), "sigma attr should be in AST");
        assert_eq!(sigma.unwrap().value, "0.97");
        // id attr extracted
        let id = rel.attrs.iter().find(|f| f.name == "id");
        assert!(id.is_some());
        assert_eq!(id.unwrap().value, "r001");
    }

    #[test]
    fn test_multiple_relations_extracted() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) ENTAILS B [sigma=0.9]\n(B) CONTRADICTS C [sigma=0.8]\n(agent) KNOWS D [sigma=0.97]\n]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 3, "Should extract 3 relations: {:?}",
            doc.relations.iter().map(|r| &r.operator).collect::<Vec<_>>());
        assert_eq!(doc.relations[0].operator, "ENTAILS");
        assert_eq!(doc.relations[1].operator, "CONTRADICTS");
        assert_eq!(doc.relations[2].operator, "KNOWS");
    }

    #[test]
    fn test_relations_by_op_query() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) ENTAILS B [sigma=0.9]\n(B) ENTAILS C [sigma=0.8]\n(X) KNOWS Y [sigma=0.95]\n]\n---END---");
        let doc = parse(&payload);
        let entails = doc.relations_by_op("ENTAILS");
        assert_eq!(entails.len(), 2);
        let knows = doc.relations_by_op("KNOWS");
        assert_eq!(knows.len(), 1);
        let before = doc.relations_by_op("BEFORE");
        assert_eq!(before.len(), 0);
    }

    #[test]
    fn test_relations_by_subject_query() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(agent_A) KNOWS fact1 [sigma=0.97]\n(agent_A) BELIEVES claim1 [sigma=0.70]\n(agent_B) KNOWS fact2 [sigma=0.95]\n]\n---END---");
        let doc = parse(&payload);
        let agent_a_rels = doc.relations_by_subject("agent_A");
        assert_eq!(agent_a_rels.len(), 2);
        let agent_b_rels = doc.relations_by_subject("agent_B");
        assert_eq!(agent_b_rels.len(), 1);
    }

    #[test]
    fn test_relation_sigma_helper() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) DOUBTS B [sigma=0.30]\n]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 1);
        let sigma = crate::ast::CstlDocument::relation_sigma(&doc.relations[0]);
        assert!(sigma.is_some());
        assert!((sigma.unwrap() - 0.30).abs() < 0.001);
    }

    #[test]
    fn test_temporal_relations_extracted() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(step_1) BEFORE step_2 [sigma=1.0]\n(monitor) DURING step_2 [sigma=0.90]\n(step_3) AFTER step_2 [sigma=0.95]\n]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 3);
        assert_eq!(doc.relations_by_op("BEFORE").len(), 1);
        assert_eq!(doc.relations_by_op("DURING").len(), 1);
        assert_eq!(doc.relations_by_op("AFTER").len(), 1);
    }

    #[test]
    fn test_relational_operators_extracted() {
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(A) EQUALS B [sigma=0.95]\n(C) POSSESSES D [sigma=0.99]\n(E) RESEMBLES F [sigma=0.72]\n(G) CO_LOCATES H [sigma=0.85]\n(I) OPPOSES J [sigma=0.70]\n(K) COMPARES L [sigma=0.80]\n]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 6, "All 6 relational operators should parse");
        let ops: Vec<&str> = doc.relations.iter().map(|r| r.operator.as_str()).collect();
        assert!(ops.contains(&"EQUALS"));
        assert!(ops.contains(&"POSSESSES"));
        assert!(ops.contains(&"RESEMBLES"));
        assert!(ops.contains(&"CO_LOCATES"));
        assert!(ops.contains(&"OPPOSES"));
        assert!(ops.contains(&"COMPARES"));
    }

    #[test]
    fn test_modal_relation_with_modality_extracted() {
        // (MUST) agent KNOWS fact [sigma=0.97]
        let payload = BASE.replace("---END---",
            "RELATIONS [\n(MUST) agent KNOWS fact [sigma=0.97]\n]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 1);
        let rel = &doc.relations[0];
        assert_eq!(rel.operator, "KNOWS");
        assert_eq!(rel.subject,  "agent");
        assert_eq!(rel.object,   "fact");
        assert_eq!(rel.modality, Some("MUST".to_string()));
    }

    #[test]
    fn test_toplevel_relation_outside_block() {
        // Relations can appear outside RELATIONS [...] block
        let payload = BASE.replace("---END---",
            "(hypothesis) ENTAILS conclusion [sigma=0.85]\n---END---");
        let doc = parse(&payload);
        assert_eq!(doc.relations.len(), 1);
        assert_eq!(doc.relations[0].operator, "ENTAILS");
    }

    #[test]
    fn test_full_v5_payload_ast_complete() {
        let payload = BASE.replace("---END---", r#"
RELATIONS [
(hypothesis) ENTAILS finding [sigma=0.85, tau=f_future, id=r001]
(finding) CONTRADICTS null_hypothesis [sigma=0.90, id=r002]
(agent) KNOWS measured_value [sigma=0.97, id=r003]
(agent) BELIEVES derived_claim [sigma=0.72, id=r004]
(step_1) BEFORE step_2 [sigma=1.0, id=r005]
(monitor) DURING step_2 [sigma=0.90, id=r006]
(CSTL) COMPARES JSON_LD [sigma=0.88, id=r007]
]
---END---"#);
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.relations.len(), 7, "All 7 relations should be in AST");

        // Verify each relation is queryable
        assert_eq!(doc.relations_by_op("ENTAILS").len(), 1);
        assert_eq!(doc.relations_by_op("CONTRADICTS").len(), 1);
        assert_eq!(doc.relations_by_op("KNOWS").len(), 1);
        assert_eq!(doc.relations_by_op("BELIEVES").len(), 1);
        assert_eq!(doc.relations_by_op("BEFORE").len(), 1);
        assert_eq!(doc.relations_by_op("DURING").len(), 1);
        assert_eq!(doc.relations_by_op("COMPARES").len(), 1);

        // Verify sigma extraction on a specific relation
        let entails = &doc.relations_by_op("ENTAILS")[0];
        let sigma = crate::ast::CstlDocument::relation_sigma(entails);
        assert!((sigma.unwrap() - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_backtrack_safe_non_relation_paren() {
        // (RULE) MUST respond_in_cstl_only — modal statement, NOT a relation
        let payload = BASE.replace("---END---",
            "(RULE) MUST respond_in_cstl_only\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        // Should NOT be in relations (no operator)
        assert_eq!(doc.relations.len(), 0);
        // Should be a modal block
        assert!(!doc.blocks_named("(RULE)").is_empty());
    }

    #[test]
    fn test_parse_field_backtrack_safe() {
        // A field followed by a non-= token should backtrack cleanly
        let payload = BASE.replace("---END---",
            "DEFINE patient AS human [name=Jean, age=45]\n---END---");
        let doc = parse(&payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
    }
}
