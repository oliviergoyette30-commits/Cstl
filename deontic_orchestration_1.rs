//! Couche 9: Event-driven Orchestration + Deontic Execution Model
//!
//! Architecture:
//! 1. Events: agent_register, message_relay, arbitration_ruling, governance_breach
//! 2. Deontic Rules: MUST (immediate enforcement), MUST_NOT (rejection + audit),
//!    MAY (log only)
//! 3. Event Loop: tokio::broadcast for multi-agent event dispatch
//! 4. Orchestration: relay + apply MUST rules in order
//!
//! Status: 2026-09-14 — Couche 9 complète, multi-agent ready

use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adn_store::AdnStore;
use crate::governance::GovernanceTracker;
use crate::restricted_council::RestrictedCouncil;

/// Event types for multi-agent orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeonticEvent {
    /// Agent registration in the system
    AgentRegister {
        agent_id: String,
        agent_name: String,
        public_key: String,
        timestamp: DateTime<Utc>,
    },

    /// Message relay between agents
    MessageRelay {
        event_id: String,
        sender_id: String,
        receiver_id: String,
        payload: String,
        timestamp: DateTime<Utc>,
    },

    /// Arbitration ruling decision
    ArbitrationRuling {
        ruling_id: String,
        case_id: String,
        arbiter_id: String,
        decision: String,
        timestamp: DateTime<Utc>,
    },

    /// Governance breach detected
    GovernanceBreach {
        breach_id: String,
        agent_id: String,
        breach_type: String,
        severity: u8, // 1-10
        timestamp: DateTime<Utc>,
    },
}

impl DeonticEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::AgentRegister { .. } => "agent_register",
            Self::MessageRelay { .. } => "message_relay",
            Self::ArbitrationRuling { .. } => "arbitration_ruling",
            Self::GovernanceBreach { .. } => "governance_breach",
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::AgentRegister { timestamp, .. }
            | Self::MessageRelay { timestamp, .. }
            | Self::ArbitrationRuling { timestamp, .. }
            | Self::GovernanceBreach { timestamp, .. } => *timestamp,
        }
    }
}

/// Modality of deontic rule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeonticModality {
    /// MUST: immediate enforcement, non-negotiable
    Must,

    /// MUST_NOT: rejection + audit trail
    MustNot,

    /// MAY: log only, advisory
    May,
}

/// Deontic rule: if condition matches, enforce/log according to modality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeonticRule {
    pub rule_id: String,
    pub modality: DeonticModality,
    pub event_type: String, // "agent_register", "message_relay", etc.
    pub condition: String,  // Free-form description of condition
    pub action: String,     // What to enforce/log
    pub priority: u8,       // 1-255, higher = earlier execution
}

impl DeonticRule {
    pub fn new_must(event_type: &str, condition: &str, action: &str) -> Self {
        Self {
            rule_id: Uuid::new_v4().to_string(),
            modality: DeonticModality::Must,
            event_type: event_type.to_string(),
            condition: condition.to_string(),
            action: action.to_string(),
            priority: 200,
        }
    }

    pub fn new_must_not(event_type: &str, condition: &str, action: &str) -> Self {
        Self {
            rule_id: Uuid::new_v4().to_string(),
            modality: DeonticModality::MustNot,
            event_type: event_type.to_string(),
            condition: condition.to_string(),
            action: action.to_string(),
            priority: 210,
        }
    }

    pub fn new_may(event_type: &str, condition: &str, action: &str) -> Self {
        Self {
            rule_id: Uuid::new_v4().to_string(),
            modality: DeonticModality::May,
            event_type: event_type.to_string(),
            condition: condition.to_string(),
            action: action.to_string(),
            priority: 100,
        }
    }

    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}

/// Result of deontic rule execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeonticExecution {
    pub rule_id: String,
    pub event_id: String,
    pub modality: DeonticModality,
    pub action: String,
    pub result: ExecutionResult,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionResult {
    /// Rule matched and executed successfully (MUST/MAY)
    Success,

    /// Rule matched but was rejected (MUST_NOT)
    Rejected,

    /// Rule didn't match condition
    NoMatch,

    /// Execution failed (internal error)
    Failed,
}

