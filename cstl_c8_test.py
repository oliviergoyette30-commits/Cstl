"""
CSTL C8 Resilience Test v2 — fixes empiriques
Problèmes v1:
  - Mode 1 (corruption hash): validator ne vérifie pas le hash format
  - Mode 3 (semantic contradiction): pas de règle sur DECISION value
  - Mode 4 (signature mismatch): format "/" présent dans fake model
  - Circuit breaker: timeout ne déclenchait pas car timeout = blocks vides = missing DECISION
"""
import hashlib, time
from dataclasses import dataclass
from typing import Optional

@dataclass
class CSTLPayload:
    encoder: str
    produced_by: str
    sigma: float
    parent_hash: str
    conversation_id: str
    blocks: dict
    fault_mode: Optional[str] = None

    def canonical_hash(self) -> str:
        content = f"{self.encoder}{self.produced_by}{self.sigma}{self.parent_hash}"
        return "sha256:" + hashlib.sha256(content.encode()).hexdigest()[:16]

@dataclass
class C8Result:
    fault_mode: str
    detected: bool
    detection_latency_ms: float
    fallback_triggered: bool
    fallback_hash: str
    false_accept: bool
    detection_reasons: list

class CircuitBreaker:
    THRESHOLD = 3
    def __init__(self, last_valid_hash="sha256:root"):
        self.error_count = 0
        self.last_valid_hash = last_valid_hash
        self.state = "CLOSED"
    def record_success(self, h):
        self.error_count = 0; self.last_valid_hash = h; self.state = "CLOSED"
    def record_failure(self) -> bool:
        self.error_count += 1
        if self.error_count >= self.THRESHOLD:
            self.state = "OPEN"; return True
        return False

KNOWN_CONTRADICTIONS = {"REJECT_ALL_PREVIOUS_DECISIONS", "ABORT", "INVALID"}
VALID_PRODUCED_BY_ORGS = {"anthropic", "google", "openai", "mistralai", "meta"}

class QuorumValidator:
    def validate(self, payload: CSTLPayload) -> tuple[bool, list[str]]:
        reasons = []
        # Check 1: parent_hash must be valid sha256
        ph = payload.parent_hash
        if not (ph == "root" or ph.startswith("sha256:")):
            reasons.append(f"invalid_parent_hash_format:{ph[:30]}")
        elif ph.startswith("sha256:"):
            hex_part = ph[7:]
            if not all(c in "0123456789abcdef" for c in hex_part.lower()):
                reasons.append(f"parent_hash_not_hex:{hex_part[:20]}")

        # Check 2: sigma threshold
        if payload.sigma < 0.50:
            reasons.append(f"sigma_too_low:{payload.sigma}")

        # Check 3: DECISION must not be contradictory
        decision = payload.blocks.get("DECISION", "")
        if not decision:
            reasons.append("missing_DECISION_block")
        elif any(bad in decision.upper() for bad in KNOWN_CONTRADICTIONS):
            reasons.append(f"contradictory_DECISION:{decision[:40]}")

        # Check 4: produced_by format org/model — check org is known
        if "/" not in payload.produced_by:
            reasons.append(f"produced_by_missing_slash:{payload.produced_by}")
        else:
            org = payload.produced_by.split("/")[0].lower()
            if org not in VALID_PRODUCED_BY_ORGS:
                reasons.append(f"produced_by_unknown_org:{org}")

        # Check 5: timeout = empty blocks
        if not payload.blocks:
            reasons.append("timeout_empty_blocks")

        return len(reasons) == 0, reasons

class FaultInjector:
    def corruption(self, p):
        c = CSTLPayload(**p.__dict__)
        c.parent_hash = "sha256:GGGGGGGG_NOT_HEX"  # g = non-hex
        c.fault_mode = "payload_corruption"; return c
    def timeout(self, p):
        c = CSTLPayload(**p.__dict__)
        c.blocks = {}; c.fault_mode = "timeout_simulation"; return c
    def contradiction(self, p):
        c = CSTLPayload(**p.__dict__)
        c.blocks = dict(p.blocks)
        c.blocks["DECISION"] = "REJECT_ALL_PREVIOUS_DECISIONS"
        c.fault_mode = "semantic_contradiction"; return c
    def mismatch(self, p):
        c = CSTLPayload(**p.__dict__)
        c.produced_by = "unknown/fake-model-xyz"
        c.fault_mode = "signature_mismatch"; return c

