//! src/server/quorum_wire.rs — colle `quorum.rs` (Layer 2, consensus BFT
//! multi-agent) au pipeline de requetes/reponses live de `handler.rs`
//! (2026-10-01). Meme motivation que `castle_wire.rs` pour CASTLE: ce
//! module existait depuis une session precedente, avec 6 tests passants
//! (`QuorumMember`/`VoteMessage`/`QuorumState`, verification de signature
//! Ed25519, seuil BFT, circuit breaker) mais `pub mod quorum;` seul --
//! aucun appelant, aucun chemin reseau, aucune persistance. Trouve par la
//! meme methode que CASTLE: un grep des modules `pub mod` jamais
//! references ailleurs dans le depot.
//!
//! Portee v1, assumee et documentee plutot que cachee:
//!
//! - **Membre = agent deja enregistre.** Reutilise `AgentRegistry`
//!   (purpose=agent_register, deja cable) au lieu de construire un
//!   deuxieme systeme d'identite parallele avec `QuorumMember`. Un agent
//!   est eligible a proposer/voter s'il est enregistre ET possede une
//!   `public_key` -- un agent legacy non signe (`public_key=None`, ex.
//!   alice/bob bootstrap) ne peut pas participer: un consensus Byzantin a
//!   besoin d'identites cryptographiques verifiables, pas juste d'un nom.
//! - **Pas de deuxieme signature.** `VoteMessage.signature` /
//!   `VoteMessage::verify_signature` existent dans `quorum.rs` mais NE
//!   SONT PAS exerces ici: le message CSTL entier qui porte ce vote est
//!   DEJA signe et verifie par STEP 2a de `handler.rs` (meme discipline
//!   que `council_decision`/`agent_register` -- voir
//!   `embedded_pubkey != registered_pubkey` plus bas, qui reprend
//!   exactement leur garde). Exiger une deuxieme signature sur un
//!   sous-ensemble quasi identique des memes champs n'ajouterait aucune
//!   garantie reelle. `VoteMessage` reste utilise comme type de valeur
//!   (vote_id/content_hash pour l'immuabilite du registre persiste).
//! - **Pas d'avancement de round automatique par timeout.** `round` reste
//!   a 1 pour toute proposition de cette version -- un vrai retry-sur-
//!   timeout exigerait un ordonnanceur de fond, hors de portee d'un
//!   cablage requete-entre/reponse-sort. `should_activate_circuit_breaker`
//!   (qui prend un `failed_round_count` qui n'existe pas encore ici) et
//!   `aggregate_health_score` (qui prend des `QuorumMember` qu'on n'a pas,
//!   voir ci-dessus) ne sont donc PAS appeles -- une activation MANUELLE
//!   existe a la place (`purpose=quorum_circuit_breaker`), reservee a
//!   `RestrictedCouncil` (meme porte d'autorite que `council_decision`).
//! - **`threshold` fige a la creation**, calcule depuis le nombre d'agents
//!   eligibles au moment de la proposition (`compute_threshold`, BFT
//!   ⌈2/3·n⌉) -- un agent qui s'enregistre APRES n'abaisse ni ne hausse le
//!   seuil d'une proposition deja ouverte, coherent avec `QuorumState`
//!   elle-meme (`threshold` est un champ fixe, jamais recalcule par
//!   `add_vote`/`check_consensus`).

use std::sync::Arc;

use super::parser::CstlPayload;
use super::quorum::{compute_threshold, QuorumState, VoteMessage};
use super::ServerContext;
use crate::signing::SignatureCheck;

/// Verifie que `sender` est un agent enregistre, avec `public_key`, ET que
/// la cle embarquee dans CE message (`META.public_key`) correspond
/// exactement a celle enregistree -- meme garde que `council_decision`
/// dans `handler.rs`, extraite ici pour etre partagee par les trois
/// purposes de ce module.
async fn verify_registered_voter(
    payload: &CstlPayload,
    ctx: &Arc<ServerContext>,
    sender: &str,
    sig_check: &SignatureCheck,
) -> Option<&'static str> {
    if *sig_check != SignatureCheck::Valid {
        return Some("signature_required");
    }
    let embedded_pubkey = payload.meta.get("public_key").cloned();
    let registered_pubkey = {
        let reg = ctx.agent_registry.read().await;
        reg.agents.iter().find(|a| a.name == sender).and_then(|a| a.public_key.clone())
    };
    if registered_pubkey.is_none() {
        return Some("sender_not_registered_or_unsigned");
    }
    if embedded_pubkey != registered_pubkey {
        return Some("public_key_mismatch");
    }
    None
}

