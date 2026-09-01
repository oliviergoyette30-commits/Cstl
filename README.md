# CSTL v5.0.0 — Compressed Semantic Transfer Language

> The first wire format designed natively for LLM-to-LLM communication.
>
> **Les relations sont plus importantes que l'information.** — [Principes fondateurs](PRINCIPES.md)

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-75%20passing-brightgreen.svg)](src/tests.rs)

---

## What is CSTL?

CSTL is a structured text format for inter-LLM communication. It fills a gap: when AI agents coordinate, the formats available today are either natural language (ambiguous, no audit trail), JSON (no modal logic, no uncertainty), or function calls (vendor-specific). CSTL is designed natively for this use case.

**Unique combination** (no existing format combines all four):

- Native deontic modalities: `MUST`, `MUST_NOT`, `MAY`, `IFF`
- Quantified uncertainty: aleatory vs epistemic, per-relation sigma values
- Provenance tracking: `produced_by`, `PARENT_HASH`, canonical SHA-256
- Deterministic parser: O(n), zero LLM required for validation

CSTL is not a JSON replacement. It is a **semantic content layer** — the third layer of the agentic stack, alongside A2A (agent coordination) and MCP (agent-tool integration), neither of which carries modality, uncertainty, or provenance natively.

---

## Empirical Results

| Test | Method | Result |
|---|---|---|
| Transport fidelity (multi-hop) | 12+ hops, cross-vendor | **99.3%** average |
| Decoding fidelity | N=10, Gemini 2.5 Pro | 100% (Wilson CI [96.3%, 100%]) |
| Encoding fidelity | N=5, 194 facts, Gemini+GPT-5 | 97.9% (Wilson CI [94.8%, 99.2%]) |
| Cross-vendor chain | 2 hops, 3 vendors, bidirectional | 96–98% |
| Semantic preservation | FraCaS (Cooper et al. 1996), 20 items, 3 independent LLM judges | 19/20 unanimous |
| Explicit-uncertainty preservation | Native-format comparison vs A2A / MCP / naive KG, 3 judges | CSTL only format to preserve it (unanimous) |

**On semantic fidelity and κ.** A global inter-rater κ is the wrong metric for CSTL and cannot reach 1.0 by design. Factual claims converge (measured σ = 0.048 between judges); value-judgment claims diverge (σ = 0.098, ~2× higher) — and that divergence is the intended trigger for quarantine and human arbitration, not an error to eliminate. Full argument: [`docs/CSTL_kappa_plafond_structurel_v2.pdf`](docs/).

---

## Architecture — 9 Layers

CSTL is not only a wire format. The syntax is layer 1 of a governance architecture:

| # | Layer | Status |
|---|---|---|
| 1 | **Transport** — wire format, SHA-256 immutable, deterministic validation | ✅ Proven (99.3%, 12+ hops) |
| 2 | **Governance / Resilience** — circuit breaker, 2/3 quorum, operator drift prevention | ✅ Tested (4/4 modes) |
| 3a | **Public fact verification** — Wikidata + SPARQL, entity resolution | ✅ Implemented |
| 3b | **Software lab + arbitration** — `RestrictedCouncil`, subprocess-isolated `ExecutionLab`, human channel | 🟡 Designed, not wired |
| 4 | **Calibration** — Laplace-smoothed scoring, per-agent/per-domain accuracy | ✅ Tested |
| 5 | **Persistent memory / provenance** — SQLite store, hash entanglement, FastAPI server | 🟡 Fragmented |
| 6 | **Human interface** — Graphify (589 nodes, 1142 edges), Obsidian vault | 🔶 Skeleton |
| 7 | **Agent discovery & routing** — CSTL-native registry, agent cards, zero external deps | ❌ To build |
| 8 | **Provenance audit** — hash-chained audit trail, deontic modality enforcement | ✅ Designed |
| 9 | **CASTLE compression mode** — session-amortized shared dictionary | 🟡 Architected, no code |

Full detail: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

**The value is the composition, not the syntax alone.** A payload that carries `MUST_NOT` is useful; a payload that carries `MUST_NOT`, knows its own provenance, quantifies its uncertainty, has contradictions detected mechanically, and routes disagreement to a human council — that is what the layers deliver together.

---

## CSTL Agentic OS

The 9 layers compose into a runtime for multi-agent coordination. The pipeline below is the operational shape of the system; components are marked by their real status.

