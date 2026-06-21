//! CSTL v4.9.3 — Security validation (Sessions #6 + #7)

/// Security scan result
pub struct SecurityReport {
    pub cleaned:  String,
    pub errors:   Vec<String>,
    pub warnings: Vec<String>,
}

/// Codepoints forbidden in any position (Session #6 Q2 + Session #7 Q1)
fn is_dangerous(ch: char) -> bool {
    let cp = ch as u32;
    matches!(cp,
        // Zero-width chars (S6 Q2)
        0x200B | 0x200C | 0x200D | 0x2060..=0x2064 | 0xFEFF |
        // Bidi controls (S7 Q1)
        0x202A..=0x202E | 0x2066..=0x2069 |
        // C1 controls
        0x0080..=0x009F
    )
}

/// Q2: Strip zero-width and bidi control characters
fn strip_dangerous(text: &str) -> (String, Vec<String>) {
    let mut cleaned = String::with_capacity(text.len());
    let mut warns   = Vec::new();
    let mut col     = 0usize;

    for ch in text.chars() {
        if is_dangerous(ch) {
            warns.push(format!(
                "SEC_Q2: stripped dangerous U+{:04X} at col {} (audit: \\u{:04X})",
                ch as u32, col, ch as u32
            ));
        } else {
            cleaned.push(ch);
            col += 1;
        }
    }
    (cleaned, warns)
}

/// Q1: Detect non-ASCII characters in keyword-like positions (line-start words)
/// Pure ASCII is enforced for structural keywords (Gemini Session #6 option B wins)
fn check_homoglyphs(text: &str) -> Vec<String> {
    let mut warns = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let word: String = line.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if word.is_empty() { continue; }
        let non_ascii: Vec<char> = word.chars().filter(|c| *c as u32 > 0x7F).collect();
        if !non_ascii.is_empty() {
            let cps: Vec<String> = non_ascii.iter()
                .map(|c| format!("U+{:04X}", *c as u32))
                .collect();
            warns.push(format!(
                "SEC_Q1: non-ASCII in keyword position {:?} at line {} — \
                 homoglyph attack? codepoints={}",
                word, line_no + 1, cps.join(" ")
            ));
        }
    }
    warns
}

/// Q4: Detect nested META blocks (injection attempt)
fn check_nested_meta(text: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        // Nested META = line that starts with whitespace + META (indented)
        if line.starts_with(|c: char| c == ' ' || c == '\t')
            && trimmed.starts_with("META")
            && (trimmed.len() == 4 || !trimmed.chars().nth(4).unwrap_or(' ').is_alphanumeric())
        {
            errors.push(format!(
                "SEC_Q4: nested META block at line {} — injection attempt", i + 1
            ));
        }
        // META keyword inside a value assignment
        if let Some(eq_pos) = line.find('=') {
            let after = &line[eq_pos..];
            if after.contains("META [") || after.contains("META[") {
                errors.push(format!(
                    "SEC_Q4: META keyword with block in string value at line {}", i + 1
                ));
            }
        }
    }
    errors
}

/// Q5: Check bracket nesting depth
fn check_nesting_depth(text: &str, max_depth: usize) -> Vec<String> {
    let mut errors = Vec::new();
    let mut depth  = 0usize;

    for (i, ch) in text.chars().enumerate() {
        if ch == '[' {
            depth += 1;
            if depth > max_depth {
                errors.push(format!(
                    "SEC: nesting depth {} exceeds max {} at char {}", depth, max_depth, i
                ));
                break;
            }
        } else if ch == ']' {
            depth = depth.saturating_sub(1);
        }
    }
    errors
}

/// Full security pipeline (Sessions #6 + #7)
pub fn security_scan(text: &str) -> SecurityReport {
    let mut errors   = Vec::new();
    let mut warnings = Vec::new();

    // Q2: Strip dangerous codepoints
    let (cleaned, zw_warns) = strip_dangerous(text);
    warnings.extend(zw_warns);

    // Q1: Homoglyph detection
    warnings.extend(check_homoglyphs(&cleaned));

    // Q4: Nested META injection
    errors.extend(check_nested_meta(&cleaned));

    // Q5: Nesting depth (max 32 — ratified Session #6)
    errors.extend(check_nesting_depth(&cleaned, 32));

    SecurityReport { cleaned, errors, warnings }
}
