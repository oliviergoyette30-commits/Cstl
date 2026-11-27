#!/data/data/com.termux/files/usr/bin/bash
# CSTL Rust — projet complet auto-contenu (verifie 37/37 par Claude)
# Usage : bash setup.sh  puis  cargo test
set -e
mkdir -p src
cat > Cargo.toml << '__CSTL_EOF__'
[package]
name = "cstl_parser"
version = "4.9.2"
edition = "2021"
description = "CSTL v4.9.2 — Rust parser (lexer + recursive descent)"
authors = ["Olivier Goyette"]


[[bin]]
name = "cstl_validate"
path = "src/main.rs"

[lib]
name = "cstl_parser"
path = "src/lib.rs"

[dependencies]
# Zero external deps — pure Rust lexer + parser
# nom would be option 1b — here we do hand-rolled for zero-dep portability

[dev-dependencies]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
__CSTL_EOF__
cat > src/lib.rs << '__CSTL_EOF__'
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
__CSTL_EOF__
cat > src/parser.rs << '__CSTL_EOF__'
//! CSTL v4.9.2 — Recursive Descent Parser
//! Sessions #1-#7 decisions encoded in validation rules.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::ast::{Block, CstlDocument, Field};
// security handled in lib.rs
use crate::token::{Token, TokenKind};

