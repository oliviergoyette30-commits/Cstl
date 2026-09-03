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
use crate::obsidian_escalation::ObsidianEscalation;
use crate::emergence;
use crate::security;
use super::audit::HashChain;
use super::parser;
use super::validator;

/// Cherche `---END---` dans `buf` et retourne l'offset EXCLUSIF juste apres
/// (et apres le `\n` qui suit immediatement, s'il y en a un) -- c'est-a-dire
/// la limite jusqu'a laquelle `drain()` doit couper pour extraire UN message
/// CSTL complet de l'accumulateur, en laissant tout ce qui suit (pipelining)
/// intact pour la prochaine iteration. `None` si aucun `---END---` complet
/// n'est encore dans le buffer.
fn find_message_end(buf: &[u8]) -> Option<usize> {
    const MARKER: &[u8] = b"---END---";
    let pos = buf.windows(MARKER.len()).position(|w| w == MARKER)?;
    let mut end = pos + MARKER.len();
    if buf.get(end) == Some(&b'\n') {
        end += 1;
    }
    Some(end)
}

pub async fn handle_connection(
    mut socket: TcpStream,
    registry: Arc<AgentRegistry>,
    chain: Arc<Mutex<HashChain>>,
    kb_verifier: Arc<KbVerifier>,
    adn_store: Arc<Mutex<AdnStore>>,
    restricted_council: Arc<RestrictedCouncil>,
    telegram: Option<Arc<TelegramNotifier>>,
    obsidian: Option<Arc<ObsidianEscalation>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = vec![0u8; 8192];
    // Trouvaille mineure de l'audit multi-angle (2026-09-03): le buffer de
    // reception etait fixe a 8192 octets et jamais reassemble -- un payload
    // CSTL legitime plus grand qu'un seul read() TCP (frequent: TCP segmente
    // arbitrairement, rien ne garantit qu'un message applicatif tienne dans
    // un seul appel read()) etait tronque en silence des que raw.len() >
    // 8192, avec un `---END---` jamais atteint -> ParseError::MissingEndMarker
    // trompeur (le payload N'ETAIT PAS malforme, juste coupe par le transport).
    // Fix: accumulateur persistant entre les read(), on n'extrait un message
    // complet que lorsque `---END---` apparait DANS l'accumulateur. Gere
    // aussi le pipelining (plusieurs payloads dans le meme flux TCP): tout
    // ce qui suit le `---END---` du message courant reste dans l'accumulateur
    // pour la prochaine iteration au lieu d'etre perdu.
    let mut accumulated: Vec<u8> = Vec::new();
    const MAX_PAYLOAD_SIZE: usize = 1024 * 1024; // 1 MiB -- tres au-dessus de tout payload CSTL reel observe cette session

    loop {
        let raw_payload = loop {
            if let Some(end) = find_message_end(&accumulated) {
                let complete: Vec<u8> = accumulated.drain(..end).collect();
                break String::from_utf8_lossy(&complete).to_string();
            }
            if accumulated.len() > MAX_PAYLOAD_SIZE {
                eprintln!(
                    "[Handler] 🚫 Payload accumule depasse MAX_PAYLOAD_SIZE ({} octets) sans ---END--- -- connexion fermee",
                    MAX_PAYLOAD_SIZE
                );
                let rejection = "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=payload_too_large]\n---END---\n";
                socket.write_all(rejection.as_bytes()).await?;
                return Ok(());
            }

            let n = socket.read(&mut buffer).await?;
            if n == 0 {
                if accumulated.is_empty() {
                    eprintln!("[Handler] Connection closed");
                } else {
                    eprintln!("[Handler] Connection closed avec {} octets incomplets (pas de ---END---)", accumulated.len());
                }
                return Ok(());
            }
            accumulated.extend_from_slice(&buffer[..n]);
        };
        eprintln!("[Handler] Received complete message ({} bytes)", raw_payload.len());

        // STEP 0: Security scan (Sessions #6+#7) -- trouve dans un audit multi-angle
        // (2026-09-03) que security::security_scan existait depuis sa creation sans
        // etre appele nulle part sur le chemin reseau reel. Corrige ici: on nettoie
        // les codepoints dangereux (zero-width/bidi/C1) AVANT le parsing, et on
        // rejette purement et simplement tout payload avec une erreur de securite
        // (META imbrique / injection, profondeur de crochets excessive) sans meme
        // tenter de le parser.
        let security_report = security::security_scan(&raw_payload);
        if !security_report.warnings.is_empty() {
            eprintln!("[Handler] 🛡️  Security warnings: {:?}", security_report.warnings);
        }
        if !security_report.errors.is_empty() {
            eprintln!("[Handler] 🚫 Security scan rejected payload: {:?}", security_report.errors);
            let rejection = format!(
                "#!CSTL v5.0.0 MODE=A\n\
                META [encoder=CstlNativeServer, produced_by=Server, status=error]\n\
                INTENT_PAYLOAD [purpose=security_rejected, details={}]\n\
                ---END---\n",
                security_report.errors.join("; ").replace("\"", "\\\"")
            );
            socket.write_all(rejection.as_bytes()).await?;
            continue;
        }
        let raw_payload = security_report.cleaned;

        // STEP 1: Parse CSTL payload
        let parse_result = parser::parse_payload(&raw_payload);
        
        match parse_result {
            Ok(payload) => {
                eprintln!("[Handler] ✅ Parse successful");
                
                // STEP 2: Validate semantically
                let validation = validator::validate_payload(&payload);
                
                if validation.valid {
                    eprintln!("[Handler] ✅ Validation passed");

                    // STEP 2c: whitelist des 35 operateurs SDL officiels + MUTUAL
                    // deprecie (semantic.rs) -- avant cette passe, aucun des deux
                    // modules de validation semantique du depot (semantic.rs,
                    // validator_semantic.rs) n'etait jamais appele sur le chemin
                    // TCP reel (trouvaille de l'audit multi-angle 2026-09-03,
                    // decouverte en creusant le fix de la desync MUTUAL). Warnings
                    // uniquement, jamais un rejet -- voir le commentaire de
                    // check_sdl_operator_whitelist pour pourquoi (les predicats KB
                    // en minuscules comme part_of/located_in/born_in ne sont pas
                    // dans ce vocabulaire et sont ignores par construction).
                    let semantic_warnings = validator::check_sdl_operator_whitelist(&payload);
                    let mut semantic_warning_lines = String::new();
                    if !semantic_warnings.is_empty() {
                        eprintln!("[Handler] ⚠️  Semantic operator warnings: {:?}", semantic_warnings);
                        for w in &semantic_warnings {
                            semantic_warning_lines.push_str(&format!(
                                "SEMANTIC_WARNING [detail={}]\n",
                                w.replace(',', ";").replace('[', "(").replace(']', ")")
                            ));
                        }
                    }

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
                                            // Trouvaille mineure de l'audit multi-angle (2026-09-03):
                                            // e.to_string() ici est un rusqlite::Error -- peut exposer
                                            // des details internes (schema, contraintes SQL...) a un
                                            // client TCP quelconque. On logge le detail complet cote
                                            // serveur (deja fait ci-dessus) mais on ne renvoie plus
                                            // qu'un code generique au client.
                                            eprintln!("[Handler] ⚠️  council decision failed: {}", e);
                                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_failed, detail=internal_error]\n---END---\n".to_string()
                                        }
                                    }
                                }
                                _ => "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_rejected, reason=missing_target_hash_or_decision]\n---END---\n".to_string(),
                            }
                        };

                        socket.write_all(response.as_bytes()).await?;
                        continue;
                    }

                    // STEP 2c: Detection d'emergence (Couche 3b, port de
                    // RevisionOrchestrator, src/emergence.rs) -- purpose=detect_emergence
                    // compare la decision d'un run TRIPARTITE deja stocke (trio_hash) a
                    // celles des runs SOLO de chaque agent (eux aussi deja stockes,
                    // soumis normalement par chaque agent avant le tripartite). Aucun
                    // appel API: les payloads compares sont deja dans l'adn_store.
                    // Comme council_decision, court-circuite le reste du pipeline.
                    if payload.intent.get("purpose").map(String::as_str) == Some("detect_emergence") {
                        let trio_hash = payload.intent.get("trio_hash").cloned().unwrap_or_default();
                        let solo_hashes_field = payload.intent.get("solo_hashes").cloned().unwrap_or_default();
                        let question = payload.intent.get("question").cloned().unwrap_or_default();
                        let solo_hashes = emergence::parse_solo_hashes(&solo_hashes_field);

                        let response = if trio_hash.is_empty() || solo_hashes.is_empty() {
                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=detect_emergence_rejected, reason=missing_trio_hash_or_solo_hashes]\n---END---\n".to_string()
                        } else {
                            let detect_result = {
                                let store = adn_store.lock().await;
                                emergence::detect_revisions(&store, &trio_hash, &solo_hashes, &question)
                            };
                            match detect_result {
                                Ok(reports) => {
                                    let mut lines = String::new();
                                    for r in &reports {
                                        lines.push_str(&format!(
                                            "EMERGENCE_REPORT [agent={}, revised={}, solo={}, trio={}, delta_sigma={}]\n",
                                            r.agent, r.revised, r.solo_decision, r.trio_decision, r.delta_sigma
                                        ));
                                    }
                                    let revised_count = reports.iter().filter(|r| r.revised).count();
                                    eprintln!(
                                        "[Handler] 🧬 detect_emergence: {} agents compares, {} revision(s) detectee(s)",
                                        reports.len(), revised_count
                                    );
                                    format!(
                                        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=detect_emergence_result, trio_hash={}, agents_compared={}, revisions_detected={}]\n{}---END---\n",
                                        trio_hash, reports.len(), revised_count, lines
                                    )
                                }
                                Err(e) => {
                                    // Meme fix que council_decision_failed juste au-dessus: erreur
                                    // sqlite interne loggee cote serveur, jamais renvoyee brute au client.
                                    eprintln!("[Handler] ⚠️  detect_emergence failed: {}", e);
                                    "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=detect_emergence_failed, detail=internal_error]\n---END---\n".to_string()
                                }
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
                    // verifie les relations DE CE PAYLOAD contre TOUT l'historique de
                    // l'adn_store, pas seulement entre elles. Ce n'est PAS un jugement
                    // de verite empirique (ca, c'est kb_verify) — c'est une verification
                    // de coherence computationnellement checkable, maintenant etendue
                    // au-dela d'un seul payload recu.
                    // Charge seulement les predicats dont ExecutionLab se sert
                    // (WHERE predicate IN (...) au niveau SQL), pas tout
                    // adn_relations -- correction du scan complet a chaque
                    // requete identifie precedemment.
                    let history_relations = {
                        let store = adn_store.lock().await;
                        store.relations_for_predicates(&execution_lab::relevant_predicates()).unwrap_or_else(|e| {
                            eprintln!("[Handler] ⚠️  adn_store.relations_for_predicates failed: {}", e);
                            Vec::new()
                        })
                    };
                    let consistency = execution_lab::check_consistency_with_history(&payload.relations, &history_relations);
                    let sigma = consistency.sigma_adjustment();
                    eprintln!(
                        "[Handler] 🧪 ExecutionLab: consistent={} contradictions={} cycles={} -> sigma={}",
                        consistency.consistent, consistency.contradictions.len(), consistency.cycles.len(), sigma
                    );

                    // Resume lisible du dilemme pour la notification Telegram — sans
                    // ca, "committer" est un clic aveugle, pas une decision informee.
                    let mut telegram_details = verification_lines.clone();
                    if !consistency.contradictions.is_empty() {
                        telegram_details.push_str("\nContradictions:\n");
                        for ct in &consistency.contradictions {
                            telegram_details.push_str(&format!(
                                "- {} {}: {} vs {}\n", ct.subject, ct.predicate, ct.object_a, ct.object_b
                            ));
                        }
                    }
                    if !consistency.cycles.is_empty() {
                        telegram_details.push_str("\nCycles:\n");
                        for cy in &consistency.cycles {
                            telegram_details.push_str(&format!("- {}: {}\n", cy.predicate, cy.path.join(" -> ")));
                        }
                    }

                    // STEP 3c-bis: Escalade Obsidian (Layer 6, portion reelle) — si
                    // ExecutionLab a detecte une contradiction/cycle, on ecrit une
                    // entree visible directement dans le vault Obsidian de
                    // l'utilisateur, en plus de la notification Telegram.
                    if !consistency.consistent {
                        if let Some(obsidian) = &obsidian {
                            if let Err(e) = obsidian.escalate(&entry.hash, sigma, &telegram_details) {
                                eprintln!("[Obsidian] ⚠️  escalade echouee: {}", e);
                            } else {
                                eprintln!("[Obsidian] 📓 Escalade ecrite dans le vault (hash={})", entry.hash);
                            }
                        }
                    }

                    // STEP 3d: Memoire persistante (Couche 5, ADN store) — le payload est
                    // stocke (ASSUMES, non-commite) avec le sigma qu'ExecutionLab vient de
                    // calculer. Rien n'est ancre (committed) ici: aucun RestrictedCouncil
                    // (quorum humain 2/3) n'existe encore pour faire ce commit.
                    // Le Result est capture dans une variable AVANT le if/else pour que
                    // le MutexGuard temporaire de `.lock().await` soit relache a la fin de
                    // cette instruction `let`, PAS a la fin de tout le if/else qui suit.
                    // Bug reel trouve et corrige ici (2026-09-03): avec
                    // `if let Err(e) = adn_store.lock().await.put(...) { } else { ...
                    // adn_store.lock().await.put_relations(...) ... }`, les regles
                    // d'extension de duree de vie des temporaires gardaient le guard du
                    // premier `.lock()` vivant pendant TOUTE la branche else -- le second
                    // `.lock()` sur le meme Mutex (non reentrant) attendait donc un verrou
                    // que la meme tache detenait deja implicitement -> deadlock permanent,
                    // confirme par un test live: la deuxieme requete d'un test en 2 payloads
                    // separes ne progressait plus jamais au-dela de ce point.
                    let put_result = adn_store.lock().await.put(
                        &entry.hash,
                        &raw_payload,
                        payload.meta.get("encoder").map(String::as_str),
                        payload.meta.get("produced_by").map(String::as_str),
                        sigma,
                        Some(&entry.parent_hash),
                        None,
                        None,
                    );
                    if let Err(e) = put_result {
                        eprintln!("[Handler] ⚠️  adn_store.put failed: {}", e);
                    } else {
                        eprintln!("[Handler] 💾 Stored in adn_store (hash={}, sigma={}, committed=false)", entry.hash, sigma);

                        // Persiste aussi les relations de ce payload pour que les
                        // requetes FUTURES puissent etre verifiees contre elles via
                        // check_consistency_with_history — sans ca, l'historique
                        // recupere plus haut resterait toujours vide.
                        let put_relations_result = adn_store.lock().await.put_relations(&entry.hash, &payload.relations);
                        if let Err(e) = put_relations_result {
                            eprintln!("[Handler] ⚠️  adn_store.put_relations failed: {}", e);
                        }

                        // STEP 3e: Notification RestrictedCouncil (portee reduite v1) —
                        // pousse un message Telegram plutot que d'attendre que le
                        // membre autorise pense a interroger l'ADN store lui-meme.
                        if let Some(telegram) = &telegram {
                            let telegram = telegram.clone();
                            let hash_for_telegram = entry.hash.clone();
                            let consistent = consistency.consistent;
                            let details_for_telegram = telegram_details.clone();
                            tokio::spawn(async move {
                                if let Err(e) = telegram.send_decision_request(&hash_for_telegram, sigma, consistent, &details_for_telegram).await {
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
                            {}\
                            AUDIT [hash={}, parent_hash={}, seq={}]\n\
                            ---END---\n",
                            payload.intent.get("sender").cloned().unwrap_or_else(|| "unknown".to_string()),
                            purpose,
                            verification_lines,
                            consistency_line,
                            semantic_warning_lines,
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

                // Trouvaille mineure de l'audit multi-angle (2026-09-03): les variantes
                // ParseError::InvalidFormat/MalformedBlock embarquent des fragments du
                // payload BRUT envoye par le client (potentiellement adversarial). Les
                // reinjecter tels quels dans le champ `details=` de la reponse wire
                // permettait a un client malveillant de casser le format wire de la
                // reponse elle-meme (ex: un fragment contenant `]` ou `,`). On classe
                // desormais l'erreur par variante (categorie sure, sans le contenu brut)
                // plutot que de renvoyer e.to_string() -- le detail complet reste loggé
                // cote serveur ci-dessus pour le debug.
                let details = match &e {
                    parser::ParseError::MissingHashbang => "missing_hashbang",
                    parser::ParseError::InvalidFormat(_) => "invalid_format",
                    parser::ParseError::MissingEndMarker => "missing_end_marker",
                    parser::ParseError::MalformedBlock(_) => "malformed_block",
                };

                let error_response = format!(
                    "#!CSTL v5.0.0 MODE=A\n\
                    META [encoder=CstlNativeServer, produced_by=Server, status=error]\n\
                    INTENT_PAYLOAD [purpose=parse_error, details={}]\n\
                    ---END---\n",
                    details
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

    // ── find_message_end (fix reassemblage TCP, audit multi-angle 2026-09-03) ──

    #[test]
    fn test_find_message_end_none_when_marker_absent() {
        assert_eq!(find_message_end(b"#!CSTL v5.0.0 MODE=A\nMETA [x=y]"), None);
    }

    #[test]
    fn test_find_message_end_splits_exactly_after_marker_and_newline() {
        let buf = b"#!CSTL v5.0.0 MODE=A\nMETA [x=y]\n---END---\nRESTE";
        let end = find_message_end(buf).expect("marker present");
        let (message, rest) = buf.split_at(end);
        assert!(message.ends_with(b"---END---\n"));
        assert_eq!(rest, b"RESTE");
    }

    #[test]
    fn test_find_message_end_handles_marker_split_across_two_reads() {
        // Simule exactement le cas qui cassait avant ce fix: le marker de fin
        // (ou tout le reste du message) arrive dans un DEUXIEME appel read().
        // Avant: chaque read() etait traite comme un message complet isole ->
        // le premier fragment (sans ---END---) declenchait un
        // ParseError::MissingEndMarker trompeur, meme si le payload etait
        // parfaitement valide et juste segmente par le transport TCP.
        let mut accumulated: Vec<u8> = b"#!CSTL v5.0.0 MODE=A\nMETA [x=y]\n---EN".to_vec();
        assert_eq!(find_message_end(&accumulated), None, "message incomplet, pas encore de ---END---");

        accumulated.extend_from_slice(b"D---\n");
        let end = find_message_end(&accumulated).expect("le marker complet est maintenant present");
        let complete: Vec<u8> = accumulated.drain(..end).collect();
        assert!(String::from_utf8(complete).unwrap().contains("---END---"));
        assert!(accumulated.is_empty(), "rien ne doit rester apres un message unique complet");
    }

    #[test]
    fn test_find_message_end_leaves_pipelined_second_message_intact() {
        // Deux payloads dans le meme flux TCP (pipelining) : seul le premier
        // doit etre extrait, le second doit rester intact dans l'accumulateur.
        let mut accumulated: Vec<u8> =
            b"#!CSTL v5.0.0 MODE=A\nMETA [a=1]\n---END---\n#!CSTL v5.0.0 MODE=A\nMETA [b=2]\n---END---\n".to_vec();
        let end = find_message_end(&accumulated).unwrap();
        let first: Vec<u8> = accumulated.drain(..end).collect();
        assert_eq!(String::from_utf8(first).unwrap(), "#!CSTL v5.0.0 MODE=A\nMETA [a=1]\n---END---\n");
        assert_eq!(
            String::from_utf8(accumulated.clone()).unwrap(),
            "#!CSTL v5.0.0 MODE=A\nMETA [b=2]\n---END---\n"
        );
    }
}
