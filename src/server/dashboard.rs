//! Tableau de bord (2026-11-25) : `GET /v1/dashboard` (page HTML, rafraichie
//! toutes les 10 s) et `GET /v1/dashboard.json`, sur le port REST (127.0.0.1
//! seulement). Lecture seule : decisions recentes, refus par raison, arrets
//! d'urgence, cles revoquees, accords en attente.

use crate::adn_store::AdnStore;
use serde_json::{json, Value};

/// Etat actuel, lu dans la base.
pub fn snapshot(store: &AdnStore, operator_stop: Option<String>) -> Value {
    let now = chrono::Utc::now().timestamp();
    let (allowed_24h, denied_24h) = store.decision_counts(now - 86_400).unwrap_or_default();
    let decisions: Vec<Value> = store.recent_decisions(100).unwrap_or_default().into_iter()
        .map(|(at, agent, action, amount, allowed, reason, rule, audit_hash)| json!({
            "at": at, "agent": agent, "action": action, "amount": amount, "allowed": allowed,
            "reason": reason, "rule": rule, "audit_hash": audit_hash,
        }))
        .collect();
    let stops: Vec<Value> = store.emergency_stops_dated().unwrap_or_default().into_iter()
        .map(|(scope, by, reason, at)| json!({"scope": scope, "set_by": by, "reason": reason, "at": at}))
        .collect();
    let revoked: Vec<Value> = store.revoked_key_list(50).unwrap_or_default().into_iter()
        .map(|(key, agent, by, reason, at)| json!({"public_key": key, "agent": agent, "revoked_by": by, "reason": reason, "at": at}))
        .collect();
    let approvals = super::approval::ApprovalPolicy::from_env();
    let pending: Vec<Value> = store.pending_approvals(now - approvals.ttl_secs, 100).unwrap_or_default().into_iter()
        .map(|r| json!({"approval_id": r.id, "agent": r.agent, "action": r.action, "amount": r.amount,
                        "requested_at": r.requested_at, "expires_at": r.requested_at + approvals.ttl_secs}))
        .collect();
    json!({
        "generated_at": now,
        "operator_stop": operator_stop,
        "last_24h": {
            "allowed": allowed_24h,
            "denied": denied_24h.iter().map(|(_, n)| n).sum::<i64>(),
            "denied_by_reason": denied_24h.iter().map(|(r, n)| json!({"reason": r, "count": n})).collect::<Vec<_>>(),
        },
        "emergency_stops": stops,
        "revoked_keys": revoked,
        "pending_approvals": pending,
        "recent_decisions": decisions,
    })
}

