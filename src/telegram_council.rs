//! src/telegram_council.rs — Pont Telegram pour RestrictedCouncil (portée réduite v1)
//!
//! Notifie sur Telegram quand une nouvelle entrée atterrit dans l'ADN store, et
//! permet de répondre par un message texte ("commit <hash> [note]" / "revoke
//! <hash> [note]") pour l'ancrer — sans avoir à taper une commande TCP soi-même.
//!
//! Portée honnête: pas de vrai webhook (le serveur local n'est pas exposé sur
//! internet) — polling `getUpdates` toutes les 2s. L'autorisation, ici, EST le
//! fait que le message vient du `chat_id` configuré (un seul membre, comme
//! `RestrictedCouncil::single_member`) — pas une vérification de nom d'utilisateur
//! Telegram, un chat_id compromis rest le seul point de confiance.

use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::adn_store::AdnStore;

pub struct TelegramNotifier {
    client: reqwest::Client,
    token: String,
    chat_id: String,
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

    /// Long-poll `getUpdates`. Retourne les (chat_id, texte) des nouveaux
    /// messages texte, et le prochain offset à utiliser.
    async fn get_updates(&self, offset: i64) -> (Vec<(String, String)>, i64) {
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
        let mut messages = Vec::new();
        let mut next_offset = offset;

        for update in &results {
            if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                next_offset = next_offset.max(update_id + 1);
            }
            let chat_id = update
                .pointer("/message/chat/id")
                .and_then(Value::as_i64)
                .map(|n| n.to_string());
            let text = update.pointer("/message/text").and_then(Value::as_str).map(String::from);
            if let (Some(chat_id), Some(text)) = (chat_id, text) {
                messages.push((chat_id, text));
            }
        }
        (messages, next_offset)
    }
}

/// Boucle de fond: poll Telegram, applique "commit <hash> [note]" / "revoke
/// <hash> [note]" directement sur l'ADN store. Un message venant d'un chat_id
/// autre que celui configuré est ignoré silencieusement (log seulement).
pub async fn run_telegram_poller(notifier: Arc<TelegramNotifier>, adn_store: Arc<Mutex<AdnStore>>) {
    eprintln!("[Telegram] Poller demarre (chat_id={})", notifier.chat_id);
    let mut offset = 0i64;
    loop {
        let (updates, next_offset) = notifier.get_updates(offset).await;
        offset = next_offset;

        for (from_chat_id, text) in updates {
            if from_chat_id != notifier.chat_id {
                eprintln!("[Telegram] Message ignore (chat_id non autorise: {})", from_chat_id);
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

            let outcome = {
                let store = adn_store.lock().await;
                if action == "commit" {
                    store.commit(hash, "Olivier", note)
                } else {
                    store.revoke(hash, "Olivier", note)
                }
            };

            let reply = match outcome {
                Ok(()) => format!("✅ {} applique sur {}", action, hash),
                Err(e) => format!("⚠️ Echec {} sur {}: {}", action, hash, e),
            };
            eprintln!("[Telegram] {}", reply);
            let _ = notifier.send_message(&reply).await;
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
