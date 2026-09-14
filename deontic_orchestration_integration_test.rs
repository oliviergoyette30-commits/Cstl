use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;

mod common {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use cstl_parser::adn_store::AdnStore;
    use cstl_parser::governance::GovernanceTracker;
    use cstl_parser::restricted_council::RestrictedCouncil;
    use cstl_parser::server::deontic_orchestration::DeonticOrchestrator;

    pub fn create_test_orchestrator() -> DeonticOrchestrator {
        let adn_store = Arc::new(Mutex::new(AdnStore::open(":memory:").unwrap()));
        let governance = Arc::new(Mutex::new(GovernanceTracker::with_defaults()));
        let council = Arc::new(RestrictedCouncil::single_member("TestCouncil"));

        DeonticOrchestrator::new(100, adn_store, governance, council)
    }
}

#[tokio::test]
async fn test_event_routing_to_multiple_agents() {
    let orchestrator = common::create_test_orchestrator();

    let rule1 = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "message_relay",
        "all_messages",
        "relay_to_agent_1",
    );

    let rule2 = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "message_relay",
        "all_messages",
        "relay_to_agent_2",
    );

    orchestrator.register_rule(rule1).await;
    orchestrator.register_rule(rule2).await;

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "agent_alice".to_string(),
        receiver_id: "agent_bob".to_string(),
        payload: "hello".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;

    assert_eq!(executions.len(), 2);
    assert!(executions.iter().all(|e| e.result == cstl_parser::server::deontic_orchestration::ExecutionResult::Success));
}

#[tokio::test]
async fn test_conflict_resolution_must_vs_must_not() {
    let orchestrator = common::create_test_orchestrator();

    let rule_must = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "message_relay",
        "all_messages",
        "relay_message",
    ).with_priority(200);

    let rule_must_not = cstl_parser::server::deontic_orchestration::DeonticRule::new_must_not(
        "message_relay",
        "sender=malicious_agent",
        "block_message",
    ).with_priority(210);

    orchestrator.register_rule(rule_must).await;
    orchestrator.register_rule(rule_must_not).await;

    let event_from_malicious = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "malicious_agent".to_string(),
        receiver_id: "victim".to_string(),
        payload: "attack".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event_from_malicious).await;

    let must_not_execution = executions
        .iter()
        .find(|e| e.modality == cstl_parser::server::deontic_orchestration::DeonticModality::MustNot);

    assert!(must_not_execution.is_some());
    assert_eq!(must_not_execution.unwrap().result, cstl_parser::server::deontic_orchestration::ExecutionResult::Rejected);
}

#[tokio::test]
async fn test_governance_breach_escalation() {
    let orchestrator = common::create_test_orchestrator();

    let rule_low_severity = cstl_parser::server::deontic_orchestration::DeonticRule::new_may(
        "governance_breach",
        "severity>=1",
        "log_low_breach",
    );

    let rule_high_severity = cstl_parser::server::deontic_orchestration::DeonticRule::new_must_not(
        "governance_breach",
        "severity>=8",
        "isolate_agent",
    ).with_priority(220);

    orchestrator.register_rule(rule_low_severity).await;
    orchestrator.register_rule(rule_high_severity).await;

    let low_severity_breach = cstl_parser::server::deontic_orchestration::DeonticEvent::GovernanceBreach {
        breach_id: Uuid::new_v4().to_string(),
        agent_id: "agent_slow".to_string(),
        breach_type: "high_latency".to_string(),
        severity: 3,
        timestamp: Utc::now(),
    };

    let executions_low = orchestrator.emit_event(low_severity_breach).await;
    let rejected_low = executions_low.iter().any(|e| e.result == cstl_parser::server::deontic_orchestration::ExecutionResult::Rejected);
    assert!(!rejected_low);

    let high_severity_breach = cstl_parser::server::deontic_orchestration::DeonticEvent::GovernanceBreach {
        breach_id: Uuid::new_v4().to_string(),
        agent_id: "agent_rogue".to_string(),
        breach_type: "unauthorized_state_change".to_string(),
        severity: 9,
        timestamp: Utc::now(),
    };

    let executions_high = orchestrator.emit_event(high_severity_breach).await;
    let rejected_high = executions_high
        .iter()
        .any(|e| e.result == cstl_parser::server::deontic_orchestration::ExecutionResult::Rejected);
    assert!(rejected_high);
}

#[tokio::test]
async fn test_replay_safety_idempotent_events() {
    let orchestrator = common::create_test_orchestrator();

    let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "agent_register",
        "all_agents",
        "register_agent",
    );

    orchestrator.register_rule(rule).await;

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::AgentRegister {
        agent_id: "agent_idempotent".to_string(),
        agent_name: "IdempotentAgent".to_string(),
        public_key: "key_123".to_string(),
        timestamp: Utc::now(),
    };

    let exec1 = orchestrator.emit_event(event.clone()).await;
    let exec2 = orchestrator.emit_event(event.clone()).await;

    assert_eq!(exec1.len(), exec2.len());
    assert_eq!(exec1[0].result, exec2[0].result);

    let all_executions = orchestrator.get_executions().await;
    assert_eq!(all_executions.len(), 2);
}