/// `purpose=quorum_propose` -- cree une nouvelle proposition. Champs
/// attendus: `INTENT_PAYLOAD.proposal_id` (unique), `INTENT_PAYLOAD.description`
/// (optionnel, texte libre pour l'audit).
pub async fn handle_quorum_propose(
    payload: &CstlPayload,
    ctx: &Arc<ServerContext>,
    sig_check: &SignatureCheck,
) -> String {
    let sender = payload.intent.get("sender").cloned().unwrap_or_default();
    let proposal_id = payload.intent.get("proposal_id").cloned().unwrap_or_default();
    let description = payload.intent.get("description").cloned().unwrap_or_default();

    if proposal_id.is_empty() {
        return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=missing_proposal_id]\n---END---\n".to_string();
    }

    if let Some(reason) = verify_registered_voter(payload, ctx, &sender, sig_check).await {
        log::error!("[Quorum] propose rejected: '{}' -- {}", sender, reason);
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason={}, sender={}]\n---END---\n",
            reason, sender
        );
    }

    let already_exists = match ctx.adn_store.lock().await.get_quorum_proposal(&proposal_id) {
        Ok(existing) => existing.is_some(),
        Err(e) => {
            log::error!("[Quorum] propose: lookup failed: {}", e);
            return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
        }
    };
    if already_exists {
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=duplicate_proposal_id, proposal_id={}]\n---END---\n",
            proposal_id
        );
    }

    let member_count = {
        let reg = ctx.agent_registry.read().await;
        reg.agents.iter().filter(|a| a.public_key.is_some()).count() as u64
    };
    let threshold = compute_threshold(member_count);
    let state = QuorumState::new(proposal_id.clone(), threshold);

    if let Err(e) = ctx.adn_store.lock().await.insert_quorum_proposal(&state, &sender, &description) {
        log::error!("[Quorum] propose: persist failed: {}", e);
        return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
    }

    log::info!(
        "[Quorum] proposal '{}' created by '{}' (member_count={}, threshold={})",
        proposal_id, sender, member_count, threshold
    );
    format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=quorum_proposal_created, proposal_id={}, threshold={}, member_count={}]\n---END---\n",
        proposal_id, threshold, member_count
    )
}