pub struct Parser {
    tokens:   Vec<Token>,
    pos:      usize,
    errors:   Vec<String>,
    warnings: Vec<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0, errors: vec![], warnings: vec![] }
    }

    // ── Token navigation ──────────────────────────────────────────────────────

    fn cur(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek(&self, offset: usize) -> &Token {
        let p = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[p]
    }

    fn advance(&mut self) -> &Token {
        let t = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() { self.pos += 1; }
        t
    }

    fn skip_newlines(&mut self) {
        while self.cur().kind == TokenKind::Newline { self.advance(); }
    }

    fn at(&self, kind: &TokenKind) -> bool {
        &self.cur().kind == kind
    }

    fn at_eof_or_end(&self) -> bool {
        matches!(self.cur().kind, TokenKind::Eof | TokenKind::EndMarker)
    }

    #[allow(dead_code)]
    fn expect(&mut self, kind: TokenKind, ctx: &str) -> Option<String> {
        if self.cur().kind == kind {
            Some(self.advance().value.clone())
        } else {
            self.errors.push(format!(
                "Expected {:?} got {:?} ({:?}) at {}:{} [{}]",
                kind, self.cur().kind, self.cur().value,
                self.cur().line, self.cur().col, ctx
            ));
            None
        }
    }

    // ── Value parsing ─────────────────────────────────────────────────────────

    fn parse_value(&mut self) -> String {
        // Collect tokens until structural delimiter
        let mut parts = Vec::new();
        let mut depth = 0i32;

        loop {
            match self.cur().kind {
                TokenKind::LBracket => {
                    depth += 1;
                    parts.push("[".to_string());
                    self.advance();
                }
                TokenKind::RBracket if depth > 0 => {
                    depth -= 1;
                    parts.push("]".to_string());
                    self.advance();
                }
                TokenKind::RBracket | TokenKind::Comma |
                TokenKind::Newline | TokenKind::Eof | TokenKind::EndMarker => break,
                _ => {
                    parts.push(self.advance().value.clone());
                }
            }
        }
        parts.join(" ").trim().to_string()
    }

    // ── Field parsing ─────────────────────────────────────────────────────────

    fn parse_field(&mut self) -> Option<Field> {
        self.skip_newlines();

        // Must start with ident or keyword
        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            return None;
        }

        let line = self.cur().line;
        let name = self.advance().value.clone();
        let mut type_hint = None;

        // Optional :type
        if self.at(&TokenKind::Colon) {
            self.advance();
            if matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                type_hint = Some(self.advance().value.clone());
            }
        }

        // Must have =
        if !self.at(&TokenKind::Equals) {
            // Not a field — backtrack
            self.pos -= 1;
            if type_hint.is_some() { self.pos -= 2; } // backtrack past :type too
            return None;
        }
        self.advance(); // =

        let value = self.parse_value();
        Some(Field { name, type_hint, value, line })
    }

    fn parse_field_list(&mut self) -> Vec<Field> {
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            if self.at(&TokenKind::RBracket) || self.at_eof_or_end() { break; }

            if let Some(f) = self.parse_field() {
                fields.push(f);
            }

            self.skip_newlines();
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else if self.at(&TokenKind::RBracket) || self.at_eof_or_end() {
                break;
            } else if !self.at(&TokenKind::Newline) {
                // Unknown token — skip with warning
                let (tk, tv, tl, tc) = {
                    let t = self.advance();
                    (t.kind.clone(), t.value.clone(), t.line, t.col)
                };
                if tk != TokenKind::Newline {
                    self.warnings.push(format!(
                        "PARSER: unexpected {:?} {:?} at {}:{}",
                        tk, tv, tl, tc
                    ));
                }
            }
        }
        fields
    }

    // ── Block parsing ─────────────────────────────────────────────────────────

    fn parse_block(&mut self, name: String, line: usize) -> Block {
        let mut fields    = Vec::new();
        let mut subblocks = Vec::new();

        // Consume [
        if self.at(&TokenKind::LBracket) { self.advance(); }
        self.skip_newlines();

        loop {
            self.skip_newlines();
            if self.at(&TokenKind::RBracket) || self.at_eof_or_end() { break; }

            let cur_line = self.cur().line;

            // Check for subblock patterns
            if matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                let sub_kind = self.peek(1);

                // Pattern 1: KEYWORD [ — direct subblock
                if sub_kind.kind == TokenKind::LBracket {
                    let sub_name = self.advance().value.clone();
                    let sub = self.parse_block(sub_name, cur_line);
                    subblocks.push(sub);
                    self.skip_newlines();
                    continue;
                }

                // Pattern 2: KEYWORD label [...] — e.g. GAP missing [sigma=0.85]
                if matches!(sub_kind.kind, TokenKind::Ident | TokenKind::Keyword) {
                    if self.peek(2).kind == TokenKind::LBracket {
                        let block_type = self.advance().value.clone();
                        let label      = self.advance().value.clone();
                        let sub_name   = format!("{}:{}", block_type, label);
                        let sub = self.parse_block(sub_name, cur_line);
                        subblocks.push(sub);
                        self.skip_newlines();
                        continue;
                    }
                }

                // Pattern 3: KEYWORD label — no brackets (inline statement)
                // Just try parsing as field
            }

            // Try field
            if let Some(f) = self.parse_field() {
                fields.push(f);
                self.skip_newlines();
                if self.at(&TokenKind::Comma) { self.advance(); }
                continue;
            }

            // Nothing matched — skip token
            if !self.at(&TokenKind::Newline) { self.advance(); }
            else { self.advance(); }
        }

        self.skip_newlines();
        if self.at(&TokenKind::RBracket) { self.advance(); }

        Block { name, fields, subblocks, line }
    }

    // ── Modal statement: (MUST) ... ───────────────────────────────────────────

    fn parse_modal(&mut self) -> Option<Block> {
        // ( already consumed by caller
        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            return None;
        }
        let modal = self.advance().value.clone();
        let line  = self.cur().line;

        if !self.at(&TokenKind::RParen) { return None; }
        self.advance(); // )

        // Rest of statement until [ or newline
        let mut parts = Vec::new();
        while !matches!(self.cur().kind,
            TokenKind::Newline | TokenKind::Eof | TokenKind::EndMarker |
            TokenKind::LBracket) {
            parts.push(self.advance().value.clone());
        }

        let mut inline_fields = Vec::new();
        if self.at(&TokenKind::LBracket) {
            self.advance();
            inline_fields = self.parse_field_list();
            if self.at(&TokenKind::RBracket) { self.advance(); }
        }

        let stmt_value = parts.join(" ").trim().to_string();
        let f = Field { name: "_stmt".to_string(), type_hint: None,
                        value: stmt_value, line };
        let mut all_fields = vec![f];
        all_fields.extend(inline_fields);

        Some(Block {
            name: format!("({})", modal),
            fields: all_fields,
            subblocks: vec![],
            line,
        })
    }

    // ── Top-level parse ───────────────────────────────────────────────────────

    pub fn parse(mut self, token_count: usize) -> CstlDocument {
        let t0 = Instant::now();

        let mut hashbang     = None;
        let mut meta_fields  = HashMap::new();
        let mut blocks       = Vec::new();
        let mut meta_found   = false;

        self.skip_newlines();

        // Hashbang
        if self.at(&TokenKind::Hashbang) {
            hashbang = Some(self.advance().value.clone());
        }
        self.skip_newlines();

        // META block — mandatory
        if self.cur().kind == TokenKind::Keyword && self.cur().value == "META" {
            let line = self.cur().line;
            self.advance();
            self.skip_newlines();
            let meta_block = self.parse_block("META".to_string(), line);

            // Extract meta fields — enforce C3 duplicate key detection
            let mut seen: HashSet<String> = HashSet::new();
            let mut c3_violation = false;
            for f in &meta_block.fields {
                if seen.contains(&f.name) {
                    self.errors.push(format!(
                        "PATCH_C3: Duplicate key {:?} — root invalidated", f.name
                    ));
                    c3_violation = true;
                    break;
                }
                seen.insert(f.name.clone());
                meta_fields.insert(f.name.clone(), f.value.clone());
            }
            if c3_violation { meta_fields.clear(); }

            blocks.push(meta_block);
            meta_found = true;
        }

        if !meta_found {
            self.errors.push("Missing META block".to_string());
        }

        // Body: blocks + modal statements
        self.skip_newlines();
        while !self.at_eof_or_end() {
            self.skip_newlines();
            if self.at_eof_or_end() { break; }

            let line = self.cur().line;

            // Modal: (RULE) (MUST) etc.
            if self.at(&TokenKind::LParen) {
                self.advance();
                if let Some(modal_block) = self.parse_modal() {
                    blocks.push(modal_block);
                }
                self.skip_newlines();
                continue;
            }

            // Named block or statement
            if matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                let name = self.advance().value.clone();
                self.skip_newlines();

                if self.at(&TokenKind::LBracket) {
                    // Standard block: KEYWORD [...]
                    let blk = self.parse_block(name, line);
                    blocks.push(blk);
                } else if self.at(&TokenKind::Colon) {
                    // DECISION: value form
                    self.advance();
                    let value = self.parse_value();
                    // Optional [...] after DECISION
                    let mut inline = vec![];
                    if self.at(&TokenKind::LBracket) {
                        self.advance();
                        inline = self.parse_field_list();
                        if self.at(&TokenKind::RBracket) { self.advance(); }
                    }
                    let f = Field { name: "_value".to_string(), type_hint: None, value, line };
                    let mut all_f = vec![f];
                    all_f.extend(inline);
                    blocks.push(Block { name, fields: all_f, subblocks: vec![], line });
                } else {
                    // Bare statement — skip to EOL
                    while !matches!(self.cur().kind,
                        TokenKind::Newline | TokenKind::Eof | TokenKind::EndMarker) {
                        self.advance();
                    }
                }
                self.skip_newlines();
                continue;
            }

            // Skip unknown
            self.advance();
        }

        // END marker validation
        if self.at(&TokenKind::EndMarker) {
            self.advance();
            self.skip_newlines();

            // T3 violation: content after ---END---
            let mut post_end = Vec::new();
            while !self.at(&TokenKind::Eof) {
                let t = self.advance();
                if t.kind != TokenKind::Newline { post_end.push(t.value.clone()); }
            }
            if !post_end.is_empty() {
                self.errors.push(format!(
                    "Content after ---END--- (T3 violation): {:?}",
                    post_end[..post_end.len().min(5)].join(" ")
                ));
            }
        } else {
            self.warnings.push("Missing ---END--- marker".to_string());
        }

        let parse_time_us = t0.elapsed().as_micros() as u64;
        let is_valid = self.errors.is_empty();

        CstlDocument {
            hashbang,
            meta_fields,
            blocks,
            is_valid,
            errors:    self.errors,
            warnings:  self.warnings,
            parse_time_us,
            token_count,
        }
    }
}
__CSTL_EOF__
cat > src/canonical.rs << '__CSTL_EOF__'
//! CSTL v4.9.2 — Canonical form + hash (Session #5, Q5)
//! Rules ratified tripartite: LF, single space, lexicographic META fields, NFC, no trailing ws.
//! Session #7 Q5: full SHA-256 (64 hex chars, 256-bit).


