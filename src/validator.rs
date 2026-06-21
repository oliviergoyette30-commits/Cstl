//! CSTL v4.9.3 — Semantic validation (Sessions #2 + #4)
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
    ("produced_by",     "string"),
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
    // Accepte "0", "1", "0.5", "1.0" etc.
    val.parse::<f64>().map_or(false, |f| f >= 0.0 && f <= 1.0)
}

fn validate_bool(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "true" | "false")
}

fn validate_hash(val: &str) -> bool {
    // Le parser META peut stocker "sha256 : value" avec espaces — on normalise.
    val == "root" || val.replace(" ", "").starts_with("sha256:")
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
    raw_meta: &str,
) -> ValidationResult {
    let mut errors   = Vec::new();
    let mut warnings = Vec::new();

    // R2 — Version dans le header (hashbang #!CSTL <version> MODE=X)
    let has_version = raw_meta.lines().next()
        .map(|first_line| first_line.starts_with("#!CSTL") && first_line.split_whitespace().count() >= 2)
        .unwrap_or(false);
    if !has_version {
        warnings.push("R2_MISSING_VERSION: hashbang absent ou version CSTL non détectée en première ligne".to_string());
    }


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

    // CONVERSATION_ID — optionnel, mais si présent : non-vide, sans espaces
    if let Some(cid) = meta.get("CONVERSATION_ID") {
        if cid.trim().is_empty() || cid.contains(' ') {
            warnings.push(format!(
                "TYPE: CSTLTypeError field='CONVERSATION_ID' expected='non-empty-no-spaces' got='{}'",
                cid
            ));
        }
    }

    // R3 — DOMAIN field (optional but validated if present)
    if let Some(domain) = meta.get("DOMAIN") {
        use crate::domains::is_known_domain;
        if !is_known_domain(domain.as_str()) {
            warnings.push(format!(
                "R3_UNKNOWN_DOMAIN: domaine '{}' non reconnu dans l'ontologie CSTL",
                domain
            ));
        }
    }

    ValidationResult { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_meta() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("sigma".to_string(), "0.9".to_string());
        m.insert("NO_PROSE".to_string(), "true".to_string());
        m.insert("RESPONSE_FORMAT".to_string(), "CSTL".to_string());
        m.insert("PARENT_HASH".to_string(), "root".to_string());
        m.insert("encoder".to_string(), "Agent_TEST".to_string());
        m.insert("produced_by".to_string(), "anthropic/claude-test".to_string());
        m
    }

    #[test]
    fn test_missing_hashbang_warns_r2() {
        let meta = minimal_meta();
        let raw = "META [encoder=Agent_TEST]\n---END---";
        let result = validate_meta(&meta, raw);
        assert!(result.warnings.iter().any(|w| w.contains("R2_MISSING_VERSION")),
                "hashbang absent devrait warner R2: {:?}", result.warnings);
    }

    #[test]
    fn test_valid_hashbang_no_warning_r2() {
        let meta = minimal_meta();
        let raw = "#!CSTL v4.9.3 MODE=A\nMETA [encoder=Agent_TEST]\n---END---";
        let result = validate_meta(&meta, raw);
        assert!(result.warnings.iter().all(|w| !w.contains("R2_MISSING_VERSION")),
                "hashbang valide ne devrait pas warner R2: {:?}", result.warnings);
    }

    #[test]
    fn test_r3_unknown_domain_warns() {
        let mut m = minimal_meta();
        m.insert("DOMAIN".to_string(), "foobar_inexistant".to_string());
        let r = validate_meta(&m, "#!CSTL v4.9.3 MODE=A");
        assert!(
            r.warnings.iter().any(|w| w.contains("R3_UNKNOWN_DOMAIN")),
            "Domaine inconnu doit déclencher R3_UNKNOWN_DOMAIN"
        );
    }

    #[test]
    fn test_r3_known_domain_no_warn() {
        let mut m = minimal_meta();
        m.insert("DOMAIN".to_string(), "medical".to_string());
        let r = validate_meta(&m, "#!CSTL v4.9.3 MODE=A");
        assert!(
            !r.warnings.iter().any(|w| w.contains("R3_UNKNOWN_DOMAIN")),
            "Domaine medical connu ne doit pas déclencher R3_UNKNOWN_DOMAIN"
        );
    }

}