```
   ┌──────────────┐
   │  Agent A     │  encodes intent + facts + modality + σ
   └──────┬───────┘
          │  CSTL payload (SHA-256 canonical)
          ▼
   ┌──────────────────────────────────────────────┐
   │  TRANSPORT  ✅   parser · validator · audit   │
   │  hash chain · PARENT_HASH · immutable trail   │
   └──────┬───────────────────────────────────────┘
          ▼
   ┌──────────────────────────────────────────────┐
   │  VALIDATION  ✅  W601–W606 semantic checks    │
   │  contradiction · entailment · calibration     │
   └──────┬───────────────────────────────────────┘
          │
          ├─── converges ──────────► ┌──────────────┐
          │                          │  Agent B  ✅ │
          │                          └──────────────┘
          │
          └─── diverges ───► ┌────────────────────────────┐
                             │  QUARANTINE  🟡            │
                             │  σ held low, not propagated│
                             └──────────┬─────────────────┘
                                        ▼
                             ┌────────────────────────────┐
                             │  RestrictedCouncil  🟡      │
                             │  human quorum 2/3           │
                             └──────────┬─────────────────┘
                                        ▼
                             ┌────────────────────────────┐
                             │  RE-INJECTION  🟡           │
                             │  verdict + full provenance  │
                             │  σ updated, audit complete  │
                             └────────────────────────────┘
```

**Legend:** ✅ operational · 🟡 designed, not production-wired · ❌ to build

The distinguishing property is not that disagreement never happens — it is that disagreement is **detected, isolated, escalated to a human, and re-injected with full provenance**. The system does not claim a machine should settle value judgments; it guarantees the machine recognises it must not settle them alone.

---

## Future Architecture — Level 4

Level 4 is defined as: *several LLMs coordinate a real task without semantic loss.* CSTL's transport layer (level 1) is the prerequisite, not the whole. Three components remain.

| Component | Role | Status |
|---|---|---|
| **Hypothesis engine** | Entanglement detection over a bounded knowledge graph (Wikidata subgraph): find node pairs with high common-neighbour overlap and no direct edge, then propose a speculative CSTL relation at deliberately low σ (`ASSUMES` / `DOUBTS`, never `KNOWS`) | 🟡 Components exist (Graphify, SPARQL integration), generative step untested |
| **Simulation / validation lab** | `ExecutionLab` for computationally checkable hypotheses (internal consistency, contradiction, temporal cycle detection). Empirical world-facts stay permanently low-σ until independently corroborated — by design, not as a gap | 🟡 Consistency checker prototyped; domain simulators require funding |
| **Human council** | `RestrictedCouncil`, 2/3 quorum — plausibility and harm filter before storage, not a truth oracle | 🟡 Designed, matches published precedent (Wikidata Primary Sources Tool pattern) |

**What CSTL contributes specifically.** Discovery is graph mathematics; validation is sandbox logic — both would work in any format. CSTL's contribution is the **auditable epistemic transport between them**:

```
generation:  ASSUMES x_influenced_y [σ=0.3 source=hypothesis_engine
                                     derived_from=common_neighbour_pattern]
                    │
                    ▼  transported intact, σ preserved
validated:   BELIEVES x_influenced_y [σ=0.75 source=hypothesis_engine
                                      validated_by=executionlab_run_xyz
                                      method=internal_consistency_check]
```

The full σ trajectory — speculation to confirmed belief, with every step attributable — is what no competing format carries natively.

**Discriminative power, not theatre.** A lab that accepts everything validates nothing. Prototype `consistency_lab.py` runs both a plausible and a deliberately contradictory hypothesis: the first passes (σ 0.30 → 0.75), the second is rejected via direct conflict *and* temporal cycle detection (σ 0.30 → 0.09). Negative controls are mandatory before any positive result counts.

---

## Audit & Traceability — the ADN layer

Every payload is content-addressed and every decision about it is logged. Two complementary systems exist:

### Rust hash chain — `src/server/audit.rs`, `audit_store.rs` ✅

```
canonical_hash(payload)          BTreeMap-sorted, PARENT_HASH excluded from its own hash
      │
      ▼
HashChain::append()  ──►  AuditEntry { hash, parent_hash, timestamp, payload }
      │
      ▼
AuditStore (SQLite)  ──►  persisted, reloadable
      │
      ▼
verify_integrity()   ──►  detects any break in the chain
```

Deterministic, tamper-evident, tested (including `test_parent_hash_excluded_from_hash`, `test_integrity_detects_break`). Known issue: async reload hits a `blocking_lock()` conflict inside the tokio runtime — deferred refactor.

### Python ADN store — `cstl_adn_store.py` 🟡

Content-addressed semantic memory with three tables:

| Table | Role |
|---|---|
| `adn_store` | hash-keyed payloads with `encoder`, `produced_by`, `sigma`, `parent_hash`, `conversation_id`, `turn`, and an **anchoring flag** (`committed`, `committed_by`, `committed_at`) |
| `adn_council_log` | every council action — `commit` / `revoke`, by whom, with a note and timestamp. **This is the human arbitration audit trail.** |
| `emergence_proofs` | per-question record of each model's *solo* answer (Claude, GPT, Gemini, others), the *final* collective decision, **who changed position**, what they changed to, and the resulting `delta_sigma` |

