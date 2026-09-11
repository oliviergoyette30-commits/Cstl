#[cfg(test)]
mod arbitrage_comprehensive_tests {
    use chrono::Utc;
    use crate::server::arbitrage::*;
    use crate::restricted_council::RestrictedCouncil;

    // Unit tests for core structs
    #[test]
    fn test_arbiter_creation_senior() {
        let arbiter = Arbiter {
            arbiter_id: "arbiter_alice".to_string(),
            public_key: "edpk1a2b3c4d5e6f".to_string(),
            authority_level: AuthorityLevel::Senior,
            stake_amount: 500_000,
            registered_at: Utc::now(),
            is_active: true,
        };
        assert_eq!(arbiter.arbiter_id, "arbiter_alice");
        assert_eq!(arbiter.authority_level, AuthorityLevel::Senior);
        assert!(arbiter.is_active);
        assert_eq!(arbiter.stake_amount, 500_000);
    }

    #[test]
    fn test_authority_level_hierarchy() {
        let trainee = AuthorityLevel::Trainee;
        let senior = AuthorityLevel::Senior;
        let expert = AuthorityLevel::Expert;
        assert!(senior > trainee);
        assert!(expert > senior);
    }

    #[test]
    fn test_all_contradiction_types() {
        let types = vec![
            ContradictionType::MutuallyExclusive,
            ContradictionType::LogicalBreak,
            ContradictionType::RefusalToComply,
            ContradictionType::InvalidProof,
        ];
        assert_eq!(types.len(), 4);
    }