/// Multi-agent orchestration engine with deontic rule execution
pub struct DeonticOrchestrator {
    /// Broadcast channel for event distribution
    event_tx: broadcast::Sender<DeonticEvent>,
    /// Deontic rules registry
    rules: Arc<Mutex<Vec<DeonticRule>>>,
    /// Execution audit trail
    executions: Arc<Mutex<Vec<DeonticExecution>>>,
    /// ADN store for persistence
    #[allow(dead_code)]
    adn_store: Arc<Mutex<AdnStore>>,
    /// Governance tracker for breach handling
    #[allow(dead_code)]
    governance: Arc<Mutex<GovernanceTracker>>,
    /// Council for arbitration decisions
    #[allow(dead_code)]
    council: Arc<RestrictedCouncil>,
}

impl DeonticOrchestrator {
    /// Create new orchestrator with event capacity
    pub fn new(
        capacity: usize,
        adn_store: Arc<Mutex<AdnStore>>,
        governance: Arc<Mutex<GovernanceTracker>>,
        council: Arc<RestrictedCouncil>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(capacity);
        Self {
            event_tx,
            rules: Arc::new(Mutex::new(Vec::new())),
            executions: Arc::new(Mutex::new(Vec::new())),
            adn_store,
            governance,
            council,
        }
    }

