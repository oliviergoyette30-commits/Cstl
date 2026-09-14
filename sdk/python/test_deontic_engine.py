import pytest
import json
import time
from datetime import datetime, timezone
from cstl_deontic_engine import (
    CstlDeonticEngine,
    DeonticEvent,
    DeonticRule,
    EventType,
    DeonticModality,
    ExecutionResult,
)


class TestDeonticEventCreation:
    def test_agent_register_event(self):
        event = DeonticEvent.agent_register("agent_1", "Alice", "alice_key")
        assert event.event_type == EventType.AGENT_REGISTER.value
        assert event.payload["agent_id"] == "agent_1"
        assert event.payload["agent_name"] == "Alice"

    def test_message_relay_event(self):
        event = DeonticEvent.message_relay("sender", "receiver", "payload")
        assert event.event_type == EventType.MESSAGE_RELAY.value
        assert event.payload["sender_id"] == "sender"
        assert event.payload["receiver_id"] == "receiver"

    def test_governance_breach_event(self):
        event = DeonticEvent.governance_breach("agent_2", "high_latency", 7)
        assert event.event_type == EventType.GOVERNANCE_BREACH.value
        assert event.payload["severity"] == 7

    def test_arbitration_ruling_event(self):
        event = DeonticEvent.arbitration_ruling("ruling_1", "case_1", "arbiter", "decision")
        assert event.event_type == EventType.ARBITRATION_RULING.value
        assert event.payload["decision"] == "decision"

    def test_event_hash_computation(self):
        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        assert len(event.hash) == 64
        assert isinstance(event.hash, str)

    def test_event_serialization(self):
        event = DeonticEvent.message_relay("sender", "receiver", "payload")
        event_dict = event.to_dict()
        assert "event_id" in event_dict
        assert "event_type" in event_dict
        assert "hash" in event_dict


class TestDeonticRuleCreation:
    def test_must_rule(self):
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        assert rule.modality == DeonticModality.MUST
        assert rule.priority == 200

    def test_must_not_rule(self):
        rule = DeonticRule.must_not("message_relay", "sender=blocked", "reject")
        assert rule.modality == DeonticModality.MUST_NOT
        assert rule.priority == 210

    def test_may_rule(self):
        rule = DeonticRule.may("governance_breach", "all_breaches", "log")
        assert rule.modality == DeonticModality.MAY
        assert rule.priority == 100

    def test_rule_with_custom_priority(self):
        rule = DeonticRule.may("test", "condition", "action")
        rule.priority = 150
        assert rule.priority == 150


