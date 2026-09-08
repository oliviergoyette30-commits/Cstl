/// CSTL Payload Parser
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CstlPayload {
    pub version: String,
    pub mode: String,
    pub meta: HashMap<String, String>,
    pub intent: HashMap<String, String>,
    pub relations: Vec<HashMap<String, String>>,
    /// Blocs DEFINE (`DEFINE <identifier> AS <entity_type> [attr_list]`,
    /// spec §9) -- ajoute le 2026-09-05 pour reconstruire R8 (coref_with)
    /// sur le VRAI format wire, sans reintroduire `ast::Block` (retire le
    /// 2026-09-04, voir semantic.rs). Avant ce fix, DEFINE n'etait meme pas
    /// reconnu par ce parser -- seuls META/INTENT_PAYLOAD/RELATION
    /// l'etaient -- donc R8 etait structurellement impossible a reconstruire,
    /// pas seulement non branche. Chaque entree est un HashMap plat, meme
    /// esprit que `relations`: "name" (identifiant, ex. "patient"),
    /// "entity_type" (ex. "human"), plus tous les attributs `[id=..., ...]`
    /// s'il y en a. Un DEFINE sans `id=` est conserve (les autres attributs
    /// restent utiles) mais ne peut jamais satisfaire un coref_with, qui
    /// reference toujours un `id`.
    pub defines: Vec<HashMap<String, String>>,
    /// Avertissements de parsing non fatals -- ex. bloc DEFINE avec crochets
    /// malformes, silencieusement ignore avant ce fix (R7, §19 : "dropped +
    /// warning" -- seul "dropped" existait). Ajoute le 2026-09-05 aux cotes
    /// de `defines`. Ne couvre PAS (encore) les blocs RELATION malformes,
    /// qui gardent leur comportement historique de drop silencieux
    /// (`if let Ok(...) = parse_block(...)` plus bas) -- hors du perimetre
    /// de R7, qui ne mentionne que DEFINE.
    pub parse_warnings: Vec<String>,
    /// Blocs `GUARDRAIL_REPORT [status=..., reason=..., ...]` -- ajoutes le
    /// 2026-09-08 pour porter en Rust les 4 blocs valides EMPIRIQUEMENT en
    /// conversation (pas en code) le 22 mai 2026 par la session tripartite
    /// Claude+Gemini+ChatGPT (voir CSTL_SPEC_v5_0.md, nouvelle section
    /// "GUARDRAIL_REPORT / SCOPE_LOCK"). Meme forme wire generique que
    /// META/INTENT_PAYLOAD/RELATION (`NOM [cle=valeur, ...]`), donc branche
    /// ici sans nouveau mecanisme de parsing -- juste un nouveau nom de bloc
    /// reconnu. Un payload peut porter plusieurs GUARDRAIL_REPORT (un LLM
    /// receveur peut bloquer plusieurs sous-instructions d'un meme payload) ;
    /// chacun est conserve tel quel, HashMap plat, meme esprit que
    /// `relations`. IMPORTANT (honnetete empirique, voir handler.rs) : le
    /// serveur ne peut que RELAYER fidelement ce contenu -- il ne peut pas
    /// verifier qu'un GUARDRAIL_REPORT recu reflete un vrai refus du LLM
    /// receveur (aucune boucle vers un LLM tiers dans ce chemin).
    pub guardrail_reports: Vec<HashMap<String, String>>,
    /// Bloc `SCOPE_LOCK [mode=STRICT|OPEN, allowed_ids="...", ...]` --
    /// meme origine et meme date que `guardrail_reports` ci-dessus. Au plus
    /// UN SCOPE_LOCK actif par payload (contrairement a GUARDRAIL_REPORT,
    /// qui peut se repeter) : un deuxieme bloc SCOPE_LOCK dans le meme
    /// payload est ignore et produit un `parse_warnings` explicite plutot
    /// que d'ecraser silencieusement le premier ou de choisir arbitrairement
    /// "le dernier gagne" (ambigu : lequel des deux scopes le client
    /// voulait-il vraiment verrouiller ?).
    pub scope_lock: Option<HashMap<String, String>>,
    pub raw: String,
}

