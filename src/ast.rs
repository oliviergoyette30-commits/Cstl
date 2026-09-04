//! CSTL v5.0.0 — AST node types
//!
//! Field/Relation sont reellement utilises par semantic.rs (branches sur le
//! pipeline TCP reel depuis l'audit multi-angle du 2026-09-03, voir
//! server/validator.rs::check_sdl_operator_whitelist et
//! validate_deontic_constraints).
//!
//! `Block` (arbre generique nom+champs+sous-blocs, pour la syntaxe
//! `(MUST)`/`(RULE)`/`(IF)`) a ete retire le 2026-09-04 (item #2 de la liste
//! des choses a faire, apres fix19) : recherche exhaustive au moment de
//! trancher entre "brancher" et "supprimer" -- AUCUN code de ce depot ne
//! construit jamais de `Block` en dehors de tests unitaires. Le tokenizer
//! (token.rs) produit des `Token`, mais rien ne les assemble en arbre
//! `Block` en production; le format reellement parle sur le fil (parser.rs)
//! est une serie de blocs `HashMap<String,String>` a plat (META/
//! INTENT_PAYLOAD/RELATION), jamais l'arbre `(MUST) { _stmt: "..." }` que
//! `Block` supposait. Le seul consommateur de `Block` etait
//! validator_semantic.rs (996 lignes, R1-R13, dont l'audit deontique R11 --
//! remplace en pratique par semantic.rs::check_axiom_d + Couche 8 depuis
//! fix19), lui-meme jamais appele hors de ses propres tests -- verifie par
//! recherche exhaustive avant suppression. Supprime plutot que branche:
//! le brancher aurait exige d'ecrire le vrai parser tokens->Block qui n'a
//! jamais existe (risque eleve de dupliquer, avec un modele de donnees
//! different, le travail que fait deja server/parser.rs -- meme raisonnement
//! que pour CstlDocument ci-dessous).
//!
//! CstlDocument (le type produit par une fonction `crate::parse()` qui
//! n'a jamais existe) a ete retire le 2026-09-03 pour la meme raison: son
//! seul et unique consommateur etait src/tests.rs, un fichier de 796 lignes
//! de tests jamais declare comme module dans lib.rs (aucun `mod tests;`),
//! donc jamais compile ni execute par `cargo test` -- decouvert en corrigeant
//! le badge de tests du README (qui pointait dessus en affichant un
//! chiffre errone).

#[derive(Debug, Clone)]
pub struct Field {
    pub name:      String,
    pub type_hint: Option<String>,
    pub value:     String,
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