/// `purpose=quorum_vote` -- enregistre un vote sur une proposition
/// existante. Champs attendus: `INTENT_PAYLOAD.proposal_id`,
/// `INTENT_PAYLOAD.decision` (`yea`/`nay`/`abstain`).
pub async fn handle_quorum_vote(
    payload: &CstlPayload,
    ctx: &Arc<ServerContext>,
    sig_check: &SignatureCheck,
) -> String {
    let sender = payload.intent.get("sender").cloned().unwrap_or_default();
    let proposal_id = payload.intent.get("proposal_id").cloned().unwrap_or_default();
    let decision = payload.intent.get("decision").cloned().unwrap_or_default();

    if proposal_id.is_empty() || decision.is_empty() {
        return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=missing_proposal_id_or_decision]\n---END---\n".to_string();
    }
    if !matches!(decision.to_lowercase().as_str(), "yea" | "nay" | "abstain") {
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=invalid_decision, decision={}]\n---END---\n",
            decision
        );
    }

    if let Some(reason) = verify_registered_voter(payload, ctx, &sender, sig_check).await {
        log::error!("[Quorum] vote rejected: '{}' on '{}' -- {}", sender, proposal_id, reason);
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason={}, sender={}]\n---END---\n",
            reason, sender
        );
    }

    let mut state = {
        let store = ctx.adn_store.lock().await;
        match store.get_quorum_proposal(&proposal_id) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return format!(
                    "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=unknown_proposal, proposal_id={}]\n---END---\n",
                    proposal_id
                );
            }
            Err(e) => {
                log::error!("[Quorum] vote: lookup failed: {}", e);
                return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
            }
        }
    };

    if state.final_decision != "unknown" {
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=proposal_already_decided, proposal_id={}, final_decision={}]\n---END---\n",
            proposal_id, state.final_decision
        );
    }
    if state.circuit_breaker_active {
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=circuit_breaker_active, proposal_id={}]\n---END---\n",
            proposal_id
        );
    }

    let already_voted = match ctx.adn_store.lock().await.has_voted(&proposal_id, &sender) {
        Ok(v) => v,
        Err(e) => {
            log::error!("[Quorum] vote: has_voted check failed: {}", e);
            return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
        }
    };
    if already_voted {
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=duplicate_vote, proposal_id={}, sender={}]\n---END---\n",
            proposal_id, sender
        );
    }

    // add_vote() ne peut echouer ici que sur circuit_breaker_active (deja
    // exclu ci-dessus) ou une decision invalide (deja validee ci-dessus) --
    // les deux cas sont donc deja geres, cet unwrap() est sur.
    state.add_vote(&decision).expect("circuit breaker et decision deja valides ci-dessus");

    let sequence_number = match ctx.adn_store.lock().await.count_quorum_votes(&proposal_id) {
        Ok(n) => (n as u64) + 1,
        Err(e) => {
            log::error!("[Quorum] vote: sequence lookup failed: {}", e);
            return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
        }
    };
    let vote = VoteMessage::new(proposal_id.clone(), sender.clone(), decision.clone(), sequence_number);

    let (consensus_reached, final_decision) = state.check_consensus();
    if consensus_reached {
        state.final_decision = final_decision.clone();
    }

    {
        let store = ctx.adn_store.lock().await;
        if let Err(e) = store.record_quorum_vote(&vote) {
            log::error!("[Quorum] vote: persist vote failed: {}", e);
            return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
        }
        if let Err(e) = store.update_quorum_proposal(&state) {
            log::error!("[Quorum] vote: persist state failed: {}", e);
            return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
        }
    }

    log::info!(
        "[Quorum] vote recorded: '{}' voted '{}' on '{}' (yea={}, nay={}, abstain={}, threshold={}, consensus={})",
        sender, decision, proposal_id, state.yea_count, state.nay_count, state.abstain_count, state.threshold, consensus_reached
    );
    format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=quorum_vote_recorded, proposal_id={}, decision={}, yea={}, nay={}, abstain={}, threshold={}, consensus_reached={}, final_decision={}]\n---END---\n",
        proposal_id, decision, state.yea_count, state.nay_count, state.abstain_count, state.threshold,
        consensus_reached, state.final_decision
    )
}

