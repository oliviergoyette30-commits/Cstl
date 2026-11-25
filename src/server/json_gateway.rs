//! src/server/json_gateway.rs -- entree JSON du serveur (2026-11-25).
//!
//! Pourquoi : les mesures (scripts/format_vs_server_benchmark.py) montrent
//! que l'avantage de CSTL face au JSON vient du SERVEUR (signatures, copie
//! qui fait foi, politique, autorisation signee, audit), pas du format. Un
//! agent qui parle JSON doit donc pouvoir en profiter sans apprendre le
//! format texte.
//!
//! Comment : `POST /v1/message` (API REST) recoit un message JSON
//!
//! ```json
//! {"version": "v5.0.0", "mode": "A",
//!  "meta": {"encoder": "a1", "produced_by": "a1", "public_key": "..."},
//!  "intent": {"purpose": "action_request", "sender": "a1", "receiver": "server",
//!             "action": "wipe_db", "request_id": "r1", "signature": "..."},
//!  "relations": [{"type": "STATE", "subject": "x", "object": "y"}]}
//! ```
//!
//! le traduit en texte CSTL EXACT, l'envoie au listener TCP local, et rend la
//! reponse en JSON. Le traitement est celui de n'importe quel message : rien
//! n'est contourne, rien n'est duplique.
//!
//! Signature : elle porte sur la forme canonique du message lu
//! (`audit::signing_bytes`), pas sur sa syntaxe. Un client signe donc
//! directement ce dictionnaire JSON avec `cstl_signing.signing_bytes` (Python)
//! ou un port equivalent : meme octets, meme signature.
//!
//! Rien n'est devine : une valeur que le texte CSTL ne peut pas porter a
//! l'identique (guillemet, retour a la ligne, espaces en bord, `---END---`)
//! est REFUSEE avec la raison, au lieu d'etre abimee -- sinon la signature
//! echouerait sans que personne comprenne pourquoi. Les valeurs doivent etre
//! des chaines : un nombre JSON n'a pas une seule ecriture texte (1.0, 1, 1e0)
//! et la signature en dependrait.
//!
//! Limites v1 : le listener TCP doit etre en clair (pas de TLS) ; toutes les
//! requetes passent par 127.0.0.1, donc partagent la meme limite de debit
//! par IP ; l'API REST n'ecoute que sur 127.0.0.1.

use serde_json::{json, Map, Value};

/// Une cle d'attribut portable sur le fil.
fn check_key(where_: &str, k: &str) -> Result<(), String> {
    if k.is_empty() || k.chars().any(|c| c.is_whitespace() || matches!(c, '=' | ',' | '[' | ']' | '"')) {
        return Err(format!("{where_} : cle invalide {k:?} (ni vide, ni espace, ni = , [ ] \")"));
    }
    Ok(())
}

/// Une valeur portable sur le fil, citee si besoin.
fn wire_value(where_: &str, k: &str, v: &Value) -> Result<String, String> {
    let s = match v {
        Value::String(s) => s.as_str(),
        _ => return Err(format!("{where_}.{k} : la valeur doit etre une chaine JSON (recu {v})")),
    };
    if s.contains('"') {
        return Err(format!("{where_}.{k} : guillemet \" non transportable sur le fil CSTL"));
    }
    if s.contains('\n') || s.contains('\r') {
        return Err(format!("{where_}.{k} : retour a la ligne non transportable sur le fil CSTL"));
    }
    if s.trim() != s {
        return Err(format!("{where_}.{k} : espaces en debut ou fin de valeur non transportables"));
    }
    if s.contains("---END---") {
        return Err(format!("{where_}.{k} : la valeur contient le marqueur ---END---"));
    }
    Ok(if s.contains(',') || s.contains('[') || s.contains(']') { format!("\"{s}\"") } else { s.to_string() })
}

fn wire_block(name: &str, where_: &str, obj: &Value) -> Result<String, String> {
    let map = obj.as_object().ok_or_else(|| format!("{where_} : un objet JSON est attendu"))?;
    let mut parts = Vec::with_capacity(map.len());
    for (k, v) in map {
        check_key(where_, k)?;
        parts.push(format!("{k}={}", wire_value(where_, k, v)?));
    }
    Ok(format!("{name} [{}]\n", parts.join(", ")))
}

/// Message JSON -> texte CSTL. Les champs inconnus sont refuses (une faute
/// de frappe ne doit pas disparaitre en silence).
pub fn json_to_wire(msg: &Value) -> Result<String, String> {
    let obj = msg.as_object().ok_or("le message doit etre un objet JSON")?;
    for k in obj.keys() {
        if !matches!(k.as_str(), "version" | "mode" | "meta" | "intent" | "relations") {
            return Err(format!("champ inconnu {k:?} (attendus : version, mode, meta, intent, relations)"));
        }
    }
    let version = obj.get("version").and_then(Value::as_str).unwrap_or("v5.0.0");
    let mode = obj.get("mode").and_then(Value::as_str).unwrap_or("A");
    let version_ok = version
        .strip_prefix('v')
        .map(|r| {
            let n: Vec<&str> = r.split('.').collect();
            (2..=3).contains(&n.len()) && n.iter().all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))
        })
        .unwrap_or(false);
    if !version_ok {
        return Err(format!("version invalide {version:?} (attendu vX.Y.Z)"));
    }
    if !matches!(mode, "A" | "B" | "C") {
        return Err(format!("mode invalide {mode:?}"));
    }
    let meta = obj.get("meta").ok_or("champ meta manquant")?;
    let intent = obj.get("intent").ok_or("champ intent manquant")?;
    let mut out = format!("#!CSTL {version} MODE={mode}\n");
    out += &wire_block("META", "meta", meta)?;
    out += &wire_block("INTENT_PAYLOAD", "intent", intent)?;
    if let Some(rels) = obj.get("relations") {
        let rels = rels.as_array().ok_or("relations : une liste est attendue")?;
        for (i, r) in rels.iter().enumerate() {
            out += &wire_block("RELATION", &format!("relations[{i}]"), r)?;
        }
    }
    out += "---END---\n";
    Ok(out)
}

