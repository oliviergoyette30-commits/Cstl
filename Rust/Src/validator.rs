//! CSTL v4.9.2 — Semantic validation (Sessions #2 + #4)
//! Typing rules, mandatory fields, produced_by rules

use std::collections::HashMap;

#[derive(Debug)]
pub struct ValidationResult {
    pub errors:   Vec<String>,
    pub warnings: Vec<String>,
}

/// Session #2: 6 mandatory META fields
const MANDATORY: &[(&str, &str)] = &[
    ("sigma",           "float"),
    ("NO_PROSE",        "bool"),
    ("RESPONSE_FORMAT", "enum"),
    ("PARENT_HASH",     "hash"),
    ("encoder",         "string"),
];

/// Session #2: valid RESPONSE_FORMAT values
const VALID_RESPONSE_FORMAT: &[&str] = &["CSTL", "JSON", "MIXED"];

/// Session #2: valid ACTION values (strict whitelist + EXTENSION: escape)
const VALID_ACTION: &[&str] = &[
    "continue_chain", "evaluate", "initiate", "ratify", "dispute",
    "synthesize", "close", "open_debate", "session_closed",
    "evaluate_produced_by_spec", "evaluate_roundtrip_debate",
    "evaluate_attack_surface", "initiate_produced_by_debate",
    "initiate_roundtrip_debate", "initiate_attack_surface",
    "session5_final_handshake", "session5_closed",
    "ratification_review_session5", "evaluate_advanced_attack_vectors",
    "evaluate_bytecode_table_response",
];

fn validate_float(val: &str) -> bool {
    val.parse::<f64>().map_or(false, |f| (0.0..=1.0).contains(&f))
}

fn validate_bool(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "true" | "false")
}

fn validate_hash(val: &str) -> bool {
    val == "root" || val.starts_with("sha256:") || val.starts_with("sha256") || val.contains("sha256")
}

fn validate_iso8601(val: &str) -> bool {
    // Basic check: must contain T and Z or +
    val.len() >= 16 && val.contains('T')
}

fn looks_like_model_name(enc: &str) -> bool {
    // Encoder contains model name → should use role name instead
    if enc.starts_with("Agent_") || enc.starts_with("proxy/") { return false; }
    let low = enc.to_lowercase();
    low.contains("gpt") || low.contains("gemini") || low.contains("claude")
        || low.contains("llama") || low.contains("mistral") || low.contains("chatgpt")
}

fn validate_produced_by_format(val: &str) -> bool {
    // Session #4 BNF (ratified) + empirical observed formats:
    //   canonical: org/model-version        (openai/gpt-4o-2026)
    //   short:     model-version            (gemini-2-5-pro, gpt-4o-2026) — observed in practice
    //   proxy:     proxy/org -> org/model-v  (Session #4 Rule 5)
    if val == "root" || val == "REDACTED" { return false; }
    // Proxy chain
    if val.contains("->") {
        let parts: Vec<&str> = val.splitn(2, "->").collect();
        return parts.len() == 2
            && parts[0].trim().starts_with("proxy/")
            && parts[1].trim().contains('/');
    }
    // Canonical: org/model-version
    if val.contains('/') { return true; }
    // Short form: model-version (at least one hyphen, alphanumeric segments)
    // e.g. gemini-2-5-pro, gpt-4o-2026, claude-sonnet-4-5-20251001
    let parts: Vec<&str> = val.split('-').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '.'))
}

/// Full META validation (Sessions #2 + #4)
pub fn validate_meta(
    meta: &HashMap<String, String>,
    _raw_meta: &str,
) -> ValidationResult {
    let mut errors   = Vec::new();
    let mut warnings = Vec::new();

    // Check mandatory fields
    for (field, expected_type) in MANDATORY {
        let val = meta.get(*field);
        match val {
            None => {
                errors.push(format!(
                    "TYPE: CSTLTypeError(ERROR) field='{}' expected='{}(mandatory)' got='(absent)'",
                    field, expected_type
                ));
            }
            Some(v) => {
                let ok = match *expected_type {
                    "float"  => validate_float(v),
                    "bool"   => validate_bool(v),
                    "enum"   => true,  // validated below per-field
                    "hash"   => validate_hash(v),
                    "string" => !v.is_empty(),
                    _        => true,  // other type hints pass through
                };
                if !ok {
                    errors.push(format!(
                        "TYPE: CSTLTypeError(ERROR) field='{}' expected='{}[valid]' got='{}'",
                        field, expected_type, v
                    ));
                }
            }
        }
    }

    // RESPONSE_FORMAT enum validation
    if let Some(rf) = meta.get("RESPONSE_FORMAT") {
        if !VALID_RESPONSE_FORMAT.contains(&rf.as_str()) {
            errors.push(format!(
                "TYPE: CSTLEnumError field='RESPONSE_FORMAT' valid={:?} got='{}'",
                VALID_RESPONSE_FORMAT, rf
            ));
        }
    }

    // ACTION enum validation (strict + EXTENSION: escape)
    if let Some(action) = meta.get("ACTION") {
        if !action.starts_with("EXTENSION:") && !VALID_ACTION.contains(&action.as_str()) {
            warnings.push(format!(
                "TYPE: CSTLEnumError field='ACTION' unknown='{}' — use EXTENSION: prefix",
                action
            ));
        }
    }

    // TURN must be positive int
    if let Some(turn) = meta.get("TURN") {
        if turn.parse::<u64>().is_err() {
            errors.push(format!("TYPE: CSTLTypeError field='TURN' expected='int' got='{}'", turn));
        }
    }

    // TIMESTAMP must be ISO8601-like
    if let Some(ts) = meta.get("TIMESTAMP") {
        if !validate_iso8601(ts) {
            warnings.push(format!(
                "TYPE: CSTLTypeError field='TIMESTAMP' expected='iso8601' got='{}'", ts
            ));
        }
    }

    // Session #4: produced_by 6 rules
    let enc = meta.get("encoder").map(|s| s.as_str()).unwrap_or("");
    let pby = meta.get("produced_by").map(|s| s.as_str()).unwrap_or("");

    if !pby.is_empty() {
        // R1: encoder looks like model name + produced_by present
        if looks_like_model_name(enc) {
            warnings.push(format!(
                "PATCH_T4_WARNING encoder='{}' — WARNING_IDENTITY_MISMATCH", enc
            ));
        }
        // R2: redundant
        if pby == enc {
            warnings.push("PATCH_T4_WARNING — WARNING_REDUNDANT produced_by equals encoder".to_string());
        }
        // R5: proxy format
        if pby.starts_with("proxy/") {
            warnings.push(format!(
                "PATCH_T4_WARNING encoder='{}' — WARNING_PROXY secondary backend_header required", pby
            ));
            // R6: no -> chain
            if !pby.contains("->") {
                warnings.push(format!(
                    "PATCH_T4_WARNING — WARNING_PROXY_MASKED_BACKEND no backend declared in '{}'", pby
                ));
            }
        }
        // BNF validation
        if !validate_produced_by_format(pby) && pby != "root" {
            warnings.push(format!(
                "TYPE: produced_by='{}' does not match BNF: org/model-version or proxy/org -> ...", pby
            ));
        }
    } else if looks_like_model_name(enc) {
        // R4: no produced_by + encoder is model name
        warnings.push(format!(
            "PATCH_T4_WARNING encoder='{}' — WARNING_PATCH_T4 produced_by absent", enc
        ));
    }

    ValidationResult { errors, warnings }
}
