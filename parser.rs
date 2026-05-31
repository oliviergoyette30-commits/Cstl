//! CSTL v4.9.3 — Recursive Descent Parser
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