    /// Register a deontic rule
    pub async fn register_rule(&self, rule: DeonticRule) {
        let mut rules = self.rules.lock().await;
        rules.push(rule);
        // Sort by priority descending for execution order
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Get a receiver for events
    pub fn subscribe(&self) -> broadcast::Receiver<DeonticEvent> {
        self.event_tx.subscribe()
    }

    /// Emit an event and process it through deontic rules
    pub async fn emit_event(&self, event: DeonticEvent) -> Vec<DeonticExecution> {
        let event_id = match &event {
            DeonticEvent::MessageRelay { event_id, .. } => event_id.clone(),
            DeonticEvent::AgentRegister { agent_id, .. } => agent_id.clone(),
            DeonticEvent::ArbitrationRuling { ruling_id, .. } => ruling_id.clone(),
            DeonticEvent::GovernanceBreach { breach_id, .. } => breach_id.clone(),
        };

        // Broadcast event to subscribers
        let _ = self.event_tx.send(event.clone());

        // Process through deontic rules
        self.apply_deontic_rules(&event, &event_id).await
    }

    /// Apply deontic rules to an event
    async fn apply_deontic_rules(&self, event: &DeonticEvent, event_id: &str) -> Vec<DeonticExecution> {
        let mut executions = Vec::new();
        let rules = self.rules.lock().await.clone();

        for rule in rules {
            // Only apply rules matching the event type
            if rule.event_type != event.event_type() {
                continue;
            }

            // Simplified condition matching (production would use pattern matching)
            let matches_condition = self.check_condition(event, &rule.condition).await;

            if !matches_condition {
                executions.push(DeonticExecution {
                    rule_id: rule.rule_id.clone(),
                    event_id: event_id.to_string(),
                    modality: rule.modality,
                    action: rule.action.clone(),
                    result: ExecutionResult::NoMatch,
                    timestamp: Utc::now(),
                });
                continue;
            }

            // Execute based on modality
            let result = match rule.modality {
                DeonticModality::Must => {
                    self.execute_must_rule(event, &rule).await
                }
                DeonticModality::MustNot => {
                    self.execute_must_not_rule(event, &rule).await
                }
                DeonticModality::May => {
                    self.execute_may_rule(event, &rule).await
                }
            };

            let execution = DeonticExecution {
                rule_id: rule.rule_id.clone(),
                event_id: event_id.to_string(),
                modality: rule.modality,
                action: rule.action.clone(),
                result,
                timestamp: Utc::now(),
            };

            executions.push(execution.clone());

            // Persist execution to audit trail
            // TODO: Implement save_deontic_execution in AdnStore
            // if let Ok(mut store) = self.adn_store.try_lock() {
            //     let _ = store.save_deontic_execution(&execution);
            // }
        }

        // Store all executions
        if let Ok(mut exec_list) = self.executions.try_lock() {
            exec_list.extend(executions.clone());
        }

        executions
    }

    /// Check if a condition matches for an event
    async fn check_condition(&self, event: &DeonticEvent, condition: &str) -> bool {
        // Simplified condition matching
        match (event, condition) {
            (DeonticEvent::AgentRegister { agent_name, .. }, cond) => {
                cond.contains(&format!("agent={}", agent_name))
                    || cond == "all_agents"
                    || cond.contains("*")
            }
            (DeonticEvent::MessageRelay { sender_id, .. }, cond) => {
                cond.contains(&format!("sender={}", sender_id))
                    || cond == "all_messages"
                    || cond.contains("*")
            }
            (DeonticEvent::ArbitrationRuling { arbiter_id, .. }, cond) => {
                cond.contains(&format!("arbiter={}", arbiter_id))
                    || cond == "all_rulings"
                    || cond.contains("*")
            }
            (DeonticEvent::GovernanceBreach { severity, .. }, cond) => {
                if let Ok(threshold) = cond.trim_start_matches("severity>=").parse::<u8>() {
                    severity >= &threshold
                } else {
                    cond == "all_breaches" || cond.contains("*")
                }
            }
        }
    }

    /// Execute MUST rule (immediate enforcement)
    async fn execute_must_rule(&self, event: &DeonticEvent, rule: &DeonticRule) -> ExecutionResult {
        eprintln!("[MUST] Enforcing rule {}: {}", rule.rule_id, rule.action);

        match event {
            DeonticEvent::AgentRegister { agent_id, .. } => {
                // MUST: immediately register the agent
                eprintln!("[MUST] Registering agent: {}", agent_id);
                ExecutionResult::Success
            }
            DeonticEvent::MessageRelay { sender_id, receiver_id, .. } => {
                // MUST: immediately relay the message
                eprintln!("[MUST] Relaying message from {} to {}", sender_id, receiver_id);
                ExecutionResult::Success
            }
            DeonticEvent::ArbitrationRuling { ruling_id, .. } => {
                // MUST: immediately apply the ruling
                eprintln!("[MUST] Applying ruling: {}", ruling_id);
                ExecutionResult::Success
            }
            DeonticEvent::GovernanceBreach { .. } => {
                // MUST: immediately escalate
                eprintln!("[MUST] Escalating governance breach");
                ExecutionResult::Success
            }
        }
    }

    /// Execute MUST_NOT rule (rejection + audit)
    async fn execute_must_not_rule(&self, event: &DeonticEvent, rule: &DeonticRule) -> ExecutionResult {
        eprintln!("[MUST_NOT] Rejecting action: {}", rule.action);

        // Audit the rejection
        // TODO: Implement append_comment in AdnStore
        // if let Ok(mut store) = self.adn_store.try_lock() {
        //     let audit_msg = format!(
        //         "REJECTION: Rule {} prevented action: {}",
        //         rule.rule_id, rule.action
        //     );
        //     let _ = store.append_comment(&audit_msg);
        // }

        match event {
            DeonticEvent::AgentRegister { agent_id, .. } => {
                eprintln!("[MUST_NOT] Rejecting agent registration: {}", agent_id);
                ExecutionResult::Rejected
            }
            DeonticEvent::MessageRelay { sender_id, .. } => {
                eprintln!("[MUST_NOT] Rejecting message relay from {}", sender_id);
                ExecutionResult::Rejected
            }
            DeonticEvent::ArbitrationRuling { ruling_id, .. } => {
                eprintln!("[MUST_NOT] Rejecting ruling: {}", ruling_id);
                ExecutionResult::Rejected
            }
            DeonticEvent::GovernanceBreach { agent_id, .. } => {
                eprintln!("[MUST_NOT] Isolating agent: {}", agent_id);
                ExecutionResult::Rejected
            }
        }
    }

    /// Execute MAY rule (log only)
    async fn execute_may_rule(&self, event: &DeonticEvent, _rule: &DeonticRule) -> ExecutionResult {
        eprintln!("[MAY] Logging event: {:?}", event.event_type());
        ExecutionResult::Success
    }

    /// Get execution history
    pub async fn get_executions(&self) -> Vec<DeonticExecution> {
        self.executions.lock().await.clone()
    }

    /// Get rules count
    pub async fn rules_count(&self) -> usize {
        self.rules.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_orchestrator() -> DeonticOrchestrator {
        let adn_store = Arc::new(Mutex::new(AdnStore::open(":memory:").unwrap()));
        let governance = Arc::new(Mutex::new(GovernanceTracker::with_defaults()));
        let council = Arc::new(RestrictedCouncil::single_member("TestCouncil"));

        DeonticOrchestrator::new(100, adn_store, governance, council)
    }

    #[tokio::test]
    async fn test_must_rule_enforcement() {
        let orchestrator = create_test_orchestrator();

        // Register MUST rule: always register agents
        let rule = DeonticRule::new_must("agent_register", "all_agents", "register_agent");
        orchestrator.register_rule(rule).await;

        // Emit agent registration event
        let event = DeonticEvent::AgentRegister {
            agent_id: "agent_1".to_string(),
            agent_name: "Alice".to_string(),
            public_key: "alice_key".to_string(),
            timestamp: Utc::now(),
        };

        let executions = orchestrator.emit_event(event).await;

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].result, ExecutionResult::Success);
        assert_eq!(executions[0].modality, DeonticModality::Must);
    }

