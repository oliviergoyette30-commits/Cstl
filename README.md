# CSTL v5.0.0 — Compressed Semantic Transfer Language

> The first wire format designed natively for LLM-to-LLM communication.
>
> **Les relations sont plus importantes que l'information.** — [Principes fondateurs](PRINCIPES.md)

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
![Tests](https://img.shields.io/badge/tests-153%20passing-brightgreen.svg)

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
| 2 | **Governance / Resilience** — circuit breaker, 2/3 quorum, operator drift prevention, Ed25519 identity | 🟡 Partial, observation-only (`src/governance.rs`, wired live 2026-09-03): a per-sender rolling-window circuit breaker (repeated `ExecutionLab` inconsistency events) and an operator-drift ratio (repeated `SEMANTIC_WARNING`) are computed on every payload and exposed in a new `GOVERNANCE [...]` response block, escalating harder via Telegram when tripped — but neither ever rejects a payload, by explicit design decision, matching the only real blocking path in this server (security/parse/validation). `RestrictedCouncil::quorum_size()` now implements real ceil(2/3·n) quorum arithmetic and `AdnStore::cast_commit_vote` tallies distinct voters per hash — exercised end-to-end with a 2-member council (`examples/governance_smoke_test.rs`) and, since 2026-09-05, with a real 3-member council configured through the actual `CSTL_COUNCIL_MEMBERS` environment variable and real `agent_register` traffic (`examples/multi_member_council_smoke_test.rs`). **New 2026-09-04** (`src/signing.rs`): Ed25519 message signing, closing part of OWASP ASI03/ASI07 (identity & inter-agent communication were previously unauthenticated plain-text strings) — optional globally, mandatory only for a sender already registered with a `public_key`, no PKI/CA (self-signature proves key possession, not prior identity); verified live end-to-end on a real TCP connection, dev and release builds (`examples/signing_registration_smoke_test.rs`, 6 scenarios). **Second update, same day**: production council membership is now configurable (`CSTL_COUNCIL_MEMBERS`, comma-separated names — `restricted_council.rs::from_env()`), replacing the hard-coded single "Olivier" member — but making membership configurable alone would have been security theater once a second member exists: `RestrictedCouncil::is_authorized()` is a bare string comparison on `sender`, so anyone on the TCP connection could previously have forged `sender=Alice` in a `council_decision` payload with zero proof, reaching quorum alone. Fixed in `handler.rs`'s `council_decision` block: a vote is now accepted only if (1) the sender is on the authorized list, (2) the message carries a **valid** Ed25519 signature (not just "any" — STEP 2a's existing check only verifies a message is internally self-consistent with whatever key IT claims, never that this key matches the one on file), and (3) the embedded `META.public_key` matches **exactly** the key registered for that name via `agent_register` — binding claimed identity to cryptographic proof, not just the message to itself. Verified live: a legitimate 2-of-3 quorum still reaches commit (`examples/governance_smoke_test.rs`, scenarios 3/4), an unsigned vote from an authorized-but-unregistered member is rejected (`signature_required`, scenario 5), and a vote signed with an attacker's own key while claiming `sender=alice_h` is rejected (`public_key_mismatch`, scenario 6) — the last one is the actual identity-forgery case this closes. **Third update, same day**: the key↔registry binding above was initially scoped to `council_decision` only — extended to STEP 2a globally so it covers all signed traffic, not just votes. Before this, a message signed by an attacker's own key while claiming `sender=<any already-registered name>` would still pass STEP 2a (the signature was internally self-consistent with the attacker's own embedded key, and STEP 2a never compared that key to the registry) for anything other than a council vote. Now a single registry lookup in STEP 2a serves both `signature_required` and a `public_key_mismatch` rejection whenever a registered sender's message carries a different key than the one on file — `agent_register` itself stays exempt (legitimate key rotation isn't checked against the old key). Verified live end-to-end: ordinary signed traffic from a registered agent still processes normally, and the same traffic signed by an impostor's key while claiming that agent's name is rejected (`public_key_mismatch`) — `examples/governance_smoke_test.rs` now covers 8 scenarios. **Fourth update, 2026-09-05**: the breaker/drift state itself was, until this pass, in-memory only (`Arc<Mutex<GovernanceTracker>>`, reset to neutral on every server restart) — a circuit that had just tripped open, or a drift ratio that had just crossed the threshold, silently forgot both the instant a process restarted. Persisted now in the same SQLite file/`Connection` as the rest (`governance_events`/`governance_alerts` tables in `adn_store.rs`, same one-event-per-payload grain as the audit trail's `save_audit_entry`, opportunistically pruned to the larger of the two rolling windows so the table never grows unbounded); `CstlNativeServer::try_with_data_path` reloads it at startup via `GovernanceTracker::with_defaults_restored`. Verified with a real restart (new `CstlNativeServer` instance against the same on-disk file, same pattern as `examples/audit_persistence_smoke_test.rs`): a breaker tripped open (3/3) and a drift ratio flagged (1.00) on the first instance both show up already open/flagged on the very first payload processed by the second instance, while a sender never seen on either instance starts neutral (`examples/governance_persistence_smoke_test.rs`, 4 scenarios). |
| 3a | **Public fact verification** — Wikidata + SPARQL, entity resolution | ✅ Implemented, wired live (`src/kb_verify.rs`) |
| 3b | **Software lab + arbitration** — `RestrictedCouncil`, subprocess-isolated `ExecutionLab`, human channel | 🟡 Partial: `ExecutionLab` (contradiction + cycle detection) wired live (`src/execution_lab.rs`); `RestrictedCouncil` wired live (`src/restricted_council.rs`) with a Telegram bridge (buttons, live reply) — 2/3 quorum arithmetic and multi-voter tallying now implemented and tested (`quorum_size()`, `AdnStore::cast_commit_vote`, Layer 2 above), but the production config still registers a single authorized member, so quorum=1 in practice today |
| 4 | **Calibration** — Laplace-smoothed scoring, per-agent/per-domain accuracy | ✅ Tested |
| 5 | **Persistent memory / provenance** — SQLite store, hash entanglement | 🟡 Built in Rust (`src/adn_store.rs`), wired live. **Fixed 2026-09-04**: the hash chain (`seq`/`parent_hash` continuity, `src/server/audit.rs::HashChain`) was, until this pass, *purely in-memory* — reset to empty on every server restart, even though `adn_store.rs` already persisted payload history with real `parent_hash` lineage. `AuditStore` (`src/server/audit_store.rs`) already implemented the fix but was dead code, called nowhere outside its own unit test, since it was written. Now wired: `CstlNativeServer::with_data_path` opens `AuditStore` against the same file as `adn_store` and seeds the chain from it at startup; `handler.rs` persists every entry. Verified with a real process kill + restart of the actual production binary (not just an in-process simulation): `[AuditStore] Loaded 2 entries from disk` on the second process, chain correctly continuing at `seq=2` instead of resetting to `seq=0/parent=root` (`examples/audit_persistence_smoke_test.rs`). **Second finding, same day, found live on the user's own machine while re-verifying the fix above**: `HashChain::append` computed `seq` as `self.entries.len()`, which silently assumed no gaps — but `AuditStore::save()` uses `INSERT OR IGNORE` (needed so a resent duplicate-content payload stays idempotent instead of erroring on the `UNIQUE(hash)` constraint), so a genuine duplicate resend leaves a gap in the persisted `seq` sequence (e.g. `seq=0` and `seq=2` on disk, no `seq=1`). After a real restart, `load_chain()` reloads exactly those (gapped) rows, `entries.len()` no longer matches the highest real `seq`, and the next `append()` recomputed a colliding `seq` — silently losing a brand-new, non-duplicate payload's persistence (confirmed live: a genuinely new payload was accepted with a normal-looking client response, `seq` colliding with an existing row, and never appeared in `audit_trail` after restart). Fixed: `seq` is now `self.entries.last().map(|e| e.seq + 1).unwrap_or(0)` — anchored to the highest real `seq`, not the entry count. Regression test added (`test_append_seq_survives_gap_from_deduplicated_reload`) simulating the exact gapped state observed on disk. **Since merged (2026-09-04, later the same day)**: `src/server/audit_store.rs`/`AuditStore` referenced above no longer exist as a separate file/type — fully folded into `AdnStore` (`save_audit_entry`/`load_chain`/`audit_count`), one schema, one `Connection`, one lock — see the "Rust hash chain" section below for the current state |
| 6 | **Human interface** — Obsidian vault escalation (`src/obsidian_escalation.rs`), wired live; Graphify (`graphifyy` PyPI package) regenerated 2026-09-04 — 793 nodes, 1646 edges, 46 communities, built from commit `f73daa6b` | ✅ Both real: Obsidian verified end-to-end against a live vault (contradiction detected by `ExecutionLab` -> written to `CSTL_Restricted_Council.md`); Graphify structure current as of commit `f73daa6b` — this run used `graphify update .` (AST re-extraction only, no LLM/API key available), so community names are structural hub labels, not the LLM-generated semantic labels from the 2026-09-03 run (967 nodes/1800 edges/63 semantically-labelled communities) — run `graphify extract . --mode deep` with an LLM key configured to restore semantic labelling; either way, re-run `graphify update .` after further commits to resync |
| 7 | **Agent discovery & routing** — CSTL-native registry, agent cards, zero external deps | ✅ Built and wired live (`src/agent_discovery.rs`, used by every request). **New 2026-09-04**: registration is now dynamic — `Arc<Mutex<AgentRegistry>>` + `purpose=agent_register` wire message (self-signed, upsert by name), verified live; a real LLM agent (`sdk/python/cstl_llm_agent.py`, Ed25519 via `cryptography`) registers and signs its own messages, verified live against the real Rust server (registration, unsigned-rejected, signed-accepted) — actual model-generated content is not verifiable in this sandbox (no `anthropic` package, no API key here) and is left for the user to confirm on their own machine |
| 8 | **Provenance audit** — hash-chained audit trail, deontic modality enforcement | 🟡 Built and wired live, not just "designed" (that badge understated what already ran in production for the hash chain, and overstated deontic modality — see below). Hash chain: real, persisted since 2026-09-04 (`HashChain`/`AuditStore`, see row 5). **Deontic modality — built 2026-09-04, same day**: the only live check (`server/validator.rs::validate_deontic_constraints`, before this fix) tested whether a single `RELATION.type` field contained both substrings `"MUST"` and `"MUST_NOT"` — broken twice over: the wire format never encodes MUST/MUST_NOT in `type` (KB predicate or SDL operator only), and `"MUST_NOT".contains("MUST")` is true in Rust, so a lone `MUST_NOT` relation was falsely rejected. The real engine (`semantic.rs::check_axiom_d`, SDL Axiom D, tested) existed but was never called live. Fixed in two parts: a `RELATION` now carries an optional `modality=MUST\|MUST_NOT\|REQUIRE\|FORBID` field, wired to the real Axiom D check (blocking, E107, intra-payload — a self-contradiction in the same message); and a new historical audit (`execution_lab::check_deontic_consistency_with_history`, same pattern as the existing factual `check_consistency_with_history`) checks modality-bearing relations against everything persisted by prior payloads — never rejecting (a disagreement across agents/time isn't a protocol error), surfaced as a `DEONTIC_AUDIT [consistent=false, violations=N]` response block. `adn_relations` gained a `modality` column via an idempotent migration, verified live against a file simulating the exact pre-existing production schema. Verified end-to-end on a real TCP connection, dev and release (`examples/deontic_audit_smoke_test.rs`, 4 scenarios) plus 9 new unit tests. |
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
   │  VALIDATION  ✅  W601–W605 semantic checks    │
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
| **Hypothesis engine** | Entanglement detection over a bounded knowledge graph (Wikidata subgraph): find node pairs with high common-neighbour overlap and no direct edge, then propose a speculative CSTL relation at deliberately low σ (`ASSUMES` / `DOUBTS`, never `KNOWS`) | 🟡 **Generative step built 2026-09-04** (`src/hypothesis_engine.rs`, wired into `src/kb_verify.rs::KbVerifier::detect_entanglement`): overlap-coefficient computation (not Jaccard — a high-degree node would tank Jaccard even at full overlap on the small side, drowning the signal), sigma formula capped at 0.35 (never reaches the 0.8 `KNOWS` threshold — see `semantic.rs::check_knows_calibration`), and CSTL relation formatting are unit-tested here (10 tests, no network). **2026-09-05**: the network orchestration itself (`query_generic_neighbors`/`has_direct_relation`/`resolve_label`/`detect_entanglement`) is now verified live against a real local HTTP server (`wiremock`, `tests/kb_verify_mock_wikidata_test.rs`, 4 tests) that reproduces Wikidata's SPARQL JSON response format — `KbVerifier::with_endpoints` makes the base URL configurable for this (`KbVerifier::new()` in production is unchanged, still hardcoded to the real wikidata.org URLs). This exercises the real HTTP call, request construction and JSON deserialization end-to-end, including a positive case (common-neighbour overlap → hypothesis generated), two negative controls (no overlap; full overlap but already directly related → no hypothesis either way), and two failure-mode cases (HTTP 500, malformed JSON → empty result, no panic). **Honest limit, still open**: this is not the real wikidata.org — the mocked JSON was written by hand from reading the code, not captured from a live response, so real-world latency, throttling, and unanticipated response shapes remain unverified. Needs confirmation on a machine with real network access to wikidata.org. |
| **Simulation / validation lab** | `ExecutionLab` for computationally checkable hypotheses (internal consistency, contradiction, transitive-cycle and temporal-cycle detection). Empirical world-facts stay permanently low-σ until independently corroborated — by design, not as a gap | 🟡 Contradiction + transitive cycle detection (any length, backtracking DFS, not limited to 2 nodes) implemented and wired live (`src/execution_lab.rs`, tested); temporal-cycle detection (code E702, 2026-09-05) added on top — a chain of `BEFORE`/`AFTER` relations across multiple subject pairs that loops back on itself, `BEFORE`/`AFTER` normalized to a single direction before building the graph; `check_consistency_with_history` cross-references each new payload against the full ADN store history (`adn_relations` table), not just relations within the same payload, for all three checks; only re-flags a contradiction/cycle when the NEW payload is what triggers it (pre-existing history-vs-history issues are not re-reported on every unrelated future request); domain simulators not built |
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

### Rust hash chain — `src/server/audit.rs`, persisted via `src/adn_store.rs` ✅

```
canonical_hash(payload)          BTreeMap-sorted, PARENT_HASH excluded from its own hash
      │
      ▼
HashChain::append()  ──►  AuditEntry { hash, parent_hash, timestamp, payload }
      │
      ▼
AdnStore (SQLite)    ──►  persisted, reloadable (audit_trail table, same
                          Connection as the ADN store — see below)
      │
      ▼
verify_integrity()   ──►  detects any break in the chain
```

**Corrected 2026-09-04**: `src/server/audit_store.rs` no longer exists — it was a
separate `AuditStore` type holding its own SQLite `Connection` to the *same* file as
`adn_store`, which meant two independent `Arc<Mutex<..>>` locks over one physical
file, a real coordination risk (not just cosmetic debt). Fully merged into `AdnStore`
(`save_audit_entry`/`load_chain`/`audit_count`, one schema, one `Connection`, one
lock). Deterministic, tamper-evident, tested (including
`test_parent_hash_excluded_from_hash`, `test_integrity_detects_break`,
`test_audit_persistence_survives_reopen_on_real_file_and_shares_adn_store_data`).
**Verifie 2026-09-05 (audit du `blocking_lock()` ci-dessus, jamais traite jusqu'ici)**:
recherche exhaustive dans `src/` et dans tout l'historique git (`git log -p --all`) --
aucun appel `.blocking_lock()` ni `std::sync::Mutex` bloquant n'existe, ni n'a jamais
existe, dans le code Rust de ce depot. Tous les verrous partages du serveur
(`agent_registry`, `chain`, `adn_store`, `governance`) sont deja des
`tokio::sync::Mutex` asynchrones (voir `src/server/mod.rs`), et le seed de la
`HashChain` au demarrage (`try_with_data_path` -> `adn_store.load_chain()`) est un
appel synchrone fait AVANT toute construction d'`Arc<Mutex<..>>` -- il ne prend donc
aucun verrou et ne peut pas entrer en conflit avec le runtime tokio. La ligne
"Known issue" ci-dessus decrivait un probleme qui ne correspond a aucun code reel
de ce depot; elle est retiree plutot que "corrigee" pour ne pas inventer un fix a un
bug qui n'existe pas.

### Rust ADN store — `src/adn_store.rs` 🟡

Content-addressed semantic memory with three tables, built natively in Rust (SQLite via `rusqlite`), wired live into the server:

| Table | Role |
|---|---|
| `adn_store` | hash-keyed payloads with `encoder`, `produced_by`, `sigma`, `parent_hash`, `conversation_id`, `turn`, and an **anchoring flag** (`committed`, `committed_by`, `committed_at`) |
| `adn_council_log` | every council action — `commit` / `revoke`, by whom, with a note and timestamp. **This is the human arbitration audit trail.** |
| `emergence_proofs` | per-question record of each model's *solo* answer (Claude, GPT, Gemini, others), the *final* collective decision, **who changed position**, what they changed to, and the resulting `delta_sigma` |

**Not built yet, honestly flagged:** TF-IDF retrieval, `get_primer()` / `load_context()` context reconstruction, and `ADNDeltaDetector` (novelty detection against existing entries). **Corrected 2026-09-04** (this line self-contradicted the rest of the document, per the repo audit): `commit()` / `revoke()` on `AdnStore` are NOT dead — `RestrictedCouncil` (`src/restricted_council.rs`) exists, is wired live in `handler.rs`'s `council_decision` block, and has actually called `commit()`/`revoke()` end-to-end (including via a Telegram button) — see the Layer 2/3b rows above and `examples/governance_smoke_test.rs`. Entries stay `committed=false` only until a real council vote reaching quorum (`quorum_size()`) commits them — not permanently by construction.

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

**Corrected 2026-09-04** (repo audit): this list previously omitted about a third of
the real whitelist (`semantic.rs::OFFICIAL_OPERATORS`, the actual list the running
server checks `RELATION.type` against, `E101`/`W601`) and mis-described deontic
modality as `RELATION.type` operators — they are not. Both fixed below.

**Logical:** `ENTAILS`, `CONTRADICTS`
**Epistemic:** `KNOWS`, `BELIEVES`, `ASSUMES`, `DOUBTS`
**Temporal (Allen 1983 subset):** `BEFORE`, `AFTER`, `DURING`
**Relational:** `EQUALS`, `POSSESSES`, `RESEMBLES`, `CO_LOCATES`, `OPPOSES`, `COMPARES`
**Causality:** `ARR`, `ARR.CREATE`, `ARR.JOIN`, `ARR.PRODUCE`, `ARR.ACCESS`
**Speech acts:** `COMMAND`, `ASK`, `STATE`, `PERFORM`, `RECOMMEND`
**Intent / dynamics (previously missing from this list):** `INTENT`, `MAINTAIN`, `TRANSFORM`, `RESIST`, `AMP`, `INH`, `PRESSURE`, `CATALYZE`, `TRANSMIT_FAITHFUL`, `TRANSMIT_INFER`
**Fallback:** `RELATE [type=custom gloss="…"]` — honest admission when no canonical operator fits
**Deprecated:** `MUTUAL` (use a specific relational operator, W601)

**Deontic modality is a separate mechanism, not a `RELATION.type` operator.** It
travels as an optional `RELATION.modality=<value>` attribute, checked by
`semantic.rs::check_axiom_d` (E107, blocking) and `execution_lab`'s historical audit
(Layer 8) — not by the operator whitelist above. The real values the engine
recognizes are `MUST` / `REQUIRE` (obligatory) and `MUST_NOT` / `FORBID` (forbidden) —
**not** `MAY`, `SHOULD`, `NOT` as an earlier version of this table claimed; those are
not checked by any live code today.

*Planned (v6):* five remaining Allen relations — `MEETS`, `OVERLAPS`, `STARTS`, `FINISHES`, `CONTAINS`. Converses handled by argument order rather than new symbols.

---

## Rust Server

```bash
cargo test --lib
```

153 tests passing, 0 failures (as of 2026-09-04, updated same day with the hypothesis engine — this number drifts with every session, verify with `cargo test --lib` rather than trusting a fixed figure; six different stale counts were found across this file, `docs/ARCHITECTURE.md` and `CSTL_SPEC_v5_0.md` before this correction). Deterministic O(n) parsing, no LLM in the validation path. Not zero-dependency: `tokio` (async TCP), `reqwest` (Wikidata SPARQL), `rusqlite` (ADN store), `sha2` (audit hash), `serde`/`serde_json` (wire responses), `unicode-normalization` (NFC canonicalization) are all real production dependencies — the "zero production dependencies" claim that stood here was true only of the v4.9.3 hand-rolled lexer/parser and stopped being accurate once the TCP server layer was added; corrected 2026-09-03 alongside the equivalent stale comment in `Cargo.toml`.

---

## Honest Limitations

- Multi-hop degradation measured to 12+ hops; **not** characterised beyond that.
- Judge-based semantic evaluation: N=10 to N=24 per run, exploratory rather than confirmatory.
- Open-weight LLMs (Llama, Mistral, Qwen): partially validated only.
- Standard-mode compression advantage largely disappears after gzip.
- CASTLE mode: architecture only, no implementation, no benchmark.
- Layer 3b: `ExecutionLab` and `RestrictedCouncil` are both wired live, and a commit has actually happened end-to-end (via a Telegram button). `RestrictedCouncil`'s 2/3 quorum arithmetic (`quorum_size()`) and multi-voter tallying (`AdnStore::cast_commit_vote`) are now implemented and tested, and since 2026-09-04 that quorum is also cryptographically enforced (signature + registry key match), configurable to more than one member via `CSTL_COUNCIL_MEMBERS` — production still runs with a single authorized member by default, so quorum=1 in practice unless configured otherwise — see Layer 2 for the new governance module this quorum logic is part of. **Verified 2026-09-05 with an actual 3-member config, end-to-end**: `examples/multi_member_council_smoke_test.rs` sets the real `CSTL_COUNCIL_MEMBERS=A,B,C` environment variable (the exact variable production reads, not a manual override), registers all 3 members over a real TCP connection with real, distinct Ed25519 key pairs via genuine `agent_register` payloads, then drives real `council_decision` votes over TCP: a single vote (1 of 3, quorum_size=2) is recorded but leaves the entry `committed=false`, pending; a second, distinct member's vote on the same entry reaches quorum and commits it — never before the second vote; and a vote signed with a key that does not match the name's registered key is rejected (`public_key_mismatch`) before it can count toward any quorum, with a follow-up legitimate vote proving the rejected attempt neither corrupted nor blocked the real tally. This is additional end-to-end coverage on top of the 2-member scenarios already in `examples/governance_smoke_test.rs` — not a different mechanism, the same one exercised through the real environment-variable and real agent-registration paths instead of test-only shortcuts. Layer 6: Obsidian escalation half is real, wired live, and verified end-to-end against a live vault; Graphify half is also real — installed (`graphifyy` PyPI) and re-run 2026-09-04 against the current codebase (793 nodes, 1646 edges, 46 communities, built from commit `f73daa6b`) via `graphify update .` (AST-only, no LLM key available in that environment — community names are structural hubs, not the LLM-generated semantic labels from the earlier 2026-09-03 run); will go stale again after further commits until `graphify update .` is re-run. Layer 7 (agent discovery/routing) is built and wired live.
- Human council resolution rate: never measured under production conditions.
- Two Rust audit/memory systems (hash chain, ADN store) — both real and wired live, but linked only by a shared hash, not unified into one schema.
- Layer 2 governance state (circuit breaker + operator drift, `src/governance.rs`) is now persisted (2026-09-05, `governance_events`/`governance_alerts` in the same SQLite file/`Connection` as `adn_store`/`audit_trail`) and reloaded at startup — no longer the in-memory-only limitation stated in earlier revisions of this file; verified with a real restart (`examples/governance_persistence_smoke_test.rs`). Still observation-only by design: neither the breaker nor the drift ratio ever rejects a payload.
- `emergence_proofs` table: real, tested CRUD (`AdnStore::put_emergence_proof`/`get_emergence_proofs`), now reachable live via `purpose=detect_emergence` on `INTENT_PAYLOAD` (`src/emergence.rs`, a Rust port of the Python `RevisionOrchestrator` that existed nowhere in this repo before). Design matches how the project's own multi-LLM sessions actually happened (`CSTL_v4_9_1_REFERENCE_DEMO`: a human relays the same question to several LLMs and forwards their responses) — no API keys, no automated cross-vendor calling; the server only compares payloads its agents already submitted. Decision matching is a naive textual comparison (trim + lowercase), not semantic. Still zero production data — nobody has run a real tripartite session through this path yet. `adn_council_log` now has real entries: `commit()`/`revoke()` are reachable (`RestrictedCouncil` → `AdnStore`, optionally via a Telegram button), and have actually been exercised.
- `ExecutionLab` consistency check now cross-references each new payload against the full ADN store history (`src/execution_lab.rs::check_consistency_with_history`, wired live in `handler.rs`), not just relations within the same payload. Covers functional-predicate contradictions, transitive cycles (`part_of`/`located_in`, any length via backtracking DFS) and, since 2026-09-05, temporal cycles (`BEFORE`/`AFTER`, code E702 — a chain across multiple subject pairs that loops back on itself, not the same thing as the pairwise E701 check in `semantic.rs`). No cross-domain simulators.
- Level 4 hypothesis engine: generative step built (`src/hypothesis_engine.rs`, 2026-09-04) and unit-tested offline (overlap coefficient, sigma formula, CSTL formatting). Network orchestration (`KbVerifier::detect_entanglement` and the methods it calls) now verified live against a local `wiremock` server reproducing Wikidata's response format (2026-09-05, `tests/kb_verify_mock_wikidata_test.rs`) — still not the real wikidata.org, needs confirmation on a machine with real network access.
- Simulation lab: consistency checking prototyped; domain-specific simulators not built.
- Zero external adopters.

---

## Formal Semantics

Deontic operators grounded in SDL (von Wright, 1951) with Kripke semantics. Epistemic operators follow Hintikka (1962). Temporal operators implement a subset of Allen's interval algebra (1983). The relations-over-information principle follows the structuralist intuition (Saussure, 1916) that elements derive value from their differential relations rather than intrinsic substance.

Full spec: [`CSTL_SPEC_v5_0.md`](CSTL_SPEC_v5_0.md)

---

## License

Apache 2.0 — Olivier Goyette
