import hashlib
import hmac
import json
import time
import uuid
from datetime import datetime, timedelta, timezone
from enum import Enum
from typing import Dict, List, Optional, Tuple
from collections import defaultdict
import threading
import queue


class DeonticModality(Enum):
    MUST = "must"
    MUST_NOT = "must_not"
    MAY = "may"


class ExecutionResult(Enum):
    SUCCESS = "success"
    REJECTED = "rejected"
    NO_MATCH = "no_match"
    FAILED = "failed"


class EventType(Enum):
    AGENT_REGISTER = "agent_register"
    MESSAGE_RELAY = "message_relay"
    ARBITRATION_RULING = "arbitration_ruling"
    GOVERNANCE_BREACH = "governance_breach"


class DeonticEvent:
    def __init__(self, event_type: str, payload: Dict, timestamp: Optional[datetime] = None):
        self.event_id = str(uuid.uuid4())
        self.event_type = event_type
        self.payload = payload
        self.timestamp = timestamp or datetime.now(timezone.utc)
        self.hash = self._compute_hash()

    def _compute_hash(self) -> str:
        content = json.dumps({
            "event_id": self.event_id,
            "event_type": self.event_type,
            "payload": self.payload,
            "timestamp": self.timestamp.isoformat(),
        }, sort_keys=True)
        return hashlib.sha256(content.encode()).hexdigest()

    def to_dict(self) -> Dict:
        return {
            "event_id": self.event_id,
            "event_type": self.event_type,
            "payload": self.payload,
            "timestamp": self.timestamp.isoformat(),
            "hash": self.hash,
        }

    @staticmethod
    def agent_register(agent_id: str, agent_name: str, public_key: str) -> "DeonticEvent":
        return DeonticEvent(
            EventType.AGENT_REGISTER.value,
            {
                "agent_id": agent_id,
                "agent_name": agent_name,
                "public_key": public_key,
            }
        )

    @staticmethod
    def message_relay(sender_id: str, receiver_id: str, payload: str) -> "DeonticEvent":
        return DeonticEvent(
            EventType.MESSAGE_RELAY.value,
            {
                "sender_id": sender_id,
                "receiver_id": receiver_id,
                "payload": payload,
            }
        )

    @staticmethod
    def governance_breach(agent_id: str, breach_type: str, severity: int) -> "DeonticEvent":
        return DeonticEvent(
            EventType.GOVERNANCE_BREACH.value,
            {
                "agent_id": agent_id,
                "breach_type": breach_type,
                "severity": severity,
            }
        )

    @staticmethod
    def arbitration_ruling(ruling_id: str, case_id: str, arbiter_id: str, decision: str) -> "DeonticEvent":
        return DeonticEvent(
            EventType.ARBITRATION_RULING.value,
            {
                "ruling_id": ruling_id,
                "case_id": case_id,
                "arbiter_id": arbiter_id,
                "decision": decision,
            }
        )


class DeonticRule:
    def __init__(
        self,
        modality: DeonticModality,
        event_type: str,
        condition: str,
        action: str,
        priority: int = 100,
    ):
        self.rule_id = str(uuid.uuid4())
        self.modality = modality
        self.event_type = event_type
        self.condition = condition
        self.action = action
        self.priority = priority
        self.match_count = 0
        self.execute_count = 0

    @staticmethod
    def must(event_type: str, condition: str, action: str, priority: int = 200) -> "DeonticRule":
        return DeonticRule(DeonticModality.MUST, event_type, condition, action, priority)

    @staticmethod
    def must_not(event_type: str, condition: str, action: str, priority: int = 210) -> "DeonticRule":
        return DeonticRule(DeonticModality.MUST_NOT, event_type, condition, action, priority)

    @staticmethod
    def may(event_type: str, condition: str, action: str, priority: int = 100) -> "DeonticRule":
        return DeonticRule(DeonticModality.MAY, event_type, condition, action, priority)


class DeonticExecution:
    def __init__(
        self,
        rule_id: str,
        event_id: str,
        modality: DeonticModality,
        action: str,
        result: ExecutionResult,
        timestamp: Optional[datetime] = None,
    ):
        self.rule_id = rule_id
        self.event_id = event_id
        self.modality = modality
        self.action = action
        self.result = result
        self.timestamp = timestamp or datetime.now(timezone.utc)

    def to_dict(self) -> Dict:
        return {
            "rule_id": self.rule_id,
            "event_id": self.event_id,
            "modality": self.modality.value,
            "action": self.action,
            "result": self.result.value,
            "timestamp": self.timestamp.isoformat(),
        }


