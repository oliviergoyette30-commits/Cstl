//! CSTL v5.0.0 — AST node types
//!
//! Field/Block/Relation sont reellement utilises par semantic.rs et
//! validator_semantic.rs (branches sur le pipeline TCP reel depuis l'audit
//! multi-angle du 2026-09-03, voir server/validator.rs::check_sdl_operator_whitelist).
//!
//! CstlDocument (le type produit par une fonction `crate::parse()` qui
//! n'a jamais existe) a ete retire le meme jour: son seul et unique
//! consommateur etait src/tests.rs, un fichier de 796 lignes de tests
//! jamais declare comme module dans lib.rs (aucun `mod tests;`), donc
//! jamais compile ni execute par `cargo test` -- decouvert en corrigeant
//! le badge de tests du README (qui pointait dessus en affichant un
//! chiffre errone). Plutot que d'ecrire de toutes pieces le parser +
//! tokenizer complet que ces 796 lignes de tests supposaient (feature
//! entiere jamais implementee, pas juste un bug a corriger -- risque
//! eleve de dupliquer, avec un modele de donnees different, le travail
//! que fait deja server/parser.rs), les deux fichiers ont ete supprimes
//! pour que le depot ne pretende plus tester quelque chose qui n'existe
//! pas.

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

