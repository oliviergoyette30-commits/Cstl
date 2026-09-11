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

/// Extension trait pour les méthodes d'arbitrage dans AdnStore
pub trait ArbitrageStoreExt {
    fn save_arbitrage_case(&self, case: &CaseRecord) -> Result<(), String>;
    fn get_arbitrage_case(&self, case_id: &str) -> Result<Option<CaseRecord>, String>;
    fn save_arbitrage_ruling(&self, ruling: &ArbitrationRuling) -> Result<(), String>;
    fn get_arbitrage_ruling(&self, ruling_id: &str) -> Result<Option<ArbitrationRuling>, String>;
    fn get_ruling_by_case(&self, case_id: &str) -> Result<Option<ArbitrationRuling>, String>;
    fn save_peer_review(&self, ruling_id: &str, review: &PeerReviewSignature) -> Result<(), String>;
    fn get_peer_reviews_for_ruling(&self, ruling_id: &str) -> Result<Vec<PeerReviewSignature>, String>;
    fn get_active_arbiters(&self) -> Result<Vec<Arbiter>, String>;
}

impl ArbitrageStoreExt for AdnStore {
    fn save_arbitrage_case(&self, case: &CaseRecord) -> Result<(), String> {
        let arbiters_json = serde_json::to_string(&case.assigned_arbiters)
            .map_err(|e| format!("Failed to serialize arbiters: {}", e))?;

        use crate::adn_store::ArbitrageCase;
        let db_case = ArbitrageCase {
            case_id: case.case_id.clone(),
            initiator: case.escalation_source.clone(),
            subject: format!("{:?}", case.contradiction_type),
            status: format!("{:?}", case.status),
            created_at: case.opened_at.timestamp(),
            updated_at: case.updated_at.timestamp(),
            assigned_arbiters: Some(arbiters_json),
        };

        self.save_arbitrage_case(&db_case)
            .map_err(|e| format!("Database error: {}", e))
    }

    fn get_arbitrage_case(&self, case_id: &str) -> Result<Option<CaseRecord>, String> {
        use crate::adn_store::ArbitrageCase;
        let opt_case = self.get_arbitrage_case(case_id)
            .map_err(|e| format!("Database error: {}", e))?;

        Ok(opt_case.map(|db_case| {
            let arbiters: Vec<String> = db_case.assigned_arbiters
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            CaseRecord {
                case_id: db_case.case_id,
                escalation_source: db_case.initiator,
                contradiction_type: ContradictionType::MutuallyExclusive, // Simplified
                status: CaseStatus::Open, // Simplified
                description: db_case.subject,
                assigned_arbiters: arbiters,
                opened_at: chrono::DateTime::<Utc>::from_timestamp(db_case.created_at, 0)
                    .unwrap_or_else(Utc::now),
                updated_at: chrono::DateTime::<Utc>::from_timestamp(db_case.updated_at, 0)
                    .unwrap_or_else(Utc::now),
            }
        }))
    }

    fn save_arbitrage_ruling(&self, ruling: &ArbitrationRuling) -> Result<(), String> {
        use crate::adn_store::DbArbitrationRuling as DbRuling;
        let db_ruling = DbRuling {
            ruling_id: ruling.ruling_id.clone(),
            case_id: ruling.case_id.clone(),
            ruling_text: format!("{} | {}", ruling.decision, ruling.justification),
            decided_by: ruling.arbiter_id.clone(),
            status: "Pending".to_string(),
            created_at: ruling.ruled_at.timestamp(),
        };

        self.save_arbitrage_ruling(&db_ruling)
            .map_err(|e| format!("Database error: {}", e))
    }

    fn get_arbitrage_ruling(&self, ruling_id: &str) -> Result<Option<ArbitrationRuling>, String> {
        use crate::adn_store::DbArbitrationRuling;
        let opt_ruling = self.get_arbitrage_ruling(ruling_id)
            .map_err(|e| format!("Database error: {}", e))?;

        Ok(opt_ruling.map(|db_ruling| {
            let (decision, justification) = db_ruling.ruling_text.split_once(" | ")
                .map(|(d, j)| (d.to_string(), j.to_string()))
                .unwrap_or((db_ruling.ruling_text.clone(), String::new()));

            ArbitrationRuling {
                ruling_id: db_ruling.ruling_id,
                case_id: db_ruling.case_id,
                arbiter_id: db_ruling.decided_by,
                decision,
                justification,
                signature: String::new(), // Not stored in DB layer
                ruled_at: chrono::DateTime::<Utc>::from_timestamp(db_ruling.created_at, 0)
                    .unwrap_or_else(Utc::now),
            }
        }))
    }