class CstlDeonticEngine:
    def __init__(self, queue_size: int = 1000, enable_graceful_degradation: bool = True):
        self.queue_size = queue_size
        self.enable_graceful_degradation = enable_graceful_degradation

        self.rules: List[DeonticRule] = []
        self.event_queue: queue.Queue = queue.Queue(maxsize=queue_size)
        self.executions: List[DeonticExecution] = []
        self.event_history: List[DeonticEvent] = []

        self.subscribers: List[callable] = []
        self.metrics = {
            "events_processed": 0,
            "rules_matched": 0,
            "executions_total": 0,
            "rejections": 0,
            "queue_size": 0,
            "avg_process_time_ms": 0.0,
        }

        self._lock = threading.RLock()
        self._running = False
        self._worker_thread = None
        self.process_times: List[float] = []

    def register_rule(self, rule: DeonticRule) -> None:
        with self._lock:
            self.rules.append(rule)
            self.rules.sort(key=lambda r: r.priority, reverse=True)

    def register_rules(self, rules: List[DeonticRule]) -> None:
        for rule in rules:
            self.register_rule(rule)

    def subscribe(self, callback: callable) -> None:
        with self._lock:
            self.subscribers.append(callback)

    def emit_event(self, event: DeonticEvent) -> List[DeonticExecution]:
        try:
            if self.event_queue.full() and self.enable_graceful_degradation:
                return self._handle_queue_overflow(event)

            self.event_queue.put_nowait(event)
            self.metrics["queue_size"] = self.event_queue.qsize()

            start_time = time.time()
            executions = self._process_event(event)
            elapsed_ms = (time.time() - start_time) * 1000
            self.process_times.append(elapsed_ms)

            if len(self.process_times) > 100:
                self.process_times.pop(0)

            self.metrics["avg_process_time_ms"] = sum(self.process_times) / len(self.process_times)
            self.metrics["events_processed"] += 1

            return executions
        except Exception as e:
            if self.enable_graceful_degradation:
                return self._handle_error(event, str(e))
            raise

    def _process_event(self, event: DeonticEvent) -> List[DeonticExecution]:
        with self._lock:
            executions = []
            self.event_history.append(event)

            for rule in self.rules:
                if rule.event_type != event.event_type:
                    continue

                if not self._check_condition(event, rule.condition):
                    executions.append(
                        DeonticExecution(
                            rule.rule_id,
                            event.event_id,
                            rule.modality,
                            rule.action,
                            ExecutionResult.NO_MATCH,
                        )
                    )
                    continue

                rule.match_count += 1
                self.metrics["rules_matched"] += 1

                result = self._execute_rule(event, rule)
                execution = DeonticExecution(
                    rule.rule_id,
                    event.event_id,
                    rule.modality,
                    rule.action,
                    result,
                )

                executions.append(execution)
                rule.execute_count += 1
                self.metrics["executions_total"] += 1

                if result == ExecutionResult.REJECTED:
                    self.metrics["rejections"] += 1

            self.executions.extend(executions)

            for callback in self.subscribers:
                try:
                    callback(event, executions)
                except Exception:
                    pass

            return executions

    def _check_condition(self, event: DeonticEvent, condition: str) -> bool:
        if condition == "*" or condition == "all":
            return True

        event_type = event.event_type

        if event_type == EventType.AGENT_REGISTER.value:
            agent_name = event.payload.get("agent_name", "")
            return (
                condition == "all_agents"
                or f"agent={agent_name}" in condition
                or condition.startswith("*")
            )

        elif event_type == EventType.MESSAGE_RELAY.value:
            sender_id = event.payload.get("sender_id", "")
            return (
                condition == "all_messages"
                or f"sender={sender_id}" in condition
                or condition.startswith("*")
            )

        elif event_type == EventType.GOVERNANCE_BREACH.value:
            severity = event.payload.get("severity", 0)
            if condition.startswith("severity>="):
                try:
                    threshold = int(condition.replace("severity>=", ""))
                    return severity >= threshold
                except ValueError:
                    return False
            return condition == "all_breaches" or condition.startswith("*")

        elif event_type == EventType.ARBITRATION_RULING.value:
            arbiter_id = event.payload.get("arbiter_id", "")
            return (
                condition == "all_rulings"
                or f"arbiter={arbiter_id}" in condition
                or condition.startswith("*")
            )

        return False

    def _execute_rule(self, event: DeonticEvent, rule: DeonticRule) -> ExecutionResult:
        if rule.modality == DeonticModality.MUST:
            return self._execute_must_rule(event, rule)
        elif rule.modality == DeonticModality.MUST_NOT:
            return self._execute_must_not_rule(event, rule)
        else:
            return self._execute_may_rule(event, rule)

    def _execute_must_rule(self, event: DeonticEvent, rule: DeonticRule) -> ExecutionResult:
        return ExecutionResult.SUCCESS

    def _execute_must_not_rule(self, event: DeonticEvent, rule: DeonticRule) -> ExecutionResult:
        return ExecutionResult.REJECTED

    def _execute_may_rule(self, event: DeonticEvent, rule: DeonticRule) -> ExecutionResult:
        return ExecutionResult.SUCCESS

    def _handle_queue_overflow(self, event: DeonticEvent) -> List[DeonticExecution]:
        try:
            old_event = self.event_queue.get_nowait()
            self.event_queue.put_nowait(event)
            return self._process_event(event)
        except queue.Empty:
            return []

    def _handle_error(self, event: DeonticEvent, error: str) -> List[DeonticExecution]:
        execution = DeonticExecution(
            "error_handler",
            event.event_id,
            DeonticModality.MAY,
            f"error_handling: {error}",
            ExecutionResult.FAILED,
        )
        self.executions.append(execution)
        self.metrics["executions_total"] += 1
        return [execution]

    def get_executions(self) -> List[DeonticExecution]:
        with self._lock:
            return self.executions.copy()

    def get_events(self) -> List[DeonticEvent]:
        with self._lock:
            return self.event_history.copy()

    def get_metrics(self) -> Dict:
        with self._lock:
            return self.metrics.copy()

    def get_rules(self) -> List[Dict]:
        with self._lock:
            return [
                {
                    "rule_id": r.rule_id,
                    "modality": r.modality.value,
                    "event_type": r.event_type,
                    "condition": r.condition,
                    "action": r.action,
                    "priority": r.priority,
                    "match_count": r.match_count,
                    "execute_count": r.execute_count,
                }
                for r in self.rules
            ]

    def clear_history(self) -> None:
        with self._lock:
            self.executions.clear()
            self.event_history.clear()
            self.process_times.clear()
            self.metrics = {
                "events_processed": 0,
                "rules_matched": 0,
                "executions_total": 0,
                "rejections": 0,
                "queue_size": 0,
                "avg_process_time_ms": 0.0,
            }

    def compute_event_hash(self, event: DeonticEvent) -> str:
        return event.hash

    def verify_event_integrity(self, event_dict: Dict) -> bool:
        if "hash" not in event_dict:
            return False

        expected_hash = event_dict.pop("hash")
        content = json.dumps(event_dict, sort_keys=True)
        computed_hash = hashlib.sha256(content.encode()).hexdigest()

        return computed_hash == expected_hash

    def export_executions(self, format: str = "json") -> str:
        with self._lock:
            executions = [e.to_dict() for e in self.executions]
            return json.dumps(executions, indent=2)

    def export_events(self, format: str = "json") -> str:
        with self._lock:
            events = [e.to_dict() for e in self.event_history]
            return json.dumps(events, indent=2)

    def get_events_by_type(self, event_type: str) -> List[DeonticEvent]:
        with self._lock:
            return [e for e in self.event_history if e.event_type == event_type]

    def get_executions_by_modality(self, modality: DeonticModality) -> List[DeonticExecution]:
        with self._lock:
            return [e for e in self.executions if e.modality == modality]

    def get_rejection_count(self) -> int:
        with self._lock:
            return sum(1 for e in self.executions if e.result == ExecutionResult.REJECTED)

    def get_success_count(self) -> int:
        with self._lock:
            return sum(1 for e in self.executions if e.result == ExecutionResult.SUCCESS)

    def get_event_by_id(self, event_id: str) -> Optional[DeonticEvent]:
        with self._lock:
            for event in self.event_history:
                if event.event_id == event_id:
                    return event
            return None

    def get_audit_trail(self, event_id: str) -> List[DeonticExecution]:
        with self._lock:
            return [e for e in self.executions if e.event_id == event_id]


