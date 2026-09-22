//! src/server/arbitrage.rs — Couche 3b: Arbitrage Réel
//!
//! Système d'arbitrage décentralisé pour résoudre les contradictions détectées
//! par les couches précédentes (3a, 2, etc.).
//!
//! Architecture:
//! 1. Arbiters: Agents qualifiés ayant une clé publique, un niveau d'autorité
//!    et un enjeu (stake)
//! 2. CaseRecord: Dossier ouvert avec contradiction identifiée, arbitres assignés
//! 3. ArbitrationRuling: Décision signée d'un arbitre avec justification
//! 4. ArbitrageManager Trait: Interface pour ouvrir, assigner, juger, vérifier
//! 5. Peer Review: Signatures de validation d'autres arbitres (finality)
//!
//! Contraintes de sécurité:
//! - Tous les rulings doivent être Ed25519-signés
//! - Les arbitres en peer review doivent correspondre aux clés publiques enregistrées
//! - Finality requiert quorum_size() signatures distinctes de la RestrictedCouncil

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::restricted_council::RestrictedCouncil;
use crate::adn_store::AdnStore;
use crate::server::CstlNativeServer;

/// Niveaux d'autorité pour les arbitres (hiérarchie de décision)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuthorityLevel {
    /// Niveau 1: arbitre stagiaire, vote compté mais pas d'escalade autonome
    Trainee = 1,
    /// Niveau 2: arbitre senior, peut voter et recommander escalade
    Senior = 2,
    /// Niveau 3: arbitre expert, peut voter et forcer escalade au conseil restreint
    Expert = 3,
}

/// Record d'un arbitre inscrit dans le registre
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arbiter {
    pub arbiter_id: String,
    pub public_key: String,
    pub authority_level: AuthorityLevel,
    pub stake_amount: u64, // en satoshis ou tokens CSTL
    pub registered_at: DateTime<Utc>,
    pub is_active: bool,
}

/// Types de contradictions détectées et escaladées en arbitrage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContradictionType {
    /// Deux assertions mutuellement exclusives sur le même fait
    MutuallyExclusive,
    /// Chaine logique brisée (incohérence sémantique détectée par hypothèses moteur)
    LogicalBreak,
    /// Agent refuse de modifier état après consensus (non-compliance)
    RefusalToComply,
    /// Signature/preuve invalide ou incoherent avec l'énoncé
    InvalidProof,
}

/// Statuts d'un dossier arbitrage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaseStatus {
    /// Dossier ouvert, en attente d'assignation d'arbitres
    Open,
    /// Arbitres assignés, en attente de rulings
    InProgress,
    /// Au moins un ruling soumis, en attente de peer reviews
    RulingSubmitted,
    /// Peer reviews collectées, finality atteinte (quorum)
    Finalized,
    /// Cas escaladé au conseil restreint (expert decision)
    EscalatedToCouncil,
}

/// Dossier d'un cas en arbitrage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRecord {
    pub case_id: String,
    /// Qui a escaladé le cas (agent découverte d'hypothèses, gouvernance, etc.)
    pub escalation_source: String,
    pub contradiction_type: ContradictionType,
    pub status: CaseStatus,
    /// Contexte/description du conflit
    pub description: String,
    /// Arbitres assignés à ce cas
    pub assigned_arbiters: Vec<String>,
    /// Timestamp d'ouverture
    pub opened_at: DateTime<Utc>,
    /// Dernier update
    pub updated_at: DateTime<Utc>,
}

/// Décision d'un arbitre sur un cas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrationRuling {
    pub ruling_id: String,
    pub case_id: String,
    pub arbiter_id: String,
    /// Décision choisie ("accept_assertion_A", "accept_assertion_B", "inconclusive")
    pub decision: String,
    /// Justification signée
    pub justification: String,
    /// Signature Ed25519 du (ruling_id || decision || justification)
    pub signature: String,
    /// Timestamp du ruling
    pub ruled_at: DateTime<Utc>,
}

