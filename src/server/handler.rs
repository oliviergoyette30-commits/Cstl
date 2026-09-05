//! Connection Handler - Wire Parser + Validator + Router
//!
//! Flow:
//! 1. Receive TCP payload
//! 2. Parse as CSTL
//! 3. Validate constraints
//! 4. Route to destination agent
//! 5. Record audit trail
//! 6. Send response

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
use crate::governance::GovernanceTracker;
use crate::agent_discovery::AgentCard;
use crate::signing::{self, SignatureCheck};
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

// Meme justification qu'accept_connections (listener.rs): 9 sous-systemes
// partages distincts, pas une struct de config qui gagnerait a etre groupee.
#[allow(clippy::too_many_arguments)]
pub async fn handle_connection(
    mut socket: TcpStream,
    registry: Arc<Mutex<AgentRegistry>>,
    chain: Arc<Mutex<HashChain>>,
    kb_verifier: Arc<KbVerifier>,
    adn_store: Arc<Mutex<AdnStore>>,
    restricted_council: Arc<RestrictedCouncil>,
    telegram: Option<Arc<TelegramNotifier>>,
    obsidian: Option<Arc<ObsidianEscalation>>,
    governance: Arc<Mutex<GovernanceTracker>>,
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
                    let mut semantic_warnings = validator::check_sdl_operator_whitelist(&payload);
                    // Item #2 de la liste des choses a faire (2026-09-04):
                    // les 11 autres checks de semantic.rs (E108/E109/E701/
                    // W502/W503/R9/R10/W602/W603/W604/W605) etaient testes
                    // depuis des mois sans jamais etre appeles par le
                    // serveur reel -- brancher aux cotes de la whitelist
                    // d'operateurs, meme format de reponse, meme politique
                    // (avertissement seul, voir le commentaire de tete de
                    // check_extended_semantic_diagnostics pour pourquoi).
                    semantic_warnings.extend(validator::check_extended_semantic_diagnostics(&payload));
                    // R8 (reconstruit le 2026-09-05 sur le vrai CstlPayload --
                    // voir validator::check_coref_with_references pour
                    // l'historique complet). Meme politique que les deux
                    // appels ci-dessus : avertissement seul, jamais un rejet.
                    semantic_warnings.extend(validator::check_coref_with_references(&payload));
                    // R7 (parser.rs) : un bloc DEFINE mal forme (en-tete ou
                    // crochets) est deja "dropped" par le parser -- on
                    // remonte aussi l'avertissement au client plutot que de
                    // le laisser uniquement dans les logs serveur (eprintln!).
                    semantic_warnings.extend(payload.parse_warnings.iter().cloned());
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

                    // STEP 2a: Verification de signature Ed25519 (Couche 2/securite,
                    // src/signing.rs) — optionnelle globalement, obligatoire seulement
                    // pour un expediteur DEJA enregistre avec une public_key. On rejette
                    // ici (bloquant, meme categorie que securite/parse/validation) plutot
                    // que de laisser passer avec un simple avertissement: une signature
                    // invalide est une tentative d'usurpation, pas un signal consultatif.
                    let sig_check = signing::check_signature(&payload);
                    let sig_sender = payload.intent.get("sender").cloned().unwrap_or_default();
                    let is_agent_register = payload.intent.get("purpose").map(String::as_str) == Some("agent_register");
                    // Trouvaille du 2026-09-04 (deuxieme passe, apres le durcissement de
                    // council_decision): `signing::check_signature` ne verifie QUE la
                    // coherence interne d'un message avec la cle QU'IL revendique
                    // lui-meme dans META.public_key -- jamais que cette cle correspond
                    // a celle REELLEMENT enregistree pour ce sender. Avant ce fix, meme
                    // avec `signature_required=true` (sender deja enregistre), un
                    // attaquant pouvait signer valablement un message ORDINAIRE avec SA
                    // PROPRE cle tout en usurpant sender=<nom deja connu> -- le message
                    // etait "Valid" (auto-coherent) et passait, sans jamais etre compare
                    // au registre. Deja corrige pour purpose=council_decision (l'action
                    // la plus a consequence) le meme jour; etendu ici a TOUT le trafic
                    // signe par un sender deja enregistre. Un seul lookup de registre
                    // sert desormais aux deux besoins (signature_required ET la
                    // comparaison de cle) au lieu de deux lookups separes comme avant.
                    let embedded_pubkey = payload.meta.get("public_key").cloned();
                    let registered_pubkey_for_sender = {
                        let reg = registry.lock().await;
                        reg.agents.iter().find(|a| a.name == sig_sender).and_then(|a| a.public_key.clone())
                    };
                    let signature_required = registered_pubkey_for_sender.is_some();
                    // purpose=agent_register est delibrement EXEMPTE de ce rejet global:
                    // ce bloc court-circuit gere lui-meme son propre sig_check juste plus
                    // bas (bloc B-2), avec des messages d'erreur specifiques au contexte
                    // d'enregistrement (self_signature_required, etc.) -- sans cette
                    // exemption, un agent_register sans signature ou avec une signature
                    // incomplete serait TOUJOURS intercepte ici avec le message generique
                    // "signature_invalid"/"incomplete_signature_fields", rendant les
                    // branches dediees du bloc agent_register inatteignables en pratique
                    // (trouvaille du smoke test live, pas anticipee dans le plan initial).
                    // C'est aussi pourquoi la comparaison de cle ci-dessous n'affecte
                    // jamais une (re)inscription legitime: un agent qui fait tourner sa
                    // cle via un NOUVEL agent_register n'est jamais intercepte ici, quelle
                    // que soit l'ancienne cle enregistree.
                    let sig_rejection: Option<&'static str> = if is_agent_register {
                        None
                    } else {
                        match &sig_check {
                            SignatureCheck::Invalid(_) => Some("signature_invalid"),
                            SignatureCheck::NotPresent if signature_required => Some("missing_signature_for_registered_agent"),
                            SignatureCheck::Valid if signature_required && embedded_pubkey != registered_pubkey_for_sender => Some("public_key_mismatch"),
                            _ => None,
                        }
                    };
                    if let Some(reason) = sig_rejection {
                        eprintln!("[Handler] 🔐 Signature rejetee pour '{}': {}", sig_sender, reason);
                        let detail = if let SignatureCheck::Invalid(d) = &sig_check { d.clone() } else { reason.to_string() };
                        let response = format!(
                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=signature_rejected, reason={}, detail={}]\n---END---\n",
                            reason, detail
                        );
                        socket.write_all(response.as_bytes()).await?;
                        continue;
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

                        // Durci le 2026-09-04, en meme temps que le passage a un
                        // conseil potentiellement multi-membres (CSTL_COUNCIL_MEMBERS,
                        // restricted_council.rs::from_env()): `is_authorized()` seul
                        // ne fait qu'une comparaison de chaine sur `sender` -- avec un
                        // seul membre ("Olivier"), n'importe qui connectait pouvait
                        // deja se pretendre "Olivier" sans preuve, mais ce n'etait pas
                        // un vrai risque tant qu'il n'y avait qu'UN acteur legitime
                        // possible de toute facon. Des qu'un DEUXIEME membre existe,
                        // ca devient un vrai theatre de securite: n'importe qui
                        // connaissant juste les noms ("Olivier", "Alice") peut
                        // fabriquer 2 votes non authentifies et atteindre le quorum
                        // seul. STEP 2a plus haut ne suffit pas non plus a lui seul:
                        // `signing::check_signature` ne verifie QUE la coherence
                        // interne du message (la signature correspond a la cle
                        // qu'IL revendique dans META.public_key) -- jamais que cette
                        // cle est bien celle enregistree pour ce nom. Un attaquant
                        // pourrait donc signer valablement avec SA PROPRE cle tout en
                        // mettant sender=Olivier dans INTENT_PAYLOAD, et STEP 2a
                        // laisserait passer (signature presente et interne-coherente).
                        // Pour un council_decision specifiquement (l'action la plus a
                        // consequence du serveur: ratifier une entree de l'adn_store),
                        // on exige donc EN PLUS que la cle publique EMBARQUEE dans ce
                        // message corresponde EXACTEMENT a celle enregistree pour ce
                        // nom via agent_register -- ca lie enfin l'identite revendiquee
                        // a la preuve cryptographique, pas seulement le message a
                        // lui-meme.
                        let embedded_pubkey = payload.meta.get("public_key").cloned();
                        let registered_pubkey = {
                            let reg = registry.lock().await;
                            reg.agents.iter().find(|a| a.name == sender).and_then(|a| a.public_key.clone())
                        };
                        let council_auth_failure: Option<&'static str> = if !restricted_council.is_authorized(&sender) {
                            Some("not_authorized")
                        } else if sig_check != SignatureCheck::Valid {
                            Some("signature_required")
                        } else if registered_pubkey.is_none() {
                            Some("sender_not_registered")
                        } else if embedded_pubkey != registered_pubkey {
                            Some("public_key_mismatch")
                        } else {
                            None
                        };

                        let response = if let Some(reason) = council_auth_failure {
                            eprintln!("[Handler] ⛔ Council decision rejected: '{}' -- {}", sender, reason);
                            format!(
                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_rejected, reason={}, sender={}]\n---END---\n",
                                reason, sender
                            )
                        } else {
                            match (target_hash, decision) {
                                (Some(hash), Some(decision)) if decision == "commit" => {
                                    // Couche 2 (gouvernance): commit passe desormais par le
                                    // quorum 2/3 (restricted_council.quorum_size()) plutot
                                    // que d'ancrer au premier vote. Avec la config actuelle
                                    // (un seul membre), quorum_size()==1 -- comportement
                                    // identique a avant ce changement, aucune regression.
                                    let quorum_size = restricted_council.quorum_size();
                                    let outcome = {
                                        let store = adn_store.lock().await;
                                        store.cast_commit_vote(&hash, &sender, note.as_deref(), quorum_size)
                                    };
                                    match outcome {
                                        Ok(vote) if vote.quorum_reached => {
                                            eprintln!("[Handler] ⚖️  RestrictedCouncil: commit APPLIQUE sur {} par {} (quorum {}/{})", hash, sender, vote.distinct_voters, vote.quorum_size);
                                            format!(
                                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=council_decision_applied, decision=commit, target_hash={}, by={}, quorum={}/{}, committed=true]\n---END---\n",
                                                hash, sender, vote.distinct_voters, vote.quorum_size
                                            )
                                        }
                                        Ok(vote) => {
                                            eprintln!("[Handler] ⚖️  RestrictedCouncil: vote enregistre sur {} par {} (quorum {}/{}, pas encore atteint)", hash, sender, vote.distinct_voters, vote.quorum_size);
                                            format!(
                                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=council_decision_recorded, decision=commit, target_hash={}, by={}, quorum={}/{}, committed=false]\n---END---\n",
                                                hash, sender, vote.distinct_voters, vote.quorum_size
                                            )
                                        }
                                        Err(e) => {
                                            eprintln!("[Handler] ⚠️  council decision failed: {}", e);
                                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_failed, detail=internal_error]\n---END---\n".to_string()
                                        }
                                    }
                                }
                                (Some(hash), Some(decision)) if decision == "revoke" => {
                                    // Revoke reste a acteur unique: la spec ne mentionne un
                                    // quorum que pour la ratification/commit.
                                    let outcome = {
                                        let store = adn_store.lock().await;
                                        store.revoke(&hash, &sender, note.as_deref())
                                    };
                                    match outcome {
                                        Ok(()) => {
                                            eprintln!("[Handler] ⚖️  RestrictedCouncil: revoke sur {} par {}", hash, sender);
                                            format!(
                                                "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=council_decision_applied, decision=revoke, target_hash={}, by={}]\n---END---\n",
                                                hash, sender
                                            )
                                        }
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
                                (Some(_), Some(other)) => {
                                    eprintln!("[Handler] ⚠️  decision inconnue: {}", other);
                                    "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=council_decision_rejected, reason=unknown_decision]\n---END---\n".to_string()
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

                    // B-2: purpose=agent_register (Couche 7, enregistrement dynamique)
                    // — reutilise sig_check deja calcule en STEP 2a (pas de double
                    // verification). Bootstrap assume: la signature prouve seulement
                    // "je possede cette cle privee", pas "je suis deja connu" -- pas de
                    // PKI/CA, cohérent avec le modele de confiance mono-operateur deja
                    // en place (RestrictedCouncil).
                    //
                    // Item #1 de la liste des choses a faire (2026-09-04): AVANT ce
                    // fix, un ré-enregistrement avec une NOUVELLE clé publique passait
                    // sur simple auto-signature -- celle-ci prouve seulement "je
                    // possède cette nouvelle clé", jamais "je suis le même agent que
                    // celui déjà enregistré sous ce nom". N'importe qui connaissant
                    // juste le NOM d'un agent déjà enregistré pouvait donc voler son
                    // identité en soumettant son propre `agent_register`. Corrigé:
                    // quand le nom est déjà enregistré avec une clé DIFFÉRENTE de la
                    // nouvelle, `INTENT_PAYLOAD.rotation_signature` (signature du même
                    // message avec l'ANCIENNE clé privée, voir
                    // signing::check_rotation_signature) devient obligatoire. Un
                    // premier enregistrement (nom inconnu) ou un ré-enregistrement
                    // avec la MÊME clé (rafraîchir capabilities/trust_score) ne
                    // change pas de comportement -- aucune régression sur le bootstrap
                    // ni sur les smoke-tests existants.
                    if payload.intent.get("purpose").map(String::as_str) == Some("agent_register") {
                        let name = payload.intent.get("name").cloned().unwrap_or_default();
                        let public_key = payload.meta.get("public_key").cloned();
                        let capabilities: Vec<String> = payload.intent.get("capabilities")
                            .map(|c| c.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                            .unwrap_or_default();
                        let trust_score: f64 = payload.intent.get("trust_score")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.5);

                        let response = if name.is_empty() || public_key.is_none() {
                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=agent_register_rejected, reason=missing_name_or_public_key]\n---END---\n".to_string()
                        } else {
                            match &sig_check {
                                SignatureCheck::Valid => {
                                    let existing_pubkey = {
                                        let reg = registry.lock().await;
                                        reg.agents.iter().find(|a| a.name == name).and_then(|a| a.public_key.clone())
                                    };
                                    let is_rotation = matches!(&existing_pubkey, Some(old) if Some(old) != public_key.as_ref());
                                    let rotation_rejection: Option<(&'static str, String)> = if !is_rotation {
                                        None
                                    } else {
                                        // unwrap() sur is_rotation: existing_pubkey est forcement Some ici.
                                        let old_pubkey = existing_pubkey.as_ref().unwrap();
                                        match signing::check_rotation_signature(&payload, old_pubkey) {
                                            SignatureCheck::Valid => None,
                                            SignatureCheck::NotPresent => Some(("rotation_proof_required", "name already registered with a different public_key -- rotation_signature (signed with the OLD private key) is required".to_string())),
                                            SignatureCheck::Invalid(detail) => Some(("rotation_proof_invalid", detail)),
                                        }
                                    };

                                    if let Some((reason, detail)) = rotation_rejection {
                                        eprintln!("[Handler] 🔐 agent_register rejete pour '{}': {} ({})", name, reason, detail);
                                        format!(
                                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=agent_register_rejected, reason={}, detail={}]\n---END---\n",
                                            reason, detail
                                        )
                                    } else {
                                        let card = AgentCard {
                                            name: name.clone(),
                                            version: payload.meta.get("encoder").cloned().unwrap_or_else(|| "unknown".to_string()),
                                            capabilities,
                                            trust_score,
                                            public_key: public_key.clone(),
                                        };
                                        registry.lock().await.register(card);
                                        if is_rotation {
                                            eprintln!("[Handler] 🔄 agent_register: '{}' -- rotation de cle prouvee, cle mise a jour (trust_score={})", name, trust_score);
                                        } else {
                                            eprintln!("[Handler] 🪪 agent_register: '{}' enregistre/mis a jour (trust_score={})", name, trust_score);
                                        }
                                        format!(
                                            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=agent_register_ack, name={}]\n---END---\n",
                                            name
                                        )
                                    }
                                }
                                SignatureCheck::Invalid(detail) => {
                                    eprintln!("[Handler] 🔐 agent_register rejete pour '{}': signature invalide ({})", name, detail);
                                    format!(
                                        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=agent_register_rejected, reason=signature_invalid, detail={}]\n---END---\n",
                                        detail
                                    )
                                }
                                SignatureCheck::NotPresent => {
                                    eprintln!("[Handler] 🔐 agent_register rejete pour '{}': signature absente (auto-signature obligatoire)", name);
                                    "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=agent_register_rejected, reason=self_signature_required]\n---END---\n".to_string()
                                }
                            }
                        };

                        socket.write_all(response.as_bytes()).await?;
                        continue;
                    }

                    // AUDIT: le serveur (orchestrateur) calcule le vrai SHA-256.
                    // L'agent envoie PARENT_HASH=root; on le remplace ici.
                    let entry = { chain.lock().await.append(&payload) };

                    // Persistance de la chaine (Couche 5/8, cable live le
                    // 2026-09-04). Sans cet appel, `chain.append()` ci-dessus
                    // reste PUREMENT en memoire et un redemarrage du serveur
                    // romprait silencieusement la continuite seq/parent_hash,
                    // alors meme que adn_store.put() (plus bas) persiste deja
                    // le payload avec ce meme hash. Echec loggue, jamais
                    // bloquant -- meme politique que le reste du pipeline de
                    // stockage (adn_store.put/put_relations ci-dessous).
                    // Depuis la fusion du 2026-09-04 (ex server/audit_store.rs,
                    // deuxieme Connection SQLite vers le meme fichier), cet
                    // appel passe par adn_store -- une seule Connection, un
                    // seul verrou. adn_store n'est tenu nulle part ailleurs a
                    // ce point du pipeline, donc pas de double-lock ici.
                    if let Err(e) = adn_store.lock().await.save_audit_entry(&entry) {
                        eprintln!("[Handler] ⚠️  adn_store.save_audit_entry failed (chain persistence): {}", e);
                    }

                    // STEP 3: Extract routing info
                    let receiver = payload.intent.get("receiver").cloned().unwrap_or_else(|| "unknown".to_string());
                    let purpose = payload.intent.get("purpose").cloned().unwrap_or_else(|| "unknown".to_string());
                    
                    eprintln!("[Handler] 🔀 Routing to: {} (purpose: {})", receiver, purpose);

                    // STEP 3b: Verification factuelle (Couche 3a) — pour chaque RELATION
                    // du payload, on interroge Wikidata via kb_verify::KbVerifier.
                    // Le module cesse ici d'etre un exemple isole et devient une
                    // capacite reelle du serveur en cours d'execution.
                    //
                    // Trouvaille mineure de l'audit multi-angle (2026-09-03): cette
                    // boucle attendait chaque appel SPARQL sortant SEQUENTIELLEMENT
                    // (.await un par un) -- un payload avec N relations bloquait la
                    // connexion (et retenait potentiellement des locks pris plus loin
                    // dans le pipeline) pendant N fois la latence reseau vers Wikidata,
                    // sans aucune borne sur N. Fix: les verifications sont lancees en
                    // parallele (tokio::task::JoinSet, deja disponible via la dependance
                    // tokio "full" existante -- pas de nouvelle dependance necessaire),
                    // et on borne explicitement le nombre de relations verifiees par
                    // payload (MAX_RELATIONS_TO_VERIFY) pour eviter qu'un payload avec
                    // des centaines de RELATION ne devienne un vecteur de déni de
                    // service par amplification de requetes sortantes.
                    const MAX_RELATIONS_TO_VERIFY: usize = 50;
                    let relations_to_verify: Vec<(usize, String, String, String)> = payload.relations.iter()
                        .enumerate()
                        .filter_map(|(idx, relation)| {
                            let subject = relation.get("subject").cloned()?;
                            let predicate = relation.get("type").cloned()?;
                            let object = relation.get("object").cloned()?;
                            Some((idx, subject, predicate, object))
                        })
                        .take(MAX_RELATIONS_TO_VERIFY)
                        .collect();
                    if payload.relations.len() > MAX_RELATIONS_TO_VERIFY {
                        eprintln!(
                            "[Handler] ⚠️  {} relations dans le payload, verification KB bornee aux {} premieres",
                            payload.relations.len(), MAX_RELATIONS_TO_VERIFY
                        );
                    }

                    // Budget mur-a-mur par relation (2026-09-05, trouvaille en direct sur
                    // une vraie machine utilisateur): `verify_relation` peut declencher
                    // jusqu'a `max_expansions` (40) appels SPARQL sequentiels lors d'une
                    // expansion de chaine transitive (`find_property_chain`), chacun avec
                    // son propre timeout de 15s -- sur un reseau lent/instable vers
                    // Wikidata (confirme en direct: erreurs `network_error` intermittentes
                    // sur la meme machine), la latence totale d'UNE SEULE relation a
                    // largement depasse le timeout socket du client Python
                    // (`cstl_client.py`, 15s), provoquant un TimeoutError cote client alors
                    // que le serveur avait deja repondu avec succes quelques secondes plus
                    // tard (log serveur: "Response sent" + "Connection closed" bien apres
                    // le timeout du client). Cette couche (3a) est documentee comme
                    // PUREMENT INFORMATIVE -- elle n'a jamais pu rejeter ni modifier le
                    // statut d'un payload (deja valide et persiste en adn_store avant
                    // d'atteindre ce bloc) -- donc la couper court sur un budget global est
                    // sans risque de correction, seulement une degradation "unproven" au
                    // lieu d'une reponse tardive. KB_VERIFICATION_WALL_CLOCK_BUDGET choisi
                    // nettement sous le timeout client (15s) pour laisser une vraie marge
                    // de securite plutot que de deplacer le probleme d'un cran.
                    const KB_VERIFICATION_WALL_CLOCK_BUDGET: std::time::Duration = std::time::Duration::from_secs(8);

                    let mut verify_tasks = tokio::task::JoinSet::new();
                    for (idx, subject, predicate, object) in relations_to_verify {
                        let kb_verifier = kb_verifier.clone();
                        verify_tasks.spawn(async move {
                            eprintln!("[Handler] 🔎 Verifying relation: {} {} {}", subject, predicate, object);
                            let result = match tokio::time::timeout(
                                KB_VERIFICATION_WALL_CLOCK_BUDGET,
                                kb_verifier.verify_relation(&subject, &predicate, &object, "fr", 4, 40),
                            ).await {
                                Ok(result) => result,
                                Err(_) => {
                                    eprintln!(
                                        "[Handler]    ⏱️  verification KB interrompue apres {:?} (budget mur-a-mur depasse, reseau Wikidata lent/instable) -- traitee comme non concluante, jamais bloquant",
                                        KB_VERIFICATION_WALL_CLOCK_BUDGET
                                    );
                                    crate::kb_verify::VerificationResult {
                                        verified: "unchallenged_unproven".to_string(),
                                        source_url: None,
                                        reason: "kb_verification_wall_clock_timeout_exceeded".to_string(),
                                        subject_qid: None,
                                        object_qid: None,
                                        check_method: None,
                                        chain: None,
                                        property_id: None,
                                    }
                                }
                            };
                            eprintln!("[Handler]    -> {} ({})", result.verified, result.reason);
                            (idx, subject, predicate, object, result)
                        });
                    }

                    let mut verification_results = Vec::new();
                    while let Some(joined) = verify_tasks.join_next().await {
                        match joined {
                            Ok(tuple) => verification_results.push(tuple),
                            Err(e) => eprintln!("[Handler] ⚠️  tache de verification KB annulee/paniquee: {}", e),
                        }
                    }
                    // Ordre de completion des taches paralleles != ordre du payload --
                    // on retrie par index d'origine pour une reponse deterministe,
                    // stable d'un run a l'autre independamment de la latence reseau.
                    verification_results.sort_by_key(|(idx, ..)| *idx);

                    let mut verification_lines = String::new();
                    for (_idx, subject, predicate, object, result) in verification_results {
                        verification_lines.push_str(&format!(
                            "VERIFICATION [subject={}, predicate={}, object={}, verified={}, source={}]\n",
                            subject,
                            predicate,
                            object,
                            result.verified,
                            result.source_url.unwrap_or_else(|| "none".to_string())
                        ));
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
                        "[Handler] 🧪 ExecutionLab: consistent={} contradictions={} cycles={} temporal_cycles={} -> sigma={}",
                        consistency.consistent, consistency.contradictions.len(), consistency.cycles.len(),
                        consistency.temporal_cycles.len(), sigma
                    );

                    // STEP 3c-deontic: Audit deontique HISTORIQUE (Couche 8, 2026-09-04)
                    // — meme principe que STEP 3c ci-dessus (charger seulement ce dont
                    // on se sert), mais pour les relations portant `modality`. Verifie
                    // les MUST/MUST_NOT du payload courant contre TOUT ce qui a ete
                    // persiste par des payloads PRECEDENTS (potentiellement d'autres
                    // agents) — une contradiction Axiome D qui s'etale sur plusieurs
                    // requetes, invisible a la verification intra-payload bloquante
                    // (server/validator.rs::validate_deontic_constraints, STEP 2).
                    // INFORMATIF SEULEMENT, comme la coherence factuelle ci-dessus:
                    // jamais de rejet, une contradiction a travers l'historique peut
                    // etre un vrai desaccord entre agents/moments, pas une erreur de
                    // protocole (contrairement a se contredire dans le meme souffle).
                    let deontic_history = {
                        let store = adn_store.lock().await;
                        store.deontic_relations_history().unwrap_or_else(|e| {
                            eprintln!("[Handler] ⚠️  adn_store.deontic_relations_history failed: {}", e);
                            Vec::new()
                        })
                    };
                    let deontic_audit = execution_lab::check_deontic_consistency_with_history(&payload.relations, &deontic_history);
                    if !deontic_audit.consistent {
                        eprintln!(
                            "[Handler] ⚖️  Deontic audit: {} violation(s) Axiome D detectee(s) contre l'historique",
                            deontic_audit.violations.len()
                        );
                        for v in &deontic_audit.violations {
                            eprintln!(
                                "[Handler]    -> ({}) {} {} vs ({}) {} {}",
                                v.required_by, v.subject, v.object, v.forbidden_by, v.subject, v.object
                            );
                        }
                    }

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
                    if !consistency.temporal_cycles.is_empty() {
                        telegram_details.push_str("\nTemporal cycles (E702):\n");
                        for cy in &consistency.temporal_cycles {
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

                    // STEP 3c-governance: Couche 2 (gouvernance/resilience) — circuit
                    // breaker + drift d'operateur, observation seule (src/governance.rs).
                    // Avant cette etape, la Couche 2 etait completement vide (aucun
                    // circuit breaker, aucune detection de drift n'existait nulle part
                    // dans ce depot). Ce module calcule un etat par expediteur et
                    // l'expose dans la reponse, escalade plus fort via Telegram si un
                    // seuil est franchi, mais NE REJETTE JAMAIS le payload — coherent
                    // avec le seul mecanisme de blocage reel du pipeline (securite/
                    // parse/validation, STEP 0/1/2 ci-dessus): tout le reste
                    // (verification KB, coherence, avertissements d'operateurs
                    // semantiques) est deja purement consultatif.
                    let governance_sender = payload.intent.get("sender").cloned().unwrap_or_else(|| "unknown".to_string());
                    let mut governance_reasons = Vec::new();
                    if !consistency.consistent {
                        governance_reasons.push(crate::governance::EventReason::Inconsistency);
                    }
                    if !semantic_warnings.is_empty() {
                        governance_reasons.push(crate::governance::EventReason::SemanticWarning);
                    }
                    let gov_state = {
                        governance.lock().await.record(&governance_sender, &governance_reasons)
                    };
                    // Persistance (2026-09-05): meme grain que l'audit trail
                    // (un evenement par payload, via save_audit_entry
                    // plus bas) -- l'etat de gouvernance survivait avant ca
                    // uniquement dans `governance` en memoire, perdu au
                    // redemarrage. `prune_before` supprime dans la meme
                    // requete tout ce qui est deja hors de la plus grande
                    // fenetre glissante (DRIFT_WINDOW), pour que la table ne
                    // grossisse jamais sans limite.
                    let gov_ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let gov_prune_before = gov_ts - crate::governance::DRIFT_WINDOW.as_secs() as i64;
                    if let Err(e) = adn_store.lock().await.save_governance_event(
                        &governance_sender,
                        gov_ts,
                        !consistency.consistent,
                        !semantic_warnings.is_empty(),
                        gov_prune_before,
                    ) {
                        eprintln!("[Handler] ⚠️  governance event persist failed: {}", e);
                    }
                    if gov_state.should_alert {
                        if let Err(e) = adn_store.lock().await.save_governance_alert(&governance_sender, gov_ts) {
                            eprintln!("[Handler] ⚠️  governance alert persist failed: {}", e);
                        }
                    }
                    eprintln!(
                        "[Handler] 🏛️  Governance: sender={} circuit={} breaker_trips={} drift_ratio={:.2} drift_flagged={}",
                        governance_sender,
                        if gov_state.circuit_open { "open" } else { "closed" },
                        gov_state.breaker_trips, gov_state.drift_ratio, gov_state.drift_flagged
                    );
                    let governance_line = format!(
                        "GOVERNANCE [sender={}, circuit={}, breaker_trips={}, drift_ratio={:.2}, drift_flagged={}]\n",
                        governance_sender,
                        if gov_state.circuit_open { "open" } else { "closed" },
                        gov_state.breaker_trips, gov_state.drift_ratio, gov_state.drift_flagged
                    );
                    if gov_state.should_alert {
                        if let Some(telegram) = &telegram {
                            let telegram = telegram.clone();
                            let sender_for_alert = governance_sender.clone();
                            let circuit_open = gov_state.circuit_open;
                            let breaker_trips = gov_state.breaker_trips;
                            let drift_flagged = gov_state.drift_flagged;
                            let drift_ratio = gov_state.drift_ratio;
                            tokio::spawn(async move {
                                let message = format!(
                                    "🚨 GOVERNANCE ALERT: sender={} circuit_open={} breaker_trips={} drift_flagged={} drift_ratio={:.2}",
                                    sender_for_alert, circuit_open, breaker_trips, drift_flagged, drift_ratio
                                );
                                if let Err(e) = telegram.send_message(&message).await {
                                    eprintln!("[Telegram] ⚠️  alerte gouvernance echouee: {}", e);
                                }
                            });
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
                    // Ligne dediee (comme deontic_audit_line ci-dessous) pour le cycle
                    // temporel (E702, 2026-09-05) -- absente quand aucun cycle temporel
                    // n'est detecte, pas de bruit sur le trafic normal. Code separe de
                    // la ligne CONSISTENCY generique ci-dessus parce qu'un cycle
                    // temporel a un code d'erreur documente (voir CSTL_SPEC_v5_0.md
                    // §16.4) que les cycles part_of/located_in n'ont pas.
                    let temporal_cycle_line = if consistency.temporal_cycles.is_empty() {
                        String::new()
                    } else {
                        let paths = consistency.temporal_cycles.iter()
                            .map(|cy| cy.path.join(" -> "))
                            .collect::<Vec<_>>()
                            .join("; ");
                        format!(
                            "SEMANTIC_WARNING [detail=E702: temporal cycle detected ({})]\n",
                            paths
                        )
                    };
                    // Absent quand aucune RELATION de ce payload ne porte de `modality`
                    // ET que l'audit contre l'historique est propre -- pas de bruit sur
                    // le trafic factuel normal (l'immense majorite des payloads).
                    let deontic_audit_line = if deontic_audit.violations.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "DEONTIC_AUDIT [consistent=false, violations={}]\n",
                            deontic_audit.violations.len()
                        )
                    };

                    // STEP 4: Try to route to agent — agent_registry est maintenant
                    // Arc<Mutex<_>> (B-1, dynamic registration): le nom est clone HORS
                    // du lock pour ne jamais tenir le MutexGuard pendant le
                    // write_all().await qui suit.
                    let routed_agent_name = {
                        let reg = registry.lock().await;
                        reg.route("communication").map(|a| a.name.clone())
                    };
                    if let Some(agent_name) = routed_agent_name {
                        eprintln!("[Handler] ✅ Found agent: {}", agent_name);

                        // STEP 5: Build response
                        let response = format!(
                            "#!CSTL v5.0.0 MODE=A\n\
                            META [encoder=CstlNativeServer, produced_by=Server, status=processed]\n\
                            INTENT_PAYLOAD [purpose=acknowledgement, sender=server, receiver={}]\n\
                            RELATION [type=received, subject={}, status=valid]\n\
                            {}\
                            {}\
                            {}\
                            {}\
                            {}\
                            {}\
                            AUDIT [hash={}, parent_hash={}, seq={}]\n\
                            ---END---\n",
                            payload.intent.get("sender").cloned().unwrap_or_else(|| "unknown".to_string()),
                            purpose,
                            verification_lines,
                            consistency_line,
                            temporal_cycle_line,
                            deontic_audit_line,
                            semantic_warning_lines,
                            governance_line,
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