/// `purpose=quorum_circuit_breaker` -- arret manuel d'une proposition
/// bloquee ou suspecte. Reserve a `RestrictedCouncil` (meme porte
/// d'autorite que `council_decision`) -- pas de mecanisme automatique de
/// detection de rounds-echoues dans cette version (voir le commentaire de
/// module). Champs attendus: `INTENT_PAYLOAD.proposal_id`,
/// `INTENT_PAYLOAD.reason` (texte libre).
pub async fn handle_quorum_circuit_breaker(
    payload: &CstlPayload,
    ctx: &Arc<ServerContext>,
    sig_check: &SignatureCheck,
) -> String {
    let sender = payload.intent.get("sender").cloned().unwrap_or_default();
    let proposal_id = payload.intent.get("proposal_id").cloned().unwrap_or_default();
    let reason = payload.intent.get("reason").cloned().unwrap_or_else(|| "unspecified".to_string());

    if proposal_id.is_empty() {
        return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=missing_proposal_id]\n---END---\n".to_string();
    }

    if !ctx.restricted_council.is_authorized(&sender) {
        log::error!("[Quorum] circuit_breaker rejected: '{}' not authorized", sender);
        return format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=not_authorized, sender={}]\n---END---\n",
            sender
        );
    }
    if *sig_check != SignatureCheck::Valid {
        return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=signature_required]\n---END---\n".to_string();
    }

    let mut state = {
        let store = ctx.adn_store.lock().await;
        match store.get_quorum_proposal(&proposal_id) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return format!(
                    "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=unknown_proposal, proposal_id={}]\n---END---\n",
                    proposal_id
                );
            }
            Err(e) => {
                log::error!("[Quorum] circuit_breaker: lookup failed: {}", e);
                return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
            }
        }
    };

    state.activate_circuit_breaker(reason.clone());
    if let Err(e) = ctx.adn_store.lock().await.update_quorum_proposal(&state) {
        log::error!("[Quorum] circuit_breaker: persist failed: {}", e);
        return "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=error]\nINTENT_PAYLOAD [purpose=quorum_rejected, reason=internal_error]\n---END---\n".to_string();
    }

    log::info!("[Quorum] circuit breaker activated on '{}' by '{}': {}", proposal_id, sender, reason);
    format!(
        "#!CSTL v5.0.0 MODE=A\nMETA [encoder=CstlNativeServer, produced_by=Server, status=processed]\nINTENT_PAYLOAD [purpose=quorum_circuit_breaker_activated, proposal_id={}, reason={}]\n---END---\n",
        proposal_id, reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adn_store::AdnStore;
    use crate::agent_discovery::{AgentCard, AgentRegistry};
    use crate::restricted_council::RestrictedCouncil;
    use crate::server::audit::HashChain;
    use crate::governance::GovernanceTracker;
    use crate::server::ServerContext;
    use crate::signing;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::HashMap;
    use tokio::sync::{Mutex, RwLock};

    /// Construit un ServerContext minimal pour les tests de ce module --
    /// mirroir reduit de ce que `CstlNativeServer::start()` construit
    /// reellement, juste assez pour exercer quorum_wire.rs en isolation.
    fn test_ctx() -> Arc<ServerContext> {
        Arc::new(ServerContext {
            agent_registry: Arc::new(RwLock::new(AgentRegistry::new())),
            chain: Arc::new(Mutex::new(HashChain::new())),
            kb_verifier: Arc::new(crate::kb_verify::KbVerifier::new()),
            adn_store: Arc::new(Mutex::new(AdnStore::open(":memory:").unwrap())),
            restricted_council: Arc::new(RestrictedCouncil::single_member("olivier")),
            telegram: None,
            obsidian: None,
            governance: Arc::new(Mutex::new(GovernanceTracker::with_defaults())),
            sigma_calibrator: Arc::new(Mutex::new(crate::calibration::SigmaCalibrator::new(0.2))),
            collect_response_corpus: false,
        })
    }

    /// Enregistre un agent de test avec une vraie paire de cles Ed25519 et
    /// retourne la cle de signature -- pour signer de vrais payloads CSTL
    /// qui passeront STEP 2a (`signing::check_signature`) exactement comme
    /// un vrai client le ferait.
    async fn register_test_agent(ctx: &Arc<ServerContext>, name: &str) -> SigningKey {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
        ctx.agent_registry.write().await.register(AgentCard {
            name: name.to_string(),
            version: "test".to_string(),
            capabilities: vec![],
            trust_score: 0.9,
            public_key: Some(public_key_hex),
        });
        signing_key
    }

    /// Construit et signe un payload CSTL minimal pour `purpose` avec les
    /// champs INTENT_PAYLOAD donnes -- signe via `signing::signing_bytes`,
    /// meme fonction que le serveur utilise pour verifier (voir
    /// `src/signing.rs` / `src/server/audit.rs::signing_bytes`).
    fn build_signed_payload(
        sender: &str,
        public_key_hex: &str,
        signing_key: &SigningKey,
        purpose: &str,
        extra_intent: &[(&str, &str)],
    ) -> CstlPayload {
        let mut intent = HashMap::new();
        intent.insert("purpose".to_string(), purpose.to_string());
        intent.insert("sender".to_string(), sender.to_string());
        intent.insert("receiver".to_string(), "server".to_string());
        for (k, v) in extra_intent {
            intent.insert(k.to_string(), v.to_string());
        }

        let mut meta = HashMap::new();
        meta.insert("encoder".to_string(), "TestAgent".to_string());
        meta.insert("produced_by".to_string(), sender.to_string());
        meta.insert("public_key".to_string(), public_key_hex.to_string());

        let mut payload = CstlPayload {
            version: "v5.0.0".to_string(),
            mode: "A".to_string(),
            meta,
            intent,
            relations: vec![],
            defines: vec![],
            parse_warnings: vec![],
            guardrail_reports: vec![],
            scope_lock: None,
            uncertainty: vec![],
            error_signal_request: None,
            raw: String::new(),
        };

        let bytes = crate::server::audit::signing_bytes(&payload);
        let signature = signing_key.sign(&bytes);
        payload.intent.insert("signature".to_string(), hex::encode(signature.to_bytes()));
        payload
    }

    #[tokio::test]
    async fn test_propose_rejects_unregistered_sender() {
        let ctx = test_ctx();
        let mut intent = HashMap::new();
        intent.insert("purpose".to_string(), "quorum_propose".to_string());
        intent.insert("sender".to_string(), "ghost".to_string());
        intent.insert("proposal_id".to_string(), "prop_1".to_string());
        let payload = CstlPayload {
            version: "v5.0.0".to_string(), mode: "A".to_string(),
            meta: HashMap::new(), intent, relations: vec![], defines: vec![],
            parse_warnings: vec![], guardrail_reports: vec![], scope_lock: None,
            uncertainty: vec![], error_signal_request: None, raw: String::new(),
        };

        let response = handle_quorum_propose(&payload, &ctx, &SignatureCheck::NotPresent).await;
        assert!(response.contains("quorum_rejected"));
        assert!(response.contains("signature_required") || response.contains("sender_not_registered"));
    }

    #[tokio::test]
    async fn test_full_propose_vote_consensus_flow() {
        let ctx = test_ctx();

        // Trois agents enregistres -- threshold BFT attendu: compute_threshold(3) = 2.
        let alice_key = register_test_agent(&ctx, "alice").await;
        let bob_key = register_test_agent(&ctx, "bob").await;
        let _carol_key = register_test_agent(&ctx, "carol").await;
        let alice_pub = hex::encode(alice_key.verifying_key().to_bytes());
        let bob_pub = hex::encode(bob_key.verifying_key().to_bytes());

        let propose_payload = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_propose",
            &[("proposal_id", "prop_live"), ("description", "test proposal")],
        );
        let sig_check = signing::check_signature(&propose_payload);
        assert_eq!(sig_check, SignatureCheck::Valid, "le payload de test doit se signer/verifier correctement");

        let response = handle_quorum_propose(&propose_payload, &ctx, &sig_check).await;
        assert!(response.contains("quorum_proposal_created"), "reponse inattendue: {response}");
        assert!(response.contains("threshold=2"), "compute_threshold(3) doit donner 2: {response}");

        // Premier vote (alice, yea) -- pas encore de consensus.
        let vote1 = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_vote",
            &[("proposal_id", "prop_live"), ("decision", "yea")],
        );
        let sig1 = signing::check_signature(&vote1);
        let r1 = handle_quorum_vote(&vote1, &ctx, &sig1).await;
        assert!(r1.contains("quorum_vote_recorded"), "reponse inattendue: {r1}");
        assert!(r1.contains("consensus_reached=false"), "un seul vote sur 2 requis ne doit pas atteindre consensus: {r1}");

        // Deuxieme vote (bob, yea) -- consensus atteint.
        let vote2 = build_signed_payload(
            "bob", &bob_pub, &bob_key, "quorum_vote",
            &[("proposal_id", "prop_live"), ("decision", "yea")],
        );
        let sig2 = signing::check_signature(&vote2);
        let r2 = handle_quorum_vote(&vote2, &ctx, &sig2).await;
        assert!(r2.contains("quorum_vote_recorded"), "reponse inattendue: {r2}");
        assert!(r2.contains("consensus_reached=true"), "2/2 yea avec threshold=2 doit atteindre consensus: {r2}");
        assert!(r2.contains("final_decision=yea"), "reponse inattendue: {r2}");

        // Un vote apres decision finale doit etre refuse.
        let late_vote = build_signed_payload(
            "carol", &hex::encode(_carol_key.verifying_key().to_bytes()), &_carol_key,
            "quorum_vote", &[("proposal_id", "prop_live"), ("decision", "nay")],
        );
        let sig3 = signing::check_signature(&late_vote);
        let r3 = handle_quorum_vote(&late_vote, &ctx, &sig3).await;
        assert!(r3.contains("proposal_already_decided"), "reponse inattendue: {r3}");
    }

    #[tokio::test]
    async fn test_vote_rejects_public_key_mismatch_impersonation() {
        // Un attaquant signe valablement avec SA PROPRE cle tout en
        // revendiquant sender=alice -- doit etre rejete, meme garde que
        // council_decision (voir le commentaire de module).
        let ctx = test_ctx();
        let alice_key = register_test_agent(&ctx, "alice").await;
        let alice_pub = hex::encode(alice_key.verifying_key().to_bytes());

        let propose_payload = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_propose",
            &[("proposal_id", "prop_imp")],
        );
        let sig_check = signing::check_signature(&propose_payload);
        handle_quorum_propose(&propose_payload, &ctx, &sig_check).await;

        let attacker_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let attacker_pub = hex::encode(attacker_key.verifying_key().to_bytes());
        // sender=alice mais signe (et META.public_key) avec la cle de l'ATTAQUANT.
        let forged_vote = build_signed_payload(
            "alice", &attacker_pub, &attacker_key, "quorum_vote",
            &[("proposal_id", "prop_imp"), ("decision", "yea")],
        );
        let sig_check2 = signing::check_signature(&forged_vote);
        assert_eq!(sig_check2, SignatureCheck::Valid, "le message lui-meme est signe correctement par l'attaquant");

        let response = handle_quorum_vote(&forged_vote, &ctx, &sig_check2).await;
        assert!(response.contains("public_key_mismatch"), "l'usurpation doit etre detectee: {response}");
    }

    #[tokio::test]
    async fn test_duplicate_vote_rejected() {
        let ctx = test_ctx();
        let alice_key = register_test_agent(&ctx, "alice").await;
        let bob_key = register_test_agent(&ctx, "bob").await;
        let alice_pub = hex::encode(alice_key.verifying_key().to_bytes());
        let bob_pub = hex::encode(bob_key.verifying_key().to_bytes());

        let propose_payload = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_propose",
            &[("proposal_id", "prop_dup")],
        );
        let sig_check = signing::check_signature(&propose_payload);
        handle_quorum_propose(&propose_payload, &ctx, &sig_check).await;

        let vote1 = build_signed_payload(
            "bob", &bob_pub, &bob_key, "quorum_vote",
            &[("proposal_id", "prop_dup"), ("decision", "yea")],
        );
        let sig1 = signing::check_signature(&vote1);
        handle_quorum_vote(&vote1, &ctx, &sig1).await;

        let vote2 = build_signed_payload(
            "bob", &bob_pub, &bob_key, "quorum_vote",
            &[("proposal_id", "prop_dup"), ("decision", "nay")],
        );
        let sig2 = signing::check_signature(&vote2);
        let r2 = handle_quorum_vote(&vote2, &ctx, &sig2).await;
        assert!(r2.contains("duplicate_vote"), "reponse inattendue: {r2}");
    }

    #[tokio::test]
    async fn test_circuit_breaker_requires_restricted_council_authorization() {
        let ctx = test_ctx(); // RestrictedCouncil::single_member("olivier")
        let alice_key = register_test_agent(&ctx, "alice").await;
        let alice_pub = hex::encode(alice_key.verifying_key().to_bytes());

        let propose_payload = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_propose",
            &[("proposal_id", "prop_cb")],
        );
        let sig_check = signing::check_signature(&propose_payload);
        handle_quorum_propose(&propose_payload, &ctx, &sig_check).await;

        // alice n'est pas membre de RestrictedCouncil (seul "olivier" l'est).
        let cb_attempt = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_circuit_breaker",
            &[("proposal_id", "prop_cb"), ("reason", "test")],
        );
        let sig2 = signing::check_signature(&cb_attempt);
        let r = handle_quorum_circuit_breaker(&cb_attempt, &ctx, &sig2).await;
        assert!(r.contains("not_authorized"), "reponse inattendue: {r}");

        // Un vote sur la proposition encore ouverte doit toujours marcher.
        let vote = build_signed_payload(
            "alice", &alice_pub, &alice_key, "quorum_vote",
            &[("proposal_id", "prop_cb"), ("decision", "yea")],
        );
        let sig3 = signing::check_signature(&vote);
        let rv = handle_quorum_vote(&vote, &ctx, &sig3).await;
        assert!(rv.contains("quorum_vote_recorded"), "reponse inattendue: {rv}");
    }
}
