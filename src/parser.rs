//! CSTL v5.0.0 — Recursive Descent Parser
//! v5.0 fixes:
//!   [FIX-1] parse_relation(): (subject) OPERATOR object [attrs] → Relation AST node
//!   [FIX-2] parse_modal(): handles (subject) OPERATOR pattern, not just (MODALITY)
//!   [FIX-3] parse_block(): Pattern for (subject) lines inside RELATIONS block
//!   [FIX-4] parse_field() backtrack: saved pos instead of manual arithmetic

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::ast::{Block, CstlDocument, Field, Relation};
use crate::token::{Token, TokenKind};

/// All v5.0 relation operators (logical, epistemic, temporal, relational, core)
const RELATION_OPERATORS: &[&str] = &[
    // v5.0 new
    "ENTAILS", "CONTRADICTS",
    "BELIEVES", "KNOWS", "ASSUMES", "DOUBTS",
    "BEFORE", "AFTER", "DURING",
    "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES", "COMPARES",
    // v5.0 deprecated (still valid)
    "MUTUAL",
    // core v4
    "ARR", "EXPRESS", "MAINTAIN", "TRANSFORM", "INTENT",
    "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "AMP", "INH", "PRESSURE", "CATALYZE",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
    "RESIST", "CAUSE",
];

fn is_relation_operator(word: &str) -> bool {
    RELATION_OPERATORS.contains(&word)
}

/// Deontic modalities valid before a relation
const MODALITIES: &[&str] = &["MUST", "MUST_NOT", "MAY", "SHOULD", "IF", "IFF", "UNLESS"];

fn is_modality(word: &str) -> bool {
    MODALITIES.contains(&word)
}

