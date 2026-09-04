# CSTL v5.0.0 — Compressed Semantic Transfer Language

> The first wire format designed natively for LLM-to-LLM communication.
>
> **Les relations sont plus importantes que l'information.** — [Principes fondateurs](PRINCIPES.md)

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
![Tests](https://img.shields.io/badge/tests-130%20passing-brightgreen.svg)

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
| 2 | **Governance / Resilience** — circuit breaker, 2/3 quorum, operator drift prevention, Ed25519 identity | 🟡 Partial, observation-only (`src/governance.rs`, wired live 2026-09-03): a per-sender rolling-window circuit breaker (repeated `ExecutionLab` inconsistency events) and an operator-drift ratio (repeated `SEMANTIC_WARNING`) are computed on every payload and exposed in a new `GOVERNANCE [...]` response block, escalating harder via Telegram when tripped — but neither ever rejects a payload, by explicit design decision, matching the only real blocking path in this server (security/parse/validation). `RestrictedCouncil::quorum_size()` now implements real ceil(2/3·n) quorum arithmetic and `AdnStore::cast_commit_vote` tallies distinct voters per hash — exercised end-to-end with a 2-member council (`examples/governance_smoke_test.rs`). Still missing: the production config (`main.rs`) registers only one member ("Olivier"), so quorum=1 in practice today; breaker/drift state is in-memory only (lost on restart). **New 2026-09-04** (`src/signing.rs`): Ed25519 message signing, closing part of OWASP ASI03/ASI07 (identity & inter-agent communication were previously unauthenticated plain-text strings) — optional globally, mandatory only for a sender already registered with a `public_key`, no PKI/CA (self-signature proves key possession, not prior identity); verified live end-to-end on a real TCP connection, dev and release builds (`examples/signing_registration_smoke_test.rs`, 6 scenarios) |
| 3a | **Public fact verification** — Wikidata + SPARQL, entity resolution | ✅ Implemented, wired live (`src/kb_verify.rs`) |
| 3b | **Software lab + arbitration** — `RestrictedCouncil`, subprocess-isolated `ExecutionLab`, human channel | 🟡 Partial: `ExecutionLab` (contradiction + cycle detection) wired live (`src/execution_lab.rs`); `RestrictedCouncil` wired live (`src/restricted_council.rs`) with a Telegram bridge (buttons, live reply) — 2/3 quorum arithmetic and multi-voter tallying now implemented and tested (`quorum_size()`, `AdnStore::cast_commit_vote`, Layer 2 above), but the production config still registers a single authorized member, so quorum=1 in practice today |
| 4 | **Calibration** — Laplace-smoothed scoring, per-agent/per-domain accuracy | ✅ Tested |
| 5 | **Persistent memory / provenance** — SQLite store, hash entanglement | 🟡 Built in Rust (`src/adn_store.rs`), wired live. **Fixed 2026-09-04**: the hash chain (`seq`/`parent_hash` continuity, `src/server/audit.rs::HashChain`) was, until this pass, *purely in-memory* — reset to empty on every server restart, even though `adn_store.rs` already persisted payload history with real `parent_hash` lineage. `AuditStore` (`src/server/audit_store.rs`) already implemented the fix but was dead code, called nowhere outside its own unit test, since it was written. Now wired: `CstlNativeServer::with_data_path` opens `AuditStore` against the same file as `adn_store` and seeds the chain from it at startup; `handler.rs` persists every entry. Verified with a real process kill + restart of the actual production binary (not just an in-process simulation): `[AuditStore] Loaded 2 entries from disk` on the second process, chain correctly continuing at `seq=2` instead of resetting to `seq=0/parent=root` (`examples/audit_persistence_smoke_test.rs`). Still not a single schema/single `Connection` — two SQLite connections into the same file, not yet merged |
| 6 | **Human interface** — Obsidian vault escalation (`src/obsidian_escalation.rs`), wired live; Graphify (`graphifyy` PyPI package, local venv) regenerated 2026-09-03 — 967 nodes, 1800 edges, 63 semantically labelled communities, built from commit `3326f917` | ✅ Both real: Obsidian verified end-to-end against a live vault (contradiction detected by `ExecutionLab` -> written to `CSTL_Restricted_Council.md`); Graphify structure + labels current as of commit `3326f917` — run `graphify update .` after further commits to resync |
| 7 | **Agent discovery & routing** — CSTL-native registry, agent cards, zero external deps | ✅ Built and wired live (`src/agent_discovery.rs`, used by every request). **New 2026-09-04**: registration is now dynamic — `Arc<Mutex<AgentRegistry>>` + `purpose=agent_register` wire message (self-signed, upsert by name), verified live; a real LLM agent (`sdk/python/cstl_llm_agent.py`, Ed25519 via `cryptography`) registers and signs its own messages, verified live against the real Rust server (registration, unsigned-rejected, signed-accepted) — actual model-generated content is not verifiable in this sandbox (no `anthropic` package, no API key here) and is left for the user to confirm on their own machine |
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
| **Hypothesis engine** | Entanglement detection over a bounded knowledge graph (Wikidata subgraph): find node pairs with high common-neighbour overlap and no direct edge, then propose a speculative CSTL relation at deliberately low σ (`ASSUMES` / `DOUBTS`, never `KNOWS`) | 🟡 SPARQL integration exists (`src/kb_verify.rs`); Graphify (now installed, used for codebase visualization) is unrelated to this feature; generative step untested |
| **Simulation / validation lab** | `ExecutionLab` for computationally checkable hypotheses (internal consistency, contradiction, temporal cycle detection). Empirical world-facts stay permanently low-σ until independently corroborated — by design, not as a gap | 🟡 Contradiction + 2-node cycle detection implemented and wired live (`src/execution_lab.rs`, tested); `check_consistency_with_history` cross-references each new payload against the full ADN store history (`adn_relations` table), not just relations within the same payload; only re-flags a contradiction/cycle when the NEW payload is what triggers it (pre-existing history-vs-history issues are not re-reported on every unrelated future request); longer cycles, temporal-cycle detection and domain simulators not built |
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

**Discriminative power, not theatre.** A lab that accepts everything validates nothing. `src/execution_lab.rs` (Rust, tested, wired live) runs both a plausible and a deliberately contradictory case: a valid transitive chain passes (`test_valid_chain_is_not_a_cycle`, σ → 0.75), a functional-predicate contradiction and a 2-node cycle are both rejected (`test_detects_functional_predicate_contradiction`, `test_detects_two_node_cycle`, σ → 0.09). Current scope: final σ is computed in one step from the consistency check — the ASSUMES(0.3) → BELIEVES(0.75) two-stage transition shown below is the target shape, not yet what the running code does. Negative controls are mandatory before any positive result counts.

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

### Rust ADN store — `src/adn_store.rs` 🟡

Content-addressed semantic memory with three tables, built natively in Rust (SQLite via `rusqlite`), wired live into the server:

| Table | Role |
|---|---|
| `adn_store` | hash-keyed payloads with `encoder`, `produced_by`, `sigma`, `parent_hash`, `conversation_id`, `turn`, and an **anchoring flag** (`committed`, `committed_by`, `committed_at`) |
| `adn_council_log` | every council action — `commit` / `revoke`, by whom, with a note and timestamp. **This is the human arbitration audit trail.** |
| `emergence_proofs` | per-question record of each model's *solo* answer (Claude, GPT, Gemini, others), the *final* collective decision, **who changed position**, what they changed to, and the resulting `delta_sigma` |

**Not built yet, honestly flagged:** TF-IDF retrieval, `get_primer()` / `load_context()` context reconstruction, and `ADNDeltaDetector` (novelty detection against existing entries). `commit()` / `revoke()` exist on `AdnStore` but nothing in the running server calls them — there is no `RestrictedCouncil` yet to make that call, so every entry currently written stays `committed=false` permanently.

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

**Honest status.** Both systems are now Rust, tested, and wired into the live server — the ADN store previously described here as Python did not actually exist anywhere in this repository until this pass (verified by exhaustive search before writing `src/adn_store.rs`). They are not yet *unified* into one schema: the hash chain and the ADN store are two separate stores, linked only by the ADN store reusing the hash chain's `hash` as its key. `RestrictedCouncil` now exists and has actually committed an entry end-to-end, including via a Telegram button; its 2/3 quorum arithmetic is now implemented and tested against a 2-member council (`AdnStore::cast_commit_vote`, `examples/governance_smoke_test.rs`) — but the production config (`main.rs`) still registers a single authorized member, so quorum=1 in practice today. `emergence_proofs` still has zero production data.

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

## Rust Server

```bash
cargo test --lib
```

130 tests passing, 0 failures. Deterministic O(n) parsing, no LLM in the validation path. Not zero-dependency: `tokio` (async TCP), `reqwest` (Wikidata SPARQL), `rusqlite` (ADN store), `sha2` (audit hash), `serde`/`serde_json` (wire responses), `unicode-normalization` (NFC canonicalization) are all real production dependencies — the "zero production dependencies" claim that stood here was true only of the v4.9.3 hand-rolled lexer/parser and stopped being accurate once the TCP server layer was added; corrected 2026-09-03 alongside the equivalent stale comment in `Cargo.toml`.

---

## Honest Limitations

- Multi-hop degradation measured to 12+ hops; **not** characterised beyond that.
- Judge-based semantic evaluation: N=10 to N=24 per run, exploratory rather than confirmatory.
- Open-weight LLMs (Llama, Mistral, Qwen): partially validated only.
- Standard-mode compression advantage largely disappears after gzip.
- CASTLE mode: architecture only, no implementation, no benchmark.
- Layer 3b: `ExecutionLab` and `RestrictedCouncil` are both wired live, and a commit has actually happened end-to-end (via a Telegram button). `RestrictedCouncil`'s 2/3 quorum arithmetic (`quorum_size()`) and multi-voter tallying (`AdnStore::cast_commit_vote`) are now implemented and tested, but the production config still registers a single authorized member, so quorum=1 in practice — see Layer 2 for the new governance module this quorum logic is part of. Layer 6: Obsidian escalation half is real, wired live, and verified end-to-end against a live vault; Graphify half is also real now — installed (`graphifyy` PyPI, local venv) and regenerated 2026-09-03 against the current codebase (967 nodes, 1800 edges, 63 semantically labelled communities, built from commit `3326f917`); will go stale again after further commits until `graphify update .` is re-run. Layer 7 (agent discovery/routing) is built and wired live.
- Human council resolution rate: never measured under production conditions.
- Two Rust audit/memory systems (hash chain, ADN store) — both real and wired live, but linked only by a shared hash, not unified into one schema.
- `emergence_proofs` table: real, tested CRUD (`AdnStore::put_emergence_proof`/`get_emergence_proofs`), now reachable live via `purpose=detect_emergence` on `INTENT_PAYLOAD` (`src/emergence.rs`, a Rust port of the Python `RevisionOrchestrator` that existed nowhere in this repo before). Design matches how the project's own multi-LLM sessions actually happened (`CSTL_v4_9_1_REFERENCE_DEMO`: a human relays the same question to several LLMs and forwards their responses) — no API keys, no automated cross-vendor calling; the server only compares payloads its agents already submitted. Decision matching is a naive textual comparison (trim + lowercase), not semantic. Still zero production data — nobody has run a real tripartite session through this path yet. `adn_council_log` now has real entries: `commit()`/`revoke()` are reachable (`RestrictedCouncil` → `AdnStore`, optionally via a Telegram button), and have actually been exercised.
- `ExecutionLab` consistency check now cross-references each new payload against the full ADN store history (`src/execution_lab.rs::check_consistency_with_history`, wired live in `handler.rs`), not just relations within the same payload. Still scoped to functional-predicate contradictions and 2-node cycles — no temporal-cycle detection, no cross-domain simulators.
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