    #[test]
    fn test_case_record_full_lifecycle() {
        let mut case = CaseRecord {
            case_id: "case_lifecycle_001".to_string(),
            escalation_source: "hypothesis_engine".to_string(),
            contradiction_type: ContradictionType::MutuallyExclusive,
            status: CaseStatus::Open,
            description: "Test lifecycle".to_string(),
            assigned_arbiters: vec![],
            opened_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert_eq!(case.status, CaseStatus::Open);

        case.assigned_arbiters = vec!["arbiter_1".to_string(), "arbiter_2".to_string()];
        case.status = CaseStatus::InProgress;
        assert_eq!(case.status, CaseStatus::InProgress);

        case.status = CaseStatus::RulingSubmitted;
        assert_eq!(case.status, CaseStatus::RulingSubmitted);

        case.status = CaseStatus::Finalized;
        assert_eq!(case.status, CaseStatus::Finalized);
    }

    #[test]
    fn test_arbitration_ruling_complete() {
        let ruling = ArbitrationRuling {
            ruling_id: "ruling_001".to_string(),
            case_id: "case_001".to_string(),
            arbiter_id: "arbiter_alice".to_string(),
            decision: "accept_assertion_A".to_string(),
            justification: "Assertion A has stronger logical support".to_string(),
            signature: "ed25519_sig_abcd1234".to_string(),
            ruled_at: Utc::now(),
        };

        assert_eq!(ruling.ruling_id, "ruling_001");
        assert_eq!(ruling.arbiter_id, "arbiter_alice");
        assert_eq!(ruling.decision, "accept_assertion_A");
    }

    #[test]
    fn test_peer_review_signature_creation() {
        let review = PeerReviewSignature {
            reviewing_arbiter_id: "arbiter_bob".to_string(),
            review_signature: "ed25519_review_sig_efgh5678".to_string(),
            reviewed_at: Utc::now(),
        };
        assert_eq!(review.reviewing_arbiter_id, "arbiter_bob");
    }

    #[test]
    fn test_arbitration_ruling_final() {
        let ruling = ArbitrationRuling {
            ruling_id: "ruling_001".to_string(),
            case_id: "case_001".to_string(),
            arbiter_id: "arbiter_alice".to_string(),
            decision: "accept_assertion_A".to_string(),
            justification: "Strong logic".to_string(),
            signature: "sig123".to_string(),
            ruled_at: Utc::now(),
        };

        let reviews = vec![
            PeerReviewSignature {
                reviewing_arbiter_id: "arbiter_bob".to_string(),
                review_signature: "review_sig_1".to_string(),
                reviewed_at: Utc::now(),
            },
        ];

        let final_ruling = ArbitrationRulingFinal {
            ruling,
            peer_reviews: reviews,
            finalized_at: Utc::now(),
        };

        assert_eq!(final_ruling.ruling.ruling_id, "ruling_001");
        assert_eq!(final_ruling.peer_reviews.len(), 1);
    }

    // Error handling tests
    #[test]
    fn test_error_case_not_found() {
        let err = ArbitrationError::CaseNotFound("case_unknown".to_string());
        let msg = err.to_string();
        assert!(msg.contains("case_unknown"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_error_invalid_signature() {
        let err = ArbitrationError::InvalidSignature;
        let msg = err.to_string();
        assert!(msg.contains("Signature"));
    }

    #[test]
    fn test_error_quorum_not_reached() {
        let err = ArbitrationError::QuorumNotReached(1, 2);
        let msg = err.to_string();
        assert!(msg.contains("1"));
        assert!(msg.contains("2"));
    }

    #[test]
    fn test_error_unauthorized_arbiter() {
        let err = ArbitrationError::UnauthorizedArbiter("arbiter_malicious".to_string());
        let msg = err.to_string();
        assert!(msg.contains("not authorized"));
    }

    // Helper function tests
    #[test]
    fn test_verify_ruling_signatures_valid() {
        let result = verify_ruling_signatures("arbiter_1", "payload", "signature");
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_ruling_signatures_empty_arbiter() {
        let result = verify_ruling_signatures("", "payload", "signature");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_finality_single_member() {
        let council = RestrictedCouncil::single_member("Olivier");
        let result = check_finality_threshold(1, &council);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_finality_multi_member_pass() {
        let council = RestrictedCouncil::new(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]);
        let result = check_finality_threshold(2, &council);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_finality_multi_member_fail() {
        let council = RestrictedCouncil::new(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]);
        let result = check_finality_threshold(1, &council);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_rulings_same_case() {
        let case_id = "case_multi_ruling_001";

        let ruling_1 = ArbitrationRuling {
            ruling_id: "ruling_1".to_string(),
            case_id: case_id.to_string(),
            arbiter_id: "arbiter_alice".to_string(),
            decision: "accept_assertion_A".to_string(),
            justification: "Reason 1".to_string(),
            signature: "sig_1".to_string(),
            ruled_at: Utc::now(),
        };

        let ruling_2 = ArbitrationRuling {
            ruling_id: "ruling_2".to_string(),
            case_id: case_id.to_string(),
            arbiter_id: "arbiter_bob".to_string(),
            decision: "accept_assertion_A".to_string(),
            justification: "Reason 2".to_string(),
            signature: "sig_2".to_string(),
            ruled_at: Utc::now(),
        };

        let rulings = vec![ruling_1, ruling_2];
        assert_eq!(rulings.len(), 2);
    }

    #[test]
    fn test_arbiter_stake_hierarchy() {
        let arbiters = vec![
            Arbiter {
                arbiter_id: "arbiter_1".to_string(),
                public_key: "key1".to_string(),
                authority_level: AuthorityLevel::Trainee,
                stake_amount: 100_000,
                registered_at: Utc::now(),
                is_active: true,
            },
            Arbiter {
                arbiter_id: "arbiter_2".to_string(),
                public_key: "key2".to_string(),
                authority_level: AuthorityLevel::Senior,
                stake_amount: 500_000,
                registered_at: Utc::now(),
                is_active: true,
            },
            Arbiter {
                arbiter_id: "arbiter_3".to_string(),
                public_key: "key3".to_string(),
                authority_level: AuthorityLevel::Expert,
                stake_amount: 1_000_000,
                registered_at: Utc::now(),
                is_active: true,
            },
        ];

        let total_stake: u64 = arbiters.iter().map(|a| a.stake_amount).sum();
        assert_eq!(total_stake, 1_600_000);
    }

    #[test]
    fn test_escalation_to_council() {
        let mut case = CaseRecord {
            case_id: "case_escalation_001".to_string(),
            escalation_source: "arbitrage_deadlock".to_string(),
            contradiction_type: ContradictionType::RefusalToComply,
            status: CaseStatus::InProgress,
            description: "Agent refuses to accept verdict".to_string(),
            assigned_arbiters: vec!["arbiter_1".to_string()],
            opened_at: Utc::now(),
            updated_at: Utc::now(),
        };

        case.status = CaseStatus::EscalatedToCouncil;
        assert_eq!(case.status, CaseStatus::EscalatedToCouncil);
    }
}