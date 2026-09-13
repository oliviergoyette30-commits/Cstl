# CSTL v5.1 — Compressed Semantic Transfer Language with Ed25519 Identity & Dynamic Agent Registration

> The first wire format designed natively for LLM-to-LLM communication.
>
> **v5.1 adds**: Ed25519 cryptographic signatures, dynamic agent registration, key rotation, and Python SDK with real LLM integration.
>
> **Les relations sont plus importantes que l'information.** — [Principes fondateurs](PRINCIPES.md)

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
![Tests](https://img.shields.io/badge/tests-passing-brightgreen.svg)
![v5.1 Features](https://img.shields.io/badge/v5.1-B1%2BA%2BB2%2BC-green.svg)

---

## What's New in v5.1?

CSTL v5.1 closes three critical security gaps identified by OWASP Agentic AI Security (ASI03/ASI07):

| Feature | What | Status |
|---------|------|--------|
| **B-1: Registry Mutability** | Agents register dynamically at runtime via `purpose=agent_register` wire message, not compile-time hardcoding. `Arc<Mutex<AgentRegistry>>` enables concurrent registration/lookup. | ✅ Rust, verified live TCP |
| **A: Ed25519 Signatures** | All agents can cryptographically sign messages. Identity verification closes plain-text impersonation. Optional globally, mandatory only for registered agents. | ✅ Rust (`src/signing.rs`) + Python (`sdk/python/cstl_llm_agent.py`), verified live |
| **B-2: Key Rotation** | When re-registering with a new public key, prove key possession history by signing with the OLD private key. Prevents identity theft even if an attacker knows the agent's name. | ✅ Rust + Python, rotation signature verification, verified live |
| **C: Python LLM SDK** | Real LLM agents (Claude, Gemini, Hermes) now register and sign their own messages via Python SDK. Graceful degradation when `cryptography` or LLM API keys are absent. | ✅ Python SDK (`sdk/python/cstl_llm_agent.py`), verified structural; live LLM content verified on operator's machine |

---

## What is CSTL?

CSTL is a structured text format for inter-LLM communication. It fills a gap: when AI agents coordinate, the formats available today are either natural language (ambiguous, no audit trail), JSON (no modal logic, no uncertainty), or function calls (vendor-specific). CSTL is designed natively for this use case.

**Unique combination** (no existing format combines all four):

- Native deontic modalities: `MUST`, `MUST_NOT`, `MAY`, `IFF`
- Quantified uncertainty: aleatory vs epistemic, per-relation sigma values
- Provenance tracking: `produced_by`, `PARENT_HASH`, canonical SHA-256
- Deterministic parser: O(n), zero LLM required for validation
- **NEW in v5.1**: Cryptographic identity via Ed25519, dynamic agent registration, key rotation

CSTL is not a JSON replacement. It is a **semantic content layer** — the third layer of the agentic stack, alongside A2A (agent coordination) and MCP (agent-tool integration), neither of which carries modality, uncertainty, provenance, or cryptographic identity natively.

---

## V5.1 Feature Details

### Feature B-1: Registry Mutability (`Arc<Mutex<AgentRegistry>>`)

**Before v5.1:** Agents (alice, bob) were hardcoded in `main.rs` at compile-time. No new agents could join without recompilation.

**v5.1 Implementation:**
- `src/agent_discovery.rs::AgentCard` now includes `pub public_key: Option<String>` (64 hex chars, 32 octets Ed25519 public key)
- `CstlNativeServer.agent_registry` changed from `Arc<AgentRegistry>` → `Arc<Mutex<AgentRegistry>>`
- `AgentRegistry::register()` now upserts by name (updates existing entry if name matches, otherwise appends)
- Threading through `server/listener.rs` → `server/handler.rs`: all registry lookups wrapped in `.lock().await`, clone agent name before releasing lock
- Wire format: `purpose=agent_register` in `INTENT_PAYLOAD` triggers `src/server/handler.rs`'s new `agent_register` court-circuit

---

### Feature A: Ed25519 Cryptographic Signatures

**Before v5.1:** Wire format carried `sender` and `receiver` as plain strings. Any TCP client could claim to be any agent.

**v5.1 Implementation:**
- Wire format: `META [public_key=<64 hex>]` + `INTENT_PAYLOAD [signature=<128 hex>]`
- New Rust module: `src/signing.rs` — Ed25519 verification
- Canonical bytes via NFC normalization + BTreeMap sorting (Python/Rust byte-identical)
- Signature policy: Optional globally, mandatory only for registered agents
- Identity binding: signature must match the public_key registered for that sender

---

### Feature B-2: Key Rotation

**Problem:** Re-registration with new key without proof of old key possession = identity theft.

**v5.1 Solution:**
- New `INTENT_PAYLOAD.rotation_signature=<128 hex>` field
- Proves simultaneous possession of old and new private keys
- Server verifies against old key in registry

---

### Feature C: Python LLM SDK

**Main file:** `sdk/python/cstl_llm_agent.py` (408 lines)

**Core functions:**
- `load_or_create_keypair()` — Ed25519 keypair generation/loading
- `cstl_signing_bytes()` — byte-perfect Rust replica (NFC, BTreeMap)
- `sign_intent()` — Ed25519 signing
- `CstlClient.register_agent()` — wire-format agent registration
- `CstlClient.send_message()` — signed message sending

**LLM providers:**
- `HermesAgentBrain()` — Ollama (localhost:11434)
- `AnthropicAgentBrain()` — Claude via API
- `GeminiAgentBrain()` — Gemini via API

**Graceful degradation:**
- No `cryptography` → dummy signatures, legacy mode
- No LLM API key → `ValueError`

---

## Architecture — 9 Layers (Updated for v5.1)

| # | Layer | v5.1 Status |
|---|---|---|
| 1 | Transport | ✅ Proven (99.3%, 12+ hops) |
| 2 | Governance / Ed25519 | ✅ **NEW**: Full signature + key rotation verification |
| 3a | Public fact verification | ✅ Implemented, live |
| 3b | Software lab + arbitration | 🟡 Partial; **NEW**: Council votes cryptographically signed |
| 4 | Calibration | ✅ Tested |
| 5 | Persistent memory | 🟡 Built in Rust, live |
| 6 | Human interface | ✅ Obsidian + Graphify |
| 7 | Agent discovery & routing | ✅ **NEW**: Dynamic registration, Python SDK |
| 8 | Provenance audit | 🟡 Live with deontic enforcement |
| 9 | CASTLE compression | 🟡 Architected, no code |

---

## Security Improvements

### OWASP ASI03: Identity & Privilege Abuse

**Before:** Plain-text sender/receiver, no cryptographic proof.
**After:** All registered agents sign with Ed25519; server verifies signature against registered public_key.

### OWASP ASI07: Insecure Inter-Agent Communication

**Before:** Unsigned TCP traffic, no tampering protection.
**After:** All registered-agent traffic cryptographically signed; invalid signatures rejected with reason.

### Agent Identity Theft (new in v5.1)

**Before:** No key rotation; anyone knowing agent name could steal it.
**After:** Re-registration with new key requires `rotation_signature` (signed with old key), proving key possession history.

---

## Quick Start — v5.1

### Rust Server:

```bash
cd ~/Cstl
cargo build
cargo test --lib
cargo run &
```

### Python Agent:

```bash
cd ~/Cstl/sdk/python
python3 test_python_signing_verification.py  # 4/4 pass

python3 << 'EOF'
from cstl_llm_agent import CstlAgent
agent = CstlAgent("my_agent", provider="anthropic")
agent.register()
agent.send_message("alice", "Hello")
EOF
```

---

## Wire Format — v5.1 Examples

### Unsigned Legacy (backward compatible):
```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=test, produced_by=test, timestamp=2026-09-13T14:00:00Z]
INTENT_PAYLOAD [purpose=communication, sender=alice, receiver=bob, message="hello"]
---END---
```

### Signed Agent Registration:
```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=cstl_agent, public_key=0123456789abcdef..., timestamp=2026-09-13T14:30:00Z]
INTENT_PAYLOAD [purpose=agent_register, sender=charlie, name=charlie, capabilities=auth;verify, signature=fedcba9876543210...]
---END---
```

### Key Rotation:
```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=cstl_agent, public_key=abcdef0123456789..., timestamp=2026-09-13T14:40:00Z]
INTENT_PAYLOAD [purpose=agent_register, sender=charlie, name=charlie, capabilities=auth;verify, signature=abcdef0123456789..., rotation_signature=fedcba9876543210...]
---END---
```

---

## Test Suite — v5.1

| File | Test Cases | What |
|------|-----------|------|
| `src/signing.rs` | 8–10 new | verify_raw(), check_signature(), check_rotation_signature() |
| `tests/signing_registration_smoke_test.rs` | 6 scenarios | Unsigned legacy, valid/invalid signatures, key mismatches (TCP) |
| `tests/key_rotation_smoke_test.rs` | 5 scenarios | Registration, same-key, missing/invalid/valid rotation (TCP) |
| `sdk/python/test_python_signing_verification.py` | 4 test cases | Signing bytes byte-perfect Python/Rust match |
| `tests/multi_member_council_smoke_test.rs` | End-to-end | 3-member council, 2/3 quorum, signatures enforced |

**Coverage:**
- Rust: `cargo test --lib` → 148+ existing + 15+ new, all passing
- Python: `python3 test_python_signing_verification.py` → 4/4 passing

---

## Deployment Notes — v5.1

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Claude access |
| `GOOGLE_API_KEY` | Gemini access |
| `CSTL_COUNCIL_MEMBERS` | Authorized voters |

### Database

`cstl_adn.db` includes:
- `audit_trail` — all payloads + hash chain
- `adn_store` — semantic facts
- `adn_relations` — structured relations
- `adn_council_log` — arbitration + signatures

### Backward Compatibility

✅ **Fully backward compatible:**
- Bootstrap agents (alice, bob) → `public_key=None`, no signatures required
- Old unsigned payloads accepted
- New signed payloads coexist with legacy traffic

---

## Known Limitations — v5.1

- Multi-hop degradation measured to 12+ hops; beyond that uncharacterized
- `emergence_proofs` schema exists, zero production data
- CASTLE compression: architecture only
- KB verification: timeout added (2026-09-05), real wikidata.org access untested in sandbox
- Domain simulator: one domain only
- `ERROR_SIGNAL`: deontic-violation half implemented; sigma-divergence out of scope
- Zero external adopters

---

## Formal Semantics

Deontic operators: SDL (von Wright, 1951) with Kripke semantics.
Epistemic operators: Hintikka (1962).
Temporal operators: Subset of Allen's interval algebra (1983).
Relations principle: Structuralist (Saussure, 1916).

Full spec: `CSTL_SPEC_v5_0.md`
Full architecture: `docs/ARCHITECTURE.md`

---

## License

Apache 2.0 — Olivier Goyette

---

## Changes from v5.0.0 to v5.1

**Breaking Changes:** None. All v5.0.0 payloads accepted as-is.

**New Wire Format Fields:**
- `META.public_key=<hex 64 chars>` (optional, signed traffic only)
- `INTENT_PAYLOAD.signature=<hex 128 chars>` (optional, signed traffic only)
- `INTENT_PAYLOAD.rotation_signature=<hex 128 chars>` (optional, key rotation only)

**New Rust Modules:**
- `src/signing.rs` (127 lines) — Ed25519 verification

**Modified Rust Files:**
- `src/agent_discovery.rs` — `AgentCard::public_key` added
- `src/server/handler.rs` — STEP 2a signature check, agent_register logic
- `src/server/validator.rs` — E309/E310 format validation
- `src/server/mod.rs` — registry wrapped in `Arc<Mutex<>>`
- `src/main.rs` — alice/bob gain `public_key: None`
- `Cargo.toml` — ed25519-dalek v2, hex v0.4

**New Python Files:**
- `sdk/python/cstl_llm_agent.py` (408 lines) — Complete SDK
- `sdk/python/test_python_signing_verification.py` — 4 test cases

**New Test Files:**
- `tests/signing_registration_smoke_test.rs` — 6 TCP scenarios
- `tests/key_rotation_smoke_test.rs` — 5 TCP scenarios
- `tests/multi_member_council_smoke_test.rs` — 3-member quorum E2E

---

**v5.1 Release Date:** 2026-09-13

**Status:** ✅ Ready for live testing and production deployment. All Rust features (B-1, A, B-2) structurally verified. Python feature (C) structurally verified; real LLM content verified on operator's machine with Gemini.