/// Une ligne `NOM [k=v, ...]` -> (NOM, champs). None si la ligne n'a pas
/// cette forme (elle reste alors dans `lines`, telle quelle).
fn parse_line(line: &str) -> Option<(String, Map<String, Value>)> {
    let open = line.find(" [")?;
    let name = &line[..open];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') || !line.ends_with(']') {
        return None;
    }
    let mut fields = Map::new();
    for part in super::parser::split_top_level_commas(&line[open + 2..line.len() - 1]) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=')?;
        fields.insert(k.trim().to_string(), Value::String(v.trim().trim_matches('"').to_string()));
    }
    Some((name.to_string(), fields))
}

/// Reponse CSTL -> JSON. `blocks` garde tous les blocs dans l'ordre ;
/// `meta` et `intent` sont les premiers blocs META et INTENT_PAYLOAD, pour
/// un acces direct ; `raw` est le texte exact recu (ce qui fait foi).
pub fn wire_to_json(text: &str) -> Value {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("").trim().to_string();
    let mut blocks = Vec::new();
    let mut other = Vec::new();
    let (mut meta, mut intent) = (Value::Null, Value::Null);
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() || line == "---END---" {
            continue;
        }
        match parse_line(line) {
            Some((name, fields)) => {
                if name == "META" && meta.is_null() {
                    meta = Value::Object(fields.clone());
                }
                if name == "INTENT_PAYLOAD" && intent.is_null() {
                    intent = Value::Object(fields.clone());
                }
                blocks.push(json!({"block": name, "fields": fields}));
            }
            None => other.push(Value::String(line.to_string())),
        }
    }
    json!({"header": header, "meta": meta, "intent": intent, "blocks": blocks, "lines": other, "raw": text})
}

/// Envoie un texte CSTL au listener TCP local et lit la reponse jusqu'a
/// `---END---`.
pub async fn relay(wire: &str, port: u16) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let fut = async {
        let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .map_err(|e| format!("serveur CSTL injoignable sur 127.0.0.1:{port} : {e}"))?;
        sock.write_all(wire.as_bytes()).await.map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = sock.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(9).any(|w| w == b"---END---") {
                break;
            }
            if buf.len() > 8 * 1024 * 1024 {
                return Err("reponse trop grande (> 8 Mio)".to_string());
            }
        }
        String::from_utf8(buf).map_err(|_| "reponse non UTF-8".to_string())
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), fut)
        .await
        .map_err(|_| "le serveur CSTL n'a pas repondu en 60 s".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{audit::signing_bytes, parser::parse_payload};

    fn sample() -> Value {
        json!({"version": "v5.0.0", "mode": "A",
               "meta": {"encoder": "a1", "produced_by": "a1"},
               "intent": {"purpose": "inform", "sender": "a1", "receiver": "server", "note": "a, b [c]"},
               "relations": [{"type": "STATE", "subject": "x", "object": "y=z"}]})
    }

    #[test]
    fn wire_text_parses_back_to_the_same_fields() {
        let p = parse_payload(&json_to_wire(&sample()).unwrap()).unwrap();
        assert_eq!(p.intent["note"], "a, b [c]");
        assert_eq!(p.relations[0]["object"], "y=z");
        assert_eq!(p.meta["encoder"], "a1");
    }

    #[test]
    fn signing_bytes_match_the_json_dictionary() {
        // Ce que signe un client JSON (cstl_signing.signing_bytes sur le
        // dictionnaire) = ce que le serveur verifie apres traduction.
        let p = parse_payload(&json_to_wire(&sample()).unwrap()).unwrap();
        let canon = String::from_utf8(signing_bytes(&p)).unwrap();
        assert_eq!(
            canon,
            "VERSION|v5.0.0\nMODE|A\nMETA|encoder=a1|produced_by=a1\nINTENT|note=a, b [c]|purpose=inform|receiver=server|sender=a1\nRELATIONS|object=y=z,subject=x,type=STATE"
        );
    }

    #[test]
    fn values_the_wire_cannot_carry_are_refused() {
        for bad in [json!("a\"b"), json!("a\nb"), json!(" a"), json!(3), json!("x---END---")] {
            let mut m = sample();
            m["intent"]["note"] = bad;
            assert!(json_to_wire(&m).is_err());
        }
        let mut m = sample();
        m["extra"] = json!(1);
        assert!(json_to_wire(&m).unwrap_err().contains("champ inconnu"));
        let mut m = sample();
        m["meta"] = json!({"a b": "x"});
        assert!(json_to_wire(&m).is_err());
    }

    #[test]
    fn response_becomes_json_blocks() {
        let r = "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, status=processed]\nINTENT_PAYLOAD [purpose=action_authorization, decision=denied, reason=\"a, b\"]\nAUDIT [hash=sha256:ab, seq=3]\n---END---\n";
        let j = wire_to_json(r);
        assert_eq!(j["intent"]["decision"], "denied");
        assert_eq!(j["intent"]["reason"], "a, b");
        assert_eq!(j["blocks"][2]["block"], "AUDIT");
        assert_eq!(j["raw"], r);
    }
}