Retrieval is TF-IDF over stored payloads, with a `get_primer()` / `load_context()` pair that reconstructs conversational context from hashes alone — so context is *referenced*, never re-transmitted. `ADNDeltaDetector` reports whether an incoming payload is genuinely new relative to what is already stored (`novel_tokens`, `closest_hash`, `delta_sigma`).

**Why `emergence_proofs` matters for Level 4.** Level 4 asks whether several LLMs coordinating produce something no single one produced alone. That table is the instrument for answering it empirically: it records the counterfactual (each solo answer) alongside the outcome, making position changes and σ shifts auditable rather than asserted.

### The lab in the audit chain

When a hypothesis is validated or rejected, the σ transition is itself an auditable event:

```
ASSUMES x_influenced_y [σ=0.3  source=hypothesis_engine
                        derived_from=common_neighbour_pattern]
        │  ← stored in adn_store, not committed
        ▼  ExecutionLab consistency check
BELIEVES x_influenced_y [σ=0.75 validated_by=executionlab_run_xyz
                         method=internal_consistency_check]
        │  ← council commits → adn_council_log entry
        ▼
   anchored (committed=1, committed_by=…, committed_at=…)
```

Nothing becomes an anchored fact without a logged human commit. Rejection is equally traceable: a contradicted hypothesis drops to σ≈0.09 and stays queryable rather than being deleted.

**Honest status.** The Rust chain is tested and operational. The Python ADN store is implemented but **not wired into the live pipeline** — the two systems are not yet unified, and the council-log/emergence tables have no production data. Consolidating them is open work.

---

## Compression

| Mode | Ratio | Status |
|---|---|---|
| Standard (raw vs JSON-LD) | ~1.45× | ✅ Measured — **disappears after gzip** |
| Standard (gzipped, deontic content) | ~1.15× | ✅ Measured |
| **CASTLE mode** | 80–85% target | 🟡 **Architected, no code, no benchmark** |

CASTLE amortizes a shared dictionary across a session rather than compressing each payload independently. **The architecture is complete; the implementation is not.** No CASTLE figure here is an empirical measurement — treat it as a design target until code and benchmark exist.

Earlier claims of 5×–200× compression were empirically refuted and are retracted.

---

## v5.0.0 Operators

**Logical:** `ENTAILS`, `CONTRADICTS`
**Epistemic:** `KNOWS`, `BELIEVES`, `ASSUMES`, `DOUBTS`
**Temporal (Allen 1983 subset):** `BEFORE`, `AFTER`, `DURING`
**Relational:** `EQUALS`, `POSSESSES`, `RESEMBLES`, `CO_LOCATES`, `OPPOSES`, `COMPARES`
**Deontic:** `MUST`, `MUST_NOT`, `MAY`, `SHOULD`, `NOT`
**Causality:** `ARR`, `ARR.CREATE`, `ARR.JOIN`, `ARR.PRODUCE`, `ARR.ACCESS`
**Speech acts:** `COMMAND`, `ASK`, `STATE`, `PERFORM`, `RECOMMEND`
**Fallback:** `RELATE [type=custom gloss="…"]` — honest admission when no canonical operator fits
**Deprecated:** `MUTUAL` (use a specific relational operator, W601)

*Planned (v6):* five remaining Allen relations — `MEETS`, `OVERLAPS`, `STARTS`, `FINISHES`, `CONTAINS`. Converses handled by argument order rather than new symbols.

---

## Rust Parser

```bash
cargo test
```

75 tests passing, 0 failures. Zero production dependencies. Deterministic O(n) parsing, no LLM in the validation path.

---

## Honest Limitations

- Multi-hop degradation measured to 12+ hops; **not** characterised beyond that.
- Judge-based semantic evaluation: N=10 to N=24 per run, exploratory rather than confirmatory.
- Open-weight LLMs (Llama, Mistral, Qwen): partially validated only.
- Standard-mode compression advantage largely disappears after gzip.
- CASTLE mode: architecture only, no implementation, no benchmark.
- Layers 3b, 5, 6, 7: designed or partial, not production-wired.
- Human council resolution rate: never measured under production conditions.
- Two parallel audit systems (Rust hash chain, Python ADN store) not yet unified.
- `adn_council_log` and `emergence_proofs` tables: implemented, zero production data.
- Level 4 hypothesis engine: generative step (proposing novel relations) not yet demonstrated.
- Simulation lab: consistency checking prototyped; domain-specific simulators not built.
- Zero external adopters.

---

## Formal Semantics

Deontic operators grounded in SDL (von Wright, 1951) with Kripke semantics. Epistemic operators follow Hintikka (1962). Temporal operators implement a subset of Allen's interval algebra (1983). The relations-over-information principle follows the structuralist intuition (Saussure, 1916) that elements derive value from their differential relations rather than intrinsic substance.

Full spec: [`CSTL_SPEC_v5_0.md`](CSTL_SPEC_v5_0.md)

---

## License

Apache 2.0 — Olivier Goyette