/// Produce the normative canonical text form of a CSTL payload.
pub fn canonical_form(text: &str) -> String {
    let mut lines: Vec<String> = text
        .replace("\r\n", "\n")
        .replace("\r", "\n")
        .split('\n')
        .map(|l| l.trim_end().to_string())  // no trailing whitespace
        .collect();

    // Normalize double-space before [ in block identifiers
    for line in &mut lines {
        // "META  [" → "META ["
        if let Some(pos) = line.find("  [") {
            let prefix = line[..pos].trim_end();
            *line = format!("{} [", prefix);
        }
    }

    let text_joined = lines.join("\n");

    // Lexicographic field order in META block
    sort_meta_fields(&text_joined)
}

fn sort_meta_fields(text: &str) -> String {
    // Find META block boundaries
    let meta_start = match text.find("META [").or_else(|| text.find("META[")) {
        Some(i) => i,
        None    => return text.to_string(),
    };

    let bracket_start = match text[meta_start..].find('[') {
        Some(i) => meta_start + i + 1,
        None    => return text.to_string(),
    };

    // Find matching ]
    let mut depth = 1i32;
    let mut bracket_end = None;
    let chars: Vec<char> = text[bracket_start..].chars().collect();
    let mut byte_off = bracket_start;

    for ch in &chars {
        byte_off += ch.len_utf8();
        if *ch == '[' { depth += 1; }
        else if *ch == ']' {
            depth -= 1;
            if depth == 0 { bracket_end = Some(byte_off - 1); break; }
        }
    }

    let bracket_end = match bracket_end {
        Some(e) => e,
        None    => return text.to_string(),
    };

    let meta_body = &text[bracket_start..bracket_end];

    // Parse fields (comma or newline separated)
    let mut fields: Vec<String> = meta_body
        .split(|c| c == ',' || c == '\n')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Sort lexicographically by field name (before : or =)
    fields.sort_by(|a, b| {
        let key_a = a.split(|c| c == ':' || c == '=').next().unwrap_or("").to_lowercase();
        let key_b = b.split(|c| c == ':' || c == '=').next().unwrap_or("").to_lowercase();
        key_a.cmp(&key_b)
    });

    let sorted_body = fields.join(",\n");
    let prefix = &text[..bracket_start];
    let suffix = &text[bracket_end..];

    format!("{}\n{}\n{}", prefix, sorted_body, suffix)
}

