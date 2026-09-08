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

    // INTENT_PAYLOAD.priority (CSTL_SPEC_v5_0.md §6, ligne ~231) -- champ
    // optionnel, DECLARE dans la grammaire depuis la v5.0 mais jamais verifie
    // ni exploite nulle part dans ce depot avant ce fix (trouvaille du
    // 2026-09-06: `grep -rn '"priority"' src/` ne retournait rien). Validation
    // de FORMAT seulement ici (l'absence du champ n'est PAS une erreur --
    // valeur par defaut implicite "normal", cf. handler.rs STEP 3-priority) ;
    // le comportement reel (escalade Telegram pour `critical`) est cable dans
    // handler.rs, pas ici -- ce fichier ne fait que rejeter les valeurs hors
    // enum. E311 (pas E306/E307/E308, deja pris/retires plus haut dans ce
    // fichier -- E309/E310 pris pour public_key/signature juste au-dessus).
    if let Some(priority) = payload.intent.get("priority") {
        const VALID_PRIORITIES: [&str; 4] = ["critical", "high", "normal", "low"];
        if !VALID_PRIORITIES.contains(&priority.as_str()) {
            result.errors.push(ValidationError {
                code: "E311".to_string(),
                message: format!(
                    "INTENT_PAYLOAD.priority doit etre l'une de: critical|high|normal|low (recu: {})",
                    priority
                ),
            });
            result.valid = false;
        }
    }

    // Validate deontic constraints (MUST/MUST_NOT)
    validate_deontic_constraints(payload, &mut result);

    // Validation FORMAT des blocs GUARDRAIL_REPORT / SCOPE_LOCK (ajoutee
    // 2026-09-08 -- voir CSTL_SPEC_v5_0.md, nouvelle section). Codes E311-E314,
    // premiers libres apres E301-E310 (verifie contre §16.4 et la grille de
    // codes deja emis dans ce fichier au 2026-09-08, aucune collision).
    validate_guardrail_reports(payload, &mut result);
    validate_scope_lock(payload, &mut result);

    eprintln!("[Validator] Valid: {}, Errors: {}, Warnings: {}", 
        result.valid, result.errors.len(), result.warnings.len());

    result
}

/// Validation FORMAT du bloc `GUARDRAIL_REPORT [status=..., reason=..., ...]`
/// (§ nouvelle section, ajoutee 2026-09-08). Un GUARDRAIL_REPORT rend visible
/// pourquoi un LLM RECEVEUR (pas ce serveur) a bloque ou partiellement execute
/// une instruction -- valide EMPIRIQUEMENT en conversation sur ChatGPT V2 lors
/// de la session tripartite du 22 mai 2026 (Claude+Gemini+ChatGPT), jamais
/// porte en Rust avant ce fix.
///
/// Portee assumee, honnetement limitee : ce check verifie uniquement que le
/// bloc a la FORME attendue (`status=` present et dans l'enumeration connue,
/// `reason=` present quand le statut n'est pas ALLOWED). Il ne peut PAS
/// verifier que le contenu reflete un vrai refus d'un LLM tiers -- rien dans
/// ce pipeline ne reboucle vers un LLM receveur pour confirmer quoi que ce
/// soit (non resolvable par le format seul, meme limite que P2 documentee
/// pour SCOPE_LOCK ci-dessous et dans la session de mai). Un payload
/// malveillant peut donc affirmer un GUARDRAIL_REPORT bidon -- ce check
/// bloque seulement les blocs MAL FORMES, pas les blocs FAUX.
fn validate_guardrail_reports(payload: &CstlPayload, result: &mut ValidationResult) {
    const KNOWN_STATUSES: [&str; 3] = ["BLOCKED", "PARTIAL", "ALLOWED"];

    for (idx, report) in payload.guardrail_reports.iter().enumerate() {
        match report.get("status") {
            None => {
                result.errors.push(ValidationError {
                    code: format!("E311[{}]", idx),
                    message: format!("GUARDRAIL_REPORT[{}]: champ 'status' manquant", idx),
                });
                result.valid = false;
            }
            Some(status) if !KNOWN_STATUSES.contains(&status.as_str()) => {
                result.errors.push(ValidationError {
                    code: format!("E312[{}]", idx),
                    message: format!(
                        "GUARDRAIL_REPORT[{}]: status={:?} hors enumeration BLOCKED|PARTIAL|ALLOWED",
                        idx, status
                    ),
                });
                result.valid = false;
            }
            Some(status) if status != "ALLOWED" && !report.contains_key("reason") => {
                // Un blocage/partiel sans motif n'est pas exploitable par
                // l'expediteur -- avertissement seul (pas un rejet) : la
                // FONCTION du bloc (rendre le refus visible) reste remplie,
                // juste moins utilement.
                result.warnings.push(format!(
                    "W606: GUARDRAIL_REPORT[{}] status={} sans champ 'reason'", idx, status
                ));
            }
            _ => {}
        }
    }
}

