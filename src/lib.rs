//! CSTL v5.0.0 — Rust Parser
//! Lexer + Recursive Descent + Security + Validation
//! Sessions #1-#7 encoded.

pub mod ast;
pub mod token;
pub mod parser;
pub mod security;
pub mod validator;
pub mod canonical;
pub mod relation_validator;
pub mod validator_semantic;
pub mod domains;
pub use ast::CstlDocument;
use token::Lexer;
use parser::Parser;
use security::security_scan;
use validator::validate_meta;
use relation_validator::validate_relations;

/// Parse a CSTL payload string into a CstlDocument.
/// Full pipeline: security → lex → parse → validate.
pub fn parse(input: &str) -> CstlDocument {
    // Session #6+#7: Security scan first
    let sec = security_scan(input);
    let text = &sec.cleaned;

    // Lex
    let mut lexer = Lexer::new(text);
    let tokens = lexer.tokenize();
    let token_count = tokens.len();

    // Parse
    let p = Parser::new(tokens);
    let mut doc = p.parse(token_count);

    // Add security errors/warnings
    doc.errors.extend(sec.errors);
    doc.warnings.extend(sec.warnings);

    // #4b: Validate hashbang version
    if let Some(ref hb) = doc.hashbang {
        let hb_norm = hb.replace('_', " ");
        if !hb_norm.contains("v4.9.3") {
            let ver = hb_norm.split_whitespace()
                .find(|t| t.starts_with('v'))
                .unwrap_or("unknown");
            doc.warnings.push(format!(
                "HASHBANG_VERSION: expected v4.9.3 got '{}' — update payload to #!CSTL v5.0.0 MODE=A",
                ver
            ));
        }
    }

    // Session #2 + #4: Validate META
    if !doc.meta_fields.is_empty() {
        let val = validate_meta(&doc.meta_fields, text);
        doc.errors.extend(val.errors);
        doc.warnings.extend(val.warnings);
    }

    // Validation sémantique (opérateurs, types DEFINE, blocs modaux déontiques)
    let sem = validator_semantic::validate_semantics(&doc.blocks, doc.meta_fields.get("DOMAIN").map(|s| s.as_str()));
    doc.warnings.extend(sem.warnings);
    doc.errors.extend(sem.errors);
    doc.is_valid = doc.errors.is_empty();


    // v5.0: relation-level validation
    let rel_val = validate_relations(text);
    doc.errors.extend(rel_val.errors);
    doc.warnings.extend(rel_val.warnings);
    doc.is_valid = doc.errors.is_empty();

    doc
}

/// Convenience: parse and return only validity
pub fn is_valid(input: &str) -> bool {
    parse(input).is_valid
}

/// Two payloads are semantically equivalent iff their canonical hashes match.
/// Version-sensitive by design (version is part of the canonical form).
pub fn equivalent(a: &str, b: &str) -> bool {
    canonical::canonical_hash(a) == canonical::canonical_hash(b)
}

mod tests;