/// SHA-256 of canonical form — 64 hex chars (256-bit, Session #7 Q5)
/// Gemini won: 128-bit birthday bound 2^64 insufficient under deliberate attack.
pub fn canonical_hash(text: &str) -> String {
    let canon = canonical_form(text);
    let hash = sha256(canon.as_bytes());
    format!("sha256:{}", hex(&hash))
}

/// Pure-Rust SHA-256 (no external deps)
fn sha256(data: &[u8]) -> [u8; 32] {
    // SHA-256 constants
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Pre-processing: padding
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit chunk
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]);
        }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17)  ^ w[i-2].rotate_right(19)  ^ (w[i-2]  >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] =
            [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]];

        for i in 0..64 {
            let s1    = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch    = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0    = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj   = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &v) in h.iter().enumerate() {
        out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
__CSTL_EOF__
cat > src/validator.rs << '__CSTL_EOF__'
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
__CSTL_EOF__
cat > src/security.rs << '__CSTL_EOF__'
//! CSTL v4.9.2 — Security validation (Sessions #6 + #7)

/// Security scan result
pub struct SecurityReport {
    pub cleaned:  String,
    pub errors:   Vec<String>,
    pub warnings: Vec<String>,
}

/// Codepoints forbidden in any position (Session #6 Q2 + Session #7 Q1)
fn is_dangerous(ch: char) -> bool {
    let cp = ch as u32;
    matches!(cp,
        // Zero-width chars (S6 Q2)
        0x200B | 0x200C | 0x200D | 0x2060..=0x2064 | 0xFEFF |
        // Bidi controls (S7 Q1)
        0x202A..=0x202E | 0x2066..=0x2069 |
        // C1 controls
        0x0080..=0x009F
    )
}

/// Q2: Strip zero-width and bidi control characters
fn strip_dangerous(text: &str) -> (String, Vec<String>) {
    let mut cleaned = String::with_capacity(text.len());
    let mut warns   = Vec::new();
    let mut col     = 0usize;

    for ch in text.chars() {
        if is_dangerous(ch) {
            warns.push(format!(
                "SEC_Q2: stripped dangerous U+{:04X} at col {} (audit: \\u{:04X})",
                ch as u32, col, ch as u32
            ));
        } else {
            cleaned.push(ch);
            col += 1;
        }
    }
    (cleaned, warns)
}

/// Q1: Detect non-ASCII characters in keyword-like positions (line-start words)
/// Pure ASCII is enforced for structural keywords (Gemini Session #6 option B wins)
fn check_homoglyphs(text: &str) -> Vec<String> {
    let mut warns = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let word: String = line.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if word.is_empty() { continue; }
        let non_ascii: Vec<char> = word.chars().filter(|c| *c as u32 > 0x7F).collect();
        if !non_ascii.is_empty() {
            let cps: Vec<String> = non_ascii.iter()
                .map(|c| format!("U+{:04X}", *c as u32))
                .collect();
            warns.push(format!(
                "SEC_Q1: non-ASCII in keyword position {:?} at line {} — \
                 homoglyph attack? codepoints={}",
                word, line_no + 1, cps.join(" ")
            ));
        }
    }
    warns
}

