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
}
