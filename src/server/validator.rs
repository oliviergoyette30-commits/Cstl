//! CSTL Semantic Validator
//! Validates CSTL payloads against deontic constraints
//!
//! Rules:
//! - META must have encoder + produced_by
//! - INTENT_PAYLOAD must have purpose + sender + receiver
//! - RELATIONS must have type + subject + object
//! - MUST constraints cannot be violated
//! - No circular dependencies

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

    // Signature Ed25519 (Couche 2/securite, src/signing.rs) -- ici, verification
    // de FORMAT seulement (longueur hex attendue), jamais d'erreur sur absence
    // (l'optionnalite globale/obligation-si-deja-enregistre vit dans handler.rs,
    // STEP 2a, pas ici). E309/E310 plutot que E306/E307 deja pris par les
    // RELATION per-index ci-dessus -- collision qui aurait ete introduite si le
    // plan initial (E306/E307) avait ete suivi tel quel.
    if let Some(pk) = payload.meta.get("public_key") {
        if pk.len() != 64 || !pk.chars().all(|c| c.is_ascii_hexdigit()) {
            result.errors.push(ValidationError {
                code: "E309".to_string(),
                message: "META.public_key doit etre 64 caracteres hexadecimaux (32 octets)".to_string(),
            });
            result.valid = false;
        }
    }
    if let Some(sig) = payload.intent.get("signature") {
        if sig.len() != 128 || !sig.chars().all(|c| c.is_ascii_hexdigit()) {
            result.errors.push(ValidationError {
                code: "E310".to_string(),
                message: "INTENT_PAYLOAD.signature doit etre 128 caracteres hexadecimaux (64 octets)".to_string(),
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

    SemanticValidator::new(&relations)
        .check_operator_whitelist()
        .into_iter()
        .map(|e| format!("{}: {}", e.code, e.message))
        .collect()
}

/// Convertit TOUTES les RELATION d'un payload (pas seulement celles qui
/// RESSEMBLENT a un operateur SDL, contrairement a `check_sdl_operator_whitelist`
/// ci-dessus) en `AstRelation`, `attrs` inclus: chaque champ de la RELATION
/// autre que subject/type/object/modality (ex. `sigma=`, `tau=`, `polarity=`)
/// devient un `ast::Field`, sans quoi `check_attribute_ontology`/
/// `check_attribute_bombing` (ci-dessous) n'auraient jamais rien a examiner
/// (attrs vide en permanence). Pas de filtre "ressemble a un operateur SDL"
/// ici: les checks de `check_additional_diagnostics` comparent `operator` a
/// des chaines SDL officielles precises (MAINTAIN, AMP, INH, KNOWS,
/// DOUBTS...), donc un predicat KB minuscule (born_in, part_of...) ne peut
/// jamais matcher par accident -- pas besoin de l'heuristique majuscules
/// utilisee pour la whitelist generique.
fn relations_to_ast_full(payload: &CstlPayload) -> Vec<AstRelation> {
    payload.relations.iter()
        .map(|r| {
            let attrs: Vec<crate::ast::Field> = r.iter()
                .filter(|(k, _)| !matches!(k.as_str(), "subject" | "type" | "object" | "modality"))
                .map(|(k, v)| crate::ast::Field {
                    name: k.clone(),
                    type_hint: None,
                    value: v.clone(),
                    line: 0,
                })
                .collect();
            AstRelation {
                subject: r.get("subject").cloned().unwrap_or_default(),
                operator: r.get("type").cloned().unwrap_or_default(),
                object: r.get("object").cloned().unwrap_or_default(),
                attrs,
                modality: r.get("modality").cloned(),
                line: 0,
            }
        })
        .collect()
}

/// Branche les 11 checks de `semantic.rs::SemanticValidator::
/// check_additional_diagnostics` (E108/E109/E701/W502/W503/R9/R10/W602/
/// W603/W604/W605) sur un payload reel -- item #2 de la liste des choses a
/// faire (2026-09-04), trouvaille annexe en supprimant le systeme Block/AST
/// mort (voir ast.rs): ces checks operent tous sur `Relation` (donc
/// branchables sans dependre d'un parser Block qui n'a jamais existe),
/// mais etaient testes depuis des mois SANS JAMAIS etre appeles par le
/// serveur reel -- seuls `check_operator_whitelist` et `check_axiom_d`
/// l'etaient.
///
/// Meme politique que `check_sdl_operator_whitelist`: AVERTISSEMENTS
/// UNIQUEMENT, jamais un rejet. Ces 11 checks n'ont jamais ete concus ni
/// testes comme des motifs de rejet d'un payload en production -- les
/// promouvoir directement en erreurs bloquantes serait un changement de
/// comportement non demande (et non verifie) sur des payloads qui passaient
/// jusqu'ici, pas juste "reveiller du code mort".
pub fn check_extended_semantic_diagnostics(payload: &CstlPayload) -> Vec<String> {
    let relations = relations_to_ast_full(payload);
    if relations.is_empty() {
        return Vec::new();
    }
    SemanticValidator::new(&relations)
        .check_additional_diagnostics()
        .into_iter()
        .map(|e| format!("{}: {}", e.code, e.message))
        .collect()
}

/// Trouvaille du 2026-09-04 (creusee en cherchant "Deontic Modality Audit",
/// intitule sans code correspondant dans docs/ARCHITECTURE.md Couche 8):
/// cette fonction, AVANT ce fix, verifiait si le champ `type` d'UNE SEULE
/// RELATION contenait a la fois les sous-chaines "MUST" et "MUST_NOT" -- un
/// double bug, pas juste une lacune:
///   1. Le format wire reel n'encode jamais MUST/MUST_NOT dans `type` --
///      `type` porte soit un predicat KB (born_in, part_of...) soit un
///      operateur SDL (EQUALS, CONTRADICTS...). Le VRAI moteur de
///      contradiction deontique (SDL Axiome D, `semantic.rs::SemanticValidator
///      ::check_axiom_d`, E107) existait deja, teste, mais n'etait JAMAIS
///      appele sur le chemin TCP reel -- seul `check_operator_whitelist()`
///      l'etait (audit multi-angle du 2026-09-03, decouvert a l'epoque pour
///      le desync MUTUAL, jamais etendu a Axiome D depuis).
///   2. Faux positif systematique: `"MUST_NOT".contains("MUST")` est vrai en
///      Rust (MUST est une sous-chaine de MUST_NOT) -- N'IMPORTE QUELLE
///      RELATION[type=MUST_NOT, ...] isolee, sans aucun MUST ailleurs,
///      declenchait ce rejet a tort.
///
/// Corrige: la modalite deontique se declare desormais via un champ
/// OPTIONNEL `modality=MUST|MUST_NOT|REQUIRE|FORBID` sur une RELATION
/// (`RELATION [type=<operateur>, subject=..., object=..., modality=MUST]`)
/// -- le format `RELATION[key=value,...]` etant deja generique (HashMap),
/// aucun changement de parseur necessaire. Cette fonction construit de
/// vraies `AstRelation` (avec `.modality` peuple) et appelle le vrai moteur
/// SDL Axiome D deja ecrit et teste (`semantic.rs`) au lieu de reinventer
/// une verification de substring.
fn validate_deontic_constraints(payload: &CstlPayload, result: &mut ValidationResult) {
    let relations: Vec<AstRelation> = payload.relations.iter()
        .map(|r| AstRelation {
            subject: r.get("subject").cloned().unwrap_or_default(),
            operator: r.get("type").cloned().unwrap_or_default(),
            object: r.get("object").cloned().unwrap_or_default(),
            attrs: Vec::new(),
            modality: r.get("modality").cloned(),
            line: 0,
        })
        .collect();

    if relations.iter().all(|r| r.modality.is_none()) {
        // Chemin rapide: aucune RELATION de ce payload ne porte de modalite
        // -- pas la peine de construire le SemanticValidator pour rien
        // (evite aussi de fabriquer des AstRelation avec operator="" pour
        // les payloads sans aucune RELATION, comme les council_decision).
        return;
    }

    for err in SemanticValidator::new(&relations).check_axiom_d() {
        result.errors.push(ValidationError {
            code: err.code, // "E107"
            message: err.message,
        });
        result.valid = false;
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

    // ── validate_deontic_constraints (branchement Axiome D sur le pipeline
    // reel, 2026-09-04) -- remplace l'ancien check casse (substring sur un
    // seul champ RELATION.type, jamais capable de detecter une vraie
    // contradiction ET generant un faux positif sur MUST_NOT isole).

    fn payload_with(relations: Vec<HashMap<String, String>>) -> CstlPayload {
        CstlPayload {
            version: "v5.0.0".into(), mode: "A".into(),
            meta: {
                let mut m = HashMap::new();
                m.insert("encoder".to_string(), "Agent".to_string());
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
            relations,
            raw: String::new(),
        }
    }

    #[test]
    fn test_lone_must_not_no_longer_false_positive() {
        // Regression directe du bug corrige: "MUST_NOT".contains("MUST") est
        // vrai en Rust -- avant ce fix, cette SEULE relation (aucun MUST
        // ailleurs) declenchait a tort E308. Doit desormais passer.
        let payload = payload_with(vec![
            relation(&[("subject", "agent_x"), ("type", "PERFORM"), ("object", "delete_prod_db"), ("modality", "MUST_NOT")]),
        ]);
        let result = validate_payload(&payload);
        assert!(result.valid, "un MUST_NOT isole ne doit plus etre un faux positif: {:?}", result.errors);
        assert!(!result.errors.iter().any(|e| e.code == "E308"), "E308 (ancien check casse) ne doit plus jamais apparaitre");
    }

    #[test]
    fn test_real_deontic_contradiction_detected_e107() {
        // Vraie contradiction: meme (subject, object) declare a la fois
        // obligatoire (MUST) et interdit (MUST_NOT) -- doit etre rejete
        // via le vrai moteur SDL Axiome D (E107), pas l'ancien substring check.
        let payload = payload_with(vec![
            relation(&[("subject", "agent_x"), ("type", "PERFORM"), ("object", "delete_prod_db"), ("modality", "MUST")]),
            relation(&[("subject", "agent_x"), ("type", "PERFORM"), ("object", "delete_prod_db"), ("modality", "MUST_NOT")]),
        ]);
        let result = validate_payload(&payload);
        assert!(!result.valid, "une vraie contradiction MUST/MUST_NOT doit etre rejetee");
        assert!(result.errors.iter().any(|e| e.code == "E107"), "attendu E107 (Axiome D): {:?}", result.errors);
    }

    #[test]
    fn test_relations_without_modality_unaffected() {
        // Chemin rapide: aucune modalite -> aucun cout, aucune interference
        // avec la validation factuelle normale (regression sur tout le
        // trafic existant, qui ne porte jamais de champ modality).
        let payload = payload_with(vec![
            relation(&[("subject", "alice"), ("type", "born_in"), ("object", "quebec")]),
        ]);
        let result = validate_payload(&payload);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_must_and_must_not_different_objects_no_contradiction() {
        // MUST sur UN objet et MUST_NOT sur un objet DIFFERENT pour le meme
        // sujet: pas une contradiction (deux obligations distinctes).
        let payload = payload_with(vec![
            relation(&[("subject", "agent_x"), ("type", "PERFORM"), ("object", "backup_db"), ("modality", "MUST")]),
            relation(&[("subject", "agent_x"), ("type", "PERFORM"), ("object", "delete_prod_db"), ("modality", "MUST_NOT")]),
        ]);
        let result = validate_payload(&payload);
        assert!(result.valid, "objets differents ne doivent pas etre traites comme contradictoires: {:?}", result.errors);
    }
}
