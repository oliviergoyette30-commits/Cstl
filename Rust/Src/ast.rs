//! CSTL v4.9.3 — Abstract Syntax Tree
//!
//! Data structures produced by the parser. A CstlDocument is the full result
//! of the parse + validate pipeline: the hashbang line, the META fields, the
//! ordered semantic blocks, and the validation outcome (errors, warnings).

use std::collections::HashMap;

/// A single field inside a block: `name:type=value` or `name=value`.
#[derive(Debug, Clone)]
pub struct Field {
    /// Field name (e.g. `encoder`, `sigma`). Special internal names:
    /// `_stmt` for bracketless statements, `_value` for bare values.
    pub name: String,
    /// Optional inline type hint (the `:type` part), if present.
    pub type_hint: Option<String>,
    /// Field value as a string.
    pub value: String,
    /// 1-based source line where the field was parsed.
    pub line: usize,
}

/// A semantic block: a named section containing fields and optional subblocks.
#[derive(Debug, Clone)]
pub struct Block {
    /// Block name (e.g. `META`, `CONSTRAINTS`, `DECISION`). Subblocks created
    /// from a `TYPE label` pattern use the name form `TYPE:label`.
    pub name: String,
    /// Ordered fields contained directly in this block.
    pub fields: Vec<Field>,
    /// Nested subblocks.
    pub subblocks: Vec<Block>,
    /// 1-based source line where the block opened.
    pub line: usize,
}

/// The full parse result of a CSTL payload.
#[derive(Debug, Clone)]
pub struct CstlDocument {
    /// The hashbang line value, if present (e.g. `CSTL v4.9.3 MODE=A`).
    pub hashbang: Option<String>,
    /// Flattened META fields as a name -> value map.
    pub meta_fields: HashMap<String, String>,
    /// Ordered semantic blocks (excluding META, which is flattened above).
    pub blocks: Vec<Block>,
    /// True when no errors were produced by the pipeline.
    pub is_valid: bool,
    /// Hard errors (E-class). A non-empty list means the payload is invalid.
    pub errors: Vec<String>,
    /// Non-fatal warnings (W-class and security advisories).
    pub warnings: Vec<String>,
    /// Parse time in microseconds.
    pub parse_time_us: u64,
    /// Number of tokens produced by the lexer.
    pub token_count: usize,
}