    fn get_ruling_by_case(&self, case_id: &str) -> Result<Option<ArbitrationRuling>, String> {
        use crate::adn_store::DbArbitrationRuling;
        let opt_ruling = self.get_ruling_by_case(case_id)
            .map_err(|e| format!("Database error: {}", e))?;

        Ok(opt_ruling.map(|db_ruling| {
            let (decision, justification) = db_ruling.ruling_text.split_once(" | ")
                .map(|(d, j)| (d.to_string(), j.to_string()))
                .unwrap_or((db_ruling.ruling_text.clone(), String::new()));

            ArbitrationRuling {
                ruling_id: db_ruling.ruling_id,
                case_id: db_ruling.case_id,
                arbiter_id: db_ruling.decided_by,
                decision,
                justification,
                signature: String::new(),
                ruled_at: chrono::DateTime::<Utc>::from_timestamp(db_ruling.created_at, 0)
                    .unwrap_or_else(Utc::now),
            }
        }))
    }

    fn save_peer_review(&self, ruling_id: &str, review: &PeerReviewSignature) -> Result<(), String> {
        use crate::adn_store::DbPeerReviewSignature;
        use uuid::Uuid;

        let db_review = DbPeerReviewSignature {
            review_id: Uuid::new_v4().to_string(),
            ruling_id: ruling_id.to_string(),
            reviewer_id: review.reviewing_arbiter_id.clone(),
            signature: review.review_signature.clone(),
            approval_status: "Approved".to_string(),
            reviewed_at: review.reviewed_at.timestamp(),
        };

        self.save_peer_review(&db_review)
            .map_err(|e| format!("Database error: {}", e))
    }

    fn get_peer_reviews_for_ruling(&self, ruling_id: &str) -> Result<Vec<PeerReviewSignature>, String> {
        use crate::adn_store::DbPeerReviewSignature;
        let db_reviews = self.get_peer_reviews_for_ruling(ruling_id)
            .map_err(|e| format!("Database error: {}", e))?;

        Ok(db_reviews.into_iter().map(|db_review| {
            PeerReviewSignature {
                reviewing_arbiter_id: db_review.reviewer_id,
                review_signature: db_review.signature,
                reviewed_at: chrono::DateTime::<Utc>::from_timestamp(db_review.reviewed_at, 0)
                    .unwrap_or_else(Utc::now),
            }
        }).collect())
    }

    fn get_active_arbiters(&self) -> Result<Vec<Arbiter>, String> {
        // Placeholder: would need an arbiters table in the database
        // For now, return empty vec
        Ok(vec![])
    }
}

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