fn esc(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn when(v: &Value) -> String {
    v.as_i64()
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_default()
}

fn table(title: &str, empty: &str, head: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut out = format!("<section><h2>{title} <span class=n>{}</span></h2>", rows.len());
    if rows.is_empty() {
        out.push_str(&format!("<p class=empty>{empty}</p></section>"));
        return out;
    }
    out.push_str("<div class=scroll><table><thead><tr>");
    for h in head {
        out.push_str(&format!("<th>{h}</th>"));
    }
    out.push_str("</tr></thead><tbody>");
    for r in rows {
        out.push_str("<tr>");
        for c in r {
            out.push_str(&format!("<td>{c}</td>"));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div></section>");
    out
}

/// Page HTML autonome (aucune ressource externe).
pub fn render_html(v: &Value) -> String {
    let arr = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    let mut body = String::new();
    let stop_banner = match v["operator_stop"].as_str() {
        Some(r) => format!("<div class='banner stop'>Arrêt de l'opérateur en vigueur : {}</div>", esc(&json!(r))),
        None => String::new(),
    };
    body.push_str(&stop_banner);
    let day = &v["last_24h"];
    body.push_str(&format!(
        "<div class=tiles><div class=tile><b>{}</b><span>permises (24 h)</span></div>\
         <div class=tile><b>{}</b><span>refusées (24 h)</span></div>\
         <div class=tile><b>{}</b><span>arrêts en vigueur</span></div>\
         <div class=tile><b>{}</b><span>accords en attente</span></div></div>",
        esc(&day["allowed"]), esc(&day["denied"]), arr("emergency_stops").len(), arr("pending_approvals").len()
    ));
    body.push_str(&table("Refus par raison (24 h)", "Aucun refus.", &["raison", "nombre"],
        day["denied_by_reason"].as_array().cloned().unwrap_or_default().iter()
            .map(|r| vec![esc(&r["reason"]), esc(&r["count"])]).collect()));
    body.push_str(&table("Arrêts d'urgence", "Aucun arrêt.", &["portée", "posé par", "raison", "depuis"],
        arr("emergency_stops").iter().map(|r| vec![esc(&r["scope"]), esc(&r["set_by"]), esc(&r["reason"]), when(&r["at"])]).collect()));
    body.push_str(&table("Accords en attente", "Aucune demande.", &["id", "agent", "action", "montant", "demandé", "expire"],
        arr("pending_approvals").iter().map(|r| vec![
            format!("<code>{}</code>", esc(&r["approval_id"])), esc(&r["agent"]), esc(&r["action"]), esc(&r["amount"]),
            when(&r["requested_at"]), when(&r["expires_at"]),
        ]).collect()));
    body.push_str(&table("Clés révoquées", "Aucune clé révoquée.", &["agent", "clé", "par", "raison", "date"],
        arr("revoked_keys").iter().map(|r| vec![
            esc(&r["agent"]), format!("<code>{}…</code>", esc(&json!(r["public_key"].as_str().unwrap_or("").chars().take(16).collect::<String>()))),
            esc(&r["revoked_by"]), esc(&r["reason"]), when(&r["at"]),
        ]).collect()));
    body.push_str(&table("Dernières décisions", "Aucune demande d'action.", &["date", "agent", "action", "montant", "décision", "raison", "règle"],
        arr("recent_decisions").iter().map(|r| {
            let ok = r["allowed"].as_bool().unwrap_or(false);
            vec![when(&r["at"]), esc(&r["agent"]), esc(&r["action"]), esc(&r["amount"]),
                 format!("<span class='{}'>{}</span>", if ok { "ok" } else { "no" }, if ok { "permise" } else { "refusée" }),
                 esc(&r["reason"]), esc(&r["rule"])]
        }).collect()));
    format!(r#"<!doctype html><html lang=fr><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1"><meta http-equiv=refresh content=10>
<title>CSTL tableau de bord</title><style>
:root{{--bg:#f7f7f5;--fg:#1d1d1b;--mut:#6b6b66;--card:#fff;--line:#e3e2dc;--ok:#1f7a4d;--no:#b3261e;--stop:#fdecea}}
@media (prefers-color-scheme:dark){{:root{{--bg:#161615;--fg:#ecebe6;--mut:#a3a29c;--card:#201f1d;--line:#34332f;--ok:#5cc28f;--no:#f2867c;--stop:#3a1d1a}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,sans-serif}}
main{{max-width:1100px;margin:0 auto;padding:24px 16px}}h1{{font-size:22px;margin:0 0 4px}}
.sub{{color:var(--mut);margin:0 0 20px}}h2{{font-size:16px;margin:0 0 10px}}.n{{color:var(--mut);font-weight:400}}
section{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px;margin:0 0 16px}}
.tiles{{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:12px;margin:0 0 16px}}
.tile{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px}}
.tile b{{display:block;font-size:26px;font-variant-numeric:tabular-nums}}.tile span{{color:var(--mut)}}
.scroll{{overflow-x:auto}}table{{border-collapse:collapse;width:100%}}
th,td{{text-align:left;padding:6px 10px;border-bottom:1px solid var(--line);white-space:nowrap;font-variant-numeric:tabular-nums}}
th{{color:var(--mut);font-weight:500}}.ok{{color:var(--ok)}}.no{{color:var(--no)}}.empty{{color:var(--mut);margin:0}}
.banner{{padding:12px 16px;border-radius:10px;margin:0 0 16px;font-weight:600}}.stop{{background:var(--stop);color:var(--no)}}
code{{font-size:13px}}</style></head><body><main><h1>CSTL — tableau de bord</h1>
<p class=sub>Mis à jour {} · lecture seule · JSON : <a href="/v1/dashboard.json">/v1/dashboard.json</a></p>{body}</main></body></html>"#,
        when(&v["generated_at"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_shows_decisions_stops_and_escapes_html() {
        let s = AdnStore::open(":memory:").unwrap();
        s.record_decision(100, "bot", "wipe_db", None, false, "forbidden", Some("MUST_NOT"), "sha256:a").unwrap();
        s.record_decision(chrono::Utc::now().timestamp(), "bot", "read_logs", None, true, "no_rule_default_allow", None, "sha256:b").unwrap();
        s.set_emergency_stop("agent:<script>", true, "owner", Some("x<y"), 5).unwrap();
        let v = snapshot(&s, Some("maintenance".into()));
        assert_eq!(v["last_24h"]["allowed"], 1);
        assert_eq!(v["recent_decisions"][0]["action"], "read_logs");
        let html = render_html(&v);
        assert!(html.contains("agent:&lt;script&gt;") && !html.contains("<script>"));
        assert!(html.contains("maintenance") && html.contains("wipe_db"));
    }
}