#[derive(Debug)]
pub enum ParseError {
    MissingHashbang,
    InvalidFormat(String),
    MissingEndMarker,
    MalformedBlock(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParseError::MissingHashbang => write!(f, "Missing CSTL hashbang"),
            ParseError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            ParseError::MissingEndMarker => write!(f, "Missing ---END--- marker"),
            ParseError::MalformedBlock(msg) => write!(f, "Malformed block: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// Enregistre un bloc SCOPE_LOCK deja parse -- factorise entre les 3 points
/// du parseur ou un bloc bracket peut se fermer (meme-ligne, multi-ligne,
/// bloc precedent sauvegarde). Voir le commentaire de `CstlPayload::scope_lock`
/// pour pourquoi un deuxieme SCOPE_LOCK est un avertissement plutot qu'un
/// ecrasement silencieux.
fn record_scope_lock(payload: &mut CstlPayload, map: HashMap<String, String>) {
    if payload.scope_lock.is_some() {
        payload.parse_warnings.push(
            "SCOPE_LOCK: bloc SCOPE_LOCK supplementaire ignore (un seul actif par payload)".to_string()
        );
    } else {
        payload.scope_lock = Some(map);
    }
}

pub fn parse_payload(raw: &str) -> Result<CstlPayload, ParseError> {
    let mut payload = CstlPayload {
        version: String::new(),
        mode: String::new(),
        meta: HashMap::new(),
        intent: HashMap::new(),
        relations: Vec::new(),
        defines: Vec::new(),
        parse_warnings: Vec::new(),
        guardrail_reports: Vec::new(),
        scope_lock: None,
        raw: raw.to_string(),
    };

    if !raw.starts_with("#!CSTL") {
        return Err(ParseError::MissingHashbang);
    }

    if !raw.contains("---END---") {
        return Err(ParseError::MissingEndMarker);
    }

    // Parse hashbang
    let hashbang_line = raw.lines().next().unwrap_or("");
    if let Some(version_mode) = hashbang_line.strip_prefix("#!CSTL ") {
        let parts: Vec<&str> = version_mode.split_whitespace().collect();
        if !parts.is_empty() {
            payload.version = parts[0].to_string();
        }
        if parts.len() > 1 {
            if let Some(mode_str) = parts[1].strip_prefix("MODE=") {
                payload.mode = mode_str.to_string();
            }
        }
    }

    // Parse blocks - FIXED for single-line blocks
    let mut in_block = false;
    let mut current_block = String::new();
    let mut block_name = String::new();

    for line in raw.lines().skip(1) {
        // Check if line starts a new block
        let is_meta = line.starts_with("META [");
        let is_intent = line.starts_with("INTENT_PAYLOAD [");
        let is_relation = line.starts_with("RELATION [");
        // GUARDRAIL_REPORT et SCOPE_LOCK (ajoutes 2026-09-08) suivent EXACTEMENT
        // la meme forme bracket generique que META/INTENT_PAYLOAD/RELATION --
        // "NOM [" -- donc aucun nouveau mecanisme de decoupage, juste deux mots-cle
        // reconnus de plus. Voir CstlPayload::guardrail_reports/scope_lock pour
        // l'origine (session tripartite du 22 mai 2026) et CSTL_SPEC_v5_0.md pour
        // la grammaire complete.
        let is_guardrail = line.starts_with("GUARDRAIL_REPORT [");
        let is_scope_lock = line.starts_with("SCOPE_LOCK [");
        // Contrairement a META/INTENT_PAYLOAD/RELATION, le mot-cle DEFINE
        // n'est pas immediatement suivi de "[" -- la grammaire reelle (spec
        // §9) est `DEFINE <identifier> AS <entity_type> [attrs]`, avec deux
        // tokens (identifiant + type) entre le mot-cle et le crochet.
        let is_define = line.starts_with("DEFINE ");

        if is_meta || is_intent || is_relation || is_define || is_guardrail || is_scope_lock {
            // Save previous block if exists
            if !current_block.is_empty() && !block_name.is_empty() {
                match block_name.as_str() {
                    "META" => payload.meta = parse_block(&current_block)?,
                    "INTENT_PAYLOAD" => payload.intent = parse_block(&current_block)?,
                    "RELATION" => {
                        if let Ok(relation) = parse_block(&current_block) {
                            payload.relations.push(relation);
                        }
                    }
                    "DEFINE" => match parse_define_block(&current_block) {
                        Ok(def) => payload.defines.push(def),
                        Err(e) => payload.parse_warnings.push(format!(
                            "R7: bloc DEFINE mal forme ignore -- {}", e
                        )),
                    },
                    "GUARDRAIL_REPORT" => {
                        if let Ok(report) = parse_block(&current_block) {
                            payload.guardrail_reports.push(report);
                        }
                    }
                    "SCOPE_LOCK" => {
                        if let Ok(lock) = parse_block(&current_block) {
                            record_scope_lock(&mut payload, lock);
                        }
                    }
                    _ => {}
                }
            }

            // Start new block
            in_block = true;
            block_name = if is_meta { "META" } else if is_intent { "INTENT_PAYLOAD" } else if is_relation { "RELATION" } else if is_guardrail { "GUARDRAIL_REPORT" } else if is_scope_lock { "SCOPE_LOCK" } else { "DEFINE" }.to_string();
            current_block = line.to_string();

            // Check if block ends on same line. Cas particulier DEFINE : le
            // groupe `[attrs]` etant optionnel dans la grammaire (§9), une
            // ligne DEFINE sans aucun '[' n'a structurellement rien a
            // attendre d'une ligne suivante -- la traiter comme multi-ligne
            // (comme META/INTENT/RELATION le font quand ']' manque) la ferait
            // engloutir la ligne ---END--- suivante par accident (bug reel
            // trouve en ecrivant le test correspondant), puisqu'aucun '['
            // jamais ouvert ne pourra jamais etre "ferme". Elle est donc
            // complete des sa propre ligne.
            if line.contains(']') || (block_name == "DEFINE" && !current_block.contains('[')) {
                match block_name.as_str() {
                    "META" => payload.meta = parse_block(&current_block)?,
                    "INTENT_PAYLOAD" => payload.intent = parse_block(&current_block)?,
                    "RELATION" => {
                        if let Ok(relation) = parse_block(&current_block) {
                            payload.relations.push(relation);
                        }
                    }
                    "DEFINE" => match parse_define_block(&current_block) {
                        Ok(def) => payload.defines.push(def),
                        Err(e) => payload.parse_warnings.push(format!(
                            "R7: bloc DEFINE mal forme ignore -- {}", e
                        )),
                    },
                    "GUARDRAIL_REPORT" => {
                        if let Ok(report) = parse_block(&current_block) {
                            payload.guardrail_reports.push(report);
                        }
                    }
                    "SCOPE_LOCK" => {
                        if let Ok(lock) = parse_block(&current_block) {
                            record_scope_lock(&mut payload, lock);
                        }
                    }
                    _ => {}
                }
                in_block = false;
                current_block.clear();
                block_name.clear();
            }
        } else if in_block && line.contains(']') {
            current_block.push('\n');
            current_block.push_str(line);

            match block_name.as_str() {
                "META" => payload.meta = parse_block(&current_block)?,
                "INTENT_PAYLOAD" => payload.intent = parse_block(&current_block)?,
                "RELATION" => {
                    if let Ok(relation) = parse_block(&current_block) {
                        payload.relations.push(relation);
                    }
                }
                "DEFINE" => match parse_define_block(&current_block) {
                    Ok(def) => payload.defines.push(def),
                    Err(e) => payload.parse_warnings.push(format!(
                        "R7: bloc DEFINE mal forme ignore -- {}", e
                    )),
                },
                "GUARDRAIL_REPORT" => {
                    if let Ok(report) = parse_block(&current_block) {
                        payload.guardrail_reports.push(report);
                    }
                }
                "SCOPE_LOCK" => {
                    if let Ok(lock) = parse_block(&current_block) {
                        record_scope_lock(&mut payload, lock);
                    }
                }
                _ => {}
            }
            in_block = false;
            current_block.clear();
        } else if in_block {
            current_block.push('\n');
            current_block.push_str(line);
        }
    }

    // R7 (suite) : un bloc DEFINE dont le crochet ouvrant n'est JAMAIS ferme
    // avant ---END--- (ex. "DEFINE patient AS human [id=e001" sans "]" nulle
    // part ensuite) ne declenche jamais la branche `line.contains(']')`
    // ci-dessus -- avant ce fix, il restait accumule dans `current_block`
    // jusqu'a la fin de la boucle puis etait perdu SANS AUCUN avertissement
    // (silencieux, pas meme un "dropped" detectable). C'est le cas le plus
    // litteral de "crochets malformes" vise par R7 (§19). Flush explicite ici
    // -- uniquement pour DEFINE (hors perimetre pour META/INTENT_PAYLOAD/
    // RELATION, qui gardent leur comportement historique : `?` propage une
    // erreur dure pour META/INTENT, drop silencieux pour RELATION).
    if block_name == "DEFINE" && !current_block.is_empty() {
        match parse_define_block(&current_block) {
            Ok(def) => payload.defines.push(def),
            Err(e) => payload.parse_warnings.push(format!(
                "R7: bloc DEFINE mal forme ignore (jamais ferme avant ---END---) -- {}", e
            )),
        }
    }

    eprintln!("[Parser] Parsed CSTL v{} MODE={}", payload.version, payload.mode);
    eprintln!("[Parser] META: {} fields", payload.meta.len());
    eprintln!("[Parser] INTENT: {} fields", payload.intent.len());
    eprintln!("[Parser] RELATIONS: {} blocks", payload.relations.len());
    eprintln!("[Parser] DEFINE: {} blocks ({} avertissement(s) R7)", payload.defines.len(), payload.parse_warnings.len());
    eprintln!("[Parser] GUARDRAIL_REPORT: {} blocks, SCOPE_LOCK: {}", payload.guardrail_reports.len(), payload.scope_lock.is_some());

    Ok(payload)
}

/// Parse un bloc `DEFINE <identifier> AS <entity_type> [attr_list]` (spec §9)
/// en un HashMap plat -- meme esprit que `parse_block`, mais avec un en-tete
/// a extraire avant les crochets (identifiant + type), pas seulement une
/// liste d'attributs. `[attr_list]` est optionnel dans la grammaire ; sans
/// lui, l'entite est quand meme enregistree (name/entity_type seuls) mais ne
/// pourra jamais satisfaire un `coref_with` (qui reference toujours un `id`).
///
/// R7 : un en-tete malforme (pas exactement "identifiant AS type") ou des
/// crochets malformes (`[` sans `]` ou vice-versa, via `parse_block`)
/// produisent tous les deux un `ParseError::MalformedBlock` -- l'appelant
/// (`parse_payload`) le transforme en avertissement `payload.parse_warnings`
/// plutot que de faire echouer tout le payload : le bloc est "dropped" (pas
/// enregistre dans `payload.defines`), avec avertissement, exactement la
/// regle R7 (§19).
fn parse_define_block(block: &str) -> Result<HashMap<String, String>, ParseError> {
    let rest = block
        .strip_prefix("DEFINE ")
        .ok_or_else(|| ParseError::MalformedBlock("bloc DEFINE sans prefixe attendu".to_string()))?;

    let header = match rest.find('[') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    let tokens: Vec<&str> = header.split_whitespace().collect();
    if tokens.len() != 3 || tokens[1] != "AS" {
        return Err(ParseError::MalformedBlock(format!(
            "en-tete DEFINE invalide (attendu '<identifiant> AS <type>'): {:?}",
            header.trim()
        )));
    }

    // Les attributs `[...]` sont optionnels dans la grammaire (§9: le groupe
    // "(SP \"[\" attr_list \"]\")?" est marque `?`). Sans '[', pas d'attrs.
    let mut map = if rest.contains('[') {
        parse_block(block)?
    } else {
        HashMap::new()
    };

    map.insert("name".to_string(), tokens[0].to_string());
    map.insert("entity_type".to_string(), tokens[2].to_string());
    Ok(map)
}

/// Coupe `content` sur les virgules de premier niveau seulement -- une virgule
/// a l'interieur d'une paire de guillemets ne separe pas deux champs. Corrige
/// une trouvaille critique de l'audit multi-angle (2026-09-03): l'ancien
/// `content.split(',')` coupait aveuglement sur TOUTE virgule, y compris dans
/// une valeur citee comme `produced_by="Report, final version"` -- le fragment
/// resultant sans '=' etait alors ignore silencieusement (pas d'erreur), et la
/// valeur d'origine finissait tronquee avec un guillemet ouvrant orphelin.
fn split_top_level_commas(content: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    for (i, ch) in content.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                parts.push(&content[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&content[start..]);
    parts
}

fn parse_block(block: &str) -> Result<HashMap<String, String>, ParseError> {
    let mut map = HashMap::new();

    let start = block.find('[').ok_or_else(|| ParseError::MalformedBlock("Missing [".to_string()))?;
    let end = block.rfind(']').ok_or_else(|| ParseError::MalformedBlock("Missing ]".to_string()))?;

    let content = &block[start + 1..end];

    for pair in split_top_level_commas(content) {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        match pair.split_once('=') {
            Some((key, value)) => {
                map.insert(key.trim().to_string(), value.trim().trim_matches('"').to_string());
            }
            None => {
                // Avant: ignore silencieusement un fragment sans '=' -- une
                // valeur mal formee (guillemet non ferme, typo) perdait des
                // donnees sans que personne ne le sache. Desormais: erreur
                // explicite plutot que perte silencieuse.
                return Err(ParseError::MalformedBlock(format!(
                    "champ sans '=': {:?}",
                    pair
                )));
            }
        }
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_payload() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
RELATION [type=equals, subject=x, object=y]
---END---"#;

        let result = parse_payload(payload_str);
        assert!(result.is_ok());

        let payload = result.unwrap();
        assert_eq!(payload.version, "v5.0.0");
        assert_eq!(payload.mode, "A");
        assert_eq!(payload.meta.get("encoder"), Some(&"Agent_CLAUDE".to_string()));
        assert_eq!(payload.intent.get("purpose"), Some(&"test".to_string()));
        assert_eq!(payload.relations.len(), 1);
    }

    #[test]
    fn test_quoted_value_with_internal_comma_is_preserved() {
        // Cas exact de l'audit multi-angle (2026-09-03): une virgule DANS une
        // valeur citee ne doit plus couper le champ en deux.
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by="Report, final version"]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.meta.get("produced_by"), Some(&"Report, final version".to_string()));
    }

    #[test]
    fn test_field_without_equals_is_now_an_explicit_parse_error() {
        // Avant: un fragment sans '=' (issu d'un guillemet non ferme, etc.)
        // etait ignore silencieusement -- perte de donnees sans erreur.
        // Desormais: ParseError::MalformedBlock explicite.
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, ceci_na_pas_de_signe_egal]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
---END---"#;

        let result = parse_payload(payload_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_trailing_comma_still_parses_fine() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude,]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.meta.get("produced_by"), Some(&"Claude".to_string()));
    }

