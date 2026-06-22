//! CSTL v5.0.0 — Semantic Validator
//!
//! S'exécute APRÈS le parser structurel (parser.rs). Le parser garantit que le
//! document est bien formé (structure, sécurité, hash). Ce module valide la
//! SÉMANTIQUE de la spec : les 36 opérateurs officiels, les 10 types d'entités
//! DEFINE, les règles R1–R7, et les opérateurs des blocs modaux déontiques.
//!
//! Il ne modifie pas le parser : il prend des blocs déjà parsés et retourne
//! des diagnostics supplémentaires (errors + warnings).
//!
//! Usage :
//!   let doc = parser.parse(token_count);
//!   let report = validate_semantics(&doc.blocks);
//!   // report.errors, report.warnings

use std::collections::HashSet;

// ── Constantes de la spec ───────────────────────────────────────────────────

/// Les 36 opérateurs FIXED officiels (spec section 8).
pub const OFFICIAL_OPERATORS: &[&str] = &[
    // Causalité
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    // Intentionnalité
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST",
    // Dynamiques
    "AMP", "INH", "PRESSURE", "CATALYZE",
    // Relationnel
    "MUTUAL", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    // v5.0 opérateurs
    "ENTAILS", "CONTRADICTS", "BELIEVES", "KNOWS", "ASSUMES", "DOUBTS", "BEFORE", "AFTER", "DURING", "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES", "COMPARES",
    // Actes de langage
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
];

/// Les 10 types d'entités DEFINE officiels (spec section 6).
pub const OFFICIAL_ENTITY_TYPES: &[&str] = &[
    "human", "agent", "document", "system", "concept",
    "place", "event", "infrastructure", "threat", "deliverable",
];

/// Les modalités déontiques reconnues.
pub const MODALITIES: &[&str] = &["MUST", "NOT", "MUST_NOT", "MAY", "IF", "REQUIRE", "FORBID"];

/// Les symboles compacts FIXED, non redéfinissables (R6).
pub const FIXED_SYMBOLS: &[&str] = &["σ", "δ", "τ", "ω", "ι"];

// ── Structures partagées avec le parser ─────────────────────────────────────
// PHASE 2 : on utilise directement les structs du parser (crate::ast) pour que
// validate_semantics puisse être appelé sur doc.blocks sans conflit de type.
use crate::ast::Block;

// ── Rapport de validation ────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SemReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub infos: Vec<String>,
}

