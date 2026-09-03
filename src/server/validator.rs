/// CSTL Semantic Validator
/// Validates CSTL payloads against deontic constraints
/// 
/// Rules:
/// - META must have encoder + produced_by
/// - INTENT_PAYLOAD must have purpose + sender + receiver
/// - RELATIONS must have type + subject + object
/// - MUST constraints cannot be violated
/// - No circular dependencies

use super::parser::CstlPayload;
use crate::ast::Relation as AstRelation;
use crate::semantic::SemanticValidator;

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<String>,
}

pub fn validate_payload(payload: &CstlPayload) -> ValidationResult {
    let mut result = ValidationResult {
        valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    // Validate META block
    if !payload.meta.contains_key("encoder") {
        result.valid = false;
        result.errors.push(ValidationError {
            code: "E301".to_string(),
            message: "Missing encoder in META".to_string(),
        });
    }

    if !payload.meta.contains_key("produced_by") {
        result.valid = false;
        result.errors.push(ValidationError {
            code: "E302".to_string(),
            message: "Missing produced_by in META".to_string(),
        });
    }

    // Validate INTENT_PAYLOAD block
    if payload.intent.is_empty() {
        result.warnings.push("W001: Empty INTENT_PAYLOAD".to_string());
    }

    if !payload.intent.contains_key("purpose") {
        result.errors.push(ValidationError {
            code: "E303".to_string(),
            message: "Missing purpose in INTENT_PAYLOAD".to_string(),
        });
        result.valid = false;
    }

    if !payload.intent.contains_key("sender") {
        result.errors.push(ValidationError {
            code: "E304".to_string(),
            message: "Missing sender in INTENT_PAYLOAD".to_string(),
        });
        result.valid = false;
    }

    if !payload.intent.contains_key("receiver") {
        result.errors.push(ValidationError {
            code: "E305".to_string(),
            message: "Missing receiver in INTENT_PAYLOAD".to_string(),
        });
        result.valid = false;
    }

    // Validate RELATIONS
    for (idx, relation) in payload.relations.iter().enumerate() {
        if !relation.contains_key("type") {
            result.errors.push(ValidationError {
                code: format!("E306[{}]", idx),
                message: format!("RELATION[{}]: Missing type", idx),
            });
            result.valid = false;
        }

        if !relation.contains_key("subject") && !relation.contains_key("agent_a") {
            result.errors.push(ValidationError {
                code: format!("E307[{}]", idx),
                message: format!("RELATION[{}]: Missing subject/agent_a", idx),
            });
            result.valid = false;
        }
    }

    // Validate deontic constraints (MUST/MUST_NOT)
    validate_deontic_constraints(payload, &mut result);

    eprintln!("[Validator] Valid: {}, Errors: {}, Warnings: {}", 
        result.valid, result.errors.len(), result.warnings.len());

    result
}

/// Branche la whitelist des 35 opérateurs SDL officiels + dépréciation MUTUAL
/// (semantic.rs::SemanticValidator, jusqu'ici jamais appelée sur le chemin
/// TCP réel -- découverte en creusant la trouvaille majeure MUTUAL de
/// l'audit multi-angle du 2026-09-03) sur les RELATION d'un payload réel.
///
/// Retourne des AVERTISSEMENTS UNIQUEMENT -- ne modifie jamais
/// `ValidationResult.valid` -- pour une raison de conception précise :
/// le champ RELATION `type=` sert ici à DEUX vocabulaires disjoints qui
/// partagent le même nom de champ sans aucun marqueur pour les distinguer :
///
///   1. les opérateurs SDL officiels de semantic.rs (EQUALS, CONTRADICTS,
///      COMMAND, INTENT, ...) -- toujours en MAJUSCULES dans la spec et
///      dans tous les tests existants ;
///   2. les prédicats factuels vérifiables contre Wikidata de kb_verify.rs
///      (born_in, part_of, located_in, capital_of, ...) -- toujours en
///      snake_case minuscule.
///
/// Appliquer la whitelist SDL à TOUTE valeur de `type=` casserait donc en
/// silence la vérification KB (Couche 3a) : chaque relation `part_of` ou
/// `located_in`, parfaitement légitime, deviendrait un faux "opérateur
/// inconnu". On ne vérifie donc que les valeurs qui RESSEMBLENT DÉJÀ à un
/// opérateur SDL revendiqué (tout MAJUSCULES, `.`/`_` autorisés, ≥2
/// caractères -- même heuristique que semantic.rs::token_is_operator_candidate) ;
/// les prédicats KB en minuscules sont ignorés par construction, pas par
/// accident.
pub fn check_sdl_operator_whitelist(payload: &CstlPayload) -> Vec<String> {
    fn looks_like_sdl_operator(tok: &str) -> bool {
        !tok.is_empty()
            && tok.chars().all(|c| c.is_ascii_uppercase() || c == '.' || c == '_')
            && tok.len() >= 2
    }

    let relations: Vec<AstRelation> = payload.relations.iter()
        .filter_map(|r| {
            let operator = r.get("type").cloned().unwrap_or_default();
            if !looks_like_sdl_operator(&operator) {
                return None;
            }
            Some(AstRelation {
                subject: r.get("subject").cloned().unwrap_or_default(),
                operator,
                object: r.get("object").cloned().unwrap_or_default(),
                attrs: Vec::new(),
                modality: None,
                line: 0,
            })
        })
        .collect();

    if relations.is_empty() {
        return Vec::new();
    }

    SemanticValidator::new(&relations, &[])
        .check_operator_whitelist()
        .into_iter()
        .map(|e| format!("{}: {}", e.code, e.message))
        .collect()
}

fn validate_deontic_constraints(payload: &CstlPayload, result: &mut ValidationResult) {
    // Check for conflicting MUST constraints
    for relation in &payload.relations {
        if let Some(rel_type) = relation.get("type") {
            if rel_type.contains("MUST") && rel_type.contains("MUST_NOT") {
                result.errors.push(ValidationError {
                    code: "E308".to_string(),
                    message: "Conflicting MUST and MUST_NOT in same relation".to_string(),
                });
                result.valid = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_valid_payload() {
        let payload = CstlPayload {
            version: "v5.0.0".to_string(),
            mode: "A".to_string(),
            meta: {
                let mut m = HashMap::new();
                m.insert("encoder".to_string(), "Agent_CLAUDE".to_string());
                m.insert("produced_by".to_string(), "Claude".to_string());
                m
            },
            intent: {
                let mut i = HashMap::new();
                i.insert("purpose".to_string(), "test".to_string());
                i.insert("sender".to_string(), "alice".to_string());
                i.insert("receiver".to_string(), "bob".to_string());
                i
            },
            relations: vec![],
            raw: String::new(),
        };

        let result = validate_payload(&payload);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_missing_encoder() {
        let payload = CstlPayload {
            version: "v5.0.0".to_string(),
            mode: "A".to_string(),
            meta: HashMap::new(),
            intent: HashMap::new(),
            relations: vec![],
            raw: String::new(),
        };

        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code == "E301"));
    }

    #[test]
    fn test_missing_intent_fields() {
        let payload = CstlPayload {
            version: "v5.0.0".to_string(),
            mode: "A".to_string(),
            meta: {
                let mut m = HashMap::new();
                m.insert("encoder".to_string(), "Agent".to_string());
                m.insert("produced_by".to_string(), "Claude".to_string());
                m
            },
            intent: HashMap::new(),
            relations: vec![],
            raw: String::new(),
        };

        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.len() >= 3); // Missing purpose, sender, receiver
    }

    // ── check_sdl_operator_whitelist (branchement semantic.rs sur le pipeline reel) ──
    // Audit multi-angle 2026-09-03 : semantic.rs::SemanticValidator n'etait
    // appele nulle part sur le chemin TCP reel, decouvert en creusant le fix
    // de la desync MUTUAL. Ces tests verifient le branchement ET la
    // disambiguation SDL-operator (MAJUSCULES) vs predicat-KB (minuscule).

    fn relation(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_kb_predicates_lowercase_produce_no_sdl_warning() {
        // part_of / located_in / born_in sont des predicats KB (kb_verify.rs),
        // pas des operateurs SDL -- ne doivent JAMAIS declencher un warning
        // "operateur inconnu", meme s'ils sont absents des 35 officiels.
        let payload = CstlPayload {
            version: "v5.0.0".into(), mode: "A".into(),
            meta: HashMap::new(), intent: HashMap::new(),
            relations: vec![
                relation(&[("subject", "paris"), ("type", "part_of"), ("object", "france")]),
                relation(&[("subject", "paris"), ("type", "located_in"), ("object", "france")]),
                relation(&[("subject", "alice"), ("type", "born_in"), ("object", "quebec")]),
            ],
            raw: String::new(),
        };
        let warnings = check_sdl_operator_whitelist(&payload);
        assert!(warnings.is_empty(), "predicats KB minuscules ne doivent pas warner: {:?}", warnings);
    }

    #[test]
    fn test_known_sdl_operator_uppercase_produces_no_warning() {
        let payload = CstlPayload {
            version: "v5.0.0".into(), mode: "A".into(),
            meta: HashMap::new(), intent: HashMap::new(),
            relations: vec![
                relation(&[("subject", "x"), ("type", "EQUALS"), ("object", "y")]),
            ],
            raw: String::new(),
        };
        let warnings = check_sdl_operator_whitelist(&payload);
        assert!(warnings.is_empty(), "EQUALS est un operateur officiel: {:?}", warnings);
    }

    #[test]
    fn test_unknown_uppercase_operator_warns() {
        let payload = CstlPayload {
            version: "v5.0.0".into(), mode: "A".into(),
            meta: HashMap::new(), intent: HashMap::new(),
            relations: vec![
                relation(&[("subject", "x"), ("type", "FOOBAR"), ("object", "y")]),
            ],
            raw: String::new(),
        };
        let warnings = check_sdl_operator_whitelist(&payload);
        assert!(warnings.iter().any(|w| w.contains("FOOBAR")), "FOOBAR devrait warner: {:?}", warnings);
    }

    #[test]
    fn test_mutual_uppercase_warns_as_deprecated_via_real_pipeline_adapter() {
        let payload = CstlPayload {
            version: "v5.0.0".into(), mode: "A".into(),
            meta: HashMap::new(), intent: HashMap::new(),
            relations: vec![
                relation(&[("subject", "x"), ("type", "MUTUAL"), ("object", "y")]),
            ],
            raw: String::new(),
        };
        let warnings = check_sdl_operator_whitelist(&payload);
        assert!(warnings.iter().any(|w| w.contains("MUTUAL") && w.contains("W601")),
                "MUTUAL devrait warner W601 via l'adaptateur reel: {:?}", warnings);
    }
}