    #[test]
    fn test_missing_hashbang() {
        let payload_str = "INVALID\n---END---";
        let result = parse_payload(payload_str);
        assert!(matches!(result, Err(ParseError::MissingHashbang)));
    }

    #[test]
    fn test_missing_end_marker() {
        let payload_str = "#!CSTL v5.0.0 MODE=A\nMETA [x=y]";
        let result = parse_payload(payload_str);
        assert!(matches!(result, Err(ParseError::MissingEndMarker)));
    }

    // ── DEFINE (spec §9), reconstruit le 2026-09-05 pour R8/R7 ──

    #[test]
    fn test_define_block_parsed_with_id() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
DEFINE patient AS human [id=e001, age=67]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.defines.len(), 1);
        let d = &payload.defines[0];
        assert_eq!(d.get("name"), Some(&"patient".to_string()));
        assert_eq!(d.get("entity_type"), Some(&"human".to_string()));
        assert_eq!(d.get("id"), Some(&"e001".to_string()));
        assert_eq!(d.get("age"), Some(&"67".to_string()));
        assert!(payload.parse_warnings.is_empty());
    }

    #[test]
    fn test_define_block_without_attrs_still_parsed() {
        // §9 : le groupe [attr_list] est optionnel dans la grammaire.
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
DEFINE patient AS human
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.defines.len(), 1);
        assert_eq!(payload.defines[0].get("name"), Some(&"patient".to_string()));
        assert_eq!(payload.defines[0].get("id"), None);
    }

    #[test]
    fn test_multiple_defines_and_relations_coexist() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
