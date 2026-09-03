//! src/emergence.rs — Port Rust de `RevisionOrchestrator` (cstl_adn_store.py,
//! jamais porte avant cette passe). Detecte automatiquement les revisions de
//! position entre les runs SOLO de chaque agent et le run TRIPARTITE final --
//! a partir de payloads DEJA stockes dans l'adn_store.
//!
//! Portee honnete, fidele au design documente du projet
//! (CSTL_v4_9_1_REFERENCE_DEMO: "You (the human) control the dialogue flow"):
//! ce module ne fait AUCUN appel API externe. Un humain envoie le meme sujet a
//! plusieurs LLM (chacun dans son interface), relaie les reponses entre eux, et
//! chaque reponse arrive au serveur CSTL comme un payload normal -- exactement
//! comme n'importe quel autre agent deja cable dans ce serveur. Ce module lit
//! ces payloads deja stockes et remplit emergence_proofs, rien de plus.
//!
//! Ce que ca prouve: un agent a change sa position apres avoir vu ses pairs.
//! Ce que ca ne prouve PAS: qu'aucun agent seul n'aurait pu arriver au meme
//! resultat -- c'est une preuve de revision, pas une preuve d'emergence
//! formelle (meme honnetete que le commentaire original du docstring Python
//! source: "C'est de la preuve de revision, pas de preuve d'emergence
//! formelle").

use crate::adn_store::AdnStore;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RevisionReport {
    pub agent: String,
    pub question: String,
    pub solo_decision: String,
    pub trio_decision: String,
    pub revised: bool,
    pub delta_sigma: f64,
    pub proof_id: Option<i64>,
}