class TestCstlDeonticEngineBasic:
    def test_engine_creation(self):
        engine = CstlDeonticEngine()
        assert engine.queue_size == 1000
        assert engine.enable_graceful_degradation is True

    def test_register_single_rule(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("test", "condition", "action")
        engine.register_rule(rule)
        assert len(engine.rules) == 1

    def test_register_multiple_rules(self):
        engine = CstlDeonticEngine()
        rules = [
            DeonticRule.must("test", "cond1", "act1"),
            DeonticRule.must_not("test", "cond2", "act2"),
            DeonticRule.may("test", "cond3", "act3"),
        ]
        engine.register_rules(rules)
        assert len(engine.rules) == 3

    def test_rule_priority_sorting(self):
        engine = CstlDeonticEngine()
        rule_low = DeonticRule.may("test", "cond", "action", 50)
        rule_high = DeonticRule.must("test", "cond", "action", 250)
        rule_mid = DeonticRule.must_not("test", "cond", "action", 150)

        engine.register_rule(rule_low)
        engine.register_rule(rule_high)
        engine.register_rule(rule_mid)

        priorities = [r.priority for r in engine.rules]
        assert priorities == [250, 150, 50]


class TestEventProcessing:
    def test_emit_agent_register_event(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        executions = engine.emit_event(event)

        assert len(executions) > 0
        assert executions[0].result == ExecutionResult.SUCCESS

    def test_emit_message_relay_event(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("message_relay", "all_messages", "relay")
        engine.register_rule(rule)

        event = DeonticEvent.message_relay("alice", "bob", "hello")
        executions = engine.emit_event(event)

        assert len(executions) > 0
        assert executions[0].modality == DeonticModality.MUST

    def test_no_matching_rules(self):
        engine = CstlDeonticEngine()
        event = DeonticEvent.message_relay("alice", "bob", "hello")
        executions = engine.emit_event(event)

        assert len(executions) == 0

    def test_metrics_tracking(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        metrics = engine.get_metrics()
        assert metrics["events_processed"] == 1
        assert metrics["executions_total"] == 1


class TestConditionMatching:
    def test_all_agents_condition(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        executions = engine.emit_event(event)

        assert len(executions) > 0
        assert executions[0].result == ExecutionResult.SUCCESS

    def test_specific_sender_condition(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must_not("message_relay", "sender=alice", "reject")
        engine.register_rule(rule)

        event = DeonticEvent.message_relay("alice", "bob", "payload")
        executions = engine.emit_event(event)

        assert executions[0].result == ExecutionResult.REJECTED

    def test_severity_threshold_condition(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must_not("governance_breach", "severity>=8", "isolate")
        engine.register_rule(rule)

        low_breach = DeonticEvent.governance_breach("agent", "type", 5)
        executions_low = engine.emit_event(low_breach)
        assert executions_low[0].result == ExecutionResult.NO_MATCH

        high_breach = DeonticEvent.governance_breach("agent", "type", 9)
        executions_high = engine.emit_event(high_breach)
        assert executions_high[0].result == ExecutionResult.REJECTED

    def test_wildcard_condition(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.may("message_relay", "*", "log")
        engine.register_rule(rule)

        event = DeonticEvent.message_relay("alice", "bob", "test")
        executions = engine.emit_event(event)

        assert len(executions) > 0


class TestDeonticModalities:
    def test_must_execution(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "*", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        executions = engine.emit_event(event)

        assert executions[0].modality == DeonticModality.MUST
        assert executions[0].result == ExecutionResult.SUCCESS

    def test_must_not_execution(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must_not("message_relay", "sender=blocked", "reject")
        engine.register_rule(rule)

        event = DeonticEvent.message_relay("blocked", "alice", "attack")
        executions = engine.emit_event(event)

        assert executions[0].modality == DeonticModality.MUST_NOT
        assert executions[0].result == ExecutionResult.REJECTED

    def test_may_execution(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.may("governance_breach", "all_breaches", "log")
        engine.register_rule(rule)

        event = DeonticEvent.governance_breach("agent", "type", 5)
        executions = engine.emit_event(event)

        assert executions[0].modality == DeonticModality.MAY
        assert executions[0].result == ExecutionResult.SUCCESS


class TestEventHistory:
    def test_event_history_tracking(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event1 = DeonticEvent.agent_register("agent_1", "Alice", "key1")
        event2 = DeonticEvent.agent_register("agent_2", "Bob", "key2")

        engine.emit_event(event1)
        engine.emit_event(event2)

        events = engine.get_events()
        assert len(events) == 2

    def test_get_events_by_type(self):
        engine = CstlDeonticEngine()
        rule1 = DeonticRule.must("agent_register", "all_agents", "register")
        rule2 = DeonticRule.must("message_relay", "all_messages", "relay")
        engine.register_rules([rule1, rule2])

        engine.emit_event(DeonticEvent.agent_register("agent_1", "Alice", "key"))
        engine.emit_event(DeonticEvent.message_relay("alice", "bob", "msg"))
        engine.emit_event(DeonticEvent.agent_register("agent_2", "Bob", "key"))

        agent_events = engine.get_events_by_type(EventType.AGENT_REGISTER.value)
        assert len(agent_events) == 2

        msg_events = engine.get_events_by_type(EventType.MESSAGE_RELAY.value)
        assert len(msg_events) == 1

    def test_get_event_by_id(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        retrieved = engine.get_event_by_id(event.event_id)
        assert retrieved is not None
        assert retrieved.event_id == event.event_id


class TestAuditTrail:
    def test_audit_trail_for_event(self):
        engine = CstlDeonticEngine()
        rule1 = DeonticRule.must("message_relay", "all_messages", "relay")
        rule2 = DeonticRule.may("message_relay", "all_messages", "log")
        engine.register_rules([rule1, rule2])

        event = DeonticEvent.message_relay("alice", "bob", "msg")
        engine.emit_event(event)

        audit = engine.get_audit_trail(event.event_id)
        assert len(audit) == 2

    def test_rejection_count(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must_not("message_relay", "sender=blocked", "reject")
        engine.register_rule(rule)

        engine.emit_event(DeonticEvent.message_relay("blocked", "alice", "1"))
        engine.emit_event(DeonticEvent.message_relay("blocked", "bob", "2"))
        engine.emit_event(DeonticEvent.message_relay("alice", "bob", "3"))

        assert engine.get_rejection_count() == 2

    def test_success_count(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        engine.emit_event(DeonticEvent.agent_register("agent_1", "Alice", "key"))
        engine.emit_event(DeonticEvent.agent_register("agent_2", "Bob", "key"))

        assert engine.get_success_count() == 2


class TestExecutionMetrics:
    def test_metrics_collection(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        for i in range(5):
            event = DeonticEvent.agent_register(f"agent_{i}", f"Agent{i}", f"key{i}")
            engine.emit_event(event)

        metrics = engine.get_metrics()
        assert metrics["events_processed"] == 5
        assert metrics["executions_total"] == 5
        assert metrics["rules_matched"] == 5

    def test_average_processing_time(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        metrics = engine.get_metrics()
        assert metrics["avg_process_time_ms"] > 0


class TestEventIntegrity:
    def test_compute_event_hash(self):
        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        hash1 = event.hash
        hash2 = event.hash
        assert hash1 == hash2

    def test_verify_event_integrity(self):
        engine = CstlDeonticEngine()
        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        event_dict = event.to_dict()

        is_valid = engine.verify_event_integrity(event_dict)
        assert is_valid is True

    def test_verify_corrupted_event(self):
        engine = CstlDeonticEngine()
        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        event_dict = event.to_dict()

        event_dict["payload"]["agent_name"] = "Bob"

        is_valid = engine.verify_event_integrity(event_dict)
        assert is_valid is False


class TestExportFunctionality:
    def test_export_executions_json(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        export = engine.export_executions("json")
        data = json.loads(export)
        assert isinstance(data, list)
        assert len(data) > 0

    def test_export_events_json(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        export = engine.export_events("json")
        data = json.loads(export)
        assert isinstance(data, list)
        assert len(data) > 0


class TestClearHistory:
    def test_clear_history(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        assert len(engine.get_executions()) > 0
        assert len(engine.get_events()) > 0

        engine.clear_history()

        assert len(engine.get_executions()) == 0
        assert len(engine.get_events()) == 0
        assert engine.get_metrics()["events_processed"] == 0


class TestMultipleRules:
    def test_multiple_rules_matching_same_event(self):
        engine = CstlDeonticEngine()
        rule1 = DeonticRule.must("message_relay", "all_messages", "relay")
        rule2 = DeonticRule.may("message_relay", "all_messages", "log")
        engine.register_rules([rule1, rule2])

        event = DeonticEvent.message_relay("alice", "bob", "msg")
        executions = engine.emit_event(event)

        assert len(executions) == 2
        assert executions[0].modality == DeonticModality.MUST
        assert executions[1].modality == DeonticModality.MAY

    def test_conflict_resolution(self):
        engine = CstlDeonticEngine()
        rule_must = DeonticRule.must("message_relay", "all_messages", "relay", 200)
        rule_must_not = DeonticRule.must_not("message_relay", "sender=alice", "block", 210)
        engine.register_rules([rule_must, rule_must_not])

        event = DeonticEvent.message_relay("alice", "bob", "msg")
        executions = engine.emit_event(event)

        assert len(executions) == 2
        assert executions[0].modality == DeonticModality.MUST_NOT
        assert executions[0].result == ExecutionResult.REJECTED


class TestGracefulDegradation:
    def test_graceful_degradation_enabled(self):
        engine = CstlDeonticEngine(queue_size=2, enable_graceful_degradation=True)
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        for i in range(3):
            event = DeonticEvent.agent_register(f"agent_{i}", f"Agent{i}", f"key{i}")
            engine.emit_event(event)

        assert engine.get_metrics()["events_processed"] > 0

    def test_subscriber_notification(self):
        engine = CstlDeonticEngine()
        rule = DeonticRule.must("agent_register", "all_agents", "register")
        engine.register_rule(rule)

        received_events = []

        def callback(event, executions):
            received_events.append((event, executions))

        engine.subscribe(callback)

        event = DeonticEvent.agent_register("agent_1", "Alice", "key")
        engine.emit_event(event)

        assert len(received_events) == 1


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