DEFINE patient AS human [id=e001]
DEFINE physician AS agent [id=e002]
RELATION [type=EQUALS, subject=e001, object=e002]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.defines.len(), 2);
        assert_eq!(payload.relations.len(), 1);
    }

    // ── R7 : DEFINE avec crochets/en-tete malformes -> dropped + warning ──

    #[test]
    fn test_r7_define_malformed_header_dropped_with_warning() {
        // En-tete sans "AS" -- ne correspond pas a "<identifiant> AS <type>".
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
DEFINE patient human [id=e001]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert!(payload.defines.is_empty(), "un DEFINE mal forme ne doit pas etre enregistre");
        assert!(payload.parse_warnings.iter().any(|w| w.starts_with("R7:")),
                "un avertissement R7 est attendu: {:?}", payload.parse_warnings);
    }

    #[test]
    fn test_r7_define_never_closed_bracket_dropped_with_warning() {
        // Crochet ouvrant jamais ferme avant ---END--- -- le cas le plus
        // litteral de "crochets malformes" (R7, §19). Avant ce fix : perdu
        // silencieusement, sans meme un avertissement.
        let payload_str = "#!CSTL v5.0.0 MODE=A\n\
            META [encoder=Agent_CLAUDE, produced_by=Claude]\n\
            INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]\n\
            DEFINE patient AS human [id=e001\n\
            ---END---";

        let payload = parse_payload(payload_str).unwrap();
        assert!(payload.defines.is_empty());
        assert!(payload.parse_warnings.iter().any(|w| w.starts_with("R7:")),
                "un avertissement R7 est attendu: {:?}", payload.parse_warnings);
    }

    #[test]
    fn test_r7_clean_define_produces_no_warning() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
