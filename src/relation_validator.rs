//! CSTL v5.0.0 — Relation-level semantic validation
//! W601: MUTUAL deprecated
//! W602: CONTRADICTS anti-symmetry
//! W603: ENTAILS transitive closure
//! W604: KNOWS sigma calibration
//! W605: DOUBTS sigma calibration
//! E701: Temporal contradiction (BEFORE + AFTER same pair)

pub struct RelationValidation {
    pub errors:   Vec<String>,
    pub warnings: Vec<String>,
}

fn check_mutual_deprecated(lines: &[&str]) -> Vec<String> {
    let mut warnings = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.contains(" MUTUAL ") || t.ends_with(" MUTUAL") || t.starts_with("MUTUAL ") {
            warnings.push(format!(
                "W601: MUTUAL operator at line {} is DEPRECATED in v5.0. \
                 Use: EQUALS | POSSESSES | RESEMBLES | CO_LOCATES | OPPOSES | COMPARES. \
                 See CSTL_SPEC_v5_0.md §8.2",
                i + 1
            ));
        }
    }
    warnings
}

fn check_contradicts_symmetry(lines: &[&str]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in lines {
        let t = line.trim();
        if t.contains("CONTRADICTS") {
            let parts: Vec<&str> = t.splitn(3, "CONTRADICTS").collect();
            if parts.len() >= 2 {
                let src = parts[0].trim().trim_matches(|c| c == '(' || c == ')').trim().to_string();
                let tgt = parts[1].trim().split_whitespace().next()
                    .unwrap_or("").trim_matches(|c| c == '(' || c == ')').trim().to_string();
                if !src.is_empty() && !tgt.is_empty() {
                    if pairs.iter().any(|(a, b)| a == &tgt && b == &src) {
                        warnings.push(format!(
                            "W602: ({}) CONTRADICTS ({}) — reverse already declared; \
                             CONTRADICTS is anti-symmetric, one direction sufficient",
                            src, tgt
                        ));
                    }
                    pairs.push((src, tgt));
                }
            }
        }
    }
    warnings
}

fn check_entails_transitivity(lines: &[&str]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut entails: Vec<(String, String)> = Vec::new();
    for line in lines {
        let t = line.trim();
        if t.contains("ENTAILS") {
            let parts: Vec<&str> = t.splitn(3, "ENTAILS").collect();
            if parts.len() >= 2 {
                let src = parts[0].trim().trim_matches(|c| c == '(' || c == ')').trim().to_string();
                let tgt = parts[1].trim().split_whitespace().next()
                    .unwrap_or("").trim_matches(|c| c == '(' || c == ')').trim().to_string();
                if !src.is_empty() && !tgt.is_empty() {
                    entails.push((src, tgt));
                }
            }
        }
    }
    for (a, b) in &entails {
        for (b2, c) in &entails {
            if b == b2 && a != c {
                let declared = entails.iter().any(|(x, y)| x == a && y == c);
                if !declared {
                    warnings.push(format!(
                        "W603: ENTAILS closure: ({})→({})→({}) but ({})→({}) not declared (recommended)",
                        a, b, c, a, c
                    ));
                }
            }
        }
    }
    warnings
}

fn check_temporal_consistency(lines: &[&str]) -> Vec<String> {
    let mut issues = Vec::new();
    let mut before_pairs: Vec<(String, String)> = Vec::new();
    let mut after_pairs:  Vec<(String, String)> = Vec::new();
    for line in lines {
        let t = line.trim();
        if t.contains(" BEFORE ") {
            let parts: Vec<&str> = t.splitn(3, " BEFORE ").collect();
            if parts.len() >= 2 {
                let src = parts[0].trim().trim_matches(|c| c == '(' || c == ')').trim().to_string();
                let tgt = parts[1].trim().split_whitespace().next()
                    .unwrap_or("").trim_matches(|c| c == '(' || c == ')').trim().to_string();
                if !src.is_empty() && !tgt.is_empty() { before_pairs.push((src, tgt)); }
            }
        }
        if t.contains(" AFTER ") {
            let parts: Vec<&str> = t.splitn(3, " AFTER ").collect();
            if parts.len() >= 2 {
                let src = parts[0].trim().trim_matches(|c| c == '(' || c == ')').trim().to_string();
                let tgt = parts[1].trim().split_whitespace().next()
                    .unwrap_or("").trim_matches(|c| c == '(' || c == ')').trim().to_string();
                if !src.is_empty() && !tgt.is_empty() { after_pairs.push((src, tgt)); }
            }
        }
    }
    for (a, b) in &before_pairs {
        if after_pairs.iter().any(|(x, y)| x == a && y == b) {
            issues.push(format!(
                "E701: ({}) declared both BEFORE and AFTER ({}) — temporal contradiction",
                a, b
            ));
        }
    }
    issues
}

fn check_epistemic_sigma(lines: &[&str]) -> Vec<String> {
    let mut warnings = Vec::new();
    const EPISTEMIC: &[&str] = &["KNOWS", "BELIEVES", "ASSUMES", "DOUBTS"];
    for line in lines {
        let t = line.trim();
        let op = EPISTEMIC.iter().find(|&&op| t.contains(op));
        if let Some(&op) = op {
            let sigma_val: Option<f64> = t.find("sigma=")
                .or_else(|| t.find("σ="))
                .and_then(|pos| {
                    let rest = &t[pos..];
                    let val_str = rest
                        .trim_start_matches("sigma=")
                        .trim_start_matches("σ=")
                        .split(|c: char| !c.is_ascii_digit() && c != '.')
                        .next()?;
                    val_str.parse().ok()
                });
            if let Some(s) = sigma_val {
                match op {
                    "KNOWS" if s < 0.8 => { warnings.push(format!(
                        "W604: KNOWS with sigma={:.2} — KNOWS implies factual certainty; \
                         consider BELIEVES or ASSUMES for sigma < 0.8", s)); }
                    "DOUBTS" if s > 0.5 => { warnings.push(format!(
                        "W605: DOUBTS with sigma={:.2} — DOUBTS implies low confidence; \
                         consider BELIEVES for sigma > 0.5", s)); }
                    _ => {}
                }
            }
        }
    }
    warnings
}

pub fn validate_relations(text: &str) -> RelationValidation {
    let lines: Vec<&str> = text.lines().collect();
    let mut errors   = Vec::new();
    let mut warnings = Vec::new();
    warnings.extend(check_mutual_deprecated(&lines));
    warnings.extend(check_contradicts_symmetry(&lines));
    warnings.extend(check_entails_transitivity(&lines));
    for issue in check_temporal_consistency(&lines) {
        if issue.starts_with("E7") { errors.push(issue); } else { warnings.push(issue); }
    }
    warnings.extend(check_epistemic_sigma(&lines));
    RelationValidation { errors, warnings }
}

pub fn is_v5_operator(op: &str) -> bool {
    matches!(op,
        "ENTAILS" | "CONTRADICTS" |
        "BELIEVES" | "KNOWS" | "ASSUMES" | "DOUBTS" |
        "BEFORE" | "AFTER" | "DURING" |
        "EQUALS" | "POSSESSES" | "RESEMBLES" |
        "CO_LOCATES" | "OPPOSES" | "COMPARES" |
        "MUTUAL"
    )
}