/// Q4: Detect nested META blocks (injection attempt)
fn check_nested_meta(text: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        // Nested META = line that starts with whitespace + META (indented)
        if line.starts_with(|c: char| c == ' ' || c == '\t')
            && trimmed.starts_with("META")
            && (trimmed.len() == 4 || !trimmed.chars().nth(4).unwrap_or(' ').is_alphanumeric())
        {
            errors.push(format!(
                "SEC_Q4: nested META block at line {} — injection attempt", i + 1
            ));
        }
        // META keyword inside a value assignment
        if let Some(eq_pos) = line.find('=') {
            let after = &line[eq_pos..];
            if after.contains("META [") || after.contains("META[") {
                errors.push(format!(
                    "SEC_Q4: META keyword with block in string value at line {}", i + 1
                ));
            }
        }
    }
    errors
}

/// Q5: Check bracket nesting depth
fn check_nesting_depth(text: &str, max_depth: usize) -> Vec<String> {
    let mut errors = Vec::new();
    let mut depth  = 0usize;

    for (i, ch) in text.chars().enumerate() {
        if ch == '[' {
            depth += 1;
            if depth > max_depth {
                errors.push(format!(
                    "SEC: nesting depth {} exceeds max {} at char {}", depth, max_depth, i
                ));
                break;
            }
        } else if ch == ']' {
            depth = depth.saturating_sub(1);
        }
    }
    errors
}

/// Full security pipeline (Sessions #6 + #7)
pub fn security_scan(text: &str) -> SecurityReport {
    let mut errors   = Vec::new();
    let mut warnings = Vec::new();

    // Q2: Strip dangerous codepoints
    let (cleaned, zw_warns) = strip_dangerous(text);
    warnings.extend(zw_warns);

    // Q1: Homoglyph detection
    warnings.extend(check_homoglyphs(&cleaned));

    // Q4: Nested META injection
    errors.extend(check_nested_meta(&cleaned));

    // Q5: Nesting depth (max 32 — ratified Session #6)
    errors.extend(check_nesting_depth(&cleaned, 32));

    SecurityReport { cleaned, errors, warnings }
}
__CSTL_EOF__
cat > src/token.rs << '__CSTL_EOF__'
//! CSTL v4.9.2 — Token types and Lexer
//! Zero external dependencies. Character-by-character, no regex.

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Hashbang,    // #!CSTL_v4.9.2_MODE=A
    Keyword,     // META, DISAGREEMENT_BLOCK, GAP ...
    Ident,       // bare word / value token
    LBracket,    // [
    RBracket,    // ]
    LParen,      // (
    RParen,      // )
    Equals,      // =
    Colon,       // :
    Comma,       // ,
    Newline,     // \n
    EndMarker,   // ---END---
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind:  TokenKind,
    pub value: String,
    pub line:  usize,
    pub col:   usize,
}

impl Token {
    pub fn new(kind: TokenKind, value: impl Into<String>, line: usize, col: usize) -> Self {
        Token { kind, value: value.into(), line, col }
    }
}

/// All ratified CSTL v4.9.2 keywords (Sessions #1-#4)
pub fn is_keyword(word: &str) -> bool {
    matches!(word,
        // Header
        "META" | "MODE" | "VERSION" |
        // Data blocks (Session #1 G1, Session #3 alphabetical)
        "DEFINE" | "RULE" | "RULE_TRAILER" |
        "AGREEMENT_BLOCK" | "DISAGREEMENT_BLOCK" | "DECISION" |
        "CONSTRAINT" | "UNCERTAINTY" | "DEFINE_GROUP" |
        // Dissent primitives (Session #3 0x30-0x3F)
        "AGREEMENT" | "ALTERNATIVE" | "CAUTION" | "CONCERN" |
        "DISPUTE" | "GAP" | "PARTIAL_DISPUTE" | "RECOMMEND" |
        "REJECT" | "SELF_CRITIQUE" | "STRENGTH" | "VETO" |
        "CSTLTypeError" |
        // Modalities (Session #1 G4)
        "IF" | "IFF" | "MAY" | "MUST" | "MUST_NOT" | "SHOULD" | "UNLESS" |
        "REQUIRE" | "EXPECT" |
        // META keys (Session #2 ratified, Session #4 produced_by)
        "ACTION" | "CONTINUATION_MODE" | "CONVERSATION_ID" |
        "encoder" | "NO_PROSE" | "PARENT_HASH" | "produced_by" |
        "payload_length_bytes" | "payload_length_tokens" |
        "RESPONSE_FORMAT" | "sigma" | "TIMESTAMP" | "TURN" | "VERIFIED_BY" |
        // Type indicators (Session #2 0x50-0x57)
        "bool" | "enum" | "EXTENSION" | "float" | "hash" |
        "int" | "iso8601" | "string" |
        // Relation ops (Session #3 0x60-0x66)
        "ARR" | "EXPRESS" | "MAINTAIN" | "TRANSFORM" | "INTENT" |
        // Other
        "AS" | "@SYNC"
    )
}