impl SemReport {
    fn err(&mut self, rule: &str, line: usize, msg: &str) {
        self.errors.push(format!("[{}] L{}: {}", rule, line, msg));
    }
    fn warn(&mut self, rule: &str, line: usize, msg: &str) {
        self.warnings.push(format!("[{}] L{}: {}", rule, line, msg));
    }
    fn info(&mut self, rule: &str, msg: &str) {
        self.infos.push(format!("[{}] {}", rule, msg));
    }
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
    pub fn summary(&self) -> String {
        format!(
            "semantic validation: {} (errors={}, warnings={}, infos={})",
            if self.is_valid() { "VALID" } else { "INVALID" },
            self.errors.len(), self.warnings.len(), self.infos.len()
        )
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn parse_sigma(value: &str) -> Option<f64> {
    // value peut être "0.72", "0.9", etc. (string en CSTL)
    value.trim().parse::<f64>().ok()
}

/// Repère un opérateur dans une relation. La spec écrit les relations sous forme
/// `source OP target [attrs]`. Dans le parser actuel, ces lignes deviennent des
/// blocs ou des champs ; on scanne donc les noms de blocs et les valeurs.
fn token_is_operator_candidate(tok: &str) -> bool {
    // Un opérateur est en MAJUSCULES, possiblement avec un point (ARR.PRODUCE).
    !tok.is_empty()
        && tok.chars().all(|c| c.is_ascii_uppercase() || c == '.' || c == '_')
        && tok.len() >= 2
}

/// Scanne une valeur (`sujet OP cible ...`) et signale tout token qui ressemble
/// à un opérateur mais n'appartient pas aux 21 officiels (et n'est pas une
/// modalité déontique). Mutualisé entre les blocs RELATIONS et les blocs modaux.
fn check_operators_in_value(
    value: &str,
    line: usize,
    op_set: &HashSet<&str>,
    r: &mut SemReport,
) {
    for tok in value.split_whitespace() {
        if token_is_operator_candidate(tok)
            && !MODALITIES.contains(&tok)
            && !op_set.contains(tok)
        {
            r.warn("R5_UNKNOWN_OPERATOR", line, &format!("opérateur '{}' hors des 21 officiels", tok));
        }
    }
}

/// Normalise une proposition pour comparaison : espaces réduits, minuscules.
fn normalize_proposition(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Polarités déontiques reconnues pour la détection de contradiction (R8).
const DEONTIC_POSITIVE: &[&str] = &["MUST", "REQUIRE", "SHALL"];
const DEONTIC_NEGATIVE: &[&str] = &["MUST_NOT", "FORBID", "NOT"];

/// Some(true) = obligation, Some(false) = interdiction, None = non déontique.
fn modal_polarity(token: &str) -> Option<bool> {
    if DEONTIC_POSITIVE.contains(&token) { Some(true) }
    else if DEONTIC_NEGATIVE.contains(&token) { Some(false) }
    else { None }
}

/// Extrait (obligation?, proposition normalisée, ligne) d'un bloc déontique.
///
/// - `(MUST)`/`(MUST_NOT)`/`(REQUIRE)`/`(FORBID)`/`(NOT)` : modalité = nom du bloc;
///   proposition = énoncé `_stmt` entier.
/// - `(RULE)` : modalité = 1er token de `_stmt` (ex. `"MUST x"`), proposition = reste.
/// - `(IF)` et blocs non déontiques : renvoie `None` (conditionnels non couverts en v1).
fn deontic_claim(block: &Block) -> Option<(bool, String, usize)> {
    if !(block.name.starts_with('(') && block.name.ends_with(')')) { return None; }
    let inner = &block.name[1..block.name.len() - 1];
    let stmt_field = block.fields.iter().find(|f| f.name == "_stmt")?;
    let stmt = stmt_field.value.as_str();
    let line = stmt_field.line;

    // Cas 1 : le nom du bloc EST la modalité.
    if let Some(is_pos) = modal_polarity(inner) {
        let prop = normalize_proposition(stmt);
        if prop.is_empty() { return None; }
        return Some((is_pos, prop, line));
    }

    // Cas 2 : bloc (RULE) avec modalité interne au début de l'énoncé.
    if inner == "RULE" {
        let mut it = stmt.split_whitespace();
        if let Some(first) = it.next() {
            if let Some(is_pos) = modal_polarity(first) {
                let prop = normalize_proposition(&it.collect::<Vec<_>>().join(" "));
                if prop.is_empty() { return None; }
                return Some((is_pos, prop, line));
            }
        }
    }

    None
}

/// Pour un bloc (IF), extrait (condition, obligation?, action, ligne).
/// Forme attendue : "CONDITION MODALITÉ ACTION" (ex. "sigma_low MUST flag_x").
/// La condition = tout ce qui précède la 1re modalité ; l'action = tout ce qui suit.
fn conditional_claim(block: &Block) -> Option<(String, bool, String, usize)> {
    if block.name != "(IF)" { return None; }
    let stmt_field = block.fields.iter().find(|f| f.name == "_stmt")?;
    let tokens: Vec<&str> = stmt_field.value.split_whitespace().collect();
    let mi = tokens.iter().position(|t| modal_polarity(t).is_some())?;
    let is_pos = modal_polarity(tokens[mi]).unwrap();
    let cond = normalize_proposition(&tokens[..mi].join(" "));
    let action = normalize_proposition(&tokens[mi + 1..].join(" "));
    if cond.is_empty() || action.is_empty() { return None; }
    Some((cond, is_pos, action, stmt_field.line))
}

/// R11 — Cohérence déontique.
/// Détecte une contradiction : une même proposition déclarée à la fois
/// obligatoire (MUST/REQUIRE) et interdite (MUST_NOT/FORBID/NOT) dans le même
/// payload. ERREUR : un jeu de directives contradictoire ne peut être valide.
/// Couvre les formes nommées (MUST)/(MUST_NOT)/... ET la forme interne (RULE).
/// Ne couvre pas (IF) (conditionnels — v2).
fn check_deontic_contradictions(blocks: &[Block], r: &mut SemReport) {
    use std::collections::{HashMap, HashSet};

    // proposition -> (vu_obligation, vu_interdiction, ligne_1re_occurrence)
    let mut seen: HashMap<String, (bool, bool, usize)> = HashMap::new();
    let mut reported: HashSet<String> = HashSet::new();

    for b in blocks {
        if let Some((is_pos, prop, line)) = deontic_claim(b) {
            let e = seen.entry(prop.clone()).or_insert((false, false, line));
            if is_pos { e.0 = true; } else { e.1 = true; }

            if e.0 && e.1 && !reported.contains(&prop) {
                reported.insert(prop.clone());
                r.err("R11_DEONTIC_CONTRADICTION", e.2, &format!(
                    "contradiction déontique: '{}' déclaré à la fois obligatoire (MUST) et interdit (MUST_NOT/FORBID)",
                    prop
                ));
            }
        }
    }

    // Conditionnels (IF) : clé = (condition, action). Une contradiction n'existe
    // que SOUS LA MÊME CONDITION. Conditions différentes => pas de conflit.
    let mut seen_cond: HashMap<(String, String), (bool, bool, usize)> = HashMap::new();
    let mut reported_cond: HashSet<(String, String)> = HashSet::new();
    let mut reported_cross: HashSet<(String, String)> = HashSet::new();

    for b in blocks {
        if let Some((cond, is_pos, action, line)) = conditional_claim(b) {
            // (a) Contradiction conditionnelle pure : même condition, action opposée.
            let key = (cond.clone(), action.clone());
            let e = seen_cond.entry(key.clone()).or_insert((false, false, line));
            if is_pos { e.0 = true; } else { e.1 = true; }

            if e.0 && e.1 && !reported_cond.contains(&key) {
                reported_cond.insert(key.clone());
                r.err("R11_DEONTIC_CONTRADICTION", e.2, &format!(
                    "contradiction déontique conditionnelle: sous la condition '{}', '{}' est à la fois obligatoire et interdit",
                    cond, action
                ));
            }

            // (b) Croisement conditionnel <-> inconditionnel : conflit POTENTIEL.
            // Ex. (IF) C MUST A  vs  (MUST_NOT) A inconditionnel : quand C est vraie,
            // A serait à la fois obligatoire et interdit. La satisfiabilité de C n'est
            // pas décidable mécaniquement -> WARNING (le document reste valide).
            if let Some(&(uncond_pos, uncond_neg, _)) = seen.get(&action) {
                let opposite = if is_pos { uncond_neg } else { uncond_pos };
                let xkey = (cond.clone(), action.clone());
                if opposite && !reported_cross.contains(&xkey) {
                    reported_cross.insert(xkey);
                    r.warn("R11_DEONTIC_CONFLICT", line, &format!(
                        "conflit déontique potentiel: '{}' est {} sous la condition '{}' mais {} de façon inconditionnelle",
                        action,
                        if is_pos { "obligatoire" } else { "interdit" },
                        cond,
                        if is_pos { "interdit" } else { "obligatoire" }
                    ));
                }
            }
        }
    }
}

// ── Validation principale ────────────────────────────────────────────────────

fn extract_iota_group(block: &Block) -> Option<String> {
    block.fields.iter()
        .find(|f| f.name == "ι" || f.name == "iota" || f.name == "i")
        .map(|f| f.value.clone())
}

fn check_intrication_coherence(blocks: &[Block], r: &mut SemReport) {
    use std::collections::HashMap;

    let mut groups: HashMap<String, Vec<(bool, String, usize)>> = HashMap::new();

    for block in blocks {
        if let Some(group_id) = extract_iota_group(block) {
            if let Some((is_obligation, proposition, line)) = deontic_claim(block) {
                let normalized = normalize_proposition(&proposition);
                groups.entry(group_id).or_default().push((is_obligation, normalized, line));
            }
        }
    }

    for (group_id, claims) in &groups {
        for i in 0..claims.len() {
            for j in (i + 1)..claims.len() {
                let (pol_a, prop_a, line_a) = &claims[i];
                let (pol_b, prop_b, _) = &claims[j];
                if prop_a == prop_b && pol_a != pol_b {
                    r.err(
                        "R13_INTRICATION_CONTRADICTION",
                        *line_a,
                        &format!(
                            "groupe ι={} : proposition '{}' apparait avec polarite opposee",
                            group_id, prop_a
                        ),
                    );
                }
            }
        }

        let has_obligation = claims.iter().any(|(pol, _, _)| *pol);
        let has_prohibition = claims.iter().any(|(pol, _, _)| !*pol);
        let all_same_proposition = claims.iter().all(|(_, prop, _)| prop == &claims[0].1);

        if has_obligation && has_prohibition && !all_same_proposition {
            let first_line = claims.first().map(|(_, _, l)| *l).unwrap_or(0);
            r.warn(
                "R13_INTRICATION_CONFLICT",
                first_line,
                &format!(
                    "groupe ι={} : melange d'obligations et d'interdictions sur des propositions distinctes",
                    group_id
                ),
            );
        }
    }
}

/// R8 — Références cross-bloc.
/// Toute entité référencée comme sujet ou cible dans un bloc RELATIONS doit
/// avoir été déclarée au préalable via un bloc DEFINE. Référence non définie
/// = WARNING (le document reste exploitable, mais incomplet/suspect).
fn check_cross_references(blocks: &[Block], r: &mut SemReport) {
    use std::collections::HashSet;

    let mut defined: HashSet<String> = HashSet::new();
    for b in blocks {
        if b.name == "DEFINE" || b.name.starts_with("DEFINE") {
            if let Some(t) = extract_entity_type(b) {
                let _ = t; // type non utilisé ici, on veut juste le nom de l'entité
            }
            for f in &b.fields {
                if f.name == "_stmt" || f.name == "_value" {
                    if let Some(first) = f.value.split_whitespace().next() {
                        defined.insert(first.to_string());
                    }
                }
            }
        }
    }

    let mut reported: HashSet<String> = HashSet::new();
    for b in blocks {
        if b.name == "RELATIONS" || b.name.starts_with("RELATIONS") {
            for f in &b.fields {
                if f.name == "_stmt" {
                    let tokens: Vec<&str> = f.value.split_whitespace().collect();
                    if tokens.len() < 3 { continue; }
                    let subject = tokens[0];
                    let target = tokens[tokens.len() - 1];
                    for candidate in [subject, target] {
                        let looks_like_ref = !candidate.starts_with('r') && !candidate.starts_with('c');
                        if looks_like_ref && !defined.contains(candidate) && !reported.contains(candidate) {
                            reported.insert(candidate.to_string());
                            r.warn("R8_UNDEFINED_REFERENCE", f.line, &format!(
                                "référence à une entité non définie: '{}' (ni DEFINE, ni id de relation/contrainte)",
                                candidate
                            ));
                        }
                    }
                }
            }
        }
    }
}

pub fn validate_semantics(blocks: &[Block], domain: Option<&str>) -> SemReport {
    let mut r = SemReport::default();
    // Merge domain-specific operators into the official operator set so
    // validate_block only needs one set (fixes domain_ops previously unused).
    let domain_ops = domain
        .map(crate::domains::domain_operators)
        .unwrap_or_default();
    let op_set: HashSet<&str> = OFFICIAL_OPERATORS.iter()
        .copied()
        .chain(domain_ops)
        .collect();
    let type_set: HashSet<&str> = OFFICIAL_ENTITY_TYPES.iter().copied().collect();

    let mut seen_ids: HashSet<String> = HashSet::new();

    for block in blocks {
        validate_block(block, &op_set, &type_set, &mut seen_ids, &mut r);
        check_attribute_ontology(block, &mut r);
    }

    // R11 : cohérence déontique inter-blocs (MUST vs MUST_NOT sur même énoncé)
    check_deontic_contradictions(blocks, &mut r);


    // — Blocs d'arbitration (Session #9) : validation champs obligatoires —
    let arbitration_rules: &[(&str, &[&str])] = &[
        ("DEADLOCK_DECLARE",     &["round", "agents"]),
        ("ARBITRATION_REQUEST",  &["requester", "issue"]),
        ("ARBITRATION_RULING",   &["ruling", "sigma"]),
        ("ARBITRATION_APPEAL",   &["appellant", "grounds"]),
        ("ARBITRATION_FINALIZE", &["outcome", "sigma"]),
        ("DEADLOCK_TRIGGER",     &["trigger", "round"]),
        ("ARBITRATION_TELEMETRY",&["metric", "value"]),
        ("IDENTITY_ALERT",       &["finding", "sigma"]),
    ];
    for block in blocks {
        let bname = block.name.to_uppercase();
        for (arb_block, required_fields) in arbitration_rules {
            if bname == *arb_block {
                for req in *required_fields {
                    let has = block.fields.iter().any(|f| f.name == *req)
                        || block.fields.iter().any(|f| f.name == "_stmt"
                            && f.value.contains(req));
                    if !has {
                        r.warn(&format!(
                            "ARB_MISSING_FIELD: bloc {} manque champ obligatoire '{}'",
                            arb_block, req
                        ), block.line, "");
                    }
                }
            }
        }
    }

    // R13 : cohérence d'intrication (groupes iota)
    check_intrication_coherence(blocks, &mut r);

    // R8 : références cross-bloc (entités non définies)
    check_cross_references(blocks, &mut r);

    // R4 (info) : ordre canonique recommandé
    let order = ["INTENT_PAYLOAD", "META", "CONSTRAINTS", "UNCERTAINTY", "DEFINE", "RELATIONS", "DECISION"];
    let names: Vec<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
    if !is_canonical_order(&names, &order) {
        r.info("R4_UNEXPECTED_BLOCK", "ordre des blocs non canonique (recommandé mais non requis)");
    }

    r
}

/// R9 — Returns the canonical value set for a known semantic attribute, or None.
/// Zero-alloc: uses a match instead of a per-call HashMap.
fn canonical_values_for(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "polarity"   => Some(&["positive", "negative", "neutral"]),
        "quantifier" => Some(&["universal", "existential", "negative", "partial",
                                "plural", "singular", "definite", "indefinite"]),
        "frequency"  => Some(&["always", "often", "sometimes", "rarely", "never",
                                "occasional", "habitual", "exclusive"]),
        "scope"      => Some(&["universal", "partial", "wide", "narrow",
                                "distributive", "collective", "reflexive", "external"]),
        "mood"       => Some(&["indicative", "imperative", "interrogative", "subjunctive",
                                "conditional", "optative"]),
        "aspect"     => Some(&["perfective", "imperfective", "progressive", "habitual",
                                "iterative", "perfect"]),
        "epistemic"  => Some(&["known", "unknown", "estimated", "inferred",
                                "believed", "doubted", "certain"]),
        "evidential" => Some(&["visual", "hearsay", "inference", "direct", "report"]),
        _            => None,
    }
}

/// R9 — Vérifie que les attributs sémantiques connus utilisent des valeurs
/// canoniques. Récursif sur les sous-blocs. WARNING si hors ontologie.
fn check_attribute_ontology(block: &Block, r: &mut SemReport) {
    for f in &block.fields {
        if let Some(canonical_values) = canonical_values_for(&f.name) {
            if !canonical_values.contains(&f.value.as_str()) {
                r.warn("R9_NON_CANONICAL_VALUE", f.line, &format!(
                    "attribut '{}={}' hors ontologie. Valeurs canoniques: {:?}",
                    f.name, f.value, canonical_values
                ));
            }
        }
    }
    for sub in &block.subblocks {
        check_attribute_ontology(sub, r);
    }
}

fn validate_block(
    block: &Block,
    op_set: &HashSet<&str>,
    type_set: &HashSet<&str>,
    seen_ids: &mut HashSet<String>,
    r: &mut SemReport,
) {
    let name = block.name.as_str();

    // ── R10 : anti-bombing strict — un bloc ne doit pas dépasser 12 attributs.
    // Au-delà, le payload est considéré comme une tentative de saturation
    // (déni de service par surcharge de champs). ERREUR, pas warning.
    const MAX_FIELDS: usize = 12;
    if block.fields.len() > MAX_FIELDS {
        r.err("R10_TOO_MANY_ATTRIBUTES", block.line, &format!(
            "bloc '{}' a {} attributs (max {} autorisés) — anti-bombing",
            name, block.fields.len(), MAX_FIELDS
        ));
    }

    // ── R1 : IDs uniques (id=eXXX) ──
    for f in &block.fields {
        if f.name == "id" && !seen_ids.insert(f.value.clone()) {
            r.err("R1_DUPLICATE_ID", f.line, &format!("ID dupliqué: {}", f.value));
        }
    }

    // ── R3 : sigma/strength hors [0,1] → clampé (warning) ──
    for f in &block.fields {
        let is_sigma = f.name == "sigma" || f.name == "σ"
            || f.type_hint.as_deref() == Some("float") && f.name.contains("sigma");
        if is_sigma {
            if let Some(s) = parse_sigma(&f.value) {
                if !(0.0..=1.0).contains(&s) {
                    r.warn("R12_SIGMA_OUT_OF_RANGE", f.line,
                        &format!("sigma {} hors [0,1] — clampé à {}", s, s.clamp(0.0, 1.0)));
                }
            } else if !f.value.is_empty() {
                r.warn("R12_SIGMA_NOT_NUMERIC", f.line, &format!("sigma non numérique: {:?}", f.value));
            }
        }
    }

    // ── Validation spécifique DEFINE : type d'entité ∈ 10 officiels (R5-like) ──
    if name == "DEFINE" || name.starts_with("DEFINE") {
        // Forme: DEFINE <id> AS <type> [attrs]
        // Dans le parser, "AS <type>" peut apparaître comme champ ou _stmt.
        if let Some(t) = extract_entity_type(block) {
            if !type_set.contains(t.as_str()) {
                r.warn("R7_INVALID_ENTITY_TYPE", block.line,
                    &format!("type d'entité inconnu '{}' (hors des 10 officiels)", t));
            }
        }
        // R7 : DEFINE mal formé (pas de type extractible)
        else {
            r.warn("R7_MALFORMED_DEFINE", block.line, "DEFINE mal formé: type d'entité non extractible");
        }
    }

    // ── Validation RELATIONS : opérateurs ∈ 21 officiels (R5) ──
    if name == "RELATIONS" || name.starts_with("RELATIONS") {
        for f in &block.fields {
            // les relations inline sont stockées dans des champs _stmt / _value
            check_operators_in_value(&f.value, f.line, op_set, r);
        }
    }

    // ── Validation des blocs modaux déontiques : (MUST) (MUST_NOT) (RULE) (IF) (MAY)… ──
    // Le parser nomme ces blocs "(XXX)" et range l'énoncé "sujet OP cible" dans _stmt.
    // C'est ici que se joue le cœur de CSTL : vérifier que l'opérateur déontique
    // déclaré utilise bien un opérateur des 21 officiels, et non un verbe inventé.
    if name.starts_with('(') && name.ends_with(')') {
        for f in &block.fields {
            if f.name == "_stmt" {
                check_operators_in_value(&f.value, f.line, op_set, r);
            }
        }
    }

    // ── R6 : symboles compacts non redéfinis ──
    for f in &block.fields {
        if FIXED_SYMBOLS.contains(&f.name.as_str()) && f.name.len() <= 2 {
            // OK: usage normal d'un symbole fixe. On vérifie juste qu'il n'est pas
            // "redéfini" via un type_hint étrange.
            if let Some(th) = &f.type_hint {
                if th != "float" {
                    r.warn("R6_REDEFINED_FIXED_SYMBOL", f.line,
                        &format!("symbole fixe '{}' avec type inattendu '{}'", f.name, th));
                }
            }
        }
    }

    // Récursion sur les sous-blocs
    for sub in &block.subblocks {
        validate_block(sub, op_set, type_set, seen_ids, r);
    }
}

/// Extrait le type d'entité d'un bloc DEFINE.
/// Cherche un champ dont la valeur suit "AS", ou un champ "AS", ou un _stmt.
fn extract_entity_type(block: &Block) -> Option<String> {
    // Cas 1 : un champ _stmt contenant "alice AS human [..]"
    for f in &block.fields {
        if f.name == "_stmt" || f.name == "_value" {
            let parts: Vec<&str> = f.value.split_whitespace().collect();
            if let Some(pos) = parts.iter().position(|&p| p == "AS") {
                if pos + 1 < parts.len() {
                    return Some(parts[pos + 1].trim_matches(|c| c == '[' || c == ']').to_string());
                }
            }
        }
        // Cas 2 : champ nommé "AS"
        if f.name == "AS" {
            return Some(f.value.clone());
        }
    }
    None
}

fn is_canonical_order(names: &[&str], order: &[&str]) -> bool {
    let mut last = 0usize;
    for n in names {
        if let Some(idx) = order.iter().position(|o| n.starts_with(o)) {
            if idx < last { return false; }
            last = idx;
        }
    }
    true
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Field;

    pub fn field(name: &str, value: &str) -> Field {
        Field { name: name.into(), type_hint: None, value: value.into(), line: 1 }
    }
    fn field_t(name: &str, th: &str, value: &str) -> Field {
        Field { name: name.into(), type_hint: Some(th.into()), value: value.into(), line: 1 }
    }
    pub fn block(name: &str, fields: Vec<Field>) -> Block {
        Block { name: name.into(), fields, subblocks: vec![], line: 1 }
    }

    #[test]
    fn test_valid_operator_passes() {
        let b = block("RELATIONS", vec![field("_stmt", "alice COMMAND parser_agent")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("COMMAND")));
    }

    #[test]
    fn test_unknown_operator_warns() {
        let b = block("RELATIONS", vec![field("_stmt", "alice FOOBAR parser_agent")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("FOOBAR") && w.contains("R5")),
                "FOOBAR devrait déclencher un warning R5");
    }

    #[test]
    fn test_all_21_operators_recognized() {
        for op in OFFICIAL_OPERATORS {
            let b = block("RELATIONS", vec![field("_stmt", &format!("a {} b", op))]);
            let r = validate_semantics(&[b], None);
            assert!(r.warnings.iter().all(|w| !w.contains(&format!("'{}'", op))),
                    "l'opérateur officiel {} ne doit pas warner", op);
        }
    }

    #[test]
    fn test_valid_entity_type_passes() {
        let b = block("DEFINE", vec![field("_stmt", "alice AS human")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("type d'entité inconnu")));
    }

    #[test]
    fn test_unknown_entity_type_warns() {
        let b = block("DEFINE", vec![field("_stmt", "alice AS wizard")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("wizard") && w.contains("R7_INVALID_ENTITY_TYPE")),
                "type 'wizard' devrait warner R5");
    }

    #[test]
    fn test_all_10_entity_types_recognized() {
        for t in OFFICIAL_ENTITY_TYPES {
            let b = block("DEFINE", vec![field("_stmt", &format!("x AS {}", t))]);
            let r = validate_semantics(&[b], None);
            assert!(r.warnings.iter().all(|w| !w.contains(&format!("'{}'", t))),
                    "le type officiel {} ne doit pas warner", t);
        }
    }

    #[test]
    fn test_sigma_out_of_range_warns() {
        let b = block("DECISION", vec![field_t("sigma", "float", "1.5")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("R12_SIGMA_OUT_OF_RANGE")),
                "sigma 1.5 devrait warner R12");
    }

    #[test]
    fn test_sigma_in_range_ok() {
        let b = block("DECISION", vec![field_t("sigma", "float", "0.88")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("R12_SIGMA")));
    }

    #[test]
    fn test_duplicate_id_errors() {
        let b1 = block("DEFINE", vec![field("id", "e001")]);
        let b2 = block("DEFINE", vec![field("id", "e001")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R1") && e.contains("e001")),
                "ID dupliqué devrait être une erreur R1");
        assert!(!r.is_valid());
    }

    #[test]
    fn test_unique_ids_ok() {
        let b1 = block("DEFINE", vec![field("id", "e001")]);
        let b2 = block("DEFINE", vec![field("id", "e002")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.is_valid());
    }

    #[test]
    fn test_malformed_define_warns_r7() {
        // DEFINE sans "AS <type>"
        let b = block("DEFINE", vec![field("_stmt", "alice something_wrong")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("R7")),
                "DEFINE mal formé devrait warner R7");
    }

    // ── NOUVEAU : validation des blocs modaux déontiques ──────────────────────

    #[test]
    fn test_modal_block_unknown_operator_warns() {
        // (MUST) parser PRESERVE deontic_modalities — PRESERVE n'est pas officiel.
        // C'est le test qui démontre que ton validate.cstl devait warner.
        let b = block("(MUST)", vec![field("_stmt", "parser PRESERVE deontic_modalities")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().any(|w| w.contains("PRESERVE") && w.contains("R5")),
                "PRESERVE dans un bloc modal devrait warner R5");
    }

    #[test]
    fn test_modal_block_valid_operator_ok() {
        // (MUST) parser COMMAND child_agent — COMMAND est officiel.
        let b = block("(MUST)", vec![field("_stmt", "parser COMMAND child_agent")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("COMMAND")),
                "COMMAND est officiel, ne doit pas warner");
    }

    #[test]
    fn test_rule_block_modality_not_flagged() {
        // (RULE) MUST respond_in_cstl_only — MUST est une modalité, pas un opérateur inconnu.
        let b = block("(RULE)", vec![field("_stmt", "MUST respond_in_cstl_only")]);
        let r = validate_semantics(&[b], None);
        assert!(r.warnings.iter().all(|w| !w.contains("R5")),
                "MUST (modalité) ne doit pas warner R5");
    }

    #[test]
    fn test_r5_domain_op_accepted() {
        // Un opérateur de domaine médical (PRESCRIRE) ne doit pas déclencher R5
        // si le domaine est déclaré dans META
        let b = block("RELATIONS", vec![field("_stmt", "alice PRESCRIRE bob")]);
        let r = validate_semantics(&[b], Some("medical"));
        assert!(
            !r.errors.iter().any(|e| e.contains("R5")),
            "PRESCRIRE avec domain=medical ne doit pas déclencher R5"
        );
    }

    // ── NOUVEAU : R8 cohérence déontique (MUST vs MUST_NOT) ───────────────────

    #[test]
    fn test_deontic_contradiction_detected() {
        // Même énoncé déclaré obligatoire ET interdit -> ERREUR R8.
        let b1 = block("(MUST)",     vec![field("_stmt", "parser ACCEPT malformed_hashbang")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "parser ACCEPT malformed_hashbang")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R11_DEONTIC_CONTRADICTION") && e.contains("contradiction")),
                "MUST + MUST_NOT sur le même énoncé doit être une erreur R8");
        assert!(!r.is_valid(), "le document doit être invalide");
    }

    #[test]
    fn test_no_contradiction_different_statements() {
        // Énoncés différents -> pas de contradiction.
        let b1 = block("(MUST)",     vec![field("_stmt", "parser PRESERVE deontic_modalities")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "parser ACCEPT malformed_hashbang")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().all(|e| !e.contains("R11_DEONTIC_CONTRADICTION")),
                "des énoncés différents ne doivent pas déclencher R8");
    }

    #[test]
    fn test_forbid_also_contradicts_must() {
        // FORBID compte comme interdiction au même titre que MUST_NOT.
        let b1 = block("(MUST)",   vec![field("_stmt", "agent SEND data")]);
        let b2 = block("(FORBID)", vec![field("_stmt", "agent SEND data")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R11_DEONTIC_CONTRADICTION")),
                "MUST + FORBID sur le même énoncé doit être une erreur R8");
    }

    // ── NOUVEAU : R8 forme interne (RULE) + cross-formes + (IF) non couvert ────

    #[test]
    fn test_rule_block_contradiction_detected() {
        // (RULE) MUST x  +  (RULE) MUST_NOT x  -> R8 (modalité interne à l'énoncé)
        let b1 = block("(RULE)", vec![field("_stmt", "MUST respond_in_cstl_only")]);
        let b2 = block("(RULE)", vec![field("_stmt", "MUST_NOT respond_in_cstl_only")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R11_DEONTIC_CONTRADICTION")),
                "(RULE) MUST vs (RULE) MUST_NOT sur le même énoncé doit être R8");
        assert!(!r.is_valid());
    }

    #[test]
    fn test_contradiction_across_forms() {
        // (MUST) x  vs  (RULE) MUST_NOT x  -> même proposition, formes différentes -> R8
        let b1 = block("(MUST)", vec![field("_stmt", "parser ACCEPT malformed_hashbang")]);
        let b2 = block("(RULE)", vec![field("_stmt", "MUST_NOT parser ACCEPT malformed_hashbang")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R11_DEONTIC_CONTRADICTION")),
                "contradiction entre forme nommée et forme (RULE) doit être détectée");
    }

    // ── NOUVEAU : R8 conditionnels (IF) ───────────────────────────────────────

    #[test]
    fn test_conditional_contradiction_same_condition() {
        // (IF) C MUST A  +  (IF) C MUST_NOT A  -> contradiction conditionnelle
        let b1 = block("(IF)", vec![field("_stmt", "sigma_low MUST flag_uncertainty")]);
        let b2 = block("(IF)", vec![field("_stmt", "sigma_low MUST_NOT flag_uncertainty")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R11_DEONTIC_CONTRADICTION") && e.contains("conditionnelle")),
                "même condition + action opposée doit être R8");
        assert!(!r.is_valid());
    }

    #[test]
    fn test_conditional_different_conditions_ok() {
        // Conditions différentes -> pas de contradiction.
        let b1 = block("(IF)", vec![field("_stmt", "sigma_low MUST flag_uncertainty")]);
        let b2 = block("(IF)", vec![field("_stmt", "sigma_high MUST_NOT flag_uncertainty")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().all(|e| !e.contains("R11_DEONTIC_CONTRADICTION")),
                "des conditions différentes ne doivent pas déclencher R8");
    }

    // ── NOUVEAU : croisement conditionnel <-> inconditionnel (warning) ─────────

    #[test]
    fn test_cross_conditional_unconditional_is_warning() {
        // (IF) C MUST A  +  (MUST_NOT) A  -> conflit POTENTIEL : warning, pas erreur.
        let b1 = block("(IF)",       vec![field("_stmt", "sigma_low MUST flag_uncertainty")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "flag_uncertainty")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().all(|e| !e.contains("R11_DEONTIC_CONTRADICTION")),
                "le croisement ne doit PAS être une erreur");
        assert!(r.warnings.iter().any(|w| w.contains("R11_DEONTIC_CONFLICT") && w.contains("potentiel")),
                "il doit produire un WARNING de conflit potentiel");
        assert!(r.is_valid(), "warning seulement -> document reste valide");
    }

    #[test]
    fn test_cross_symmetric_warning() {
        // (IF) C MUST_NOT A  +  (MUST) A  -> warning (sens inverse).
        let b1 = block("(IF)",   vec![field("_stmt", "sigma_low MUST_NOT send_data")]);
        let b2 = block("(MUST)", vec![field("_stmt", "send_data")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.warnings.iter().any(|w| w.contains("R11_DEONTIC_CONFLICT") && w.contains("potentiel")),
                "MUST_NOT conditionnel vs MUST inconditionnel doit warner");
        assert!(r.is_valid());
    }

    #[test]
    fn test_cross_same_polarity_no_warning() {
        // (IF) C MUST A  +  (MUST) A  -> même polarité, aucun conflit.
        let b1 = block("(IF)",   vec![field("_stmt", "sigma_low MUST flag_x")]);
        let b2 = block("(MUST)", vec![field("_stmt", "flag_x")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.warnings.iter().all(|w| !w.contains("potentiel")),
                "même polarité ne doit pas produire de conflit potentiel");
    }

    #[test]
    fn test_intrication_contradiction() {
        let b1 = block("(MUST)",     vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "mission_01")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "mission_01")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().any(|e| e.contains("R13_INTRICATION_CONTRADICTION")),
                "meme groupe iota, contradiction: {:?}", r.errors);
    }

    #[test]
    fn test_intrication_conflict_warning() {
        let b1 = block("(MUST)",     vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "mission_01")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "agent_B MAINTAIN system_Y"), field("ι", "mission_01")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.warnings.iter().any(|w| w.contains("R13_INTRICATION_CONFLICT")),
                "groupe iota mixte = warning: {:?}", r.warnings);
        assert!(r.is_valid(), "warning ne doit pas invalider");
    }

    #[test]
    fn test_intrication_different_groups_no_cross() {
        let b1 = block("(MUST)",     vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "group_A")]);
        let b2 = block("(MUST_NOT)", vec![field("_stmt", "agent_A PERFORM task_X"), field("ι", "group_B")]);
        let r = validate_semantics(&[b1, b2], None);
        assert!(r.errors.iter().all(|e| !e.contains("R13")),
                "groupes differents ne se contaminent pas: {:?}", r.errors);
    }


    #[test]
    fn test_r6_fixed_symbol_normal_no_warn() {
        // σ avec type_hint float = usage canonique, pas de warning R6
        let b = block("META", vec![
            field("σ", "0.9"),
        ]);
        let r = validate_semantics(&[b], None);
        assert!(
            !r.warnings.iter().any(|w| w.contains("R6")),
            "sigma:float normal ne doit pas déclencher R6"
        );
    }

    #[test]
    fn test_r6_fixed_symbol_redefined_warns() {
        // σ avec type_hint string = redéfinition invalide
        let mut f = field("σ", "hello");
        f.type_hint = Some("string".to_string());
        let b = block("META", vec![f]);
        let r = validate_semantics(&[b], None);
        assert!(
            r.warnings.iter().any(|w| w.contains("R6")),
            "sigma avec type_hint string doit déclencher R6"
        );
    }


    #[test]
    fn test_arb_missing_field_warns() {
        // DEADLOCK_DECLARE sans 'round' doit déclencher ARB_MISSING_FIELD
        let b = block("DEADLOCK_DECLARE", vec![field("agents", "A,B")]);
        let r = validate_semantics(&[b], None);
        assert!(
            r.warnings.iter().any(|w| w.contains("ARB_MISSING_FIELD")),
            "DEADLOCK_DECLARE sans round doit déclencher ARB_MISSING_FIELD"
        );
    }

    #[test]
    fn test_arb_complete_no_warn() {
        // IDENTITY_ALERT avec tous les champs ne doit pas déclencher ARB_MISSING_FIELD
        let b = block("IDENTITY_ALERT", vec![
            field("finding", "swap_detecte"),
            field("sigma", "0.94"),
        ]);
        let r = validate_semantics(&[b], None);
        assert!(
            !r.warnings.iter().any(|w| w.contains("ARB_MISSING_FIELD")),
            "IDENTITY_ALERT complet ne doit pas déclencher ARB_MISSING_FIELD"
        );
    }

}