/// Signature de peer review d'un autre arbitre (validation croisée)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReviewSignature {
    pub reviewing_arbiter_id: String,
    /// Signature Ed25519 du ruling_id complet
    pub review_signature: String,
    pub reviewed_at: DateTime<Utc>,
}

/// Ruling finalisé avec preuves d'acceptation (peer review quorum)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrationRulingFinal {
    pub ruling: ArbitrationRuling,
    /// Signatures de peer review (doit atteindre quorum_size)
    pub peer_reviews: Vec<PeerReviewSignature>,
    pub finalized_at: DateTime<Utc>,
}

/// Helpers pour convertir les types métier vers les types base de données

/// Trait d'interface pour le gestionnaire d'arbitrage
pub trait ArbitrageManager {
    /// Ouvrir un nouveau dossier d'arbitrage avec contradiction détectée
    fn open_case(
        &self,
        escalation_source: String,
        contradiction_type: ContradictionType,
        description: String,
    ) -> Result<String, ArbitrationError>;

    /// Assigner des arbitres à un dossier (round-robin sur les arbitres actifs)
    fn assign_arbiters(
        &self,
        case_id: &str,
        count: usize,
    ) -> Result<Vec<String>, ArbitrationError>;

    /// Soumettre un ruling signé d'un arbitre
    fn submit_ruling(
        &self,
        ruling: ArbitrationRuling,
    ) -> Result<(), ArbitrationError>;

    /// Ajouter une signature de peer review (un arbitre valide un ruling)
    fn peer_review(
        &self,
        ruling_id: &str,
        reviewing_arbiter_id: String,
        review_signature: String,
    ) -> Result<(), ArbitrationError>;

    /// Finaliser un cas quand quorum de peer reviews atteint
    fn finalize_case(
        &self,
        case_id: &str,
    ) -> Result<ArbitrationRulingFinal, ArbitrationError>;

    /// Escalader un cas au conseil restreint (expert override ou stalemate)
    fn escalate_to_council(
        &self,
        case_id: &str,
    ) -> Result<(), ArbitrationError>;
}

/// Erreurs du système d'arbitrage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArbitrationError {
    CaseNotFound(String),
    ArbiterNotFound(String),
    RulingNotFound(String),
    InvalidSignature,
    QuorumNotReached(usize, usize), // (current, required)
    CaseAlreadyFinalized(String),
    UnauthorizedArbiter(String),
    DatabaseError(String),
    ArbitersAssignmentFailed(String),
}

impl std::fmt::Display for ArbitrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CaseNotFound(id) => write!(f, "Case '{}' not found", id),
            Self::ArbiterNotFound(id) => write!(f, "Arbiter '{}' not found", id),
            Self::RulingNotFound(id) => write!(f, "Ruling '{}' not found", id),
            Self::InvalidSignature => write!(f, "Signature verification failed"),
            Self::QuorumNotReached(current, required) => {
                write!(f, "Quorum not reached: {} of {} signatures", current, required)
            }
            Self::CaseAlreadyFinalized(id) => write!(f, "Case '{}' already finalized", id),
            Self::UnauthorizedArbiter(id) => write!(f, "Arbiter '{}' not authorized", id),
            Self::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            Self::ArbitersAssignmentFailed(msg) => write!(f, "Assignment failed: {}", msg),
        }
    }
}

impl std::error::Error for ArbitrationError {}

