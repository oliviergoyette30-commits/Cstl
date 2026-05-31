# CSTL v4.9.3 — Formal Specification

**Compressed Semantic Transfer Language**
Status: stable
Author: Olivier Goyette
License: MIT

This document specifies CSTL v4.9.3, the standalone mode of the protocol. It
supersedes SPEC_v4.0. The networked mode (CASTLE) is deferred to v5.0 and is
not specified here.

---

## 1. Scope and positioning

CSTL is a textual, self-sufficient semantic protocol for LLM-to-LLM
communication. Each payload is independently parseable, carries its full
semantic content, and requires no shared infrastructure between agents.

CSTL is not a transport protocol (cf. MCP, A2A) and not a graph query language
(cf. G²CP). It is a payload format. The design target is preservation of
deontic modalities (`[MUST]`/`[NOT]`/`[IF]`), uncertainty markers, temporality,
and force/strength across agent-to-agent hops — information that free text
loses and that JSON Schema encodes only by convention.

---

## 2. Document structure

A CSTL payload is a single block delimited by a hashbang line and an
`---END---` marker:

```
#!CSTL v4.9.3 MODE=A
META [ ... ]
<blocks>
---END---
```

The hashbang uses spaces, not underscores: `#!CSTL v4.9.3 MODE=A`.

`MODE=A` denotes standalone mode. Modes B and C (CASTLE, networked dictionary
sharing) are reserved and not implemented in this version.

---

## 3. META block

The META block is mandatory and carries the payload's self-description.

### 3.1 Mandatory fields

| Field | Type | Description |
|-------|------|-------------|
| `encoder` | role string | The agent role, e.g. `Agent_CLAUDE` |
| `produced_by` | model id | The real model identifier, or `UNKNOWN` |
| `sigma` | float 0.0–1.0 | Declared confidence of the payload |
| `RESPONSE_FORMAT` | enum | `CSTL` |
| `NO_PROSE` | bool | `true` |
| `PARENT_HASH` | hash | `sha256:...` or `root` |

### 3.2 Optional fields

| Field | Type | Description |
|-------|------|-------------|
| `TURN` | int | Position in a multi-turn exchange |
| `TIMESTAMP` | iso8601 | Encoding timestamp |
| `CONVERSATION_ID` | string | Session identifier |

### 3.3 Field syntax

Field names in META do not carry inline type annotations (no `:float`,
`:enum`, `:bool` suffixes on the field name itself). Type is defined by this
specification, not by the payload.

### 3.4 `produced_by` semantics (Session #4)

`produced_by` records the real model that generated the payload, distinct from
the declared `encoder` role. The canonical form is `org/model-version`
(e.g. `anthropic/claude-sonnet`, `openai/gpt-5.5`, `google/gemini-2.5-flash`).

Six validator rules govern `produced_by`:

- R1 IDENTITY_MISMATCH — encoder role and produced_by disagree implausibly
- R2 REDUNDANT — produced_by duplicates encoder with no new information
- R3 clean — canonical, no warning
- R4 PATCH_T4 — normalization applied
- R5 PROXY — `proxy/org -> canonical` form
- R6 PROXY_MASKED_BACKEND — proxy hides the real backend

`produced_by=UNKNOWN` is a legal value for open-weight models without reliable
self-knowledge. This was ratified empirically in Session #8 after Mistral Large
and Llama-3-70B were observed to misdeclare their own identity.

---

## 4. Semantic blocks

CSTL organizes content into ordered blocks. The canonical block order places
constraints before relations:

1. `INTENT_PAYLOAD` — why the message is being sent (global intent)
2. `CONSTRAINTS` — all `[NOT]`/`[MUST]`/`[IF]` deontic statements first
3. `UNCERTAINTY` — `UNKNOWN` vs `ESTIMATED strength=0.6` markers
4. `DEFINE` — agent and system declarations
5. `RELATIONS` — typed relations between defined entities
6. `DECISION` — the payload's resolved decision with sigma

Deontic modalities are first-class: `[MUST]`, `[NOT]`, `[IF]` are part of the
grammar, not conventions layered on top.

---

## 5. Canonical form and hashing (Session #5, #7)

### 5.1 Canonical form — five rules

1. Line endings normalized to LF
2. Runs of whitespace collapsed to a single space
3. META fields sorted lexicographically
4. Unicode normalized to NFC
5. No trailing whitespace

### 5.2 Canonical hash

`canonical_hash()` computes SHA-256 over the canonical form and emits a 256-bit
digest (64 hex characters), prefixed `sha256:`.

The 256-bit width was ratified in Session #7 over a competing 128-bit proposal:
the birthday bound on 128 bits is insufficient for collision resistance at
protocol scale.

### 5.3 Round-trip idempotency