/// Validation FORMAT du bloc `SCOPE_LOCK [mode=STRICT|OPEN, ...]` (ajoutee
/// 2026-09-08). SCOPE_LOCK: STRICT reduit la derive additive (le recepteur
/// ajoute des references externes non demandees) en forcant le recepteur a
/// rester dans le scope du payload recu -- valide EMPIRIQUEMENT en
/// conversation sur Gemini V2, session tripartite du 22 mai 2026.
///
/// Meme limite honnete que `validate_guardrail_reports` ci-dessus : ce check
/// (et `check_scope_lock_drift` plus bas) ne peuvent verifier que la FORME et
/// une coherence INTERNE au payload -- jamais que le LLM receveur a
/// REELLEMENT respecte le scope verrouille dans sa reponse en langage naturel.
/// Non resolvable par le format seul (comme P2, session du 22 mai) : seul un
/// vrai LLM tiers connecte en boucle (hors de ce depot) pourrait le confirmer.
fn validate_scope_lock(payload: &CstlPayload, result: &mut ValidationResult) {
    const KNOWN_MODES: [&str; 2] = ["STRICT", "OPEN"];

    let Some(lock) = &payload.scope_lock else { return; };
    match lock.get("mode") {
        None => {
            result.errors.push(ValidationError {
                code: "E313".to_string(),
                message: "SCOPE_LOCK: champ 'mode' manquant".to_string(),
            });
            result.valid = false;
        }
        Some(mode) if !KNOWN_MODES.contains(&mode.as_str()) => {
            result.errors.push(ValidationError {
                code: "E314".to_string(),
                message: format!("SCOPE_LOCK: mode={:?} hors enumeration STRICT|OPEN", mode),
            });
            result.valid = false;
        }
        _ => {}
    }
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

/// R8 -- reconstruit le 2026-09-05 sur le VRAI modele de donnees du chemin
/// serveur (`CstlPayload`: `relations`/`defines` en `HashMap<String,String>`
/// plats), PAS sur `ast::Block` (retire le 2026-09-04 -- voir semantic.rs et
/// CSTL_SPEC_v5_0.md §19 pour l'historique complet). L'ancienne
/// implementation (`defined_entities()`/`check_undefined_entity_reference()`)
/// dependait d'un arbre `Block` que ni le tokenizer ni le parser reel n'ont
/// jamais construit hors tests -- elle etait donc morte a vie, pas juste non
/// branchee. Cette reconstruction n'a ete possible qu'apres avoir d'abord
/// appris a `server::parser::parse_payload` a reconnaitre les blocs DEFINE
/// (spec §9, `DEFINE <identifier> AS <entity_type> [attrs]`) : avant ca,
/// AUCUNE entite DEFINE n'existait meme cote serveur -- `payload.defines`
/// aurait ete vide en permanence, rendant R8 structurellement invalide dans
/// N'IMPORTE QUELLE implementation.
///
/// Portee assumee : verifie uniquement les DEFINE du MEME payload (comme le
/// dit la spec §13, "un DEFINE anterieur" -- pas l'historique cross-payload
/// de `execution_lab::check_deontic_consistency_with_history`, qui repond a
/// une question differente -- coherence deontique dans le temps, pas
/// resolution de coreference locale). Avertissement seul, meme politique que
/// `check_sdl_operator_whitelist`/`check_extended_semantic_diagnostics`: un
/// coref orphelin est un signal de qualite (faute de frappe probable sur un
/// id, ou DEFINE oublie), pas une contradiction structurelle qui justifierait
/// un rejet.
///
/// Code choisi : `R8` (chaine litterale, meme convention que R9/R10 dans
/// semantic.rs -- pas de collision, verifie contre §16.4 et les codes E/W
/// deja emis par ce fichier et par semantic.rs).
pub fn check_coref_with_references(payload: &CstlPayload) -> Vec<String> {
    let defined_ids: std::collections::HashSet<&str> = payload.defines.iter()
        .filter_map(|d| d.get("id").map(String::as_str))
        .collect();

    payload.relations.iter()
        .filter_map(|r| r.get("coref_with").map(|id| (r, id.as_str())))
        .filter(|(_, id)| !defined_ids.contains(id))
        .map(|(r, id)| format!(
            "R8: coref_with={} ne correspond a aucun DEFINE de ce payload (relation subject={:?}, object={:?})",
            id,
            r.get("subject").map(String::as_str).unwrap_or(""),
            r.get("object").map(String::as_str).unwrap_or(""),
        ))
        .collect()
}

/// W607 -- derive de scope (ajoutee 2026-09-08, aux cotes de R8 dont elle
/// reprend la structure : meme fichier de donnees (`relations`/`defines`
/// intra-payload), meme politique (avertissement seul, jamais un rejet).
///
/// Quand `SCOPE_LOCK [mode=STRICT, allowed_ids="e001;e002"]` est actif ET
/// que la liste `allowed_ids` est fournie, chaque RELATION dont le subject
/// OU l'object n'apparait ni dans `allowed_ids` ni dans les `id=` des
/// DEFINE de ce meme payload declenche un avertissement -- signal qu'une
/// entite hors du scope verrouille a ete introduite.
///
/// LIMITE HONNETE (voir aussi `validate_scope_lock`) : ceci detecte une
/// derive DEJA visible dans les RELATION structurees de CE payload -- pas
/// une derive dans le texte libre d'une reponse en langage naturel produite
/// par un LLM tiers, que ce format ne peut structurellement pas observer.
/// C'est le seul "effet observable" que le format seul permet de verifier :
/// le respect REEL du scope par le LLM receveur reste non verifiable sans un
/// vrai LLM connecte en boucle.
///
/// Quand `allowed_ids` est absent, aucun avertissement n'est possible faute
/// de reference contre laquelle comparer -- `mode=STRICT` seul confirme
/// simplement le scope actif au client (voir handler.rs, ligne SCOPE_LOCK_ACK).
pub fn check_scope_lock_drift(payload: &CstlPayload) -> Vec<String> {
    let Some(lock) = &payload.scope_lock else { return Vec::new(); };
    if lock.get("mode").map(String::as_str) != Some("STRICT") {
        return Vec::new();
    }
    let Some(allowed_raw) = lock.get("allowed_ids") else { return Vec::new(); };

    let mut allowed: std::collections::HashSet<&str> = allowed_raw
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    for d in &payload.defines {
        if let Some(id) = d.get("id") {
            allowed.insert(id.as_str());
        }
    }

    let mut warnings = Vec::new();
    for r in &payload.relations {
        for field in ["subject", "object"] {
            if let Some(val) = r.get(field) {
                if !val.is_empty() && !allowed.contains(val.as_str()) {
                    warnings.push(format!(
                        "W607: SCOPE_LOCK STRICT actif -- RELATION.{}={} hors de allowed_ids/DEFINE de ce payload",
                        field, val
                    ));
                }
            }
        }
    }
    warnings
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

    let validator = SemanticValidator::new(&relations);
    for err in validator.check_axiom_d().into_iter().chain(validator.check_axiom_k_entailment()) {
        result.errors.push(ValidationError {
            code: err.code, // "E107" ou "E110"
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
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
    fn test_real_deontic_distributed_contradiction_detected_e110() {
        // Contradiction distribuee via une chaine ENTAILS EXPLICITE dans le
        // payload -- E107 seul ne la voit pas (objets differents: drug_A vs
        // monitoring_required), E110 doit la detecter sur le vrai pipeline.
        let payload = payload_with(vec![
            relation(&[("subject", "physician"), ("type", "PRESCRIBE"), ("object", "drug_A"), ("modality", "MUST")]),
            relation(&[("subject", "drug_A"), ("type", "ENTAILS"), ("object", "monitoring_required")]),
            relation(&[("subject", "physician"), ("type", "PERFORM"), ("object", "monitoring_required"), ("modality", "MUST_NOT")]),
        ]);
        let result = validate_payload(&payload);
        assert!(!result.valid, "une contradiction distribuee via ENTAILS doit etre rejetee");
        assert!(result.errors.iter().any(|e| e.code == "E110"), "attendu E110 (Axiome K): {:?}", result.errors);
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

    // ── check_coref_with_references (R8, reconstruit le 2026-09-05 sur le
    // vrai CstlPayload -- PAS ast::Block, voir le commentaire de tete de la
    // fonction pour l'historique complet de la trouvaille du 2026-09-04) ──

    fn define(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_r8_coref_with_valid_reference_no_warning() {
        let mut payload = payload_with(vec![
            relation(&[("subject", "e002"), ("type", "EQUALS"), ("object", "e001"), ("coref_with", "e001")]),
        ]);
        payload.defines = vec![define(&[("name", "patient"), ("entity_type", "human"), ("id", "e001")])];

        let warnings = check_coref_with_references(&payload);
        assert!(warnings.is_empty(), "coref_with vers un DEFINE existant ne doit pas warner: {:?}", warnings);
    }

    #[test]
    fn test_r8_coref_with_undefined_reference_warns() {
        let mut payload = payload_with(vec![
            relation(&[("subject", "e003"), ("type", "EQUALS"), ("object", "e999"), ("coref_with", "e999")]),
        ]);
        payload.defines = vec![define(&[("name", "patient"), ("entity_type", "human"), ("id", "e001")])];

        let warnings = check_coref_with_references(&payload);
        assert!(warnings.iter().any(|w| w.starts_with("R8:") && w.contains("e999")),
                "coref_with orphelin (e999, jamais DEFINE) doit warner R8: {:?}", warnings);
    }

    #[test]
    fn test_r8_no_coref_with_attribute_no_warning() {
        // Chemin rapide implicite : aucune relation ne porte coref_with ->
        // aucun cout, aucune interference avec le trafic existant (qui n'a
        // jamais porte cet attribut jusqu'ici).
        let payload = payload_with(vec![
            relation(&[("subject", "alice"), ("type", "born_in"), ("object", "quebec")]),
        ]);
        let warnings = check_coref_with_references(&payload);
        assert!(warnings.is_empty());
    }

    // ── INTENT_PAYLOAD.priority (E311, CSTL_SPEC_v5_0.md §6) ──

    #[test]
    fn test_priority_absent_valid_default_behavior_unchanged() {
        // Regression: aucun champ priority -> aucune erreur, comportement
        // identique a avant ce fix pour tout le trafic existant.
        let payload = payload_with(vec![]);
        assert!(!payload.intent.contains_key("priority"));
        let result = validate_payload(&payload);
        assert!(result.valid);
        assert!(!result.errors.iter().any(|e| e.code == "E311"));
    }

    #[test]
    fn test_priority_each_valid_enum_value_accepted() {
        for v in ["critical", "high", "normal", "low"] {
            let mut payload = payload_with(vec![]);
            payload.intent.insert("priority".to_string(), v.to_string());
            let result = validate_payload(&payload);
            assert!(result.valid, "priority={} devrait etre valide: {:?}", v, result.errors);
        }
    }

    #[test]
    fn test_priority_invalid_value_rejected_e311() {
        let mut payload = payload_with(vec![]);
        payload.intent.insert("priority".to_string(), "urgent".to_string());
        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code == "E311"), "attendu E311: {:?}", result.errors);
    }

    #[test]
    fn test_priority_case_sensitive_uppercase_rejected() {
        // La grammaire (§6) liste des litteraux minuscules -- CRITICAL en
        // majuscules n'est pas une valeur valide de l'enum.
        let mut payload = payload_with(vec![]);
        payload.intent.insert("priority".to_string(), "CRITICAL".to_string());
        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code == "E311"));
    }

    #[test]
    fn test_r8_no_defines_at_all_still_warns_on_coref() {
        // Payload qui utilise coref_with sans avoir JAMAIS defini l'entite
        // (aucun DEFINE du tout, pas seulement le mauvais id) -- doit quand
        // meme warner, pas planter ni faire un faux-negatif silencieux.
        let payload = payload_with(vec![
            relation(&[("subject", "e002"), ("type", "EQUALS"), ("object", "e001"), ("coref_with", "e001")]),
        ]);
        assert!(payload.defines.is_empty());

        let warnings = check_coref_with_references(&payload);
        assert!(warnings.iter().any(|w| w.starts_with("R8:")));
    }

    // ── GUARDRAIL_REPORT / SCOPE_LOCK (ajoutes 2026-09-08, session
    // tripartite du 22 mai 2026 -- voir CSTL_SPEC_v5_0.md) ──

    #[test]
    fn test_guardrail_report_valid_status_and_reason_no_error() {
        let mut payload = payload_with(vec![]);
        payload.guardrail_reports = vec![define(&[("status", "BLOCKED"), ("reason", "policy_violation")])];
        let result = validate_payload(&payload);
        assert!(result.valid, "{:?}", result.errors);
    }

    #[test]
    fn test_guardrail_report_missing_status_is_e311() {
        let mut payload = payload_with(vec![]);
        payload.guardrail_reports = vec![define(&[("reason", "x")])];
        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code.starts_with("E311")), "{:?}", result.errors);
    }

    #[test]
    fn test_guardrail_report_unknown_status_is_e312() {
        let mut payload = payload_with(vec![]);
        payload.guardrail_reports = vec![define(&[("status", "MAYBE")])];
        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code.starts_with("E312")), "{:?}", result.errors);
    }

    #[test]
    fn test_guardrail_report_blocked_without_reason_warns_w606_not_error() {
        let mut payload = payload_with(vec![]);
        payload.guardrail_reports = vec![define(&[("status", "BLOCKED")])];
        let result = validate_payload(&payload);
        assert!(result.valid, "l'absence de reason est un avertissement, pas une erreur: {:?}", result.errors);
        assert!(result.warnings.iter().any(|w| w.starts_with("W606")), "{:?}", result.warnings);
    }

    #[test]
    fn test_guardrail_report_allowed_without_reason_no_warning() {
        let mut payload = payload_with(vec![]);
        payload.guardrail_reports = vec![define(&[("status", "ALLOWED")])];
        let result = validate_payload(&payload);
        assert!(result.valid);
        assert!(!result.warnings.iter().any(|w| w.starts_with("W606")));
    }

    #[test]
    fn test_scope_lock_strict_valid_no_error() {
        let mut payload = payload_with(vec![]);
        payload.scope_lock = Some(define(&[("mode", "STRICT"), ("allowed_ids", "e001;e002")]));
        let result = validate_payload(&payload);
        assert!(result.valid, "{:?}", result.errors);
    }

    #[test]
    fn test_scope_lock_missing_mode_is_e313() {
        let mut payload = payload_with(vec![]);
        payload.scope_lock = Some(define(&[("allowed_ids", "e001")]));
        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code == "E313"), "{:?}", result.errors);
    }

    #[test]
    fn test_scope_lock_unknown_mode_is_e314() {
        let mut payload = payload_with(vec![]);
        payload.scope_lock = Some(define(&[("mode", "LOOSE")]));
        let result = validate_payload(&payload);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.code == "E314"), "{:?}", result.errors);
    }

    #[test]
    fn test_no_scope_lock_no_error_no_warning() {
        // Chemin rapide implicite : aucun SCOPE_LOCK -> aucun cout, aucune
        // interference avec le trafic existant (qui n'a jamais porte ce bloc).
        let payload = payload_with(vec![
            relation(&[("subject", "alice"), ("type", "born_in"), ("object", "quebec")]),
        ]);
        let result = validate_payload(&payload);
        assert!(result.valid);
        assert!(check_scope_lock_drift(&payload).is_empty());
    }

    // ── check_scope_lock_drift (W607) ──

    #[test]
    fn test_scope_lock_drift_detects_relation_outside_allowed_ids() {
        let mut payload = payload_with(vec![
            relation(&[("subject", "e001"), ("type", "EQUALS"), ("object", "e999")]),
        ]);
        payload.scope_lock = Some(define(&[("mode", "STRICT"), ("allowed_ids", "e001")]));
        let warnings = check_scope_lock_drift(&payload);
        assert!(warnings.iter().any(|w| w.starts_with("W607:") && w.contains("e999")), "{:?}", warnings);
    }

    #[test]
    fn test_scope_lock_drift_allows_ids_from_defines_too() {
        // Un DEFINE de ce meme payload elargit implicitement le scope, sans
        // avoir besoin d'etre repete dans allowed_ids.
        let mut payload = payload_with(vec![
            relation(&[("subject", "e001"), ("type", "EQUALS"), ("object", "e002")]),
        ]);
        payload.defines = vec![define(&[("name", "physician"), ("entity_type", "agent"), ("id", "e002")])];
        payload.scope_lock = Some(define(&[("mode", "STRICT"), ("allowed_ids", "e001")]));
        let warnings = check_scope_lock_drift(&payload);
        assert!(warnings.is_empty(), "e002 est DEFINE dans ce payload, pas de derive: {:?}", warnings);
    }

    #[test]
    fn test_scope_lock_open_mode_never_warns_on_drift() {
        let mut payload = payload_with(vec![
            relation(&[("subject", "e001"), ("type", "EQUALS"), ("object", "e999")]),
        ]);
        payload.scope_lock = Some(define(&[("mode", "OPEN"), ("allowed_ids", "e001")]));
        assert!(check_scope_lock_drift(&payload).is_empty());
    }

    #[test]
    fn test_scope_lock_strict_without_allowed_ids_never_warns() {
        // Sans allowed_ids, aucune reference contre laquelle comparer --
        // mode=STRICT seul ne peut pas produire de faux-positif par defaut.
        let mut payload = payload_with(vec![
            relation(&[("subject", "e001"), ("type", "EQUALS"), ("object", "e999")]),
        ]);
        payload.scope_lock = Some(define(&[("mode", "STRICT")]));
        assert!(check_scope_lock_drift(&payload).is_empty());
    }
}