/// Extrait la "decision" d'un payload CSTL brut. Port fidele des deux styles
/// que reconnaissait la version Python: "DECISION: <valeur>" jusqu'a la fin de
/// ligne ou un `[`, ou un bloc "DECISION [ ... ]".
///
/// Corrige une trouvaille majeure de l'audit multi-angle (2026-09-03): la
/// forme bloc tronquait auparavant a 80 caracteres AVANT la comparaison dans
/// decisions_differ() -- deux decisions genuinement differentes partageant
/// un prefixe de 80+ caracteres identiques etaient jugees IDENTIQUES,
/// manquant silencieusement une vraie revision de position. La troncature
/// n'a plus lieu d'etre: rien dans le format ne borne la longueur d'une
/// decision, et decisions_differ() ne fait qu'une comparaison textuelle,
/// pas d'affichage necessitant une limite.
fn extract_decision(payload: &str) -> Option<String> {
    if let Some(idx) = payload.find("DECISION:") {
        let rest = &payload[idx + "DECISION:".len()..];
        let end = rest.find(['\n', '[']).unwrap_or(rest.len());
        let value = rest[..end].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    if let Some(idx) = payload.find("DECISION") {
        let rest = payload[idx + "DECISION".len()..].trim_start();
        if let Some(rest) = rest.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                let value = rest[..end].trim().to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Comparaison textuelle naive (trim + lowercase), pas semantique -- comme
/// l'original Python. "Option B" et "option_b" sont consideres identiques;
/// une reformulation semantiquement equivalente mais textuellement differente
/// serait, elle, comptee a tort comme une revision. Limite connue, pas cachee.
fn decisions_differ(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() != b.trim().to_lowercase()
}

/// Compare la decision SOLO de chaque agent (deja stockee dans l'adn_store sous
/// son propre hash) a la decision TRIPARTITE finale (`trio_hash`). Pour chaque
/// agent dont la decision differe, enregistre automatiquement une entree dans
/// `emergence_proofs` via `AdnStore::put_emergence_proof`. Aucun hash inconnu
/// (trio ou solo) ne fait echouer l'appel -- il est simplement ignore, comme
/// dans l'original.
pub fn detect_revisions(
    store: &AdnStore,
    trio_hash: &str,
    solo_hashes: &HashMap<String, String>,
    question: &str,
) -> Result<Vec<RevisionReport>, rusqlite::Error> {
    let trio_entry = match store.get(trio_hash)? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    let trio_decision = extract_decision(&trio_entry.payload).unwrap_or_default();
    let trio_sigma = trio_entry.sigma;

    let mut all_solo_decisions: HashMap<String, String> = HashMap::new();
    let mut solo_entries: Vec<(String, String, f64)> = Vec::new();
    for (agent, hash) in solo_hashes {
        if let Some(entry) = store.get(hash)? {
            let decision = extract_decision(&entry.payload).unwrap_or_default();
            all_solo_decisions.insert(agent.clone(), decision.clone());
            solo_entries.push((agent.clone(), decision, entry.sigma));
        }
    }
    let all_solo_json = serde_json::to_string(&all_solo_decisions).unwrap_or_default();
    let effective_question = if question.is_empty() {
        trio_entry.conversation_id.clone().unwrap_or_default()
    } else {
        question.to_string()
    };

    let mut reports = Vec::new();
    for (agent, solo_decision, solo_sigma) in solo_entries {
        let revised = decisions_differ(&solo_decision, &trio_decision);
        let delta_sigma = trio_sigma - solo_sigma;
        let mut proof_id = None;
        if revised {
            proof_id = Some(store.put_emergence_proof(
                &effective_question,
                &all_solo_json,
                &trio_decision,
                Some(&agent),
                Some(&trio_decision),
                Some(delta_sigma),
            )?);
        }
        reports.push(RevisionReport {
            agent,
            question: effective_question.clone(),
            solo_decision,
            trio_decision: trio_decision.clone(),
            revised,
            delta_sigma,
            proof_id,
        });
    }

    Ok(reports)
}

/// Encode un solo_hashes de wire format (`Agent:hash;Agent:hash`) en HashMap.
/// Utilise `;` entre agents et le PREMIER `:` pour separer le nom du hash --
/// le hash lui-meme commence par "sha256:" et contient donc un `:`, d'ou le
/// split limite a 2. Format choisi pour tenir dans un champ INTENT_PAYLOAD:
/// la grammaire du wire format separe les champs sur `,` et `key=value` sur
/// le premier `=`, donc ni `:` ni `;` ne posent de probleme.
pub fn parse_solo_hashes(field: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in field.split(';') {
        if let Some((agent, hash)) = pair.split_once(':') {
            let agent = agent.trim();
            let hash = hash.trim();
            if !agent.is_empty() && !hash.is_empty() {
                out.insert(agent.to_string(), hash.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_decision_colon_style() {
        let payload = "META [x=y]\nDECISION: option_B_ratified [sigma=0.91]\n---END---";
        assert_eq!(extract_decision(payload).as_deref(), Some("option_B_ratified"));
    }

    #[test]
    fn test_extract_decision_block_style() {
        let payload = "META [x=y]\nDECISION [option_B_ratified]\n---END---";
        assert_eq!(extract_decision(payload).as_deref(), Some("option_B_ratified"));
    }

    #[test]
    fn test_extract_decision_absent() {
        assert_eq!(extract_decision("META [x=y]\n---END---"), None);
    }

    #[test]
    fn test_extract_decision_block_style_not_truncated_past_80_chars() {
        let long_value = format!("{}AAAA", "X".repeat(80));
        let payload = format!("META [x=y]\nDECISION [{}]\n---END---", long_value);
        assert_eq!(extract_decision(&payload).as_deref(), Some(long_value.as_str()));
    }

    #[test]
    fn test_detect_revisions_catches_a_change_hidden_past_80_chars() {
        // Avant ce fix: deux decisions partageant les 80 premiers caracteres
        // mais differant apres (AAAA vs BBBB) etaient tronquees a 80 chars
        // AVANT comparaison -> jugees identiques -> revision manquee.
        let store = AdnStore::open(":memory:").unwrap();
        let shared_prefix = "X".repeat(80);
        let solo_decision = format!("DECISION [{}AAAA]", shared_prefix);
        let trio_decision = format!("DECISION [{}BBBB]", shared_prefix);
        store.put("solo_x", &solo_decision, None, None, 0.80, None, None, None).unwrap();
        store.put("trio_x", &trio_decision, None, None, 0.90, None, None, None).unwrap();

        let mut solo_hashes = HashMap::new();
        solo_hashes.insert("Agent_X".to_string(), "solo_x".to_string());

        let reports = detect_revisions(&store, "trio_x", &solo_hashes, "Q_long").unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].revised, "la revision cachee apres 80 caracteres doit etre detectee");
    }

    #[test]
    fn test_parse_solo_hashes_roundtrip() {
        let parsed = parse_solo_hashes("Agent_CLAUDE:sha256:abc123;Agent_GPT:sha256:def456");
        assert_eq!(parsed.get("Agent_CLAUDE").map(String::as_str), Some("sha256:abc123"));
        assert_eq!(parsed.get("Agent_GPT").map(String::as_str), Some("sha256:def456"));
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn test_detect_revisions_flags_the_agent_that_changed() {
        let store = AdnStore::open(":memory:").unwrap();
        store.put("solo_claude", "DECISION: option_C [sigma=0.82]", None, None, 0.82, None, None, None).unwrap();
        store.put("solo_gpt", "DECISION: option_D [sigma=0.84]", None, None, 0.84, None, None, None).unwrap();
        store.put("trio_final", "DECISION: option_D [sigma=0.91]", None, None, 0.91, None, None, None).unwrap();

        let mut solo_hashes = HashMap::new();
        solo_hashes.insert("Agent_CLAUDE".to_string(), "solo_claude".to_string());
        solo_hashes.insert("Agent_GPT".to_string(), "solo_gpt".to_string());

        let reports = detect_revisions(&store, "trio_final", &solo_hashes, "Q1").unwrap();
        assert_eq!(reports.len(), 2);

        let claude_report = reports.iter().find(|r| r.agent == "Agent_CLAUDE").unwrap();
        assert!(claude_report.revised);
        assert!(claude_report.proof_id.is_some());
        assert!((claude_report.delta_sigma - 0.09).abs() < 1e-9);

        let gpt_report = reports.iter().find(|r| r.agent == "Agent_GPT").unwrap();
        assert!(!gpt_report.revised);
        assert!(gpt_report.proof_id.is_none());

        let proofs = store.get_emergence_proofs().unwrap();
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0].position_changed_by.as_deref(), Some("Agent_CLAUDE"));
    }

    #[test]
    fn test_detect_revisions_unknown_trio_hash_returns_empty() {
        let store = AdnStore::open(":memory:").unwrap();
        let solo_hashes = HashMap::new();
        let reports = detect_revisions(&store, "does_not_exist", &solo_hashes, "Q1").unwrap();
        assert!(reports.is_empty());
    }
}