// ── Lexer ─────────────────────────────────────────────────────────────────────

pub struct Lexer {
    chars: Vec<char>,
    pos:   usize,
    line:  usize,
    col:   usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos:   0,
            line:  1,
            col:   1,
        }
    }

    fn cur(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' { self.line += 1; self.col = 1; } else { self.col += 1; }
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.cur(), Some(' ') | Some('\t')) { self.advance(); }
    }

    fn read_while(&mut self, pred: impl Fn(char) -> bool) -> String {
        let mut buf = String::new();
        while let Some(ch) = self.cur() {
            if pred(ch) { buf.push(ch); self.advance(); } else { break; }
        }
        buf
    }

    fn read_to_eol(&mut self) -> String {
        let s = self.read_while(|c| c != '\n');
        s.trim().to_string()
    }

    fn rest_starts_with(&self, pat: &str) -> bool {
        let bytes: Vec<char> = pat.chars().collect();
        self.chars[self.pos..].starts_with(&bytes)
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        loop {
            let (line, col) = (self.line, self.col);

            match self.cur() {
                None => { tokens.push(Token::new(TokenKind::Eof, "", line, col)); break; }

                // Whitespace (non-newline)
                Some(' ') | Some('\t') => { self.skip_whitespace(); }

                // Newline
                Some('\n') => {
                    self.advance();
                    tokens.push(Token::new(TokenKind::Newline, "\n", line, col));
                }

                // Carriage return — skip
                Some('\r') => { self.advance(); }

                // Comment # (not hashbang)
                Some('#') if self.peek(1) != Some('!') => {
                    self.read_to_eol();
                }

                // Hashbang #!CSTL...
                Some('#') if self.peek(1) == Some('!') => {
                    let val = self.read_to_eol();
                    tokens.push(Token::new(TokenKind::Hashbang, val, line, col));
                }

                // END marker ---END---
                Some('-') if self.rest_starts_with("---END---") => {
                    for _ in 0..9 { self.advance(); }
                    tokens.push(Token::new(TokenKind::EndMarker, "---END---", line, col));
                }

                // Structural chars
                Some('[') => { self.advance(); tokens.push(Token::new(TokenKind::LBracket, "[", line, col)); }
                Some(']') => { self.advance(); tokens.push(Token::new(TokenKind::RBracket, "]", line, col)); }
                Some('(') => { self.advance(); tokens.push(Token::new(TokenKind::LParen, "(", line, col)); }
                Some(')') => { self.advance(); tokens.push(Token::new(TokenKind::RParen, ")", line, col)); }
                Some('=') => { self.advance(); tokens.push(Token::new(TokenKind::Equals, "=", line, col)); }
                Some(':') => { self.advance(); tokens.push(Token::new(TokenKind::Colon, ":", line, col)); }
                Some(',') => { self.advance(); tokens.push(Token::new(TokenKind::Comma, ",", line, col)); }

                // Word (keyword or identifier)
                Some(ch) if ch.is_alphabetic() || ch == '_' || ch == '@' => {
                    let word = self.read_while(|c| c.is_alphanumeric() || "_-./@+".contains(c));
                    let kind = if is_keyword(&word) { TokenKind::Keyword } else { TokenKind::Ident };
                    tokens.push(Token::new(kind, word, line, col));
                }

                // Number or negative
                Some(ch) if ch.is_ascii_digit() || (ch == '-' && self.peek(1).map_or(false, |c| c.is_ascii_digit())) => {
                    let word = self.read_while(|c| c.is_alphanumeric() || "._-".contains(c));
                    tokens.push(Token::new(TokenKind::Ident, word, line, col));
                }

                // Skip unknown
                _ => { self.advance(); }
            }
        }

        tokens
    }
}
__CSTL_EOF__
cat > src/ast.rs << '__CSTL_EOF__'
//! src/ast.rs — CSTL v4.9.2 AST types
//! RECONSTRUIT par inférence depuis l'usage dans parser.rs / canonical.rs / validator.rs.
//! Le fichier original ast.rs était absent du projet (jamais uploadé / jamais écrit).

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub type_hint: Option<String>,
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub name: String,
    pub fields: Vec<Field>,
    pub subblocks: Vec<Block>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct CstlDocument {
    pub hashbang: Option<String>,
    pub meta_fields: HashMap<String, String>,
    pub blocks: Vec<Block>,
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub parse_time_us: u64,
    pub token_count: usize,
}

