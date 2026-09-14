use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionState {
    Open,
    UnderReview,
    Arbitration,
    Appealed,
    Ruled,
    Closed,
    Failed,
}

impl DecisionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DecisionState::Open => "open",
            DecisionState::UnderReview => "under_review",
            DecisionState::Arbitration => "arbitration",
            DecisionState::Appealed => "appealed",
            DecisionState::Ruled => "ruled",
            DecisionState::Closed => "closed",
            DecisionState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateTransitionError {
    InvalidTransition { from: String, to: String },
    ExpiredDecision,
    NoArbiterAssigned,
    NoRulingProvided,
    InsufficientEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionContext {
    pub case_id: String,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub summary: String,
    pub evidence: Vec<String>,
    pub arbiters: Vec<String>,
    pub ruling: Option<String>,
    pub appeal_count: u8,
}

#[derive(Debug, Clone)]
pub struct DecisionLifecycle {
    pub case_id: String,
    pub current_state: DecisionState,
    pub context: DecisionContext,
    pub state_history: Vec<(DecisionState, DateTime<Utc>)>,
    pub version: u64,
    pub ttl_minutes: u32,
}

impl DecisionLifecycle {
    pub fn new(case_id: String, creator: String, summary: String) -> Self {
        let now = Utc::now();
        let context = DecisionContext {
            case_id: case_id.clone(),
            created_at: now,
            created_by: creator,
            summary,
            evidence: Vec::new(),
            arbiters: Vec::new(),
            ruling: None,
            appeal_count: 0,
        };

        Self {
            case_id,
            current_state: DecisionState::Open,
            context,
            state_history: vec![(DecisionState::Open, now)],
            version: 1,
            ttl_minutes: 10080,
        }
    }

    pub fn add_evidence(&mut self, evidence: String) {
        self.context.evidence.push(evidence);
        self.version += 1;
    }

    pub fn assign_arbiter(&mut self, arbiter_id: String) -> Result<(), StateTransitionError> {
        match self.current_state {
            DecisionState::Open | DecisionState::UnderReview => {
                self.context.arbiters.push(arbiter_id);
                self.version += 1;
                Ok(())
            }
            _ => Err(StateTransitionError::InvalidTransition {
                from: self.current_state.as_str().to_string(),
                to: "assign_arbiter".to_string(),
            }),
        }
    }

    pub fn transition_to(&mut self, target_state: DecisionState) -> Result<(), StateTransitionError> {
        self.check_expired()?;

        let valid_transition = match (self.current_state, target_state) {
            (DecisionState::Open, DecisionState::UnderReview) => true,
            (DecisionState::UnderReview, DecisionState::Arbitration) => {
                !self.context.arbiters.is_empty()
            }
            (DecisionState::Arbitration, DecisionState::Ruled) => {
                self.context.ruling.is_some()
            }
            (DecisionState::Ruled, DecisionState::Closed) => true,
            (DecisionState::Open, DecisionState::Failed) => true,
            (DecisionState::UnderReview, DecisionState::Failed) => true,
            (DecisionState::Ruled, DecisionState::Appealed) => {
                self.context.appeal_count < 3
            }
            (DecisionState::Appealed, DecisionState::Arbitration) => true,
            (DecisionState::Appealed, DecisionState::Closed) => true,
            _ => false,
        };

        if !valid_transition {
            return Err(StateTransitionError::InvalidTransition {
                from: self.current_state.as_str().to_string(),
                to: target_state.as_str().to_string(),
            });
        }

        self.current_state = target_state;
        self.state_history.push((target_state, Utc::now()));
        self.version += 1;

        Ok(())
    }

    pub fn set_ruling(&mut self, ruling: String) -> Result<(), StateTransitionError> {
        if self.current_state != DecisionState::Arbitration &&
           self.current_state != DecisionState::Appealed {
            return Err(StateTransitionError::InvalidTransition {
                from: self.current_state.as_str().to_string(),
                to: "set_ruling".to_string(),
            });
        }

        self.context.ruling = Some(ruling);
        self.version += 1;
        Ok(())
    }

    pub fn appeal(&mut self) -> Result<(), StateTransitionError> {
        if self.current_state != DecisionState::Ruled {
            return Err(StateTransitionError::InvalidTransition {
                from: self.current_state.as_str().to_string(),
                to: "appealed".to_string(),
            });
        }

        if self.context.appeal_count >= 3 {
            return Err(StateTransitionError::InvalidTransition {
                from: "max_appeals".to_string(),
                to: "appealed".to_string(),
            });
        }

        self.transition_to(DecisionState::Appealed)?;
        self.context.appeal_count += 1;
        Ok(())
    }

    pub fn check_expired(&self) -> Result<(), StateTransitionError> {
        let created = self.context.created_at;
        let ttl = Duration::minutes(self.ttl_minutes as i64);
        let expiration = created + ttl;

        if Utc::now() > expiration &&
           self.current_state != DecisionState::Closed &&
           self.current_state != DecisionState::Failed {
            return Err(StateTransitionError::ExpiredDecision);
        }

        Ok(())
    }

    pub fn time_to_expiration_seconds(&self) -> i64 {
        let created = self.context.created_at;
        let ttl = Duration::minutes(self.ttl_minutes as i64);
        let expiration = created + ttl;
        let remaining = expiration - Utc::now();
        remaining.num_seconds().max(0)
    }

    pub fn is_final_state(&self) -> bool {
        matches!(
            self.current_state,
            DecisionState::Closed | DecisionState::Failed
        )
    }

    pub fn finalize(&mut self) -> Result<(), StateTransitionError> {
        if self.is_final_state() {
            return Ok(());
        }

        self.transition_to(DecisionState::Closed)
    }
}

pub struct DecisionStore {
    decisions: HashMap<String, DecisionLifecycle>,
}

impl DecisionStore {
    pub fn new() -> Self {
        Self {
            decisions: HashMap::new(),
        }
    }

    pub fn create_decision(
        &mut self,
        case_id: String,
        creator: String,
        summary: String,
    ) -> DecisionLifecycle {
        let decision = DecisionLifecycle::new(case_id.clone(), creator, summary);
        self.decisions.insert(case_id, decision.clone());
        decision
    }

    pub fn get_decision(&self, case_id: &str) -> Option<&DecisionLifecycle> {
        self.decisions.get(case_id)
    }

    pub fn get_decision_mut(&mut self, case_id: &str) -> Option<&mut DecisionLifecycle> {
        self.decisions.get_mut(case_id)
    }

    pub fn list_decisions(&self) -> Vec<&DecisionLifecycle> {
        self.decisions.values().collect()
    }

    pub fn decisions_by_state(&self, state: DecisionState) -> Vec<&DecisionLifecycle> {
        self.decisions
            .values()
            .filter(|d| d.current_state == state)
            .collect()
    }

    pub fn cleanup_expired(&mut self) {
        self.decisions.retain(|_, decision| {
            !(decision.check_expired().is_err() && !decision.is_final_state())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_lifecycle_creation() {
        let lifecycle = DecisionLifecycle::new(
            "case_001".to_string(),
            "alice".to_string(),
            "Unauthorized access attempt".to_string(),
        );

        assert_eq!(lifecycle.current_state, DecisionState::Open);
        assert_eq!(lifecycle.case_id, "case_001");
        assert_eq!(lifecycle.context.appeal_count, 0);
        assert_eq!(lifecycle.version, 1);
    }

    #[test]
    fn test_transition_open_to_under_review() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_002".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        let result = lifecycle.transition_to(DecisionState::UnderReview);
        assert!(result.is_ok());
        assert_eq!(lifecycle.current_state, DecisionState::UnderReview);
        assert_eq!(lifecycle.version, 2);
    }

    #[test]
    fn test_invalid_transition_open_to_closed() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_003".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        let result = lifecycle.transition_to(DecisionState::Closed);
        assert!(result.is_err());
        assert_eq!(lifecycle.current_state, DecisionState::Open);
    }

    #[test]
    fn test_arbiter_assignment() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_004".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        let result = lifecycle.assign_arbiter("arbiter_bob".to_string());
        assert!(result.is_ok());
        assert_eq!(lifecycle.context.arbiters.len(), 1);
        assert_eq!(lifecycle.version, 2);
    }

    #[test]
    fn test_arbitration_requires_arbiter() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_005".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.transition_to(DecisionState::UnderReview).ok();

        let result = lifecycle.transition_to(DecisionState::Arbitration);
        assert!(result.is_err());

        lifecycle.assign_arbiter("arbiter_charlie".to_string()).ok();
        let result = lifecycle.transition_to(DecisionState::Arbitration);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ruling_transition() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_006".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.transition_to(DecisionState::UnderReview).ok();
        lifecycle.assign_arbiter("arbiter_diana".to_string()).ok();
        lifecycle.transition_to(DecisionState::Arbitration).ok();

        let result = lifecycle.set_ruling("Agent must comply within 24h".to_string());
        assert!(result.is_ok());

        let result = lifecycle.transition_to(DecisionState::Ruled);
        assert!(result.is_ok());
    }

    #[test]
    fn test_appeal_workflow() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_007".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.transition_to(DecisionState::UnderReview).ok();
        lifecycle.assign_arbiter("arbiter_eve".to_string()).ok();
        lifecycle.transition_to(DecisionState::Arbitration).ok();
        lifecycle.set_ruling("Initial ruling".to_string()).ok();
        lifecycle.transition_to(DecisionState::Ruled).ok();

        let result = lifecycle.appeal();
        assert!(result.is_ok());
        assert_eq!(lifecycle.current_state, DecisionState::Appealed);
        assert_eq!(lifecycle.context.appeal_count, 1);
    }

    #[test]
    fn test_max_appeals_limit() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_008".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.transition_to(DecisionState::UnderReview).ok();
        lifecycle.assign_arbiter("arbiter_frank".to_string()).ok();
        lifecycle.transition_to(DecisionState::Arbitration).ok();
        lifecycle.set_ruling("Ruling".to_string()).ok();
        lifecycle.transition_to(DecisionState::Ruled).ok();

        for i in 0..2 {
            let result = lifecycle.appeal();
            assert!(result.is_ok(), "Appeal {} failed", i + 1);
            lifecycle.transition_to(DecisionState::Arbitration).ok();
            lifecycle.set_ruling(format!("Ruling {}", i + 2)).ok();
            lifecycle.transition_to(DecisionState::Ruled).ok();
        }

        let result = lifecycle.appeal();
        assert!(result.is_ok(), "3rd appeal should succeed");
        assert_eq!(lifecycle.context.appeal_count, 3);

        lifecycle.transition_to(DecisionState::Arbitration).ok();
        lifecycle.set_ruling("Ruling 4".to_string()).ok();
        lifecycle.transition_to(DecisionState::Ruled).ok();

        let result = lifecycle.appeal();
        assert!(result.is_err(), "4th appeal should fail");
        assert_eq!(lifecycle.context.appeal_count, 3);
    }

    #[test]
    fn test_add_evidence() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_009".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.add_evidence("Log entry 1".to_string());
        lifecycle.add_evidence("Log entry 2".to_string());

        assert_eq!(lifecycle.context.evidence.len(), 2);
        assert_eq!(lifecycle.version, 3);
    }

    #[test]
    fn test_decision_store_create_and_retrieve() {
        let mut store = DecisionStore::new();

        let decision = store.create_decision(
            "case_010".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        assert_eq!(decision.case_id, "case_010");

        let retrieved = store.get_decision("case_010");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().case_id, "case_010");
    }

    #[test]
    fn test_decision_store_by_state() {
        let mut store = DecisionStore::new();

        store.create_decision(
            "case_011".to_string(),
            "alice".to_string(),
            "Test 1".to_string(),
        );

        let mut decision2 = store.create_decision(
            "case_012".to_string(),
            "alice".to_string(),
            "Test 2".to_string(),
        );

        decision2.transition_to(DecisionState::UnderReview).ok();
        store.decisions.insert("case_012".to_string(), decision2);

        let open_decisions = store.decisions_by_state(DecisionState::Open);
        assert_eq!(open_decisions.len(), 1);

        let under_review = store.decisions_by_state(DecisionState::UnderReview);
        assert_eq!(under_review.len(), 1);
    }

    #[test]
    fn test_time_to_expiration() {
        let lifecycle = DecisionLifecycle::new(
            "case_013".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        let ttl_seconds = lifecycle.time_to_expiration_seconds();
        assert!(ttl_seconds > 0);
        assert!(ttl_seconds <= 10080 * 60);
    }

    #[test]
    fn test_finalize_decision() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_014".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.transition_to(DecisionState::UnderReview).ok();
        lifecycle.assign_arbiter("arbiter".to_string()).ok();
        lifecycle.transition_to(DecisionState::Arbitration).ok();
        lifecycle.set_ruling("Ruling".to_string()).ok();
        lifecycle.transition_to(DecisionState::Ruled).ok();

        let result = lifecycle.finalize();
        assert!(result.is_ok());
        assert_eq!(lifecycle.current_state, DecisionState::Closed);
    }

    #[test]
    fn test_state_history() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_015".to_string(),
            "alice".to_string(),
            "Test case".to_string(),
        );

        lifecycle.transition_to(DecisionState::UnderReview).ok();
        lifecycle.transition_to(DecisionState::Failed).ok();

        assert_eq!(lifecycle.state_history.len(), 3);
        assert_eq!(lifecycle.state_history[0].0, DecisionState::Open);
        assert_eq!(lifecycle.state_history[1].0, DecisionState::UnderReview);
        assert_eq!(lifecycle.state_history[2].0, DecisionState::Failed);
    }

    #[test]
    fn test_full_workflow() {
        let mut lifecycle = DecisionLifecycle::new(
            "case_016".to_string(),
            "alice".to_string(),
            "Unauthorized access attempt by agent_bob".to_string(),
        );

        lifecycle.add_evidence("Log: unauthorized_access at 10:00".to_string());
        lifecycle.add_evidence("Log: failed_auth at 10:01".to_string());

        lifecycle.transition_to(DecisionState::UnderReview).ok();
        lifecycle.assign_arbiter("arbiter_charlie".to_string()).ok();
        lifecycle.transition_to(DecisionState::Arbitration).ok();
        lifecycle.set_ruling("Agent_bob must be isolated for 24 hours".to_string()).ok();
        lifecycle.transition_to(DecisionState::Ruled).ok();

        assert_eq!(lifecycle.current_state, DecisionState::Ruled);
        assert_eq!(lifecycle.context.evidence.len(), 2);
        assert!(lifecycle.context.ruling.is_some());

        lifecycle.finalize().ok();
        assert_eq!(lifecycle.current_state, DecisionState::Closed);
        assert!(lifecycle.is_final_state());
    }
}