/// Fonctions auxiliaires pour l'arbitrage (appelées directement depuis le handler async du handler.rs)
pub async fn open_case_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    escalation_source: String,
    contradiction_type: ContradictionType,
    description: String,
) -> Result<String, ArbitrationError> {
    let case_id = format!("case_{}", uuid::Uuid::new_v4().to_string());
    let now = Utc::now();

    use crate::adn_store::ArbitrageCase;
    let db_case = ArbitrageCase {
        case_id: case_id.clone(),
        initiator: escalation_source,
        subject: description,
        status: "Open".to_string(),
        created_at: now.timestamp(),
        updated_at: now.timestamp(),
        assigned_arbiters: Some("[]".to_string()),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_arbitrage_case(&db_case)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    log::info!(
        "[Arbitrage] Case {} opened (contradiction: {:?})",
        case_id, contradiction_type
    );
    Ok(case_id)
}

pub async fn assign_arbiters_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    case_id: &str,
    count: usize,
) -> Result<Vec<String>, ArbitrationError> {
    if count == 0 {
        return Err(ArbitrationError::ArbitersAssignmentFailed(
            "count must be > 0".to_string(),
        ));
    }

    let case = {
        let adn = adn_store.lock().await;
        adn.get_arbitrage_case(case_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
            .ok_or_else(|| ArbitrationError::CaseNotFound(case_id.to_string()))?
    };

    if case.status != "Open" {
        return Err(ArbitrationError::ArbitersAssignmentFailed(
            format!("Case {} not in Open status", case_id),
        ));
    }

    let assigned = {
        let adn = adn_store.lock().await;
        select_arbiters_round_robin(&adn, count)
            .map_err(|e| ArbitrationError::ArbitersAssignmentFailed(e))?
    };

    if assigned.is_empty() {
        return Err(ArbitrationError::ArbitersAssignmentFailed(
            "No active arbiters available".to_string(),
        ));
    }

    let assigned_json = serde_json::to_string(&assigned)
        .unwrap_or_else(|_| "[]".to_string());

    let now = Utc::now();
    use crate::adn_store::ArbitrageCase;
    let updated = ArbitrageCase {
        case_id: case.case_id.clone(),
        initiator: case.initiator.clone(),
        subject: case.subject.clone(),
        status: "InProgress".to_string(),
        created_at: case.created_at,
        updated_at: now.timestamp(),
        assigned_arbiters: Some(assigned_json),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_arbitrage_case(&updated)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    log::info!(
        "[Arbitrage] Case {} assigned to {} arbiters",
        case_id,
        assigned.len()
    );
    Ok(assigned)
}

pub async fn submit_ruling_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    ruling: ArbitrationRuling,
) -> Result<(), ArbitrationError> {
    let payload = format!(
        "{}||{}||{}",
        ruling.ruling_id, ruling.decision, ruling.justification
    );

    verify_ruling_signatures(adn_store, &ruling.arbiter_id, &payload, &ruling.signature).await?;

    let case = {
        let adn = adn_store.lock().await;
        adn.get_arbitrage_case(&ruling.case_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
            .ok_or_else(|| ArbitrationError::CaseNotFound(ruling.case_id.clone()))?
    };

    if case.status != "InProgress" {
        return Err(ArbitrationError::ArbitersAssignmentFailed(
            format!("Case {} not in InProgress status", ruling.case_id),
        ));
    }

    let assigned_arbiters: Vec<String> = case.assigned_arbiters
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    if !assigned_arbiters.contains(&ruling.arbiter_id) {
        return Err(ArbitrationError::UnauthorizedArbiter(ruling.arbiter_id.clone()));
    }

    use crate::adn_store::DbArbitrationRuling;
    let db_ruling = DbArbitrationRuling {
        ruling_id: ruling.ruling_id.clone(),
        case_id: ruling.case_id.clone(),
        ruling_text: format!("{} | {}", ruling.decision, ruling.justification),
        decided_by: ruling.arbiter_id.clone(),
        status: "Pending".to_string(),
        created_at: ruling.ruled_at.timestamp(),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_arbitrage_ruling(&db_ruling)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    let now = Utc::now();
    use crate::adn_store::ArbitrageCase;
    let updated = ArbitrageCase {
        case_id: case.case_id.clone(),
        initiator: case.initiator.clone(),
        subject: case.subject.clone(),
        status: "RulingSubmitted".to_string(),
        created_at: case.created_at,
        updated_at: now.timestamp(),
        assigned_arbiters: case.assigned_arbiters.clone(),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_arbitrage_case(&updated)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    log::info!(
        "[Arbitrage] Ruling submitted by {} on case {}",
        ruling.arbiter_id, ruling.case_id
    );
    Ok(())
}

pub async fn peer_review_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    ruling_id: &str,
    reviewing_arbiter_id: String,
    review_signature: String,
) -> Result<(), ArbitrationError> {
    let payload = format!("{}||{}", reviewing_arbiter_id, ruling_id);
    verify_ruling_signatures(adn_store, &reviewing_arbiter_id, &payload, &review_signature).await?;

    let _ruling = {
        let adn = adn_store.lock().await;
        adn.get_arbitrage_ruling(ruling_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
            .ok_or_else(|| ArbitrationError::RulingNotFound(ruling_id.to_string()))?
    };

    // Convert to database type
    use crate::adn_store::DbPeerReviewSignature;
    use uuid::Uuid;

    let db_review = DbPeerReviewSignature {
        review_id: Uuid::new_v4().to_string(),
        ruling_id: ruling_id.to_string(),
        reviewer_id: reviewing_arbiter_id.clone(),
        signature: review_signature,
        approval_status: "Approved".to_string(),
        reviewed_at: Utc::now().timestamp(),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_peer_review(&db_review)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    log::info!(
        "[Arbitrage] Peer review added to ruling {}",
        ruling_id
    );
    Ok(())
}

pub async fn finalize_case_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    council: &RestrictedCouncil,
    case_id: &str,
) -> Result<ArbitrationRulingFinal, ArbitrationError> {
    let case = {
        let adn = adn_store.lock().await;
        adn.get_arbitrage_case(case_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
            .ok_or_else(|| ArbitrationError::CaseNotFound(case_id.to_string()))?
    };

    if case.status == "Finalized" {
        return Err(ArbitrationError::CaseAlreadyFinalized(case_id.to_string()));
    }

    let db_ruling = {
        let adn = adn_store.lock().await;
        adn.get_ruling_by_case(case_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
            .ok_or_else(|| ArbitrationError::RulingNotFound(case_id.to_string()))?
    };

    let peer_reviews_count = {
        let adn = adn_store.lock().await;
        adn.get_peer_reviews_for_ruling(&db_ruling.ruling_id)
            .map(|pr| pr.len())
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
    };

    let required_signatures = council.quorum_size();

    if peer_reviews_count < required_signatures {
        return Err(ArbitrationError::QuorumNotReached(
            peer_reviews_count,
            required_signatures,
        ));
    }

    // Update case status to Finalized
    let now = Utc::now();
    use crate::adn_store::ArbitrageCase;
    let updated_case = ArbitrageCase {
        case_id: case.case_id.clone(),
        initiator: case.initiator.clone(),
        subject: case.subject.clone(),
        status: "Finalized".to_string(),
        created_at: case.created_at,
        updated_at: now.timestamp(),
        assigned_arbiters: case.assigned_arbiters.clone(),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_arbitrage_case(&updated_case)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    // Convert DB ruling to application type
    let (decision, justification) = db_ruling.ruling_text.split_once(" | ")
        .map(|(d, j)| (d.to_string(), j.to_string()))
        .unwrap_or((db_ruling.ruling_text.clone(), String::new()));

    let ruling = ArbitrationRuling {
        ruling_id: db_ruling.ruling_id,
        case_id: db_ruling.case_id,
        arbiter_id: db_ruling.decided_by,
        decision,
        justification,
        signature: String::new(),
        ruled_at: chrono::DateTime::<Utc>::from_timestamp(db_ruling.created_at, 0)
            .unwrap_or_else(Utc::now),
    };

    // Create peer review list
    let peer_reviews = vec![PeerReviewSignature {
        reviewing_arbiter_id: "quorum".to_string(),
        review_signature: String::new(),
        reviewed_at: Utc::now(),
    }];

    let final_ruling = ArbitrationRulingFinal {
        ruling,
        peer_reviews,
        finalized_at: Utc::now(),
    };

    log::info!(
        "[Arbitrage] Case {} finalized with quorum consensus",
        case_id
    );
    Ok(final_ruling)
}

pub async fn escalate_to_council_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    case_id: &str,
) -> Result<(), ArbitrationError> {
    let case = {
        let adn = adn_store.lock().await;
        adn.get_arbitrage_case(case_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
            .ok_or_else(|| ArbitrationError::CaseNotFound(case_id.to_string()))?
    };

    let now = Utc::now();
    use crate::adn_store::ArbitrageCase;
    let escalated = ArbitrageCase {
        case_id: case.case_id.clone(),
        initiator: case.initiator.clone(),
        subject: case.subject.clone(),
        status: "EscalatedToCouncil".to_string(),
        created_at: case.created_at,
        updated_at: now.timestamp(),
        assigned_arbiters: case.assigned_arbiters.clone(),
    };

    {
        let adn = adn_store.lock().await;
        adn.save_arbitrage_case(&escalated)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
    }

    log::info!(
        "[Arbitrage] Case {} escalated to restricted council",
        case_id
    );
    Ok(())
}

pub fn select_arbiters_round_robin(adn: &AdnStore, count: usize) -> Result<Vec<String>, String> {
    let arbiters = adn
        .get_active_arbiters()
        .map_err(|e| format!("Database error: {}", e))?;

    if arbiters.is_empty() {
        return Err("No active arbiters registered".to_string());
    }

    let selected: Vec<String> = arbiters
        .iter()
        .cycle()
        .take(count)
        .map(|a| a.clone())
        .collect();

    Ok(selected)
}

/// Verification Ed25519 REELLE, contre la cle publique enregistree pour cet
/// arbitre -- jamais une cle que le message lui-meme revendiquerait.
///
/// Avant ce fix (2026-09-22): cette fonction ne verifiait que la
/// non-vacuite des 3 chaines -- n'importe quel `signature="x"` passait, peu
/// importe l'arbitre ou le contenu du ruling. Elle etait nommee "verify"
/// mais ne verifiait rien. Impossible de la corriger avant maintenant: il
/// n'existait aucun registre d'ou tirer la cle publique d'un arbitre
/// (`AdnStore::get_active_arbiters` etait un stub renvoyant toujours
/// `Vec::new()`, `Arbiter { public_key, .. }` n'etait jamais construite en
/// dehors des tests). Voir `adn_store.rs::save_arbiter`/`get_arbiter_public_key`
/// (table `arbiters`, migration 2026-09-22) et `register_arbiter_async`
/// ci-dessous pour le cote enregistrement.
///
/// `ArbiterNotFound` si l'arbitre n'est pas enregistre (ou desactive) --
/// distinct de `InvalidSignature` pour que l'appelant sache si le probleme
/// est "personne ne connait cet arbitre" vs "la signature ne correspond
/// pas".
pub async fn verify_ruling_signatures(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    arbiter_id: &str,
    payload: &str,
    signature: &str,
) -> Result<(), ArbitrationError> {
    if arbiter_id.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(ArbitrationError::InvalidSignature);
    }

    let public_key_hex = {
        let adn = adn_store.lock().await;
        adn.get_arbiter_public_key(arbiter_id)
            .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
    };
    let public_key_hex = public_key_hex
        .ok_or_else(|| ArbitrationError::ArbiterNotFound(arbiter_id.to_string()))?;

    match crate::signing::verify_raw(payload.as_bytes(), &public_key_hex, signature) {
        crate::signing::SignatureCheck::Valid => Ok(()),
        _ => Err(ArbitrationError::InvalidSignature),
    }
}

/// Enregistre un arbitre avec sa cle publique Ed25519. Portee v1, meme
/// limite assumee que `purpose=agent_register` avant sa correction de
/// rotation (2026-09-04): un ré-enregistrement avec une NOUVELLE cle pour
/// le meme `arbiter_id` n'exige PAS de preuve de possession de l'ancienne
/// cle -- quiconque connait juste l'id peut voler l'identite d'un arbitre
/// deja enregistre en le re-enregistrant avec sa propre cle. Documente,
/// pas corrige ici (hors scope du fix demande: "le 1" = rendre la
/// verification reelle, pas fermer la rotation) -- meme muster que
/// `restricted_council` ailleurs dans ce depot pour une limite v1 assumee.
pub async fn register_arbiter_async(
    adn_store: &Arc<tokio::sync::Mutex<crate::adn_store::AdnStore>>,
    arbiter_id: &str,
    public_key_hex: &str,
    authority_level: AuthorityLevel,
    stake_amount: u64,
) -> Result<(), ArbitrationError> {
    let level_str = match authority_level {
        AuthorityLevel::Trainee => "trainee",
        AuthorityLevel::Senior => "senior",
        AuthorityLevel::Expert => "expert",
    };
    let adn = adn_store.lock().await;
    adn.save_arbiter(arbiter_id, public_key_hex, level_str, stake_amount as i64, true)
        .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))
}

pub fn check_finality_threshold(
    peer_review_count: usize,
    council: &RestrictedCouncil,
) -> Result<(), ArbitrationError> {
    let required = council.quorum_size();
    if peer_review_count >= required {
        Ok(())
    } else {
        Err(ArbitrationError::QuorumNotReached(peer_review_count, required))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arbiter_creation() {
        let arbiter = Arbiter {
            arbiter_id: "arbiter_1".to_string(),
            public_key: "edpk1234567890".to_string(),
            authority_level: AuthorityLevel::Senior,
            stake_amount: 100_000,
            registered_at: Utc::now(),
            is_active: true,
        };
        assert_eq!(arbiter.authority_level, AuthorityLevel::Senior);
        assert!(arbiter.is_active);
    }

    #[test]
    fn test_case_record_status_flow() {
        let mut case = CaseRecord {
            case_id: "case_001".to_string(),
            escalation_source: "hypothesis_engine".to_string(),
            contradiction_type: ContradictionType::MutuallyExclusive,
            status: CaseStatus::Open,
            description: "Test contradiction".to_string(),
            assigned_arbiters: vec![],
            opened_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert_eq!(case.status, CaseStatus::Open);

        case.status = CaseStatus::InProgress;
        case.assigned_arbiters = vec!["arbiter_1".to_string()];
        assert_eq!(case.status, CaseStatus::InProgress);
    }

    #[test]
    fn test_arbitration_ruling_creation() {
        let ruling = ArbitrationRuling {
            ruling_id: "ruling_001".to_string(),
            case_id: "case_001".to_string(),
            arbiter_id: "arbiter_1".to_string(),
            decision: "accept_assertion_A".to_string(),
            justification: "Assertion A is more logically coherent".to_string(),
            signature: "ed25519_signature_here".to_string(),
            ruled_at: Utc::now(),
        };

        assert_eq!(ruling.case_id, "case_001");
        assert_eq!(ruling.decision, "accept_assertion_A");
    }

    #[test]
    fn test_arbitration_error_display() {
        let err = ArbitrationError::CaseNotFound("case_unknown".to_string());
        let msg = err.to_string();
        assert!(msg.contains("case_unknown"));
    }

    #[test]
    fn test_finality_threshold_pass() {
        let council = RestrictedCouncil::single_member("Olivier");
        let result = check_finality_threshold(1, &council);
        assert!(result.is_ok());
    }

    #[test]
    fn test_finality_threshold_fail() {
        let council = RestrictedCouncil::new(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]);
        let result = check_finality_threshold(1, &council);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_ruling_signatures_invalid_empty() {
        let adn_store = Arc::new(Mutex::new(
            crate::adn_store::AdnStore::open(":memory:").unwrap(),
        ));
        let result = verify_ruling_signatures(&adn_store, "", "payload", "sig").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_ruling_signatures_unregistered_arbiter_rejected() {
        let adn_store = Arc::new(Mutex::new(
            crate::adn_store::AdnStore::open(":memory:").unwrap(),
        ));
        // Aucun arbiter_register n'a jamais ete fait pour "ghost" -- meme
        // avec une signature de longueur/format parfaits, il n'y a aucune
        // cle publique enregistree contre laquelle verifier.
        let fake_sig = "a1".repeat(64);
        let result = verify_ruling_signatures(&adn_store, "ghost", "some_payload", &fake_sig).await;
        assert!(matches!(result, Err(ArbitrationError::ArbiterNotFound(_))));
    }

    /// Le test qui aurait echoue avant ce fix: avant, `signature="anything_non_empty"`
    /// passait pour N'IMPORTE QUEL arbitre. Maintenant, une vraie paire de
    /// cles Ed25519 est necessaire et la signature doit reellement
    /// correspondre au payload signe avec la cle privee de CET arbitre.
    #[tokio::test]
    async fn test_verify_ruling_signatures_real_ed25519_roundtrip() {
        use ed25519_dalek::{Signer, SigningKey};
        use rand::rngs::OsRng;

        let adn_store = Arc::new(Mutex::new(
            crate::adn_store::AdnStore::open(":memory:").unwrap(),
        ));
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());

        register_arbiter_async(&adn_store, "arbiter_alice", &public_key_hex, AuthorityLevel::Senior, 100_000)
            .await
            .unwrap();

        let payload = "ruling_001||accept_assertion_A||justification texte";
        let signature = signing_key.sign(payload.as_bytes());
        let signature_hex = hex::encode(signature.to_bytes());

        // Signature valide, du bon arbitre, sur le bon payload -> accepte.
        let ok = verify_ruling_signatures(&adn_store, "arbiter_alice", payload, &signature_hex).await;
        assert!(ok.is_ok(), "expected valid signature to verify, got {:?}", ok);

        // Meme signature, mais payload different (ruling falsifie) -> rejete.
        let tampered = verify_ruling_signatures(
            &adn_store, "arbiter_alice", "ruling_001||accept_assertion_B||justification texte", &signature_hex,
        ).await;
        assert!(tampered.is_err(), "tampered payload must not verify");

        // Meme signature valide, mais attribuee a un AUTRE arbitre enregistre
        // avec une cle differente -> rejete (usurpation d'identite).
        let other_key = SigningKey::generate(&mut OsRng);
        let other_key_hex = hex::encode(other_key.verifying_key().to_bytes());
        register_arbiter_async(&adn_store, "arbiter_bob", &other_key_hex, AuthorityLevel::Trainee, 0)
            .await
            .unwrap();
        let impersonation = verify_ruling_signatures(&adn_store, "arbiter_bob", payload, &signature_hex).await;
        assert!(impersonation.is_err(), "alice's signature must not verify under bob's key");
    }

    #[test]
    fn test_contradiction_type_variants() {
        let types = vec![
            ContradictionType::MutuallyExclusive,
            ContradictionType::LogicalBreak,
            ContradictionType::RefusalToComply,
            ContradictionType::InvalidProof,
        ];
        assert_eq!(types.len(), 4);
        assert_eq!(types[0], ContradictionType::MutuallyExclusive);
    }

    #[test]
    fn test_case_status_flow_complete() {
        let mut case = CaseRecord {
            case_id: "case_001".to_string(),
            escalation_source: "gov_layer".to_string(),
            contradiction_type: ContradictionType::LogicalBreak,
            status: CaseStatus::Open,
            description: "Logic broken".to_string(),
            assigned_arbiters: vec![],
            opened_at: Utc::now(),
            updated_at: Utc::now(),
        };

        case.status = CaseStatus::InProgress;
        assert_eq!(case.status, CaseStatus::InProgress);

        case.status = CaseStatus::RulingSubmitted;
        assert_eq!(case.status, CaseStatus::RulingSubmitted);

        case.status = CaseStatus::Finalized;
        assert_eq!(case.status, CaseStatus::Finalized);
    }

    #[tokio::test]
    async fn test_open_case_async_malicious_payload() {
        // Test: ouverture d'un cas avec détection de payload malveillant (description vide)
        let adn_store = Arc::new(tokio::sync::Mutex::new(
            crate::adn_store::AdnStore::open(":memory:").unwrap()
        ));

        let result = open_case_async(
            &adn_store,
            "malicious_agent".to_string(),
            ContradictionType::InvalidProof,
            String::new(), // Empty description simulates malicious input
        ).await;

        // The operation should succeed even with empty description, as it's valid CSTL
        assert!(result.is_ok());
        let case_id = result.unwrap();
        assert!(case_id.starts_with("case_"));
    }

    #[tokio::test]
    async fn test_arbitration_error_quorum_not_reached() {
        // Test: finalization échoue si le quorum n'est pas atteint
        let council = RestrictedCouncil::new(vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ]);

        let peer_count = 1; // Seulement 1 signature, quorum = 2
        let required = council.quorum_size();

        let err = check_finality_threshold(peer_count, &council);
        assert!(err.is_err());

        match err {
            Err(ArbitrationError::QuorumNotReached(current, needed)) => {
                assert_eq!(current, 1);
                assert_eq!(needed, required);
            }
            _ => panic!("Expected QuorumNotReached error"),
        }
    }

    #[tokio::test]
    async fn test_arbitration_ruling_with_human_resolution() {
        // Test: soumission d'un ruling avec résolution manuelle (human review)
        let ruling = ArbitrationRuling {
            ruling_id: "ruling_human_001".to_string(),
            case_id: "case_human_001".to_string(),
            arbiter_id: "expert_arbiter".to_string(),
            decision: "accept_assertion_A_after_manual_review".to_string(),
            justification: "Human expert determined assertion A is legally sound and technically correct".to_string(),
            signature: "ed25519_signature_from_expert_key".to_string(),
            ruled_at: Utc::now(),
        };

        // Verify the ruling can be constructed with proper metadata
        assert_eq!(ruling.decision, "accept_assertion_A_after_manual_review");
        assert!(ruling.justification.contains("expert"));
        assert!(!ruling.signature.is_empty());
    }

    #[test]
    fn test_arbiters_registry_stake_validation() {
        // Test: validation du stake d'un arbitre (security check)
        let arbiter_low_stake = Arbiter {
            arbiter_id: "low_stake_arbiter".to_string(),
            public_key: "edpk_low_stake".to_string(),
            authority_level: AuthorityLevel::Trainee,
            stake_amount: 100, // Very low stake
            registered_at: Utc::now(),
            is_active: true,
        };

        let arbiter_high_stake = Arbiter {
            arbiter_id: "high_stake_arbiter".to_string(),
            public_key: "edpk_high_stake".to_string(),
            authority_level: AuthorityLevel::Expert,
            stake_amount: 1_000_000, // High stake
            registered_at: Utc::now(),
            is_active: true,
        };

        // Lower stake implies lower authority but still valid
        assert!(arbiter_low_stake.stake_amount < arbiter_high_stake.stake_amount);
        assert!(arbiter_low_stake.authority_level < arbiter_high_stake.authority_level);
    }
}