#[tokio::test]
async fn test_arbitration_ruling_enforcement() {
    let orchestrator = common::create_test_orchestrator();

    let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "arbitration_ruling",
        "all_rulings",
        "enforce_ruling",
    ).with_priority(200);

    orchestrator.register_rule(rule).await;

    let ruling = cstl_parser::server::deontic_orchestration::DeonticEvent::ArbitrationRuling {
        ruling_id: Uuid::new_v4().to_string(),
        case_id: "case_2026_001".to_string(),
        arbiter_id: "arbiter_alice".to_string(),
        decision: "Agent must comply within 24 hours".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(ruling).await;

    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].modality, cstl_parser::server::deontic_orchestration::DeonticModality::Must);
    assert_eq!(executions[0].result, cstl_parser::server::deontic_orchestration::ExecutionResult::Success);
}

#[tokio::test]
async fn test_priority_based_rule_execution() {
    let orchestrator = common::create_test_orchestrator();

    let rule_low_priority = cstl_parser::server::deontic_orchestration::DeonticRule::new_may(
        "message_relay",
        "all_messages",
        "log_message",
    ).with_priority(50);

    let rule_medium_priority = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "message_relay",
        "all_messages",
        "relay_message",
    ).with_priority(150);

    let rule_high_priority = cstl_parser::server::deontic_orchestration::DeonticRule::new_must_not(
        "message_relay",
        "sender=attacker",
        "block_message",
    ).with_priority(250);

    orchestrator.register_rule(rule_low_priority).await;
    orchestrator.register_rule(rule_medium_priority).await;
    orchestrator.register_rule(rule_high_priority).await;

    assert_eq!(orchestrator.rules_count().await, 3);

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "normal_sender".to_string(),
        receiver_id: "receiver".to_string(),
        payload: "message".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;

    assert_eq!(executions.len(), 3);
}

#[tokio::test]
async fn test_multi_agent_registration_workflow() {
    let orchestrator = common::create_test_orchestrator();

    let rule_register = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "agent_register",
        "all_agents",
        "process_registration",
    ).with_priority(200);

    orchestrator.register_rule(rule_register).await;

    let agents = vec!["alice", "bob", "charlie", "diana", "eve"];

    for agent_name in agents.iter() {
        let event = cstl_parser::server::deontic_orchestration::DeonticEvent::AgentRegister {
            agent_id: format!("agent_{}", agent_name),
            agent_name: agent_name.to_string(),
            public_key: format!("key_{}", agent_name),
            timestamp: Utc::now(),
        };

        let executions = orchestrator.emit_event(event).await;
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].result, cstl_parser::server::deontic_orchestration::ExecutionResult::Success);
    }

    let all_executions = orchestrator.get_executions().await;
    assert_eq!(all_executions.len(), agents.len());
}

#[tokio::test]
async fn test_event_broadcast_to_multiple_subscribers() {
    let orchestrator = common::create_test_orchestrator();

    let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "agent_register",
        "all_agents",
        "register",
    );

    orchestrator.register_rule(rule).await;

    let _rx1 = orchestrator.subscribe();
    let _rx2 = orchestrator.subscribe();

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::AgentRegister {
        agent_id: "agent_broadcast_test".to_string(),
        agent_name: "BroadcastAgent".to_string(),
        public_key: "broadcast_key".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;
    assert_eq!(executions.len(), 1);
}

#[tokio::test]
async fn test_mixed_modality_execution_order() {
    let orchestrator = common::create_test_orchestrator();

    let must_not_rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must_not(
        "message_relay",
        "sender=blocked",
        "reject",
    ).with_priority(220);

    let must_rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "message_relay",
        "all_messages",
        "relay",
    ).with_priority(200);

    let may_rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_may(
        "message_relay",
        "all_messages",
        "log",
    ).with_priority(100);

    orchestrator.register_rule(must_not_rule).await;
    orchestrator.register_rule(must_rule).await;
    orchestrator.register_rule(may_rule).await;

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "normal".to_string(),
        receiver_id: "receiver".to_string(),
        payload: "test".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;

    assert_eq!(executions.len(), 3);

    let must_not_exec = &executions[0];
    assert_eq!(must_not_exec.modality, cstl_parser::server::deontic_orchestration::DeonticModality::MustNot);
    assert_eq!(must_not_exec.result, cstl_parser::server::deontic_orchestration::ExecutionResult::NoMatch);
}