pub struct Parser {
    tokens:    Vec<Token>,
    pos:       usize,
    errors:    Vec<String>,
    warnings:  Vec<String>,
    relations: Vec<Relation>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0, errors: vec![], warnings: vec![], relations: vec![] }
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

    fn save_pos(&self) -> usize { self.pos }

    fn restore_pos(&mut self, saved: usize) { self.pos = saved; }

    // ── Value parsing ─────────────────────────────────────────────────────────

    fn parse_value(&mut self) -> String {
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
                _ => { parts.push(self.advance().value.clone()); }
            }
        }
        parts.join(" ").trim().to_string()
    }

    // ── Field parsing ─────────────────────────────────────────────────────────

    /// [FIX-4] Backtrack uses saved pos, not manual arithmetic
    fn parse_field(&mut self) -> Option<Field> {
        self.skip_newlines();

        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            return None;
        }

        let saved = self.save_pos(); // [FIX-4]
        let line  = self.cur().line;
        let name  = self.advance().value.clone();
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
            self.restore_pos(saved); // [FIX-4] clean backtrack
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

    // ── [FIX-1] Relation parsing ──────────────────────────────────────────────
    //
    // Parses: [(MODALITY)] (subject) OPERATOR object [attrs]
    //
    // Examples:
    //   (hypothesis) ENTAILS finding [sigma=0.85, tau=future]
    //   (MUST) agent KNOWS fact [sigma=0.97]
    //   (step_1) BEFORE step_2 [sigma=1.0]
    //
    // Called when cur() == LParen

    fn try_parse_relation(&mut self) -> Option<Relation> {
        let saved = self.save_pos();
        let line  = self.cur().line;

        // Must start with (
        if !self.at(&TokenKind::LParen) {
            return None;
        }
        self.advance(); // consume (

        // Read identifier inside parens
        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            self.restore_pos(saved);
            return None;
        }
        let inner = self.advance().value.clone();

        if !self.at(&TokenKind::RParen) {
            self.restore_pos(saved);
            return None;
        }
        self.advance(); // consume )

        // Case A: (MODALITY) subject OPERATOR object [attrs]
        // e.g. (MUST) agent KNOWS fact [sigma=0.97]
        if is_modality(&inner) {
            let modality = inner.clone();
            // subject
            if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                self.restore_pos(saved);
                return None;
            }
            let subject = self.advance().value.clone();

            // operator
            if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                self.restore_pos(saved);
                return None;
            }
            let op_candidate = self.cur().value.clone();
            if !is_relation_operator(&op_candidate) {
                self.restore_pos(saved);
                return None;
            }
            self.advance();

            // object
            if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                self.restore_pos(saved);
                return None;
            }
            let object = self.advance().value.clone();

            // optional [attrs]
            let attrs = self.parse_optional_attrs();

            return Some(Relation {
                subject,
                operator: op_candidate,
                object,
                attrs,
                modality: Some(modality),
                line,
            });
        }

        // Case B: (subject) OPERATOR object [attrs]
        // e.g. (hypothesis) ENTAILS finding [sigma=0.85]
        let subject = inner;

        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            self.restore_pos(saved);
            return None;
        }
        let op_candidate = self.cur().value.clone();
        if !is_relation_operator(&op_candidate) {
            // Not a relation — restore and let parse_modal handle it
            self.restore_pos(saved);
            return None;
        }
        self.advance(); // consume operator

        // object
        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            self.restore_pos(saved);
            return None;
        }
        let object = self.advance().value.clone();

        // optional [attrs]
        let attrs = self.parse_optional_attrs();

        Some(Relation {
            subject,
            operator: op_candidate,
            object,
            attrs,
            modality: None,
            line,
        })
    }

    fn parse_optional_attrs(&mut self) -> Vec<Field> {
        if self.at(&TokenKind::LBracket) {
            self.advance();
            let fields = self.parse_field_list();
            if self.at(&TokenKind::RBracket) { self.advance(); }
            fields
        } else {
            vec![]
        }
    }

    // ── [FIX-2] Modal statement ───────────────────────────────────────────────
    //
    // parse_modal is called after ( is consumed at top-level.
    // It now correctly distinguishes:
    //   (MODALITY) subject verb ...   → modal statement (non-relation)
    //   (subject) OPERATOR ...        → handled by try_parse_relation before reaching here
    //
    // This function only handles true modal statements like:
    //   (RULE) MUST respond_in_cstl_only
    //   (MUST) team ADMINISTER aspirin

    fn parse_modal(&mut self) -> Option<Block> {
        if !matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
            return None;
        }
        let modal = self.advance().value.clone();
        let line  = self.cur().line;

        if !self.at(&TokenKind::RParen) { return None; }
        self.advance(); // )

        // Collect rest of statement until [ or newline
        let mut parts = Vec::new();
        while !matches!(self.cur().kind,
            TokenKind::Newline | TokenKind::Eof | TokenKind::EndMarker |
            TokenKind::LBracket) {
            parts.push(self.advance().value.clone());
        }

        let mut inline_fields = vec![];
        if self.at(&TokenKind::LBracket) {
            self.advance();
            inline_fields = self.parse_field_list();
            if self.at(&TokenKind::RBracket) { self.advance(); }
        }

        let stmt_value = parts.join(" ").trim().to_string();
        let f = Field { name: "_stmt".to_string(), type_hint: None, value: stmt_value, line };
        let mut all_fields = vec![f];
        all_fields.extend(inline_fields);

        Some(Block {
            name: format!("({})", modal),
            fields: all_fields,
            subblocks: vec![],
            line,
        })
    }

    // ── Block parsing ─────────────────────────────────────────────────────────

    /// [FIX-3] parse_block now handles (subject) OPERATOR lines inside blocks
    /// e.g. inside RELATIONS [ ... ] or CONSTRAINTS [ ... ]
    fn parse_block(&mut self, name: String, line: usize) -> Block {
        let mut fields    = Vec::new();
        let mut subblocks = Vec::new();

        if self.at(&TokenKind::LBracket) { self.advance(); }
        self.skip_newlines();

        loop {
            self.skip_newlines();
            if self.at(&TokenKind::RBracket) || self.at_eof_or_end() { break; }

            let cur_line = self.cur().line;

            // [FIX-3] (subject) OPERATOR object [attrs] inside a block
            if self.at(&TokenKind::LParen) {
                if let Some(rel) = self.try_parse_relation() {
                    // Store as a subblock for backward compat + add to doc.relations
                    // The subblock name encodes the relation for querying
                    let rel_name = format!("REL:{}:{}:{}", rel.subject, rel.operator, rel.object);
                    let mut rel_fields = rel.attrs.clone();
                    rel_fields.push(Field {
                        name: "_subject".to_string(), type_hint: None,
                        value: rel.subject.clone(), line: rel.line,
                    });
                    rel_fields.push(Field {
                        name: "_operator".to_string(), type_hint: None,
                        value: rel.operator.clone(), line: rel.line,
                    });
                    rel_fields.push(Field {
                        name: "_object".to_string(), type_hint: None,
                        value: rel.object.clone(), line: rel.line,
                    });
                    if let Some(ref m) = rel.modality {
                        rel_fields.push(Field {
                            name: "_modality".to_string(), type_hint: None,
                            value: m.clone(), line: rel.line,
                        });
                    }
                    subblocks.push(Block {
                        name: rel_name, fields: rel_fields, subblocks: vec![], line: rel.line
                    });
                    self.relations.push(rel);
                    self.skip_newlines();
                    continue;
                }
                // Not a relation — fall through to parse_modal style
                self.advance(); // consume (
                if let Some(modal_blk) = self.parse_modal() {
                    subblocks.push(modal_blk);
                }
                self.skip_newlines();
                continue;
            }

            // Keyword [ — direct subblock
            if matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                let sub_kind = self.peek(1);

                if sub_kind.kind == TokenKind::LBracket {
                    let sub_name = self.advance().value.clone();
                    let sub = self.parse_block(sub_name, cur_line);
                    subblocks.push(sub);
                    self.skip_newlines();
                    continue;
                }

                // KEYWORD label [...] — e.g. GAP missing [sigma=0.85]
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
            }

            // Try field (key=value)
            if let Some(f) = self.parse_field() {
                fields.push(f);
                self.skip_newlines();
                if self.at(&TokenKind::Comma) { self.advance(); }
                continue;
            }

            // Nothing matched — skip token with warning if not newline
            let (tk, tv, tl) = {
                let t = self.advance();
                (t.kind.clone(), t.value.clone(), t.line)
            };
            if tk != TokenKind::Newline && tk != TokenKind::RBracket {
                self.warnings.push(format!(
                    "PARSER: skipped unrecognized token {:?} {:?} at line {}",
                    tk, tv, tl
                ));
            }
        }

        self.skip_newlines();
        if self.at(&TokenKind::RBracket) { self.advance(); }

        Block { name, fields, subblocks, line }
    }

    // ── Top-level parse ───────────────────────────────────────────────────────

    pub fn parse(mut self, token_count: usize) -> CstlDocument {
        let t0 = Instant::now();

        let mut hashbang    = None;
        let mut meta_fields = HashMap::new();
        let mut blocks      = Vec::new();
        let mut meta_found  = false;

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

        // Body
        self.skip_newlines();
        while !self.at_eof_or_end() {
            self.skip_newlines();
            if self.at_eof_or_end() { break; }

            let line = self.cur().line;

            // [FIX-2] Top-level (subject) OPERATOR ... or (MODAL) ...
            if self.at(&TokenKind::LParen) {
                // Try relation first
                if let Some(rel) = self.try_parse_relation() {
                    // Top-level relation — encode as block + store in relations
                    let rel_name = format!("REL:{}:{}:{}", rel.subject, rel.operator, rel.object);
                    let mut rel_fields = rel.attrs.clone();
                    rel_fields.push(Field { name: "_subject".to_string(), type_hint: None, value: rel.subject.clone(), line: rel.line });
                    rel_fields.push(Field { name: "_operator".to_string(), type_hint: None, value: rel.operator.clone(), line: rel.line });
                    rel_fields.push(Field { name: "_object".to_string(), type_hint: None, value: rel.object.clone(), line: rel.line });
                    blocks.push(Block { name: rel_name, fields: rel_fields, subblocks: vec![], line: rel.line });
                    self.relations.push(rel);
                } else {
                    // Modal statement
                    self.advance(); // consume (
                    if let Some(modal_block) = self.parse_modal() {
                        blocks.push(modal_block);
                    }
                }
                self.skip_newlines();
                continue;
            }

            // Named block or statement
            if matches!(self.cur().kind, TokenKind::Ident | TokenKind::Keyword) {
                let name = self.advance().value.clone();
                self.skip_newlines();

                if self.at(&TokenKind::LBracket) {
                    let blk = self.parse_block(name, line);
                    blocks.push(blk);
                } else if self.at(&TokenKind::Colon) {
                    self.advance();
                    let value = self.parse_value();
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

            self.advance();
        }

        // END marker
        if self.at(&TokenKind::EndMarker) {
            self.advance();
            self.skip_newlines();
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
        let is_valid      = self.errors.is_empty();

        CstlDocument {
            hashbang,
            meta_fields,
            blocks,
            relations: self.relations,
            is_valid,
            errors:    self.errors,
            warnings:  self.warnings,
            parse_time_us,
            token_count,
        }
    }
}