if __name__ == "__main__":
    engine = CstlDeonticEngine()

    rule_must = DeonticRule.must(
        EventType.AGENT_REGISTER.value,
        "all_agents",
        "register_agent",
        priority=200
    )

    rule_must_not = DeonticRule.must_not(
        EventType.MESSAGE_RELAY.value,
        "sender=blocked",
        "reject_message",
        priority=210
    )

    rule_may = DeonticRule.may(
        EventType.GOVERNANCE_BREACH.value,
        "all_breaches",
        "log_breach",
        priority=100
    )

    engine.register_rules([rule_must, rule_must_not, rule_may])

    event1 = DeonticEvent.agent_register("agent_1", "Alice", "alice_key")
    executions1 = engine.emit_event(event1)
    print(f"Event 1: {len(executions1)} executions")

    event2 = DeonticEvent.message_relay("normal_sender", "receiver", "payload")
    executions2 = engine.emit_event(event2)
    print(f"Event 2: {len(executions2)} executions")

    event3 = DeonticEvent.governance_breach("agent_2", "high_latency", 7)
    executions3 = engine.emit_event(event3)
    print(f"Event 3: {len(executions3)} executions")

    metrics = engine.get_metrics()
    print(f"\nMetrics: {json.dumps(metrics, indent=2)}")
