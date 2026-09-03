//! src/telegram_council.rs — Pont Telegram pour RestrictedCouncil (portée réduite v1)
//!
//! Notifie sur Telegram quand une nouvelle entrée atterrit dans l'ADN store, avec
//! deux boutons inline (✅ Commit / ❌ Revoke) — un vrai vote en un clic, pas besoin
//! de taper une commande. Le texte "commit <hash> [note]" / "revoke <hash> [note]"
//! reste aussi supporté (utile pour ajouter une note).
//!
//! Portée honnête: pas de vrai webhook (le serveur local n'est pas exposé sur
//! internet) — polling `getUpdates` toutes les 500ms. L'autorisation, ici, EST le
//! fait que le message/clic vient du `chat_id` configuré — pas une vérification
//! de nom d'utilisateur Telegram, un seul membre (comme
//! `RestrictedCouncil::single_member`).
//!
//! Limite technique: `callback_data` de Telegram est plafonné à 64 octets, bien
//! trop court pour un sha256 complet ("sha256:" + 64 hex = 71 caracteres). Les
//! boutons encodent donc un hash COURT (16 hex apres "sha256:"), resolu via
//! `AdnStore::get_by_short_id()`.

use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::adn_store::AdnStore;

pub struct TelegramNotifier {
    client: reqwest::Client,
    token: String,
    chat_id: String,
}

/// Longueur du hash court utilisé dans callback_data (hex chars apres "sha256:").
const SHORT_ID_LEN: usize = 16;

fn short_id(full_hash: &str) -> String {
    full_hash.strip_prefix("sha256:").unwrap_or(full_hash).chars().take(SHORT_ID_LEN).collect()
}

enum Update {
    Text { chat_id: String, text: String },
    Callback { chat_id: String, callback_id: String, data: String },
}

impl TelegramNotifier {
    /// `None` si `TELEGRAM_BOT_TOKEN` ou `TELEGRAM_CHAT_ID` absent de
    /// l'environnement — le serveur continue de fonctionner sans notification,
    /// dégradation propre plutôt qu'un crash au démarrage.
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("TELEGRAM_BOT_TOKEN").ok()?;
        let chat_id = std::env::var("TELEGRAM_CHAT_ID").ok()?;
        Some(Self { client: reqwest::Client::new(), token, chat_id })
    }

    pub async fn send_message(&self, text: &str) -> Result<(), reqwest::Error> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        self.client
            .post(&url)
            .form(&[("chat_id", self.chat_id.as_str()), ("text", text)])
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        Ok(())
    }

    /// Notification avec boutons ✅ Commit / ❌ Revoke pour un hash donné.
    pub async fn send_decision_request(&self, full_hash: &str, sigma: f64, consistent: bool, details: &str) -> Result<(), reqwest::Error> {
        let id = short_id(full_hash);
        let text = format!(
            "🔔 Décision requise\nhash: {}\ncohérence: {} (sigma={})\n\nVérification (copiable):\n```\n{}\n```",
            full_hash,
            if consistent { "OK" } else { "⚠️ CONTRADICTION/CYCLE" },
            sigma,
            details.trim()
        );
        let reply_markup = json!({
            "inline_keyboard": [[
                {"text": "✅ Commit", "callback_data": format!("c:{}", id)},
                {"text": "❌ Revoke", "callback_data": format!("r:{}", id)}
            ]]
        });
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        self.client
            .post(&url)
            .form(&[
                ("chat_id", self.chat_id.as_str()),
                ("text", text.as_str()),
                ("parse_mode", "Markdown"),
                ("reply_markup", reply_markup.to_string().as_str()),
            ])
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        Ok(())
    }

    async fn answer_callback(&self, callback_id: &str, text: &str) {
        let url = format!("https://api.telegram.org/bot{}/answerCallbackQuery", self.token);
        let _ = self
            .client
            .post(&url)
            .form(&[("callback_query_id", callback_id), ("text", text)])
            .timeout(Duration::from_secs(10))
            .send()
            .await;
    }

    /// Long-poll `getUpdates`. Retourne les nouveaux messages (texte ou clic de
    /// bouton), et le prochain offset à utiliser.
    async fn get_updates(&self, offset: i64) -> (Vec<Update>, i64) {
        let url = format!("https://api.telegram.org/bot{}/getUpdates", self.token);
        let resp = self
            .client
            .get(&url)
            .query(&[("offset", offset.to_string()), ("timeout", "20".to_string())])
            .timeout(Duration::from_secs(25))
            .send()
            .await;

        let json: Value = match resp {
            Ok(r) => match r.json().await {
                Ok(j) => j,
                Err(_) => return (vec![], offset),
            },
            Err(_) => return (vec![], offset),
        };

        let results = json.get("result").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut updates = Vec::new();
        let mut next_offset = offset;

        for update in &results {
            if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                next_offset = next_offset.max(update_id + 1);
            }

            if let Some(cq) = update.get("callback_query") {
                let chat_id = cq.pointer("/message/chat/id").and_then(Value::as_i64).map(|n| n.to_string());
                let callback_id = cq.get("id").and_then(Value::as_str).map(String::from);
                let data = cq.get("data").and_then(Value::as_str).map(String::from);
                if let (Some(chat_id), Some(callback_id), Some(data)) = (chat_id, callback_id, data) {
                    updates.push(Update::Callback { chat_id, callback_id, data });
                }
                continue;
            }

            let chat_id = update.pointer("/message/chat/id").and_then(Value::as_i64).map(|n| n.to_string());
            let text = update.pointer("/message/text").and_then(Value::as_str).map(String::from);
            if let (Some(chat_id), Some(text)) = (chat_id, text) {
                updates.push(Update::Text { chat_id, text });
            }
        }
        (updates, next_offset)
    }
}