DEFINE patient AS human [id=e001]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert!(payload.parse_warnings.is_empty());
    }

    // ── GUARDRAIL_REPORT / SCOPE_LOCK (ajoutes 2026-09-08, session
    // tripartite du 22 mai 2026 -- voir CSTL_SPEC_v5_0.md) ──

    #[test]
    fn test_guardrail_report_parsed() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
GUARDRAIL_REPORT [status=BLOCKED, reason=policy_violation, blocked_operator=PERFORM]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.guardrail_reports.len(), 1);
        let g = &payload.guardrail_reports[0];
        assert_eq!(g.get("status"), Some(&"BLOCKED".to_string()));
        assert_eq!(g.get("reason"), Some(&"policy_violation".to_string()));
        assert_eq!(g.get("blocked_operator"), Some(&"PERFORM".to_string()));
    }

    #[test]
    fn test_multiple_guardrail_reports_all_kept() {
        // Un LLM receveur peut bloquer plusieurs sous-instructions distinctes
        // du meme payload -- contrairement a SCOPE_LOCK, plusieurs
        // GUARDRAIL_REPORT sont tous conserves, pas seulement le premier.
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
GUARDRAIL_REPORT [status=BLOCKED, reason=r1]
GUARDRAIL_REPORT [status=PARTIAL, reason=r2]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.guardrail_reports.len(), 2);
    }

    #[test]
    fn test_scope_lock_parsed() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
