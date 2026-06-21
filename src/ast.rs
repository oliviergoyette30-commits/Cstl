//! src/ast.rs — CSTL v4.9.3 AST types
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
