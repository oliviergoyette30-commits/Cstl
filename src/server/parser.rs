/// CSTL Payload Parser
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CstlPayload {
    pub version: String,
    pub mode: String,
    pub meta: HashMap<String, String>,
    pub intent: HashMap<String, String>,
    pub relations: Vec<HashMap<String, String>>,
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

pub fn parse_payload(raw: &str) -> Result<CstlPayload, ParseError> {
    let mut payload = CstlPayload {
        version: String::new(),
        mode: String::new(),
        meta: HashMap::new(),
        intent: HashMap::new(),
        relations: Vec::new(),
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

        if is_meta || is_intent || is_relation {
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
                    _ => {}
                }
            }

            // Start new block
            in_block = true;
            block_name = if is_meta { "META" } else if is_intent { "INTENT_PAYLOAD" } else { "RELATION" }.to_string();
            current_block = line.to_string();

            // Check if block ends on same line
            if line.contains(']') {
                match block_name.as_str() {
                    "META" => payload.meta = parse_block(&current_block)?,
                    "INTENT_PAYLOAD" => payload.intent = parse_block(&current_block)?,
                    "RELATION" => {
                        if let Ok(relation) = parse_block(&current_block) {
                            payload.relations.push(relation);
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
                _ => {}
            }
            in_block = false;
            current_block.clear();
        } else if in_block {
            current_block.push('\n');
            current_block.push_str(line);
        }
    }

    eprintln!("[Parser] Parsed CSTL v{} MODE={}", payload.version, payload.mode);
    eprintln!("[Parser] META: {} fields", payload.meta.len());
    eprintln!("[Parser] INTENT: {} fields", payload.intent.len());
    eprintln!("[Parser] RELATIONS: {} blocks", payload.relations.len());

    Ok(payload)
}

fn parse_block(block: &str) -> Result<HashMap<String, String>, ParseError> {
    let mut map = HashMap::new();

    let start = block.find('[').ok_or_else(|| ParseError::MalformedBlock("Missing [".to_string()))?;
    let end = block.rfind(']').ok_or_else(|| ParseError::MalformedBlock("Missing ]".to_string()))?;

    let content = &block[start + 1..end];

    for pair in content.split(',') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().trim_matches('"').to_string());
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
}