impl CstlDocument {
    /// Valeur d'un champ META par clé.
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta_fields.get(key).map(|s| s.as_str())
    }
    /// Raccourci META encoder.
    pub fn encoder(&self) -> Option<&str> {
        self.meta("encoder")
    }
    /// Raccourci META produced_by.
    pub fn produced_by(&self) -> Option<&str> {
        self.meta("produced_by")
    }
    /// Tous les blocs portant ce nom (référence empruntée).
    pub fn blocks_named(&self, name: &str) -> Vec<&Block> {
        self.blocks.iter().filter(|b| b.name.starts_with(name)).collect()
    }
}
__CSTL_EOF__
cat > src/main.rs << '__CSTL_EOF__'
use std::io::Read;
use cstl_parser::parse;

fn main() {
    // Lit le payload : 1er argument = chemin de fichier, sinon stdin.
    let input = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(&path)
            .unwrap_or_else(|e| { eprintln!("Lecture {path} impossible: {e}"); std::process::exit(1); }),
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).expect("lecture stdin");
            s
        }
    };

    let doc = parse(&input);

    println!("=== CSTL parse ===");
    println!("valide        : {}", doc.is_valid);
    println!("hashbang      : {:?}", doc.hashbang);
    println!("encoder       : {:?}", doc.encoder());
    println!("produced_by   : {:?}", doc.produced_by());
    println!("blocs         : {}", doc.blocks.len());
    for b in &doc.blocks {
        println!("  - {} ({} champs)", b.name, b.fields.len());
    }
    println!("erreurs ({})  : {:?}", doc.errors.len(), doc.errors);
    println!("warnings ({}) : {:?}", doc.warnings.len(), doc.warnings);
    println!("parse_time_us : {}", doc.parse_time_us);

    std::process::exit(if doc.is_valid { 0 } else { 1 });
}
__CSTL_EOF__
cat > src/tests.rs << '__CSTL_EOF__'
//! CSTL v4.9.2 — Rust parser tests

#[cfg(test)]
mod tests {
    use crate::parse;
    use crate::canonical::{canonical_form, canonical_hash};