#[tokio::test]
async fn test_audit_trail_persistence() {
    let orchestrator = common::create_test_orchestrator();

    let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "governance_breach",
        "all_breaches",
        "audit",
    );

    orchestrator.register_rule(rule).await;

    for i in 1..=5 {
        let event = cstl_parser::server::deontic_orchestration::DeonticEvent::GovernanceBreach {
            breach_id: Uuid::new_v4().to_string(),
            agent_id: format!("agent_{}", i),
            breach_type: "test_breach".to_string(),
            severity: (i as u8),
            timestamp: Utc::now(),
        };

        let _ = orchestrator.emit_event(event).await;
    }

    let audit_trail = orchestrator.get_executions().await;
    assert_eq!(audit_trail.len(), 5);

    let all_successful = audit_trail
        .iter()
        .all(|e| e.result == cstl_parser::server::deontic_orchestration::ExecutionResult::Success);
    assert!(all_successful);
}

#[tokio::test]
async fn test_conditional_rule_matching() {
    let orchestrator = common::create_test_orchestrator();

    let rule_specific_sender = cstl_parser::server::deontic_orchestration::DeonticRule::new_must_not(
        "message_relay",
        "sender=specific_agent",
        "block",
    ).with_priority(210);

    let rule_all_messages = cstl_parser::server::deontic_orchestration::DeonticRule::new_may(
        "message_relay",
        "all_messages",
        "log",
    ).with_priority(100);

    orchestrator.register_rule(rule_specific_sender).await;
    orchestrator.register_rule(rule_all_messages).await;

    let event1 = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "specific_agent".to_string(),
        receiver_id: "receiver".to_string(),
        payload: "blocked".to_string(),
        timestamp: Utc::now(),
    };

    let executions1 = orchestrator.emit_event(event1).await;
    let rejected_count1 = executions1
        .iter()
        .filter(|e| e.result == cstl_parser::server::deontic_orchestration::ExecutionResult::Rejected)
        .count();
    assert_eq!(rejected_count1, 1);

    let event2 = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "other_agent".to_string(),
        receiver_id: "receiver".to_string(),
        payload: "allowed".to_string(),
        timestamp: Utc::now(),
    };

    let executions2 = orchestrator.emit_event(event2).await;
    let rejected_count2 = executions2
        .iter()
        .filter(|e| e.result == cstl_parser::server::deontic_orchestration::ExecutionResult::Rejected)
        .count();
    assert_eq!(rejected_count2, 0);
}

#[tokio::test]
async fn test_no_rule_match_all_events_logged() {
    let orchestrator = common::create_test_orchestrator();

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "alice".to_string(),
        receiver_id: "bob".to_string(),
        payload: "test".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;

    assert_eq!(executions.len(), 0);
    let history = orchestrator.get_executions().await;
    assert_eq!(history.len(), 0);
}

#[tokio::test]
async fn test_sequential_event_processing() {
    let orchestrator = Arc::new(common::create_test_orchestrator());

    let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "agent_register",
        "all_agents",
        "register",
    );

    orchestrator.register_rule(rule).await;

    for i in 0..5 {
        let event = cstl_parser::server::deontic_orchestration::DeonticEvent::AgentRegister {
            agent_id: format!("agent_{}", i),
            agent_name: format!("Agent{}", i),
            public_key: format!("key_{}", i),
            timestamp: Utc::now(),
        };

        let _ = orchestrator.emit_event(event).await;
    }

    let all_executions = orchestrator.get_executions().await;
    assert_eq!(all_executions.len(), 5);
}

#[tokio::test]
async fn test_error_handling_malformed_event() {
    let orchestrator = common::create_test_orchestrator();

    let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_must(
        "message_relay",
        "all_messages",
        "relay",
    );

    orchestrator.register_rule(rule).await;

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::MessageRelay {
        event_id: Uuid::new_v4().to_string(),
        sender_id: "".to_string(),
        receiver_id: "".to_string(),
        payload: "".to_string(),
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;
    assert_eq!(executions.len(), 1);
}

#[tokio::test]
async fn test_rule_priority_execution_order() {
    let orchestrator = common::create_test_orchestrator();

    for priority in vec![100, 150, 200, 50] {
        let rule = cstl_parser::server::deontic_orchestration::DeonticRule::new_may(
            "governance_breach",
            "all_breaches",
            &format!("action_priority_{}", priority),
        ).with_priority(priority);

        orchestrator.register_rule(rule).await;
    }

    let event = cstl_parser::server::deontic_orchestration::DeonticEvent::GovernanceBreach {
        breach_id: Uuid::new_v4().to_string(),
        agent_id: "test_agent".to_string(),
        breach_type: "test".to_string(),
        severity: 5,
        timestamp: Utc::now(),
    };

    let executions = orchestrator.emit_event(event).await;

    assert_eq!(executions.len(), 4);

    let priorities: Vec<_> = executions
        .iter()
        .map(|e| {
            e.action
                .trim_start_matches("action_priority_")
                .parse::<u8>()
                .unwrap_or(0)
        })
        .collect();

    let is_sorted_desc = priorities.windows(2).all(|w| w[0] >= w[1]);
    assert!(is_sorted_desc);
}
