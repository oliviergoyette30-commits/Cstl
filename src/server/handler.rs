/// Connection Handler - Wire Parser + Validator + Router
/// 
/// Flow:
/// 1. Receive TCP payload
/// 2. Parse as CSTL
/// 3. Validate constraints
/// 4. Route to destination agent
/// 5. Record audit trail
/// 6. Send response

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::agent_discovery::AgentRegistry;
use crate::kb_verify::KbVerifier;
use crate::adn_store::AdnStore;
use crate::execution_lab;
use crate::restricted_council::RestrictedCouncil;
use crate::telegram_council::TelegramNotifier;
use super::audit::HashChain;
use super::parser;
use super::validator;

pub async fn handle_connection(
    mut socket: TcpStream,
    registry: Arc<AgentRegistry>,
    chain: Arc<Mutex<HashChain>>,
    kb_verifier: Arc<KbVerifier>,
    adn_store: Arc<Mutex<AdnStore>>,
    restricted_council: Arc<RestrictedCouncil>,
    telegram: Option<Arc<TelegramNotifier>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = vec![0; 8192];
    
    loop {
        let n = socket.read(&mut buffer).await?;
        
        if n == 0 {
            eprintln!("[Handler] Connection closed");
            return Ok(());
        }
        
        let raw_payload = String::from_utf8_lossy(&buffer[..n]).to_string();
        eprintln!("[Handler] Received {} bytes", n);
        
        // STEP 1: Parse CSTL payload
        let parse_result = parser::parse_payload(&raw_payload);
        
        match parse_result {
            Ok(payload) => {
                eprintln!("[Handler] ✅ Parse successful");
                
                // STEP 2: Validate semantically
                let validation = validator::validate_payload(&payload);
                
                if validation.valid {
                    eprintln!("[Handler] ✅ Validation passed");

                    // STEP 2b: Decision du RestrictedCouncil (couche 3b, portee reduite v1)
                    // — purpose=council_decision est traite a part: ce n'est pas un
                    // nouveau fait a verifier/stocker, c'est une action sur une entree
                    // DEJA dans l'adn_store. On court-circuite le reste du pipeline.
                    if payload.intent.get("purpose").map(String::as_str) == Some("council_decision") {
                        let sender = payload.intent.get("sender").cloned().unwrap_or_default();
                        let target_hash = payload.intent.get("target_hash").cloned();
                        let decision = payload.intent.get("decision").cloned();
                        let note = payload.intent.get("note").cloned();

                        let response = if !restricted_council.is_authorized(&sender) {
                            eprintln!("[Handler] ⛔ Council decision rejected: '{}' non autorise", sender);
                            format!(
                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_rejected, reason=not_authorized, sender={}]\n---END---\n",
                                sender
                            )
                        } else {
                            match (target_hash, decision) {
                                (Some(hash), Some(decision)) => {
                                    let outcome = {
                                        let store = adn_store.lock().await;
                                        match decision.as_str() {
                                            "commit" => store.commit(&hash, &sender, note.as_deref()),
                                            "revoke" => store.revoke(&hash, &sender, note.as_deref()),
                                            other => {
                                                eprintln!("[Handler] ⚠️  decision inconnue: {}", other);
                                                Ok(())
                                            }
                                        }
                                    };
                                    match outcome {
                                        Ok(()) if decision == "commit" || decision == "revoke" => {
                                            eprintln!("[Handler] ⚖️  RestrictedCouncil: {} sur {} par {}", decision, hash, sender);
                                            format!(
                                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=council_decision_applied, decision={}, target_hash={}, by={}]\n---END---\n",
                                                decision, hash, sender
                                            )
                                        }
                                        Ok(()) => format!(
                                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_rejected, reason=unknown_decision]\n---END---\n"
                                        ),
                                        Err(e) => {
                                            eprintln!("[Handler] ⚠️  council decision failed: {}", e);
                                            format!(
                                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_failed, detail={}]\n---END---\n",
                                                e.to_string().replace("\"", "'")
                                            )
                                        }
                                    }
                                }
                                _ => "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_rejected, reason=missing_target_hash_or_decision]\n---END---\n".to_string(),
                            }
                        };

                        socket.write_all(response.as_bytes()).await?;
                        continue;
                    }

                    // AUDIT: le serveur (orchestrateur) calcule le vrai SHA-256.
                    // L'agent envoie PARENT_HASH=root; on le remplace ici.
                    let entry = { chain.lock().await.append(&payload) };
                    
                    // STEP 3: Extract routing info
                    let receiver = payload.intent.get("receiver").cloned().unwrap_or_else(|| "unknown".to_string());
                    let purpose = payload.intent.get("purpose").cloned().unwrap_or_else(|| "unknown".to_string());
                    
                    eprintln!("[Handler] 🔀 Routing to: {} (purpose: {})", receiver, purpose);

                    // STEP 3b: Verification factuelle (Couche 3a) — pour chaque RELATION
                    // du payload, on interroge Wikidata via kb_verify::KbVerifier.
                    // Le module cesse ici d'etre un exemple isole et devient une
                    // capacite reelle du serveur en cours d'execution.
                    let mut verification_lines = String::new();
                    for relation in &payload.relations {
                        let subject = relation.get("subject").cloned();
                        let object = relation.get("object").cloned();
                        let predicate = relation.get("type").cloned();
                        if let (Some(subject), Some(predicate), Some(object)) = (subject, predicate, object) {
                            eprintln!("[Handler] 🔎 Verifying relation: {} {} {}", subject, predicate, object);
                            let result = kb_verifier.verify_relation(&subject, &predicate, &object, "fr", 4, 40).await;
                            eprintln!("[Handler]    -> {} ({})", result.verified, result.reason);
                            verification_lines.push_str(&format!(
                                "VERIFICATION [subject={}, predicate={}, object={}, verified={}, source={}]\n",
                                subject,
                                predicate,
                                object,
                                result.verified,
                                result.source_url.unwrap_or_else(|| "none".to_string())
                            ));
                        }
                    }

                    // STEP 3c: Coherence interne (ExecutionLab, couche 3b partielle) —
                    // verifie les relations DE CE PAYLOAD entre elles (contradictions
                    // sur un predicat fonctionnel, cycles sur une chaine transitive).
                    // Ce n'est PAS un jugement de verite empirique (ca, c'est kb_verify) —
                    // c'est une verification de coherence computationnellement checkable.
                    let consistency = execution_lab::check_consistency(&payload.relations);
                    let sigma = consistency.sigma_adjustment();
                    eprintln!(
                        "[Handler] 🧪 ExecutionLab: consistent={} contradictions={} cycles={} -> sigma={}",
                        consistency.consistent, consistency.contradictions.len(), consistency.cycles.len(), sigma
                    );

                    // STEP 3d: Memoire persistante (Couche 5, ADN store) — le payload est
                    // stocke (ASSUMES, non-commite) avec le sigma qu'ExecutionLab vient de
                    // calculer. Rien n'est ancre (committed) ici: aucun RestrictedCouncil
                    // (quorum humain 2/3) n'existe encore pour faire ce commit.
                    if let Err(e) = adn_store.lock().await.put(
                        &entry.hash,
                        &raw_payload,
                        payload.meta.get("encoder").map(String::as_str),
                        payload.meta.get("produced_by").map(String::as_str),
                        sigma,
                        Some(&entry.parent_hash),
                        None,
                        None,
                    ) {
                        eprintln!("[Handler] ⚠️  adn_store.put failed: {}", e);
                    } else {
                        eprintln!("[Handler] 💾 Stored in adn_store (hash={}, sigma={}, committed=false)", entry.hash, sigma);

                        // STEP 3e: Notification RestrictedCouncil (portee reduite v1) —
                        // pousse un message Telegram plutot que d'attendre que le
                        // membre autorise pense a interroger l'ADN store lui-meme.
                        if let Some(telegram) = &telegram {
                            let notice = format!(
                                "🔔 Nouvelle entree ADN store
hash: {}
coherence: {} (sigma={})
Repondre: commit {} [note] ou revoke {} [note]",
                                entry.hash, if consistency.consistent { "OK" } else { "CONTRADICTION/CYCLE" }, sigma,
                                entry.hash, entry.hash
                            );
                            let telegram = telegram.clone();
                            tokio::spawn(async move {
                                if let Err(e) = telegram.send_message(&notice).await {
                                    eprintln!("[Telegram] ⚠️  envoi echoue: {}", e);
                                }
                            });
                        }
                    }

                    let consistency_line = format!(
                        "CONSISTENCY [consistent={}, contradictions={}, cycles={}, sigma={}]\n",
                        consistency.consistent, consistency.contradictions.len(), consistency.cycles.len(), sigma
                    );
                    
                    // STEP 4: Try to route to agent
                    if let Some(agent) = registry.route("communication") {
                        eprintln!("[Handler] ✅ Found agent: {}", agent.name);
                        
                        // STEP 5: Build response
                        let response = format!(
                            "#!CSTL v5.0.0 MODE=A\n\
                            META [encoder=CstlNativeServer, produced_by=Server, status=processed]\n\
                            INTENT_PAYLOAD [purpose=acknowledgement, sender=server, receiver={}]\n\
                            RELATION [type=received, subject={}, status=valid]\n\
                            {}\
                            {}\
                            AUDIT [hash={}, parent_hash={}, seq={}]\n\
                            ---END---\n",
                            payload.intent.get("sender").cloned().unwrap_or_else(|| "unknown".to_string()),
                            purpose,
                            verification_lines,
                            consistency_line,
                            entry.hash,
                            entry.parent_hash,
                            entry.seq
                        );
                        
                        socket.write_all(response.as_bytes()).await?;
                        eprintln!("[Handler] ✅ Response sent");
                    } else {
                        let error_response = "#!CSTL v5.0.0 MODE=A\nINTENT_PAYLOAD [purpose=error, status=no_agent]\n---END---\n";
                        socket.write_all(error_response.as_bytes()).await?;
                        eprintln!("[Handler] ❌ No agent found");
                    }
                } else {
                    // Validation failed - send error response
                    eprintln!("[Handler] ❌ Validation failed: {} errors", validation.errors.len());
                    
                    let error_msg = validation.errors.iter()
                        .map(|e| format!("{}: {}", e.code, e.message))
                        .collect::<Vec<_>>()
                        .join("; ");
                    
                    let error_response = format!(
                        "#!CSTL v5.0.0 MODE=A\n\
                        META [encoder=CstlNativeServer, produced_by=Server, status=error]\n\
                        INTENT_PAYLOAD [purpose=validation_error, errors={}]\n\
                        ---END---\n",
                        error_msg.replace("\"", "\\\"")
                    );
                    
                    socket.write_all(error_response.as_bytes()).await?;
                }
            }
            Err(e) => {
                // Parse failed - send parse error
                eprintln!("[Handler] ❌ Parse failed: {}", e);
                
                let error_response = format!(
                    "#!CSTL v5.0.0 MODE=A\n\
                    META [encoder=CstlNativeServer, produced_by=Server, status=error]\n\
                    INTENT_PAYLOAD [purpose=parse_error, details={}]\n\
                    ---END---\n",
                    e.to_string().replace("\"", "\\\"")
                );
                
                socket.write_all(error_response.as_bytes()).await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_module_compiles() {
        // Just verify the module compiles
        // Real testing requires async runtime
    }
}
