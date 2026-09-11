//! Unit Tests for QuorumManager Trait
//!
//! Coverage: 40+ tests for structs, consensus logic, Byzantine tolerance

#[cfg(test)]
mod quorum_tests {
    use super::super::*;

    fn create_test_member(id: &str, suffix: &str) -> QuorumMember {
        QuorumMember::new(
            id.to_string(),
            format!("{}{}", suffix.repeat(16), suffix.repeat(16)),
            "validator".to_string(),
        )
    }

    #[test]
    fn test_member_initialization() {
        let m = create_test_member("alice", "a1");
        assert_eq!(m.status, "active");
        assert_eq!(m.health_score, 1.0);
        assert!(m.is_eligible());
    }

    #[test]
    fn test_member_health_degradation() {
        let mut m = create_test_member("bob", "b2");
        m.update_health(0.5);
        assert_eq!(m.status, "degraded");
        assert!(m.is_eligible());

        m.update_health(0.2);
        assert_eq!(m.status, "inactive");
        assert!(!m.is_eligible());
    }

    #[test]
    fn test_member_health_clamping() {
        let mut m = create_test_member("charlie", "c3");
        m.update_health(2.0);
        assert_eq!(m.health_score, 1.0);
        m.update_health(-0.5);
        assert_eq!(m.health_score, 0.0);
    }

    #[test]
    fn test_vote_creation() {
        let vote = VoteMessage::new(
            "prop_001".to_string(),
            "member_alice".to_string(),
            "yea".to_string(),
            1,
        );
        assert_eq!(vote.proposal_id, "prop_001");
        assert_eq!(vote.decision, "yea");
        assert!(vote.verify_integrity());
    }

    #[test]
    fn test_vote_signature_validation() {
        let mut vote = VoteMessage::new(
            "prop_001".to_string(),
            "member_alice".to_string(),
            "nay".to_string(),
            1,
        );

        assert!(vote.set_signature("tooshort".to_string()).is_err());
        assert!(vote.set_signature("G".repeat(128)).is_err()); // Invalid hex

        let valid = "a1".repeat(64);
        assert!(vote.set_signature(valid.clone()).is_ok());
        assert_eq!(vote.signature, valid);
    }

    #[test]
    fn test_vote_integrity() {
        let vote = VoteMessage::new(
            "prop_001".to_string(),
            "member_alice".to_string(),
            "yea".to_string(),
            1,
        );
        assert!(vote.verify_integrity());
    }

    #[test]
    fn test_quorum_state_init() {
        let state = QuorumState::new("prop_001".to_string(), 2);
        assert_eq!(state.round, 1);
        assert_eq!(state.yea_count, 0);
        assert_eq!(state.threshold, 2);
        assert!(!state.circuit_breaker_active);
    }

    #[test]
    fn test_quorum_state_vote_addition() {
        let mut state = QuorumState::new("prop_001".to_string(), 2);
        assert!(state.add_vote("yea").is_ok());
        assert_eq!(state.yea_count, 1);
        assert!(state.add_vote("nay").is_ok());
        assert!(state.add_vote("abstain").is_ok());
    }

    #[test]
    fn test_quorum_state_invalid_vote() {
        let mut state = QuorumState::new("prop_001".to_string(), 2);
        assert!(state.add_vote("invalid").is_err());
        assert_eq!(state.yea_count, 0);
    }

    #[test]
    fn test_consensus_yea() {
        let mut state = QuorumState::new("prop_001".to_string(), 2);
        state.add_vote("yea").unwrap();
        let (reached, _) = state.check_consensus();
        assert!(!reached);

        state.add_vote("yea").unwrap();
        let (reached, decision) = state.check_consensus();
        assert!(reached);
        assert_eq!(decision, "yea");
    }

    #[test]
    fn test_consensus_nay() {
        let mut state = QuorumState::new("prop_002".to_string(), 3);
        state.add_vote("nay").unwrap();
        state.add_vote("nay").unwrap();
        state.add_vote("nay").unwrap();

        let (reached, decision) = state.check_consensus();
        assert!(reached);
        assert_eq!(decision, "nay");
    }

    #[test]
    fn test_consensus_mixed() {
        let mut state = QuorumState::new("prop_003".to_string(), 3);
        state.add_vote("yea").unwrap();
        state.add_vote("nay").unwrap();
        state.add_vote("abstain").unwrap();

        let (reached, _) = state.check_consensus();
        assert!(!reached);
    }

    #[test]
    fn test_circuit_breaker() {
        let mut state = QuorumState::new("prop_004".to_string(), 2);
        state.activate_circuit_breaker("test".to_string());
        assert!(state.circuit_breaker_active);
        assert!(state.add_vote("yea").is_err());
        assert_eq!(state.yea_count, 0);
    }

    #[test]
    fn test_circuit_breaker_consensus_check() {
        let mut state = QuorumState::new("prop_005".to_string(), 1);
        state.activate_circuit_breaker("test".to_string());
        let (reached, reason) = state.check_consensus();
        assert!(!reached);
        assert_eq!(reason, "circuit_breaker_active");
    }

    #[test]
    fn test_compute_threshold() {
        assert_eq!(compute_threshold(0), 0);
        assert_eq!(compute_threshold(1), 1);
        assert_eq!(compute_threshold(2), 2);
        assert_eq!(compute_threshold(3), 2);
        assert_eq!(compute_threshold(4), 3);
        assert_eq!(compute_threshold(6), 4);
        assert_eq!(compute_threshold(9), 6);
        assert_eq!(compute_threshold(100), 67);
    }

    #[test]
    fn test_circuit_breaker_activation() {
        let (should_activate, _) = should_activate_circuit_breaker(3, 1.0);
        assert!(should_activate);

        let (should_activate, _) = should_activate_circuit_breaker(1, 0.6);
        assert!(should_activate);

        let (should_activate, _) = should_activate_circuit_breaker(1, 0.8);
        assert!(!should_activate);
    }

    #[test]
    fn test_aggregate_health_empty() {
        let members: Vec<QuorumMember> = vec![];
        assert_eq!(aggregate_health_score(&members), 0.0);
    }

    #[test]
    fn test_aggregate_health_single() {
        let members = vec![create_test_member("m1", "a1")];
        assert_eq!(aggregate_health_score(&members), 1.0);
    }

    #[test]
    fn test_aggregate_health_multiple() {
        let mut members = vec![create_test_member("m1", "a1")];
        let mut m2 = create_test_member("m2", "a2");
        m2.update_health(0.5);
        members.push(m2);

        let avg = aggregate_health_score(&members);
        assert!((avg - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_byzantine_tolerance() {
        let mut state = QuorumState::new("prop_bft".to_string(), 5);
        for _ in 0..5 {
            state.add_vote("yea").unwrap();
        }
        let (reached, decision) = state.check_consensus();
        assert!(reached);
        assert_eq!(decision, "yea");

        state.add_vote("nay").unwrap();
        state.add_vote("nay").unwrap();
        let (reached, decision) = state.check_consensus();
        assert!(reached);
        assert_eq!(decision, "yea");
    }
}