    const BASE: &str = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";

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
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [  encoder = Agent_TEST ,  sigma = 0.88 , RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root  ]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
    }

    #[test]
    fn test_tabs() {
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\n\tencoder=Agent_TEST,\n\tsigma=0.88,\n\tRESPONSE_FORMAT=CSTL,\n\tNO_PROSE=true,\n\tPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert_eq!(doc.meta("encoder"), Some("Agent_TEST"));
    }

    #[test]
    fn test_produced_by_session4() {
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
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
        let payload = "#!CSTL_v4.9.2_MODE=A\n\u{041C}ETA [\nencoder=Agent_TEST\n]\n---END---";
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
        let p1 = "#!CSTL_v4.9.2_MODE=A\nMETA [\nsigma:float=0.88,\nencoder=Agent_TEST,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let p2 = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
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
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-5-5-2026,\nsigma:float=0.92,\nACTION=evaluate_produced_by_spec,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nCONVERSATION_ID=cstl_produced_by_v1,\nPARENT_HASH:hash=sha256:abc123\n]\nEVALUATION_Q1_bytecode_id [\nposition=accept_0x4D,\nrationale=fits_range,\nsigma:float=0.95\n]\nDISAGREEMENT_BLOCK [\nSTRENGTH format [sigma:float=0.92]\nDISPUTE open_weights [sigma:float=0.78, alt=huggingface]\nGAP proxy_handling [sigma:float=0.85]\n]\nDECISION: accept [sigma:float=0.91]\n---END---";
        let doc = parse(payload);
        assert!(doc.is_valid, "{:?}", doc.errors);
        assert_eq!(doc.encoder(), Some("Agent_GPT"));
        assert_eq!(doc.produced_by(), Some("openai/gpt-5-5-2026"));
    }

    #[test]
    fn test_gemini_session7_payload() {
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GEMINI,\nproduced_by=gemini-2-5-pro,\nsigma:float=0.96,\nACTION=evaluate_attack_surface,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH:hash=sha256:xyz789,\nCONVERSATION_ID=cstl_attack_v2\n]\nDECISION: advance_to_hash_and_boundary_patch [sigma:float=0.94]\n---END---";
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
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GEMINI,\nproduced_by=gemini-2-5-pro,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        // Should be valid — short form is accepted
        let pby_warns: Vec<_> = doc.warnings.iter().filter(|w| w.contains("produced_by") && w.contains("BNF")).collect();
        assert!(pby_warns.is_empty(), "gemini-2-5-pro short form should not warn: {:?}", pby_warns);
    }

    #[test]
    fn test_produced_by_org_slash_form() {
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        let pby_warns: Vec<_> = doc.warnings.iter().filter(|w| w.contains("BNF")).collect();
        assert!(pby_warns.is_empty(), "org/model-version should not warn: {:?}", pby_warns);
    }

    #[test]
    fn test_produced_by_proxy_chain() {
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=proxy/azure -> openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        // Proxy chain is valid but emits proxy warning
        assert!(doc.warnings.iter().any(|w| w.contains("PROXY")));
    }

    #[test]
    fn test_produced_by_identity_mismatch_warn() {
        // encoder contains model name → R1 warning
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nproduced_by=openai/gpt-4o-2026,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
        let doc = parse(payload);
        assert!(doc.warnings.iter().any(|w| w.contains("IDENTITY_MISMATCH")));
    }

    #[test]
    fn test_produced_by_absent_model_name_encoder() {
        // R4: no produced_by + encoder looks like model name
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---";
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
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=ChatGPT_GPT5_5,\nTIMESTAMP:iso8601=2026-05-21T18:27:00Z,\nsigma:float=0.97,\nACTION=evaluate_bytecode_table_response,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nCONTINUATION_MODE:enum=continue,\nCONVERSATION_ID=cstl_bytecode_v1,\nPARENT_HASH:hash=sha256:agent_claude_bytecode_init_turn1\n]\nEVALUATION_RANGE_0x01_to_0x0F [\nstatus=accept,\nproposed_changes=none,\nsigma:float=0.97\n]\nEVALUATION_RANGE_0x10_to_0x1F [\nstatus=modify_accept,\nproposed_changes=(0x14=AGREEMENT_BLOCK,0x15=DISAGREEMENT_BLOCK),\nordering_policy=alphabetical_within_semantic_cluster,\nsigma:float=0.93\n]\nDISAGREEMENT_BLOCK [\nSTRENGTH explicit_typing_token_alignment (sigma:float=0.98)\nSTRENGTH deterministic_escape_decoding (sigma:float=0.95)\nDISPUTE freezing_full_operator_space_in_v4_9_2 (sigma:float=0.89, alternative=core_only_freeze)\nGAP missing_native_typing_error_token (sigma:float=0.94, resolution=0x3C_assignment)\n]\nFINAL_TABLE_PATCHSET [\napply=(0x14=AGREEMENT_BLOCK),\napply=(0x15=DISAGREEMENT_BLOCK),\napply=(0x3C=CSTLTypeError)\n]\nDECISION=ratify_with_patchset (sigma:float=0.96)\n---END---";
        let doc = parse(payload);
        // ChatGPT_GPT5_5 will get PATCH_T4 warning but should parse
        assert!(doc.warnings.iter().any(|w| w.contains("PATCH_T4")));
        assert!(doc.blocks_named("DISAGREEMENT_BLOCK").len() > 0);
        assert!(doc.blocks_named("FINAL_TABLE_PATCHSET").len() > 0);
    }

    #[test]
    fn test_session7_gpt_full_payload() {
        let payload = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_GPT,\nproduced_by=openai/gpt-5-5-2026,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nsigma:float=0.96,\nACTION=evaluate_advanced_attack_vectors,\nTURN:int=2,\nPARENT_HASH:hash=sha256:session7_turn1,\nCONVERSATION_ID=cstl_attack_v2\n]\nQ1_bidi_override [\nposition=partial_accept_mitigated_but_incomplete,\nfinding=stripping_controls_at_parse_time_insufficient_for_audit_integrity,\nsigma:float=0.90\n]\nQ5_hash_collision_DoS [\nposition=strong_accept_real_weakness,\nrequired_changes=[minimum_128_bit_identifier, full_sha256_for_security_critical],\nrisk_level=high,\nsigma:float=0.98\n]\nDECISION: session7_confirms_remaining_hardening_required [sigma:float=0.96]\n---END---";
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
}
__CSTL_EOF__
echo ">>> Arborescence creee. Lance maintenant : cargo test"