/// Applique une decision (commit/revoke) recue via Telegram. Le commit passe
/// desormais par le meme quorum 2/3 (Couche 2, gouvernance) que
/// `purpose=council_decision` recu par TCP -- pas de double-standard entre
/// les deux entrees. Retourne un message pret a renvoyer au chat.
fn apply_decision(store_lock: &AdnStore, hash: &str, action: &str, note: Option<&str>, quorum_size: usize) -> Result<String, rusqlite::Error> {
    match action {
        "commit" | "c" => {
            let outcome = store_lock.cast_commit_vote(hash, "Olivier", note, quorum_size)?;
            Ok(if outcome.quorum_reached {
                format!("✅ commit applique sur {} (quorum {}/{})", hash, outcome.distinct_voters, outcome.quorum_size)
            } else {
                format!("🗳️ vote enregistre sur {} (quorum {}/{}, pas encore atteint)", hash, outcome.distinct_voters, outcome.quorum_size)
            })
        }
        "revoke" | "r" => {
            store_lock.revoke(hash, "Olivier", note)?;
            Ok(format!("✅ revoke applique sur {}", hash))
        }
        _ => Ok(format!("⚠️ action inconnue: {}", action)),
    }
}

/// Boucle de fond: poll Telegram, applique les décisions (boutons ou texte)
/// directement sur l'ADN store. Un message/clic venant d'un chat_id autre que
/// celui configuré est ignoré silencieusement (log seulement).
pub async fn run_telegram_poller(
    notifier: Arc<TelegramNotifier>,
    adn_store: Arc<Mutex<AdnStore>>,
    restricted_council: Arc<crate::restricted_council::RestrictedCouncil>,
) {
    eprintln!("[Telegram] Poller demarre (chat_id={})", notifier.chat_id);
    let mut offset = 0i64;
    loop {
        let (updates, next_offset) = notifier.get_updates(offset).await;
        offset = next_offset;

        for update in updates {
            match update {
                Update::Callback { chat_id, callback_id, data } => {
                    if chat_id != notifier.chat_id {
                        eprintln!("[Telegram] Callback ignore (chat_id non autorise: {})", chat_id);
                        continue;
                    }
                    let (action, id) = match data.split_once(':') {
                        Some((a, id)) => (a, id),
                        None => continue,
                    };
                    let full_action = match action {
                        "c" => "commit",
                        "r" => "revoke",
                        _ => continue,
                    };

                    let resolved = {
                        let store = adn_store.lock().await;
                        store.get_by_short_id(id)
                    };
                    let reply = match resolved {
                        Ok(Some(entry)) => {
                            let quorum_size = restricted_council.quorum_size();
                            let outcome = {
                                let store = adn_store.lock().await;
                                apply_decision(&store, &entry.hash, full_action, Some("via bouton Telegram"), quorum_size)
                            };
                            match outcome {
                                Ok(msg) => msg,
                                Err(e) => format!("⚠️ Echec {} sur {}: {}", full_action, entry.hash, e),
                            }
                        }
                        Ok(None) => format!("⚠️ Hash court '{}' introuvable dans l'adn_store", id),
                        Err(e) => format!("⚠️ Erreur de lecture: {}", e),
                    };
                    eprintln!("[Telegram] {}", reply);
                    notifier.answer_callback(&callback_id, full_action).await;
                    let _ = notifier.send_message(&reply).await;
                }
                Update::Text { chat_id, text } => {
                    if chat_id != notifier.chat_id {
                        eprintln!("[Telegram] Message ignore (chat_id non autorise: {})", chat_id);
                        continue;
                    }
                    let parts: Vec<&str> = text.trim().splitn(3, ' ').collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let action = parts[0].to_lowercase();
                    let hash = parts[1];
                    let note = parts.get(2).copied();
                    if action != "commit" && action != "revoke" {
                        continue;
                    }

                    let quorum_size = restricted_council.quorum_size();
                    let outcome = {
                        let store = adn_store.lock().await;
                        apply_decision(&store, hash, &action, note, quorum_size)
                    };
                    let reply = match outcome {
                        Ok(msg) => msg,
                        Err(e) => format!("⚠️ Echec {} sur {}: {}", action, hash, e),
                    };
                    eprintln!("[Telegram] {}", reply);
                    let _ = notifier.send_message(&reply).await;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