`BinaryWireFormat.compile`/`decompile` is idempotent: re-encoding a decoded
payload yields a byte-identical result (P2 == P3). Four formal test vectors
(`ROUNDTRIP_TEST_VECTORS`) anchor this property.

---

## 6. Error codes (21 normative codes)

| Range | Category |
|-------|----------|
| E101–E105 | Parser structure |
| E106–E112 | Mutuality (7 forms) |
| E201–E205 | Security (PATCH_C*) |
| E301–E304 | Typing |
| E401–E404 | Resource quotas |
| W501–W503 | produced_by warnings |

### 6.1 Mutuality forms (E106–E112)

Multi-payload exchanges can form invalid cycles. The seven mutuality errors,
checked by `SessionValidator` across a set of payloads:

- E106 CircularHashRef
- E107 CircularAgree
- E108 CircularParentHash
- E109 MutualIdentitySwap
- E110 MutualArbitration
- E111 CircularDecision
- E112 MutualProducedBy

---

## 7. Security profile (Session #6, #7)

A `SECURITY_PROFILE` constant defines all security parameters.

- SEC_Q1 — non-ASCII in keyword position → WARNING (E-class on injection)
- SEC_Q2 — zero-width characters stripped → WARNING
- SEC_Q4 — nested META injection → ERROR
- SEC_Q5 — maximum nesting depth 32 (ratified)
- Bidirectional control characters are hex-escaped in audit warnings
- `CSTLTruncationError` is raised explicitly on truncated decompile input

---

## 8. Arbitration (Session #9)

Seven CSTL blocks specify deadlock resolution between agents:

`DEADLOCK_DECLARE`, `ARBITRATION_REQUEST`, `ARBITRATION_RULING`,
`ARBITRATION_APPEAL`, `ARBITRATION_FINALIZE`, `DEADLOCK_TRIGGER`,
`ARBITRATION_TELEMETRY`.

The deadlock threshold is normative: 3 rounds.

An `IDENTITY_ALERT` block supports impostor detection. In Session #9 a
Llama-3-70B instance presented itself as Claude; the impersonation was detected
via an invalid `PARENT_HASH`.

---

## 9. Reference implementations

### 9.1 Rust parser (production)

Pure Rust, zero external dependencies, SHA-256 implemented in-crate.

Modules: `ast`, `token`, `parser`, `security`, `validator`, `canonical`.
Entry points: `parse(input) -> CstlDocument`, `is_valid(input) -> bool`.
CLI: `cstl_validate` (standalone binary).

- 41 tests, 0 compiler warnings
- 13–40 µs per payload
- Hand-rolled lexer + recursive-descent parser (chosen over `nom` for
  zero-dependency portability)

### 9.2 Python parser (reference)

The Python implementation is the readable reference. `CSTLError` derives from
`Exception`. Helpers `cstl.equivalent()` and `cstl.canonicalize()` are
provided. Python test count: 201.

---

## 10. Validation status

CSTL v4.9.3 has been validated zero-shot on five LLMs: Claude, GPT, Gemini,
Mistral Large (spontaneous), and Llama-3-70B (via pre-fill primer).

A noted finding: `produced_by` self-declaration is unstable on open-weight
models lacking brand-specific RLHF. The mitigation is a PRESERVE primer
(pre-filled identity) rather than a FILL primer (self-declaration), with
`produced_by=UNKNOWN` as a legal fallback.

---

## 11. Known limitations

These limitations are documented deliberately. They are open work, not hidden
defects.

- **Semantic search uses TF-IDF, not embeddings.** Synonyms ("résilier" vs
  "annuler") are not reliably matched in the ADN store. Embeddings
  (sentence-transformers + a vector index) are the intended fix and are not yet
  implemented.
- **`sigma` is self-declared.** Nothing validates that a declared confidence
  corresponds to real accuracy. Empirical calibration against verifiable tasks
  is future work.
- **Benchmark sample size is small.** Some protocols are evaluated at n=15.
- **Single human auditor.** Evaluation to date relied substantially on a single
  judge. Independent and blind evaluation is required for a credible academic
  claim.

---

## 12. Deferred to v5.0

- CASTLE networked mode (modes B/C, shared-dictionary compression)
- `SELF_DECLARE` block
- ADN delta-payload mode
- `∿` simulation mode and `«` archaeological mode (exploratory)

---

## Appendix A — Minimal payload

```
#!CSTL v4.9.3 MODE=A
META [
encoder=Agent_CLAUDE,
produced_by=anthropic/claude-sonnet,
sigma=0.88,
RESPONSE_FORMAT=CSTL,
NO_PROSE=true,
PARENT_HASH=root
]
CONSTRAINTS [
[MUST] sender DELIVER audit_trail,
[NOT] system PERFORM auto_decision
]
UNCERTAINTY [
compliance INFERRED [sigma=0.70]
]
DECISION: example_decision [sigma=0.88]
---END---
```
