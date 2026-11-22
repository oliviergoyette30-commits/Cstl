//! src/server/schema.rs -- TYPAGE des relations (2026-11-22).
//!
//! Un auteur de politique declare la structure d'un type de relation :
//!
//! ```text
//! RELATION [type=SCHEMA, subject=ORDER, object=v1, amount=num, date=date, paid=bool]
//! ```
//!
//! Le type decrit suit les regles habituelles du serveur : en minuscules
//! (`order_record`, comme les predicats `located_in`), ou un operateur officiel
//! ou de domaine en MAJUSCULES (`ORDER` existe en supply_chain).
//!
//! Ensuite, toute relation `type=ORDER` dont un attribut declare n'a pas le
//! bon type est REFUSEE (le message entier, rien n'est stocke) :
//! `INTENT_PAYLOAD [purpose=schema_rejected, detail=...]`. Un attribut absent
//! reste permis ; un attribut non declare aussi. Le schema le plus recent d'un
//! type remplace les precedents. Avec `CSTL_POLICY_AUTHORS`, seuls les schemas
//! d'un auteur de politique comptent (comme les regles, roles et faits).
//!
//! Pourquoi : recommandation tiree d'une evaluation externe du CSTL compact
//! (« typage strict des relations et des champs ») -- avant, un montant
//! « abc » ou une date « hier » etaient stockes tels quels, et seule la couche
//! 9 (conditions des regles) verifiait ses propres montants et dates.

use std::collections::HashMap;

/// Types reconnus.
pub const TYPES: [&str; 5] = ["num", "int", "date", "bool", "str"];

/// La valeur `v` est-elle du type `ty` ? (`ty` inconnu : faux.)
pub fn check_value(ty: &str, v: &str) -> bool {
    let v = v.trim();
    match ty {
        "num" => v.parse::<f64>().is_ok_and(f64::is_finite),
        "int" => v.parse::<i64>().is_ok(),
        "date" => is_date(v),
        "bool" => matches!(v, "true" | "false" | "1" | "0"),
        "str" => true,
        _ => false,
    }
}

/// AAAA-MM-JJ, ou date et heure ISO 8601 (RFC 3339).
fn is_date(v: &str) -> bool {
    chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok() || chrono::DateTime::parse_from_rfc3339(v).is_ok()
}

const RESERVED: [&str; 3] = ["type", "subject", "object"];

/// Colonnes typees d'une declaration SCHEMA : (attribut, type). Erreur si un
/// type n'est pas reconnu.
pub fn columns(attrs: &HashMap<String, String>) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (k, ty) in attrs {
        if RESERVED.contains(&k.as_str()) {
            continue;
        }
        if !TYPES.contains(&ty.as_str()) {
            return Err(format!("type inconnu {ty} pour {k} (types : {})", TYPES.join(", ")));
        }
        out.push((k.clone(), ty.clone()));
    }
    out.sort();
    Ok(out)
}

/// Verifie une relation contre les schemas connus (type de relation ->
/// colonnes). `None` si elle est conforme (ou sans schema).
pub fn violation(relation: &HashMap<String, String>, schemas: &HashMap<String, Vec<(String, String)>>) -> Option<String> {
    let ty = relation.get("type")?;
    let cols = schemas.get(ty)?;
    for (k, want) in cols {
        if let Some(v) = relation.get(k) {
            if !check_value(want, v) {
                return Some(format!("{ty}.{k}={v} n'est pas de type {want}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn values_are_checked_by_type() {
        assert!(check_value("num", "450.5") && check_value("num", "-3") && !check_value("num", "abc") && !check_value("num", "NaN"));
        assert!(check_value("int", "42") && !check_value("int", "4.2"));
        assert!(check_value("date", "2026-11-22") && check_value("date", "2026-11-22T10:00:00Z") && !check_value("date", "hier"));
        assert!(!check_value("date", "2026-02-30"));
        assert!(check_value("bool", "true") && check_value("bool", "0") && !check_value("bool", "oui"));
        assert!(check_value("str", "n'importe quoi") && !check_value("money", "1"));
    }

    #[test]
    fn relations_are_checked_against_their_schema() {
        let cols = columns(&m(&[("type", "SCHEMA"), ("subject", "ORDER"), ("object", "v1"), ("amount", "num"), ("date", "date")])).unwrap();
        let schemas: HashMap<String, Vec<(String, String)>> = [("ORDER".to_string(), cols)].into();
        assert_eq!(violation(&m(&[("type", "ORDER"), ("amount", "450"), ("date", "2026-11-04")]), &schemas), None);
        assert_eq!(violation(&m(&[("type", "ORDER"), ("subject", "o1")]), &schemas), None, "attribut absent : permis");
        assert!(violation(&m(&[("type", "ORDER"), ("amount", "beaucoup")]), &schemas).unwrap().contains("ORDER.amount"));
        assert_eq!(violation(&m(&[("type", "EVENT"), ("amount", "x")]), &schemas), None, "type sans schema");
        assert!(columns(&m(&[("amount", "money")])).is_err());
    }
}
