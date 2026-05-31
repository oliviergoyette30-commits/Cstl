//! CSTL v4.9.2 — Rust Parser
//! Lexer + Recursive Descent + Security + Validation
//! Sessions #1-#7 encoded.

pub mod ast;
pub mod token;
pub mod parser;
pub mod security;
pub mod validator;
pub mod canonical;

pub use ast::CstlDocument;
use token::Lexer;
use parser::Parser;
use security::security_scan;
use validator::validate_meta;

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

    // Session #2 + #4: Validate META
    if !doc.meta_fields.is_empty() {
        let val = validate_meta(&doc.meta_fields, text);
        doc.errors.extend(val.errors);
        doc.warnings.extend(val.warnings);
        doc.is_valid = doc.errors.is_empty();
    }

    doc
}

/// Convenience: parse and return only validity
pub fn is_valid(input: &str) -> bool {
    parse(input).is_valid
}

mod tests;