SCOPE_LOCK [mode=STRICT, allowed_ids="e001;e002"]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        let lock = payload.scope_lock.expect("SCOPE_LOCK attendu");
        assert_eq!(lock.get("mode"), Some(&"STRICT".to_string()));
        assert_eq!(lock.get("allowed_ids"), Some(&"e001;e002".to_string()));
    }

    #[test]
    fn test_second_scope_lock_ignored_with_warning() {
        // Voir CstlPayload::scope_lock : un deuxieme bloc dans le meme
        // payload n'ecrase pas le premier -- il est ignore, avec avertissement.
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
SCOPE_LOCK [mode=STRICT, allowed_ids="e001"]
SCOPE_LOCK [mode=OPEN]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        let lock = payload.scope_lock.expect("le premier SCOPE_LOCK doit rester actif");
        assert_eq!(lock.get("mode"), Some(&"STRICT".to_string()));
        assert!(payload.parse_warnings.iter().any(|w| w.contains("SCOPE_LOCK")));
    }

    #[test]
    fn test_guardrail_report_and_scope_lock_coexist_with_other_blocks() {
        let payload_str = r#"#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude]
INTENT_PAYLOAD [purpose=test, sender=alice, receiver=bob]
DEFINE patient AS human [id=e001]
SCOPE_LOCK [mode=STRICT, allowed_ids="e001"]
GUARDRAIL_REPORT [status=ALLOWED]
RELATION [type=EQUALS, subject=e001, object=e001]
---END---"#;

        let payload = parse_payload(payload_str).unwrap();
        assert_eq!(payload.defines.len(), 1);
        assert!(payload.scope_lock.is_some());
        assert_eq!(payload.guardrail_reports.len(), 1);
        assert_eq!(payload.relations.len(), 1);
    }
}