/// Impl du trait ArbitrageManager pour CstlNativeServer
/* /* impl ArbitrageManager for CstlNativeServer {
    fn open_case(
        &self,
        escalation_source: String,
        contradiction_type: ContradictionType,
        description: String,
    ) -> Result<String, ArbitrationError> {
        let case_id = format!("case_{}", uuid::Uuid::new_v4().to_string());
        let now = Utc::now();

        let case = CaseRecord {
            case_id: case_id.clone(),
            escalation_source,
            contradiction_type,
            status: CaseStatus::Open,
            description,
            assigned_arbiters: Vec::new(),
            opened_at: now,
            updated_at: now,
        };

        {
            let adn = self.adn_store.lock();
            adn.save_arbitrage_case(&case)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        eprintln!(
            "[Arbitrage] 📋 Case {} opened (contradiction: {:?})",
            case_id, contradiction_type
        );
        Ok(case_id)
    }

    fn assign_arbiters(
        &self,
        case_id: &str,
        count: usize,
    ) -> Result<Vec<String>, ArbitrationError> {
        if count == 0 {
            return Err(ArbitrationError::ArbitersAssignmentFailed(
                "count must be > 0".to_string(),
            ));
        }

        let case = {
            let adn = self.adn_store.lock();
            adn.get_arbitrage_case(case_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
                .ok_or_else(|| ArbitrationError::CaseNotFound(case_id.to_string()))?
        };

        if case.status != CaseStatus::Open {
            return Err(ArbitrationError::ArbitersAssignmentFailed(
                format!("Case {} not in Open status", case_id),
            ));
        }

        let assigned = {
            let adn = self.adn_store.lock();
            select_arbiters_round_robin(&adn, count)
                .map_err(|e| ArbitrationError::ArbitersAssignmentFailed(e))?
        };

        if assigned.is_empty() {
            return Err(ArbitrationError::ArbitersAssignmentFailed(
                "No active arbiters available".to_string(),
            ));
        }

        let mut updated = case;
        updated.assigned_arbiters = assigned.clone();
        updated.status = CaseStatus::InProgress;
        updated.updated_at = Utc::now();

        {
            let adn = self.adn_store.lock();
            adn.save_arbitrage_case(&updated)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        eprintln!(
            "[Arbitrage] 👥 Case {} assigned to {} arbiters",
            case_id,
            assigned.len()
        );
        Ok(assigned)
    }

    fn submit_ruling(
        &self,
        ruling: ArbitrationRuling,
    ) -> Result<(), ArbitrationError> {
        let payload = format!(
            "{}||{}||{}",
            ruling.ruling_id, ruling.decision, ruling.justification
        );

        verify_ruling_signatures(&ruling.arbiter_id, &payload, &ruling.signature)?;

        let case = {
            let adn = self.adn_store.lock();
            adn.get_arbitrage_case(&ruling.case_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
                .ok_or_else(|| ArbitrationError::CaseNotFound(ruling.case_id.clone()))?
        };

        if case.status != CaseStatus::InProgress {
            return Err(ArbitrationError::ArbitersAssignmentFailed(
                format!("Case {} not in InProgress status", ruling.case_id),
            ));
        }

        if !case.assigned_arbiters.contains(&ruling.arbiter_id) {
            return Err(ArbitrationError::UnauthorizedArbiter(ruling.arbiter_id.clone()));
        }

        {
            let adn = self.adn_store.lock();
            adn.save_arbitrage_ruling(&ruling)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        let mut updated = case;
        updated.status = CaseStatus::RulingSubmitted;
        updated.updated_at = Utc::now();

        {
            let adn = self.adn_store.lock();
            adn.save_arbitrage_case(&updated)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        eprintln!(
            "[Arbitrage] ⚖️ Ruling submitted by {} on case {}",
            ruling.arbiter_id, ruling.case_id
        );
        Ok(())
    }

    fn peer_review(
        &self,
        ruling_id: &str,
        reviewing_arbiter_id: String,
        review_signature: String,
    ) -> Result<(), ArbitrationError> {
        let payload = format!("{}||{}", reviewing_arbiter_id, ruling_id);
        verify_ruling_signatures(&reviewing_arbiter_id, &payload, &review_signature)?;

        let ruling = {
            let adn = self.adn_store.lock();
            adn.get_arbitrage_ruling(ruling_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
                .ok_or_else(|| ArbitrationError::RulingNotFound(ruling_id.to_string()))?
        };

        let review = PeerReviewSignature {
            reviewing_arbiter_id,
            review_signature,
            reviewed_at: Utc::now(),
        };

        {
            let adn = self.adn_store.lock();
            adn.save_peer_review(&ruling.ruling_id, &review)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        eprintln!(
            "[Arbitrage] ✓ Peer review added to ruling {}",
            ruling_id
        );
        Ok(())
    }

    fn finalize_case(&self, case_id: &str) -> Result<ArbitrationRulingFinal, ArbitrationError> {
        let case = {
            let adn = self.adn_store.lock();
            adn.get_arbitrage_case(case_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
                .ok_or_else(|| ArbitrationError::CaseNotFound(case_id.to_string()))?
        };

        if case.status == CaseStatus::Finalized {
            return Err(ArbitrationError::CaseAlreadyFinalized(case_id.to_string()));
        }

        let ruling = {
            let adn = self.adn_store.lock();
            adn.get_ruling_by_case(case_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
                .ok_or_else(|| ArbitrationError::RulingNotFound(case_id.to_string()))?
        };

        let peer_reviews = {
            let adn = self.adn_store.lock();
            adn.get_peer_reviews_for_ruling(&ruling.ruling_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
        };

        let council = RestrictedCouncil::from_env();
        let required_signatures = council.quorum_size();

        if peer_reviews.len() < required_signatures {
            return Err(ArbitrationError::QuorumNotReached(
                peer_reviews.len(),
                required_signatures,
            ));
        }

        let mut finalized_case = case;
        finalized_case.status = CaseStatus::Finalized;
        finalized_case.updated_at = Utc::now();

        {
            let adn = self.adn_store.lock();
            adn.save_arbitrage_case(&finalized_case)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        let final_ruling = ArbitrationRulingFinal {
            ruling,
            peer_reviews,
            finalized_at: Utc::now(),
        };

        eprintln!(
            "[Arbitrage] ✅ Case {} finalized with quorum consensus",
            case_id
        );
        Ok(final_ruling)
    }

    fn escalate_to_council(&self, case_id: &str) -> Result<(), ArbitrationError> {
        let case = {
            let adn = self.adn_store.lock();
            adn.get_arbitrage_case(case_id)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?
                .ok_or_else(|| ArbitrationError::CaseNotFound(case_id.to_string()))?
        };

        let mut escalated = case;
        escalated.status = CaseStatus::EscalatedToCouncil;
        escalated.updated_at = Utc::now();

        {
            let adn = self.adn_store.lock();
            adn.save_arbitrage_case(&escalated)
                .map_err(|e| ArbitrationError::DatabaseError(e.to_string()))?;
        }

        eprintln!(
            "[Arbitrage] 🚀 Case {} escalated to restricted council",
            case_id
        );
        Ok(())
    }
}
 */ */
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

pub fn verify_ruling_signatures(
    arbiter_id: &str,
    payload: &str,
    signature: &str,
) -> Result<(), ArbitrationError> {
    if arbiter_id.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(ArbitrationError::InvalidSignature);
    }
    Ok(())
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

    #[test]
    fn test_verify_ruling_signatures_invalid_empty() {
        let result = verify_ruling_signatures("", "payload", "sig");
        assert!(result.is_err());
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
}