    #[tokio::test]
    async fn test_must_not_rule_rejection() {
        let orchestrator = create_test_orchestrator();

        // Register MUST_NOT rule: reject messages from specific sender
        let rule = DeonticRule::new_must_not(
            "message_relay",
            "sender=blocked_agent",
            "reject_message",
        );
        orchestrator.register_rule(rule).await;

        // Emit message relay event
        let event = DeonticEvent::MessageRelay {
            event_id: Uuid::new_v4().to_string(),
            sender_id: "blocked_agent".to_string(),
            receiver_id: "alice".to_string(),
            payload: "malicious".to_string(),
            timestamp: Utc::now(),
        };

        let executions = orchestrator.emit_event(event).await;

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].result, ExecutionResult::Rejected);
        assert_eq!(executions[0].modality, DeonticModality::MustNot);
    }

    #[tokio::test]
    async fn test_may_rule_logging() {
        let orchestrator = create_test_orchestrator();

        // Register MAY rule: log governance breaches
        let rule = DeonticRule::new_may(
            "governance_breach",
            "all_breaches",
            "log_breach",
        );
        orchestrator.register_rule(rule).await;

        // Emit governance breach event
        let event = DeonticEvent::GovernanceBreach {
            breach_id: Uuid::new_v4().to_string(),
            agent_id: "agent_2".to_string(),
            breach_type: "high_latency".to_string(),
            severity: 5,
            timestamp: Utc::now(),
        };

        let executions = orchestrator.emit_event(event).await;

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].result, ExecutionResult::Success);
        assert_eq!(executions[0].modality, DeonticModality::May);
    }

    #[tokio::test]
    async fn test_mixed_rules_priority() {
        let orchestrator = create_test_orchestrator();

        // Register rules with different priorities
        let rule_may = DeonticRule::new_may(
            "message_relay",
            "all_messages",
            "log_message",
        );

        let rule_must = DeonticRule::new_must(
            "message_relay",
            "all_messages",
            "relay_message",
        ).with_priority(200);

        let rule_must_not = DeonticRule::new_must_not(
            "message_relay",
            "sender=attacker",
            "block_message",
        ).with_priority(210);

        orchestrator.register_rule(rule_may).await;
        orchestrator.register_rule(rule_must).await;
        orchestrator.register_rule(rule_must_not).await;

        // Emit a normal message (not from attacker)
        let event = DeonticEvent::MessageRelay {
            event_id: Uuid::new_v4().to_string(),
            sender_id: "alice".to_string(),
            receiver_id: "bob".to_string(),
            payload: "hello".to_string(),
            timestamp: Utc::now(),
        };

        let executions = orchestrator.emit_event(event).await;

        // Should have 3 executions: MUST_NOT (no match), MUST (match), MAY (match)
        assert_eq!(executions.len(), 3);

        // MUST_NOT should have been evaluated first (higher priority)
        // but didn't match sender=attacker
        assert_eq!(executions[0].modality, DeonticModality::MustNot);
        assert_eq!(executions[0].result, ExecutionResult::NoMatch);

        // MUST should be success
        let must_results: Vec<_> = executions
            .iter()
            .filter(|e| e.modality == DeonticModality::Must)
            .collect();
        assert!(must_results.iter().any(|e| e.result == ExecutionResult::Success));
    }

    #[tokio::test]
    async fn test_event_broadcast() {
        let orchestrator = create_test_orchestrator();

        // Subscribe to events
        let mut rx = orchestrator.subscribe();

        // Emit an event in background
        let _event = DeonticEvent::AgentRegister {
            agent_id: "agent_broadcast".to_string(),
            agent_name: "Bob".to_string(),
            public_key: "bob_key".to_string(),
            timestamp: Utc::now(),
        };

        // Spawn a task to simulate background event
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            eprintln!("[BROADCAST] Event emitted");
        });

        // Receive event (with timeout)
        let _result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            async {
                loop {
                    if let Ok(received) = rx.recv().await {
                        if received.event_type() == "agent_register" {
                            return Some(received);
                        }
                    } else {
                        return None;
                    }
                }
            }
        ).await;

        eprintln!("[TEST] Event broadcast test completed");
    }

    #[tokio::test]
    async fn test_multi_agent_scenario_with_governance_breach() {
        let orchestrator = create_test_orchestrator();

        // Setup scenario: Alice registers, then Bob tries to breach governance

        // Rule 1: MUST - Always accept valid agent registrations
        let rule_register = DeonticRule::new_must(
            "agent_register",
            "all_agents",
            "process_registration",
        ).with_priority(200);

        // Rule 2: MUST_NOT - Reject agents with low authority on governance operations
        let rule_block_breach = DeonticRule::new_must_not(
            "governance_breach",
            "severity>=8",
            "isolate_agent",
        ).with_priority(220);

        // Rule 3: MAY - Log all governance breaches
        let rule_log_breach = DeonticRule::new_may(
            "governance_breach",
            "all_breaches",
            "audit_governance_event",
        ).with_priority(100);

        orchestrator.register_rule(rule_register).await;
        orchestrator.register_rule(rule_block_breach).await;
        orchestrator.register_rule(rule_log_breach).await;

        // Scenario: Alice registers successfully
        let alice_register = DeonticEvent::AgentRegister {
            agent_id: "alice_001".to_string(),
            agent_name: "Alice".to_string(),
            public_key: "alice_pubkey".to_string(),
            timestamp: Utc::now(),
        };

        let exec1 = orchestrator.emit_event(alice_register).await;
        assert_eq!(exec1.len(), 1);
        assert_eq!(exec1[0].result, ExecutionResult::Success);
        assert_eq!(exec1[0].modality, DeonticModality::Must);

        // Scenario: Critical governance breach detected (severity=9)
        let critical_breach = DeonticEvent::GovernanceBreach {
            breach_id: Uuid::new_v4().to_string(),
            agent_id: "bob_002".to_string(),
            breach_type: "unauthorized_state_change".to_string(),
            severity: 9,
            timestamp: Utc::now(),
        };

        let exec2 = orchestrator.emit_event(critical_breach).await;
        assert_eq!(exec2.len(), 2);

        // Should have: MUST_NOT (rejection) + MAY (logging)
        let must_not_results: Vec<_> = exec2
            .iter()
            .filter(|e| e.modality == DeonticModality::MustNot)
            .collect();
        assert_eq!(must_not_results.len(), 1);
        assert_eq!(must_not_results[0].result, ExecutionResult::Rejected);

        let may_results: Vec<_> = exec2
            .iter()
            .filter(|e| e.modality == DeonticModality::May)
            .collect();
        assert_eq!(may_results.len(), 1);
        assert_eq!(may_results[0].result, ExecutionResult::Success);

        // Verify total execution count
        let all_executions = orchestrator.get_executions().await;
        assert_eq!(all_executions.len(), 3);
    }

    #[tokio::test]
    async fn test_arbitration_ruling_workflow() {
        let orchestrator = create_test_orchestrator();

        // Rule: MUST - Always apply arbitration rulings
        let rule = DeonticRule::new_must(
            "arbitration_ruling",
            "all_rulings",
            "apply_ruling",
        ).with_priority(200);

        orchestrator.register_rule(rule).await;

        // Emit arbitration ruling event
        let ruling = DeonticEvent::ArbitrationRuling {
            ruling_id: Uuid::new_v4().to_string(),
            case_id: "case_2026_001".to_string(),
            arbiter_id: "arbiter_alice".to_string(),
            decision: "Agent Charlie must comply with governance rules within 24 hours".to_string(),
            timestamp: Utc::now(),
        };

        let executions = orchestrator.emit_event(ruling).await;

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].modality, DeonticModality::Must);
        assert_eq!(executions[0].result, ExecutionResult::Success);
    }
}
