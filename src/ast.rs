//! CSTL v5.0.0 — AST node types
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Field {
    pub name:      String,
    pub type_hint: Option<String>,
    pub value:     String,
    pub line:      usize,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub name:      String,
    pub fields:    Vec<Field>,
    pub subblocks: Vec<Block>,
    pub line:      usize,
}

#[derive(Debug, Clone)]
pub struct Relation {
    pub subject:  String,
    pub operator: String,
    pub object:   String,
    pub attrs:    Vec<Field>,
    pub modality: Option<String>,
    pub line:     usize,
}

#[derive(Debug)]
pub struct CstlDocument {
    pub hashbang:      Option<String>,
    pub meta_fields:   HashMap<String, String>,
    pub blocks:        Vec<Block>,
    pub relations:     Vec<Relation>,
    pub is_valid:      bool,
    pub errors:        Vec<String>,
    pub warnings:      Vec<String>,
    pub parse_time_us: u64,
    pub token_count:   usize,
}

impl CstlDocument {
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta_fields.get(key).map(|s| s.as_str())
    }
    pub fn encoder(&self) -> Option<&str> { self.meta("encoder") }
    pub fn produced_by(&self) -> Option<&str> { self.meta("produced_by") }
    pub fn blocks_named(&self, prefix: &str) -> Vec<&Block> {
        self.blocks.iter()
            .filter(|b| b.name == prefix
                || b.name.starts_with(&format!("{}_", prefix))
                || b.name.starts_with(&format!("{}:", prefix)))
            .collect()
    }
    pub fn relations_by_op(&self, op: &str) -> Vec<&Relation> {
        self.relations.iter().filter(|r| r.operator == op).collect()
    }
    pub fn relations_by_subject(&self, subj: &str) -> Vec<&Relation> {
        self.relations.iter().filter(|r| r.subject == subj).collect()
    }
    pub fn relation_sigma(rel: &Relation) -> Option<f64> {
        rel.attrs.iter()
            .find(|f| f.name == "sigma" || f.name == "σ")
            .and_then(|f| f.value.parse().ok())
    }
}