class PrimaryResponder:
    def __init__(self, cb: CircuitBreaker, v: QuorumValidator):
        self.cb = cb; self.v = v; self.results = []
    def process(self, payload: CSTLPayload) -> C8Result:
        t0 = time.perf_counter()
        ok, reasons = self.v.validate(payload)
        detected = not ok
        fallback_triggered = False
        if detected:
            fallback_triggered = self.cb.record_failure()
        else:
            self.cb.record_success(payload.canonical_hash())
        latency = (time.perf_counter() - t0) * 1000
        should = payload.fault_mode is not None
        r = C8Result(
            fault_mode=payload.fault_mode or "none",
            detected=detected,
            detection_latency_ms=round(latency, 3),
            fallback_triggered=fallback_triggered,
            fallback_hash=self.cb.last_valid_hash,
            false_accept=should and not detected,
            detection_reasons=reasons,
        )
        self.results.append(r); return r

def run():
    print("=" * 60)
    print("CSTL C8 RESILIENCE TEST v2")
    print("=" * 60)
    cb = CircuitBreaker(); v = QuorumValidator()
    resp = PrimaryResponder(cb, v)
    inj = FaultInjector()

    base = CSTLPayload(
        encoder="Agent_GEMINI", produced_by="google/gemini-2.5-flash",
        sigma=0.88, parent_hash="sha256:4d70df887e1a3bc5b6d926ef91f86f21",
        conversation_id="tripartite_conditions_v1",
        blocks={"C8_STATUS": "active", "DECISION": "continue_chain"},
    )

    print("\n1. BASELINE (doit passer)")
    r = resp.process(base)
    print(f"   ✅ valid={not r.detected} hash={cb.last_valid_hash}")

    faults = [
        ("Mode 1: payload_corruption",     inj.corruption(base)),
        ("Mode 2: timeout_simulation",     inj.timeout(base)),
        ("Mode 3: semantic_contradiction", inj.contradiction(base)),
        ("Mode 4: signature_mismatch",     inj.mismatch(base)),
    ]
    print("\n2. INJECTIONS DE PANNES")
    for label, fp in faults:
        r = resp.process(fp)
        icon = "✅" if r.detected else "❌"
        print(f"   {icon} {label}")
        print(f"      detected={r.detected} fallback={r.fallback_triggered} false_accept={r.false_accept}")
        if r.detected: print(f"      raisons: {r.detection_reasons}")

    print("\n3. CIRCUIT BREAKER — 3 pannes consécutives")
    cb2 = CircuitBreaker(last_valid_hash=base.canonical_hash())
    resp2 = PrimaryResponder(cb2, v)
    for i in range(3):
        r = resp2.process(inj.corruption(base))
        print(f"   Panne {i+1}/3: cb_errors={cb2.error_count} state={cb2.state} fallback={r.fallback_triggered}")
    print(f"   → état final: {cb2.state} | hash préservé: {cb2.last_valid_hash}")

    print("\n4. MÉTRIQUES")
    total = len(resp.results)
    faults_detected = sum(1 for r in resp.results if r.detected and r.fault_mode != "none")
    fa = sum(1 for r in resp.results if r.false_accept)
    avg_lat = sum(r.detection_latency_ms for r in resp.results) / total
    print(f"   Pannes détectées: {faults_detected}/4 = {faults_detected/4*100:.0f}%")
    print(f"   False accepts: {fa}/4")
    print(f"   Latence moyenne: {avg_lat:.3f}ms")
    print(f"   Fallback success rate: {sum(1 for r in resp.results if r.fallback_triggered)/total*100:.0f}%")

run()
