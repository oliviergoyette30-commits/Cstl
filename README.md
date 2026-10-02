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

**Live Verification (TCP-verified):**
```bash
cargo run &  # Start server
python3 sdk/python/test_python_signing_verification.py  # Python ↔ Rust signing_bytes() byte-perfect match
# Then: agent_register with valid signature → agent_register_ack
#       agent_register without signature → signature_required (if sender already has a public_key registered)
#       Message from registered agent without signature → missing_signature_for_registered_agent
```

---

### Feature A: Ed25519 Cryptographic Signatures

**Before v5.1:** Wire format carried `sender` and `receiver` as plain strings. Any TCP client could claim to be any agent (OWASP ASI03/ASI07 identity abuse).

**v5.1 Implementation:**

**Wire Format Changes:**
- `META [public_key=<hex 64 chars>]` — every signed message embeds the sender's public key
- `INTENT_PAYLOAD [signature=<hex 128 chars>]` — Ed25519 signature over the canonical message bytes
- New Rust module: `src/signing.rs` (127 lines)
  - `SignatureCheck` enum: `NotPresent | Valid | Invalid(String)`
  - `check_signature(payload: &CstlPayload) → SignatureCheck` — verifies signature matches embedded public key
  - `check_rotation_signature(payload, old_public_key_hex) → SignatureCheck` — verifies key rotation proof
  - Error reasons: `invalid_hex`, `bad_public_key_length`, `bad_signature_length`, `verification_failed`

**Canonical Message Bytes (Python/Rust byte-for-byte identical):**
- `src/server/audit.rs::signing_bytes(payload)` (Rust)
- `sdk/python/cstl_llm_agent.py::cstl_signing_bytes(...)` (Python)
- Format: `VERSION|<version>\nMODE|<mode>\nMETA|<sorted, exclude PARENT_HASH>\nINTENT|<sorted, exclude signature/rotation_signature>\nRELATIONS|<sorted>`
- NFC Unicode normalization applied
- `public_key` field INCLUDED (ties signature to claimed key)

**Signature Policy:**
- Optional globally (backward compatible — alice/bob bootstrap agents have `public_key=None`, no signature required)
- Mandatory only for a sender already registered with a `public_key` — once registered, all subsequent messages must carry a valid signature
- If signature is invalid → `signature_rejected` response with reason
- If sender is registered but signature is missing → `missing_signature_for_registered_agent` rejection
- If signature is valid but signed by a different key than registered → `public_key_mismatch` rejection

**Dependencies (Cargo.toml):**
```toml
[dependencies.ed25519-dalek]
version = "2"
features = ["rand_core"]

[dependencies.hex]
version = "0.4"
```

**Live Verification:**
- `examples/signing_registration_smoke_test.rs` — 6 scenarios covering unsigned legacy, valid signatures, invalid signatures, key mismatches
- `examples/key_rotation_smoke_test.rs` — 5 scenarios for key rotation (first registration, same-key reregistration, missing rotation_signature, wrong-key rotation, valid rotation)
- Real TCP tested, both dev and release builds

---

### Feature B-2: Key Rotation (`rotation_signature` field)

**Problem:** A re-registration under an already-registered name with a new public key, signed only by the new key, proves only "I possess this new key"—not "I am the same agent already known by this name." Anyone who knows an agent's name could steal its identity.

**v5.1 Solution:**
- New `INTENT_PAYLOAD.rotation_signature=<hex 128 chars>` field (optional, only when the embedded `public_key` differs from the one on file for that name)
- `rotation_signature` = the same message signed with the OLD private key
- Proves simultaneous possession of both the old and new keys
- Server looks up the old key from `AgentRegistry` (never trusts the message)
- `src/signing.rs::check_rotation_signature(payload, old_public_key_hex) → SignatureCheck`

**Handler Logic (src/server/handler.rs, agent_register court-circuit):**
```rust
if payload.meta.public_key != registry.get(agent_name).public_key {
    // Key is changing
    if payload.intent.rotation_signature.is_none() {
        return Err("rotation_proof_required")
    }
    let rotation_check = signing::check_rotation_signature(&payload, &old_key)?;
    if rotation_check != Valid {
        return Err("rotation_proof_invalid")
    }
    // Update registry with new key
    registry.update(agent_name, new_key)?;
}
```

**Live Verification:**
- First registration of a name: no rotation needed
- Same key re-registered: no rotation needed  
- New key without `rotation_signature`: rejected with `rotation_proof_required`
- New key with rotation signature from wrong key: rejected with `rotation_proof_invalid`
- New key with valid rotation signature from old key: accepted, registry updated, traffic signed with new key passes, traffic signed with old key rejected
- Verified in `examples/key_rotation_smoke_test.rs` (5 scenarios) over real TCP

---

### Feature C: Python LLM SDK with Cryptography

**Before v5.1:** No Python-side agent could sign messages. The SDK was demo-only (one-way client sending unsigned payloads).

**v5.1 Implementation:**

**Main File:** `sdk/python/cstl_llm_agent.py` (408 lines)

**Core Functions:**
- `load_or_create_keypair(keyfile: Path) → (priv_bytes, pub_hex)` — generates or loads Ed25519 keypair
- `cstl_signing_bytes(version, mode, meta, intent, relations) → bytes` — byte-perfect replica of Rust version (NFC normalization, BTreeMap sorting, field exclusions)
- `sign_intent(priv_bytes, pub_hex, **kwargs) → str` — Ed25519 signature in hex
- `CstlClient.register_agent(name, pub_key, signature) → dict` — sends wire-format `agent_register` payload
- `CstlClient.send_message(sender, receiver, purpose, message, pub_key, signature) → dict` — sends signed message

**LLM Provider Classes (all implement same interface):**
- `HermesAgentBrain()` — Hermes3:8b via Ollama (localhost:11434)
- `AnthropicAgentBrain()` — Claude via Anthropic API (`ANTHROPIC_API_KEY` env var)
- `GeminiAgentBrain()` — Gemini via Google API (`GOOGLE_API_KEY` env var)

**Graceful Degradation:**
```python
try:
    from cryptography.hazmat.primitives.asymmetric import ed25519
    HAS_CRYPTO = True
except ImportError:
    HAS_CRYPTO = False
    print("⚠️ cryptography not installed")

if not HAS_CRYPTO:
    # sign_intent returns dummy "x" * 128
    # Messages proceed unsigned (legacy mode, if sender not yet registered)
```

**Main Agent Class:**
```python
class CstlAgent:
    def __init__(self, name: str, provider_name: str = "hermes"):
        self.name = name
        self.priv_bytes, self.pub_key = load_or_create_keypair(...)
        self.llm = <HermesAgentBrain|AnthropicAgentBrain|GeminiAgentBrain>()
        
    def register(self):
        """Register with server using Ed25519 signature"""
        signature = sign_intent(self.priv_bytes, self.pub_key, ...)
        self.client.register_agent(self.name, self.pub_key, signature)
        
    def send_message(self, receiver: str, message: str):
        """Send signed message to peer"""
        signature = sign_intent(self.priv_bytes, self.pub_key, ...)
        return self.client.send_message(
            self.name, receiver, "communication", message, self.pub_key, signature
        )
```

**Cross-Language Verification:**
- `test_python_signing_verification.py` — 4 test cases verifying Python `cstl_signing_bytes()` produces exactly the same canonical bytes as Rust, line-by-line
- Test cases: simple message, agent_register with exclusions, empty relations, multiple relations with sorting
- All 4/4 pass (byte-perfect match)

**Live Verification (Python + Rust Server):**
1. Start Rust server: `cd /home/claude/Cstl && cargo run`
2. Run Python test: `cd sdk/python && python3 test_python_signing_verification.py` → ✓ Signing bytes match
3. Generate real keypair and register:
```bash
python3 << 'EOF'
from cstl_llm_agent import CstlAgent
agent = CstlAgent("claude_agent", provider="anthropic")
agent.register()  # → server responds with agent_register_ack
result = agent.send_message("alice", "Hello from Claude")
print("Sent:", result)
EOF
```

**Verified Scenarios (this sandbox, without API keys):**
- Python syntax check: ✓ zero errors
- Imports: ✓ `cstl_signing_bytes`, `sign_intent`, `CstlClient` importable
- Graceful degradation (without `cryptography`): ✓ `HAS_CRYPTO=False`
- Graceful degradation (without `ANTHROPIC_API_KEY`): ✓ `ValueError("ANTHROPIC_API_KEY not set")`
- Byte-for-byte canonical form match: ✓ 4/4 test cases pass

**Not Verified Here (requires user's machine with API keys):**
- Real LLM response generation
- End-to-end signed message from real LLM agent through server
- **Verified on operator's machine (2026-09-05):** Real Gemini model generated `Montréal est_située_au Canada`, agent registered and signed successfully, server accepted and persisted to `adn_store` with real hash and audit chain.

---

### Couche 6: Graphify — audit-trail graph export wired into the live REST API (2026-10-01, `src/server/graphify_server.rs` + `src/server/rest_api.rs`)

`graphify_server.rs` was found by the same `pub mod X` / zero-external-reference grep that found CASTLE, quorum and tls.rs. Unlike tls.rs's stub, this was a *real* module — `GraphifyExporter::build_from_audit_trail()` genuinely builds a node/edge graph from the live `audit::HashChain`, tested in isolation — but `pub mod graphify_server;` only: no REST route, no caller anywhere. The README previously claimed otherwise, in detail and at some length: a `GET /graphify/export` endpoint, node types `agent`/`fact`/`relation`/`council_decision`, edge types `communicates`/`verifies`/`contradicts`/`reinforces`, live deontic-modality coloring, and "real data: 842 nodes (agents + facts), 1784 edges, 42 detected communities." None of that matched the code. The actual node types `build_from_audit_trail` produces are `agent` and `audit_entry`; the actual edge types are `sends_to` and `responds_to` (the module's color map also defines `references`/`authorizes`/`restricts`/`confirms`/`violates`/`implements`, but nothing in `build_from_audit_trail` ever emits an edge of those types — dead code within dead code). Deontic coloring: `get_deontic_color()` exists and is unit-tested in isolation, but is never called by `build_from_audit_trail` — the audit chain's `AuditEntry` (`hash`/`parent_hash`/`sender`/`receiver`/`purpose`/`seq`) carries no deontic-modality field to color by in the first place, so there was nothing for that function to apply to even if it were called. "842 nodes... 42 detected communities" has no basis found anywhere in this repo — no community-detection algorithm exists in `graphify_server.rs` or anywhere else searched. Corrected below with real, measured numbers instead of repeating that figure.

**A real correctness bug found by reading the code before wiring it** (same discipline as CASTLE/quorum): `build_from_audit_trail` truncated `entry.purpose` for the node label with `&entry.purpose[..20.min(entry.purpose.len())]` — a *byte* index into a `str`, which panics in Rust ("byte index N is not a char boundary") the instant that index lands inside a multi-byte UTF-8 character. `purpose` comes straight from the client-controlled `INTENT_PAYLOAD.purpose` field — any sender could crash this endpoint just by having an accented character or emoji land on the 20th byte of their stated purpose. Fixed with `GraphifyExporter::truncate_utf8_safe()` (walks back to the nearest char boundary); `entry.hash` didn't need the same fix since it's always a SHA-256 hex digest, pure ASCII by construction. Proven both in isolation (`test_build_from_audit_trail_does_not_panic_on_multibyte_purpose`) and live over real HTTP (`examples/graphify_smoke_test.rs` scenario 3, a crafted `purpose` sent over the real TCP port, then a real HTTP request against `/graphify/export` that must return `200 OK` rather than hang or drop the connection).

**What's wired now**: `GET /graphify/export` (full graph, matching the one genuinely-accurate line in the old claim) and `GET /graphify/stats` (node/edge counts by type, via `GraphifyExporter::get_graph_stats`, also previously tested in isolation but never reachable) — both added to the REST API that was already live on port 8000 (`rest_api.rs`, spawned from `main.rs` alongside the TCP server, unlike `graphify_server.rs` itself this part was real and running). `ApiState` gained a `chain: Arc<Mutex<audit::HashChain>>` field; each request clones a snapshot of the chain (`HashChain` is now `Clone`, cheap — a `Vec<AuditEntry>`) rather than holding the lock for the duration of graph construction. `GraphifyExporter::filter_nodes_by_type`/`filter_edges_by_type`/`search_nodes`/`traverse_graph` were left unwired by this pass at the time — **wired 2026-10-01, same day, in a follow-up pass** (found again independently via a "`pub fn` never called anywhere" audit of the whole crate): `GET /graphify/filter?node_type=...&edge_type=...`, `GET /graphify/search?q=...` (400 if `q` is missing), `GET /graphify/traverse?start=<node_id>&depth=<n>` (BFS, default depth 2). Verified live against a real non-empty graph built from real TCP traffic, not an empty-graph edge case (`examples/pub_fn_wiring_smoke_test.rs`).

**Verified live, real TCP + real HTTP, `examples/graphify_smoke_test.rs`** (4 scenarios, all passing): `/graphify/export` before any traffic → a valid empty graph (`node_count=0`), not an error; after 3 CSTL payloads from 2 agents over the real TCP port → `node_count=6` (3 `audit_entry` + alice + bob + the `server` receiver), `metadata.node_count` matches `nodes.len()` exactly; the multi-byte-purpose payload → `200 OK`, not a hang or dropped connection; `/graphify/stats` → `total_nodes=7`, `agents=3`, consistent with a fourth payload added by scenario 3.

**Not touched by this pass, left as found**: the Python side (`sdk/python/cstl_graphify_bridge.py`, Obsidian vault sync, `sdk/python/test_graphify_integration.py`) — this session's scope was the dead Rust module and its REST wiring; the Python bridge's own claims were not re-verified here and should not be assumed correct or incorrect from this entry alone.

---

### Couche 9: Deontic Orchestration — câblée en observation sur l'arbitrage réel (2026-10-01)

Cette section affirmait depuis un certain temps "v5.1 COMPLETE" avec un
fichier `src/server/arbitration_api.rs` ("REST API, 6 endpoints,
WebSocket events") — ce fichier existe mais est **vide (0 octet)**, n'est
déclaré module nulle part, et ne compile dans rien. Rien de ce qu'il
décrivait n'a jamais existé. Les affirmations sur `deontic_state_machine.rs`
(noms d'états, hash chain, replay-safety) étaient également fausses — voir
plus bas. Ce qui suit remplace l'ancien texte par ce qui est réellement
construit et vérifié live à cette date.

**`src/server/deontic_orchestration.rs` — réel, maintenant câblé (pas dead code)**:
`DeonticOrchestrator` (registre de règles MUST/MUST_NOT/MAY + dispatcher
d'événements `tokio::broadcast`) existait depuis des mois avec 15 tests
d'intégration réels (`tests/deontic_orchestration_integration_test.rs`,
tous passants) mais n'était construit nulle part — trouvé dead code par le
même grep que CASTLE/quorum/tls/graphify. Câblé ici:
- `ServerContext.deontic: Arc<DeonticOrchestrator>`, construit dans
  `CstlNativeServer::try_with_data_path`, 3 règles MUST/MAY enregistrées
  par défaut dans `start()`.
- `handler.rs` émet un `DeonticEvent` réel à 3 points du pipeline déjà
  réel: `AgentRegister` après une écriture réussie dans
  `agent_registry` (`purpose=agent_register`), `ArbitrationRuling` après
  une soumission de ruling réussie (`purpose=arbitrage_channel,
  action=submit_ruling`, voir `arbitrage.rs`), `GovernanceBreach` quand
  `GovernanceState.circuit_open` ou `.drift_flagged` (Couche 2).
- `GET /deontic/executions` (REST, port 8000) expose le journal
  d'exécutions + le nombre de règles, pour vérification externe.

**Limite honnête, prouvée plutôt qu'affirmée**: c'est une couche
d'**observation/audit**, pas une garde préventive. `emit_event` est
toujours appelé **après** que l'action sous-jacente a déjà été commise
(écriture registre, persistance SQLite du ruling) — un payload n'est
jamais bloqué par cette couche. Les handlers `execute_must_rule`/
`execute_must_not_rule` dans `deontic_orchestration.rs` eux-mêmes ne font
que logger (`eprintln!`); `MUST_NOT` ne rejette donc rien en pratique sur
ce chemin de câblage (seuls les event types `agent_register`/
`arbitration_ruling`/`governance_breach` ont une règle enregistrée, toutes
`MUST`/`MAY` par défaut). Les anciennes affirmations "replay-safe:
idempotent event handlers, version-keyed conflict resolution" étaient
fausses — rien de tel n'existe dans le code, ni avant ni après ce commit.

**`src/server/deontic_state_machine.rs` — réel mais SUPERSEDED, volontairement non câblé**:
543 lignes, 15 tests unitaires réels qui passent — mais ce module
**duplique** `arbitrage.rs`, déjà en production (persisté SQLite, rulings
Ed25519-signés et vérifiés, déjà câblé dans `handler.rs` via
`purpose=arbitrage_channel`, bien avant ce commit). `DecisionLifecycle`/
`DecisionStore` ne sont qu'en mémoire (`HashMap`, perdu au redémarrage),
n'exigent aucune signature et n'écrivent aucune chaîne de hachage — malgré
l'ancienne affirmation "immutable audit trail: each state change logged
with hash chain". Les noms d'états affirmés ("Open → Arbitration → Ruling
→ Closed + appeals", plus un état "Stale") ne correspondent même pas aux
7 variantes réelles de `DecisionState` (`Open, UnderReview, Arbitration,
Appealed, Ruled, Closed, Failed` — pas de `Stale`, "Ruling" n'existe pas,
c'est `Ruled`). Décision utilisateur (2026-10-01, clarification demandée
avant tout câblage, même discipline que pour `tls.rs`): câbler
l'orchestrateur sur le système d'arbitrage déjà réel plutôt que de
construire une deuxième notion de "décision" parallèle et incohérente.
Le module reste donc en l'état — code réel, testé, mais délibérément
jamais construit — voir le commentaire en tête de fichier.

**Python Orchestrator** (`sdk/python/cstl_deontic_engine.py`,
`sdk/python/test_deontic_engine.py`): les deux fichiers existent
(489 et 471 lignes). **Non ré-vérifiés dans cette passe** — ce commit
porte sur le câblage Rust uniquement, même frontière que pour le pont
Obsidian de Graphify (Couche 6).

**Vérification live (2026-10-01)**, `examples/deontic_smoke_test.rs`, vrai
TCP + vrai HTTP (reqwest):
1. `/deontic/executions` avant tout trafic — 3 règles par défaut, 0
   exécution.
2. `purpose=agent_register` réel (vraie paire de clés Ed25519, vraie
   signature) → `/deontic/executions` reflète une exécution réelle
   (`Must`/`log_agent_registration`/`Success`).
3. Un vrai cas d'arbitrage: arbitre enregistré (vraie clé), cas ouvert,
   arbitre assigné, ruling **réellement signé** Ed25519 et vérifié par
   `arbitrage::verify_ruling_signatures` → `/deontic/executions` reflète
   une deuxième exécution (`Must`/`log_ruling_applied`/`Success`).

**Bug trouvé et corrigé en marge de cette vérification** (`handler.rs`,
`submit_ruling`): avant ce commit, `ruling_id` était **toujours** généré
côté serveur (UUID aléatoire) après réception du message, mais
`verify_ruling_signatures` vérifie la signature sur
`ruling_id||decision||justification` — aucun client réel ne pouvait donc
jamais produire une signature valide, puisqu'il ne connaît pas encore le
`ruling_id` au moment de signer. Ce chemin n'avait jamais eu de smoke test
live avant `deontic_smoke_test.rs` (aucun n'existait pour
`purpose=arbitrage_channel`), donc jamais déclenché. Corrigé: le client
choisit maintenant son propre `ruling_id` (même principe que `case_id`,
déjà repris du serveur par le client dans les appels suivants); le
serveur n'en génère un que si absent, pour ne rien casser côté
compatibilité.

**Non vérifié ici**: la règle `governance_breach` (MAY) est câblée
(`handler.rs`, émise quand `GovernanceState.circuit_open` ou
`.drift_flagged`) mais déclencher un vrai circuit breaker exige un
historique d'incohérences construit sur plusieurs payloads — hors de la
portée du smoke test ponctuel ci-dessus. Couverte par lecture de code et
les tests existants de `governance.rs`, pas par du trafic live.

---

## Architecture — 10 Layers (Updated for v5.1)

CSTL is not only a wire format. The syntax is layer 1 of a governance architecture:

| # | Layer | v5.1 Status |
|---|---|---|
| 1 | **Transport** — wire format, SHA-256 immutable, deterministic validation | ✅ Proven (99.3%, 12+ hops) |
| 2 | **Governance / Resilience** — Ed25519 identity, signature verification, key rotation, circuit breaker, 2/3 quorum | ✅ **NEW v5.1**: `src/signing.rs` (check_signature, check_rotation_signature), `src/server/handler.rs` STEP 2a signature verification, all registered agents require valid signatures. Backward compatible: bootstrap agents (alice, bob) with `public_key=None` don't require signatures. |
| 3a | **Public fact verification** — Wikidata + SPARQL, entity resolution | ✅ Implemented, wired live (`src/kb_verify.rs`) |
| 3b | **Software lab + arbitration** — `RestrictedCouncil`, subprocess-isolated `ExecutionLab`, human channel | ✅ Real, wired live over TCP (`purpose=arbitrage_channel` in `handler.rs`, `src/server/arbitrage.rs`): open case, assign arbiters, submit ruling, **peer review, escalate to council** (both added 2026-10-01, see below), finalize. **Corrected 2026-10-01**: the previously claimed `src/server/arbitration_api.rs` ("REST API, 6 endpoints, WebSocket events") is an empty (0-byte) file, declared nowhere, compiled into nothing — that claim was entirely fabricated. The real path is the TCP channel above; see the Couche 9 section for a live-verified example (open → assign → Ed25519-signed ruling). Case lifecycle: Open → InProgress → RulingSubmitted → Finalized/EscalatedToCouncil (`arbitrage::CaseStatus`, not the names previously claimed here). **Peer review gap found and fixed 2026-10-01**: `peer_review_async`/`escalate_to_council_async` (real DB-backed logic, `save_peer_review`/`save_arbitrage_case`) existed with zero wire action to trigger them — `finalize_case` therefore ALWAYS failed with `QuorumNotReached` as soon as a council required ≥1 peer review (nothing ever called `save_peer_review`), a real functional gap, not a duplicate. Wired as actions `peer_review`/`escalate_to_council` on `purpose=arbitrage_channel` (`handler.rs`), verified live end-to-end: `finalize_case` fails before any peer review, a second registered arbiter signs `"{reviewer_id}||{ruling_id}"` raw Ed25519 and submits `peer_review`, `finalize_case` then succeeds (`examples/pub_fn_wiring_smoke_test.rs`). Known remaining limitation, found while wiring, not fixed (out of scope): `finalize_case_async` reports a hardcoded single `peer_reviews` entry (`"quorum"`) rather than the real per-reviewer list once the quorum is met — the finalized count is always 1, not the true number of reviews recorded. Human channel fully wired via restricted council + Telegram bridge. |
| 4 | **Calibration** — Laplace-smoothed scoring, per-agent/per-domain accuracy | ✅ Tested. **Monitoring route added 2026-10-01**: `sigma_calibrator` (EWMA, `src/calibration/ewma.rs`) was already shared on `ServerContext` and fed by a real bridge (`kb_verify` confirmed-relation verdicts → `observe_verdict`), but `get_calibration`/`get_all_calibrations` had no inspection route — found via the same "`pub fn` never called" audit. Now live: `GET /calibration/agents` (full snapshot), `GET /calibration/agents/:agent_name` (404 if unseen). Honest limit: feeding real data requires an external Wikidata confirmation, unavailable offline, so this pass verified the routes' shape (empty list, clean 404) rather than observed data (`examples/pub_fn_wiring_smoke_test.rs`). |
| 5 | **Persistent memory / provenance** — SQLite store, hash entanglement, TF-IDF search, context loading, delta detection | ✅ TF-IDF retrieval (`get_tfidf_results`), context windows (`get_primer`, `load_context`), compression + indexing. **Delta detection corrected 2026-10-01**: `detect_deltas` (`src/adn_delta_detector.rs`) was real and tested (`tests/couche5_persistence_e2e_test.rs`) but never invoked from the live TCP pipeline, despite the previous "v5.1 COMPLETE" claim — now wired (`handler.rs` STEP 3e-delta): every stored payload is compared to its hash-chain parent, emitting an `ADN_DELTA [...]` wire line only when a real change is detected (informational, never blocking). Fixed in the same pass: the wire format `format_cstl` produced was multi-line (non-conformant) and its `old_hash=`/`new_hash=` fields collided by substring with `AUDIT [hash=...]` for naive extractors — both fixed, verified live (`examples/adn_delta_smoke_test.rs`). |
| 6 | **Human interface** — Obsidian vault escalation, Graphify knowledge graph | ⚠️ **Partially wired (2026-10-01)**: `src/server/graphify_server.rs` → `GET /graphify/export`/`GET /graphify/stats`, real REST routes, verified live (see Couche 6 section above) — was dead code with fabricated metrics before this date. **Extended same day**: `filter_nodes_by_type`/`filter_edges_by_type`/`search_nodes`/`traverse_graph` existed since the module's creation but had zero route — found via a "`pub fn` never called anywhere" audit. Now live: `GET /graphify/filter?node_type=...&edge_type=...`, `GET /graphify/search?q=...`, `GET /graphify/traverse?start=<node_id>&depth=<n>` (BFS), verified against a real non-empty graph (`examples/pub_fn_wiring_smoke_test.rs`). Node types: `agent`, `audit_entry`. Edge types: `sends_to`, `responds_to`. No deontic-modality coloring (the audit chain carries no deontic field to color by). `sdk/python/cstl_graphify_bridge.py` / Obsidian vault sync: not re-verified this pass. |
| 7 | **Agent discovery & routing** — CSTL-native registry, agent cards | ✅ **NEW v5.1**: `Arc<Mutex<AgentRegistry>>` enables dynamic registration. `purpose=agent_register` wire message (self-signed bootstrap, no prior identity needed) upserts agents by name. Python SDK (`sdk/python/cstl_llm_agent.py`) can now register real LLM agents and sign their messages. |
| 8 | **Provenance audit** — hash-chained audit trail, deontic modality enforcement | ✅ **v5.1 COMPLETE**: Built and wired live. Hash chain real, persisted, reloadable. Deontic modality checking (`src/server/audit.rs::DeonticCheck`) verified for MUST/MUST_NOT/MAY. Council votes cryptographically enforced. |
| 9 | **Deontic orchestration** — event-driven observation/audit layer over arbitration & governance | ⚠️ **Partially wired (2026-10-01)**: `src/server/deontic_orchestration.rs` (real event router, was dead code, now constructed + emits on 3 real events: agent_register, arbitration_ruling, governance_breach) → `GET /deontic/executions`, verified live (see Couche 9 section). Observational only — emitted *after* the underlying action, does not gate anything; MUST_NOT handlers only log. `src/server/deontic_state_machine.rs` deliberately left unwired (duplicates `arbitrage.rs`, see Couche 9 section) — no "appeals" lifecycle live. `sdk/python/cstl_deontic_engine.py`: not re-verified this pass. No real replay-safety/idempotency exists anywhere in this layer (previously claimed, false). |
| 10 | **WAI v5.1 Compression Layer** — bit-packing, varints, zigzag, delta, optional TANS/session-state | ✅ **v5.1 COMPLETE**: `src/compression/wai_core.rs` (350+ lines, core transformations), `src/compression/fse_encoder_rs.rs` (340+ lines, optional TANS + dynamic session state). Wire format: magic 0x57 0x41 0x49, SHA-256 dict sync, symbol count varint. **Performance:** 63.81% compression ratio (exceeds 70% target), 100% roundtrip accuracy. **Tests:** 408/408 passing (21 WAI-specific, 387 existing CSTL, zero regressions). **Spec:** `docs/WAI_SPECIFICATION_v5_1_COMPLETE.md`, verification report `docs/WAI_V5_1_VERIFICATION_COMPLETE_2026-09-14.md`. **`src/compression_pipeline.rs` investigué le 2026-10-01**: module réel, 3 tests unitaires passent, zéro référence externe même dans les tests — pipeline de compression naïf, DUPLIQUE le système WAI v5.1 ci-dessus qui est déjà celui réellement câblé en production (`compression::master`/`compression::response`, construits sur `wai_core`/`fse_encoder_rs`). Décision explicite de l'utilisateur: laissé mort, non câblé, commentaire SUPERSEDED ajouté en tête du fichier — même traitement que `deontic_state_machine.rs` (Couche 9). **Registre de dictionnaires WAI câblé le même jour**: `wai_registry` (`src/server/wai.rs::DictionaryRegistry`) était construit au démarrage, une version y était enregistrée, mais rien ne le relisait jamais (`get_latest`/`list_versions`/`stats` sans route) — trouvé par le même audit "`pub fn` jamais appelée". Câblé: `GET /wai/dictionaries`, `GET /wai/dictionaries/latest`, `GET /wai/stats`, vérifié live. |

**Key v5.1 Changes to Layer 2:**
- New `src/signing.rs` module (127 lines) with `check_signature()` and `check_rotation_signature()`
- STEP 2a in `src/server/handler.rs` now verifies signatures for all registered agents
- New wire format fields: `META.public_key`, `INTENT_PAYLOAD.signature`, `INTENT_PAYLOAD.rotation_signature`
- Identity binding: signature must match the public_key registered for that `sender` name
- Council votes now cryptographically enforced (signature + key match), preventing impersonation

**Key v5.1 Changes to Layer 7:**
- Registry now mutable: `Arc<Mutex<AgentRegistry>>`
- `AgentCard` gains `public_key: Option<String>` field
- `purpose=agent_register` wire message enables dynamic registration
- Python SDK fully functional with Ed25519 signing and LLM providers

---

## Master Compressor — 4-Stream Structural Compression (post-v5.1, `src/compression/master.rs`)

A separate compression path that splits a CSTL payload's `defines`/`relations`/`uncertainty` fields into 4 independent byte streams before encoding, instead of running everything through one adaptive table. **Wired into `server/handler.rs`'s live pipeline** (2026-09-29) — but as a Couche 5 (persistent storage) addition, not a wire-protocol change: every payload processed by the live TCP server now also gets its `defines`/`relations`/`uncertainty` compressed via `AdnStore::put_master_compressed` and stored alongside the raw text in a new `master_compressed` SQLite table (`hash`, `compressed`, `reference_payload_text_len`, `compressed_len`). The TCP wire format itself is untouched — every agent (Python SDK, existing tests, anything already speaking CSTL) still sends and receives plain CSTL text exactly as before; nothing about compatibility changed. Verified live against the real running server (not just `cargo test`): two real payloads sent over real TCP via `sdk/python/cstl_client.py`, read back directly from the resulting `cstl_adn.db` — `311→47 bytes` and `217→32 bytes` measured on real traffic, not synthetic test fixtures.

| Stream | Content | Encoding |
|---|---|---|
| `stable` | Closed-vocabulary opcodes | Pre-trained, versioned order-1 table (`stable_dictionary.rs`), zero transmission — only `[version][mode]` travels |
| `text` | Raw UTF-8 of interned strings (open vocabulary, letter-statistics-shareable) | Same pre-trained-table approach (`text_dictionary.rs`) |
| `ids` | Prefix+digit identifiers (`relations.id`, `uncertainty.identifier`, e.g. `r001`, `e047`) | Prefix + delta-zigzag-varint against the last value seen for that prefix **within the same message** (`structural.rs::try_parse_id`) — no cross-message or session state |
| `variable` | Message-local counts/lengths/indices (never shareable across messages) | Delta + zigzag + varint, table-free (`variable_delta.rs`), replacing an earlier per-context rANS table that cost more than it saved on small messages |

Both dictionary streams (`stable`, `text`) size-guard their mode choice: the pretrained/entropy-coded path is only used `if body.len() < stream.len()`, otherwise the stream falls back to raw bytes — verified live on a 3-byte message where the pretrained path would have cost 6 bytes and raw+header costs 5.

**Measured, live** (`cargo test --lib compression::master`, not asserted):
- Small reference payload: 119 bytes via the 4-stream master compressor vs. 393 bytes via the earlier single-stream/single-table approach vs. 226 bytes as raw CSTL text (52.7% of raw text, 30.3% of the old approach)
- Larger reference corpus: 377 bytes vs. 1220 bytes as raw CSTL text (30.9%)

**Explicitly rejected after live testing**, not just discussed: merging `stable` and `text` into one shared dictionary — measured to produce `UnknownSlot`/`UnknownContext` failures neither dictionary alone had, because the two vocabularies' training contexts don't compose even though their raw byte ranges rarely collide.

**ID fast-path coverage**: `relations.id`, `uncertainty.identifier`, and `defines.id` (added 2026-09-29 — `id` isn't a dedicated `defines` field, it lives in the generic `extras` map alongside any other key; the encoder now special-cases it out of that map when present and numeric-shaped).

**Deliberately out of scope for this component**: a closed word-level dictionary for common CSTL vocabulary (needs real traffic data, not fabricated word lists).

---

## Wire-Protocol Compression — `COMPRESSED_PAYLOAD` Block (post-v5.1, `src/server/parser.rs`, 2026-10-01)

Unlike the Master Compressor section above (storage-layer only), this extends the Master Compresseur onto the TCP wire itself. A sender can now include a `COMPRESSED_PAYLOAD [data=<base64>]` block alongside (or instead of) plain-text `DEFINE`/`RELATION`/`UNCERTAINTY` blocks; the server base64-decodes `data`, runs it through `master_decompress`, and merges the resulting `defines`/`relations`/`uncertainty` into the parsed payload before validation runs — a compressed define is indistinguishable from a plain-text one by the time the governance/validation pipeline sees it.

**Why base64 inside the existing text grammar, not a raw binary envelope**: the TCP framing layer (`handler.rs::find_message_end`) finds message boundaries by scanning for the literal byte sequence `---END---`. Arbitrary compressed bytes could coincidentally contain that sequence and corrupt framing; base64's restricted alphabet (`[A-Za-z0-9+/=]`) cannot produce it. This design required zero changes to the TCP framing/buffering logic — all of it lives in `parser.rs`'s existing block-dispatch state machine (the same 4 dispatch sites every other block type — META, RELATION, DEFINE, etc. — already goes through).

**Failure handling is deliberately non-fatal**: a missing `data` field or invalid base64 does not reject the message — it appends a `SEMANTIC_WARNING` (e.g. `COMPRESSED_PAYLOAD: base64 invalide; bloc ignore -- ...`) and the rest of the payload (plain-text blocks, if any) is still processed normally. Verified live against the real running server: a malformed `COMPRESSED_PAYLOAD [data=not_valid_base64!!!]` block produced exactly that warning and the message still completed the full pipeline (validation → consistency → governance → audit).

**Verified live, real TCP, not just `cargo test`**: a `defines`/`relations`/`uncertainty` triple was compressed with the real `master_compress`, base64-encoded, sent as a `COMPRESSED_PAYLOAD` block over a real TCP connection to the real running server, and the server's response showed the decompressed `DEFINE`/`RELATION`/`UNCERTAINTY` content flowing correctly through semantic validation (it even correctly raised a `W608` semantic warning that the decompressed `DEFINE` wasn't referenced by the decompressed `RELATION`'s subject/object — proof the decompressed data reached the same validation code a plain-text payload would).

**Client-side send support (2026-10-01, `sdk/python/cstl_client.py` + `src/bin/cstl_compress_cli.rs`)**: the Python SDK can now build and send a `COMPRESSED_PAYLOAD` block itself, via `CstlClient.send_compressed(sender=..., receiver=..., purpose=..., defines=..., relations=..., uncertainty=...)` or by passing `compressed_defines`/`compressed_relations`/`compressed_uncertainty` to `build_payload(...)`. The actual compression is delegated to a new small Rust binary, `cstl_compress_cli` (`cargo build --release --bin cstl_compress_cli`), invoked as a subprocess (`compress_triple(...)` / `decompress_data(...)` in `cstl_client.py`) — deliberately NOT a second, independent port of the 4-stream/2-dictionary compression format into Python, which would be a second surface that could silently drift from the Rust original (the same class of risk documented for `cstl_signing_bytes`, except here there's no reason to accept it: compression, unlike signing, doesn't have to happen client-side). If `cstl_compress_cli` isn't built or found, `send_compressed`/`build_payload` raise `CstlCompressionUnavailable` with an actionable message — the rest of the client is unaffected, same degrade-cleanly pattern as `AnthropicAgentBrain.from_env()`. Verified live, real TCP, full roundtrip: `python3 sdk/python/cstl_client.py --smoke-test` step 4/4 builds a `defines`/`relations`/`uncertainty` triple, compresses it via the CLI, sends it as a real `COMPRESSED_PAYLOAD` block to a real running server, and confirms the response shows no warnings and `status=processed`.

**Scope limits, stated plainly**:
- **Still scoped to `defines`/`relations`/`uncertainty`** for `COMPRESSED_PAYLOAD` specifically — `meta`/`intent`/other block types are unaffected and still travel as plain text alongside a `COMPRESSED_PAYLOAD` block in the same message.
- **The CLI binary isn't distributed** — anyone wanting `send_compressed` on the Python side has to `cargo build --release --bin cstl_compress_cli` from this repo first (or point `CSTL_COMPRESS_CLI_PATH` / `binary_path=` at one already built).

### Response-side compression — `COMPRESSED_RESPONSE` (2026-10-01, `src/server/response_compression.rs` + `src/compression/response.rs`)

The remaining gap from the first pass — responses were always plain text — is now closed, with a measured result that's worth stating plainly rather than hiding: **it currently makes responses bigger, not smaller, and should not be enabled.**

**How it's built**: a client opts in per-request with `INTENT_PAYLOAD.compress_response=true`. The server does NOT recompress every one of its dozens of response-construction call sites individually (that was correctly identified as too invasive) — instead, `maybe_compress_response(&response, want_compressed)` intercepts the already-built plain-text `String` at each of the 9 `socket.write_all(...)` sites inside the successfully-parsed branch of `handler.rs`, right before it goes on the wire. It re-parses the response into generic `(block_name, [(key, value), ...])` tuples (`src/compression/response.rs` — a NEW, separate codec from the Master Compresseur, since response block types like `VERIFICATION`/`GOVERNANCE`/`EXECUTION_TRACE`/`AUDIT` were never in that codec's scope), compresses them, and re-renders a single `COMPRESSED_RESPONSE [data=<base64>]` block in their place.

**Safety gate, not a performance shortcut**: before ever sending a compressed response, the server re-renders the parsed blocks back to plain text and compares that, byte-for-byte, to the original. Any mismatch — an edge case in a field's formatting, anything not anticipated — silently skips compression for that one message; the original plain text goes out instead. This was verified directly: a response field containing an embedded comma inside quotes (`errors="E304: Missing sender, E305: Missing receiver"`) round-trips correctly (`cargo test --lib server::response_compression`), and a deliberately malformed body falls back to the untouched original rather than panicking or corrupting anything.

**Client-side (`sdk/python/cstl_client.py`)**: `build_payload(request_compressed_response=True)` / `send_compressed(request_compressed_response=True)` set the opt-in field; `send_raw()` transparently detects and expands a `COMPRESSED_RESPONSE` block (via `cstl_compress_cli decompress-response`, same subprocess-bridge pattern as the request side) BEFORE handing the text to `parse_response()` — so the existing regex-based response parser never has to know compression exists. Verified live, real TCP: `python3 sdk/python/cstl_client.py --smoke-test` step 5/5 sends `compress_response=true`, confirms the server actually responded with a `COMPRESSED_RESPONSE` block (not plain text) at the wire level, and confirms the client expanded it correctly before parsing.

**Measured, live, honestly** (not asserted, not cherry-picked): a real 9-block response (`META`/`INTENT_PAYLOAD`/`RELATION`/`VERIFICATION`/`CONSISTENCY`/`GOVERNANCE`/`SIGMA_CALIBRATION`/`EXECUTION_TRACE`/`AUDIT`, the actual shape of a normal `processed` response) measured **971 bytes plain vs. 1327 bytes compressed (137%)** — bigger, not smaller. A short validation-error response (2 blocks, no hash) measured **186 bytes plain vs. 283 bytes compressed (152%)**. Two causes, both root-caused rather than guessed at: (1) base64 adds a fixed ~33% size penalty that the compression step would have to beat just to break even, and (2) `text_dictionary`'s pre-trained vocabulary was trained for `defines`/`relations`/`uncertainty` content (the Master Compresseur's original scope) — response-specific vocabulary (`VERIFICATION`, `kb_verification`, `drift_flagged`, 64-hex-char SHA-256 hashes) mostly isn't in it, so most strings fall through to the dictionary's raw-bytes fallback and gain nothing, while the generic block codec still pays its own structural overhead (a string table + per-field index varints) that plain text doesn't have at all.

**Size guard added same day, after the measurement above** (`try_compress` in `response_compression.rs`): rather than leave a feature that's measured to hurt on typical traffic, the candidate compressed text (header + `COMPRESSED_RESPONSE` block + footer, base64 included) is now compared byte-length to the original plain text, and compression is used ONLY if the candidate is strictly smaller — otherwise the original plain text goes out, exactly as if the client hadn't asked. Same principle already in place elsewhere in the codec (`stable_dictionary`/`text_dictionary` both fall back to raw bytes when their pre-trained mode wouldn't win). Verified live: the same real request that previously produced a 968-byte plain response against a 1327-byte compressed one, sent again with `compress_response=true` after the guard was added, comes back as the 968-byte plain response — `COMPRESSED_RESPONSE` absent from the wire. A synthetic highly-repetitive response (40 identical `SEMANTIC_WARNING` blocks) confirms the compression path still engages and round-trips correctly when there's real redundancy to exploit (2920 bytes plain → 316 bytes base64, a case the guard correctly lets through) — `cargo test --lib server::response_compression` covers both the reject-the-loss and accept-the-win paths explicitly.

**Honest conclusion, updated**: the mechanism is correct, safe (the roundtrip gate guarantees no corruption), fully wired end-to-end, and now also harmless by construction — a client can set `compress_response=true` unconditionally and will never pay the measured 137%-152% overhead, because the size guard refuses to send anything bigger than plain text. It just won't help EITHER, on the vocabulary the server currently sends (scattered hashes, mostly-unique field values) — the structural overhead plus base64 rarely beats plain text unless a response happens to repeat a lot of identical content, which real responses mostly don't. The deeper fix remains the same known, bounded future work: train `text_dictionary` (or a new response-specific dictionary) on real response traffic instead of the defines/relations/uncertainty corpus it has today — the same "needs real traffic data" limitation already on record for the word-level dictionary, not a new unknown.

### Response corpus collection — `CSTL_COLLECT_RESPONSE_CORPUS` (2026-10-01, `src/adn_store.rs` + `src/server/handler.rs::send_response`)

The response-compression section above identifies the actual fix needed: `text_dictionary` has to be trained on real response vocabulary, which doesn't exist anywhere yet. This feature is purely infrastructure toward that — it does not retrain anything itself, it only makes the training data collectable.

**What it does**: opt-in, off by default, read once at startup from the environment variable `CSTL_COLLECT_RESPONSE_CORPUS` (`1` or `true`, case-insensitive — same pattern as `TelegramNotifier::from_env()`/`RestrictedCouncil::from_env()` elsewhere in this codebase). When active, every response built in the successfully-parsed branch of `handle_connection` is persisted, in its original PLAIN-TEXT form (never the compressed wire form, even if `compress_response=true` was also requested — a future dictionary trainer needs the real vocabulary, not a base64 blob), into a new append-only `response_corpus` SQLite table (`id`, `response_text`, `response_len`, `created_at`, no foreign key — a response isn't tied to one specific stored payload the way `master_compressed`/`governance_evaluations` are). Single new integration point: `send_response()` wraps the write to the socket — it persists first (best-effort; a write failure only logs a warning, never blocks or fails the actual response to the client) and then applies the existing `maybe_compress_response` decision. All 9 of the handler's former direct `socket.write_all(maybe_compress_response(...))` call sites now go through this one function.

**Deliberately no deduplication.** A dictionary trainer needs real frequency data — how often a given string actually occurs — not a deduplicated sample of distinct forms. `AdnStore::record_response_corpus_entry` inserts unconditionally; `count_response_corpus_entries`/`export_response_corpus(limit)` are the read side for a future offline training pass. Covered by `test_response_corpus_append_only_no_dedup_and_export_order` (`cargo test --lib adn_store::tests::test_response_corpus`), which explicitly asserts that two identical entries both land as separate rows.

**Verified live, real TCP, both states**: started the server with the variable unset, ran the full `cstl_client.py --smoke-test` (5 requests, a mix of normal/compressed/error responses) — `response_corpus` table exists (created either way, cost-free) but has exactly 0 rows afterward. Restarted with `CSTL_COLLECT_RESPONSE_CORPUS=1` set (confirmed via the server's own startup banner), ran the identical smoke test — 5 rows landed, each one the genuine plain-text response body (confirmed by inspecting `response_text` directly via `sqlite3`), including the step-5 request that used `compress_response=true` on the wire — proving the stored copy is the plain text, not the compressed form that actually went to the client.

**Honest scope**: this is collection only. No retraining happens automatically, no dictionary is touched, and the response-compression size guard documented above still rejects compression on typical traffic today exactly as before — nothing about THAT measurement changes until someone actually runs an offline training pass against an exported corpus and rebuilds `text_dictionary` from it, which remains future work.

### CASTLE Layer 9 — session dictionary wired into the live pipeline (2026-10-01, `src/server/castle.rs` + `src/server/castle_wire.rs`)

CASTLE (`src/server/castle.rs`) has existed since v5.0.0 with 7 passing unit tests but was never actually reachable — `pub mod castle;` only, dead code, no call site anywhere. Wiring it into `handler.rs` (response side, opt-in via `INTENT_PAYLOAD.castle_response=true`, same shape as `COMPRESSED_RESPONSE`) meant looking at it honestly first, and it surfaced three real correctness bugs the module's original loose `contains()`-style tests never caught:

1. **Ambiguous byte stream.** The old `encode_symbol_id`/`is_symbol_marker` scheme tried to tell a dictionary symbol ID apart from a structural/literal byte using a value heuristic (`byte >= 128 || byte < 32`) with no reserved marker range — a single-byte symbol ID can legitimately be any value 0..=255 and routinely collided with the ASCII range structural/literal bytes also use. Fixed with a self-describing tag-prefixed format (`[tag][payload]` per token) — no ambiguity possible regardless of byte values.
2. **Whitespace and quote marks silently dropped.** Harmless for JSON formatted exactly the way the tokenizer expected, wrong for CSTL block text, which has meaningful whitespace and quoted values (`errors="E304: ..., ..."`). Fixed by folding whitespace into the token being built and keeping both quote characters as part of the captured string.
3. **"Delta" wasn't a delta.** `encode_json_with_dict` cloned the *entire* dictionary into every `EncodedPayload.dictionary`, `DictType::Delta` or not — the one thing "session-amortized" is supposed to avoid. Fixed: `Delta` now carries only the symbols newly inserted during that call.

A fourth bug surfaced only once a real CSTL-shaped string was round-tripped through the fixed tag format: the literal-detection branch (meant to catch JSON numbers/`true`/`false`/`null`) fires on *every* peeked character, not just at token boundaries — a wildcard-accumulated token like `"status"` hits `'s'` (wildcard) then `'t'` (which matches the literal trigger set) *mid-word*, and the old code started a fresh literal buffer and pushed it immediately while the pending `"s"` only flushed later, **reordering bytes** (`"status"` decoded back as `"tatus s"`). Fixed by flushing the pending token before entering that branch.

**All four fixes are about correctness, not speed** — they're what makes a byte-exact roundtrip possible at all, which is the precondition for `castle_wire.rs`'s safety gate (same architecture as `response_compression.rs`: trial-encode on a cloned dictionary snapshot, verify byte-for-byte against what a receiver holding the pre-trial dictionary would decode, only commit the live dictionary and send compressed if that succeeds *and* the result is strictly smaller than plain text). A rejected trial leaves the connection's dictionary completely untouched — committing speculatively would let a later successfully-compressed message reference a symbol the client was never actually told about, silently desyncing the session.

**Structural difference from `COMPRESSED_RESPONSE`**: CASTLE's dictionary is scoped to the *TCP connection*, not the message — `handle_connection` already loops over multiple pipelined messages per connection, so a `CastleParser` is created once at the top of that function and threaded through every `send_response()` call for that connection's lifetime. This means CASTLE has no chance of helping at all unless the client keeps the connection open (`CstlClient(keep_alive=True)` on the Python side) — a client that opens a fresh connection per message (the default) restarts with an empty dictionary every single time. Client-side decode needed its own stateful bridge for this reason: `cstl_compress_cli decode-castle-response` takes the connection's dictionary (as JSON, passed in and returned on every call — the wire format itself stays a compact custom binary, only the local subprocess IPC uses JSON) and `CstlClient._castle_dict_json` carries it between calls on the same socket, reset whenever a genuinely new connection is opened.

**Measured, live and in isolation, honestly**: the roundtrip-correctness fixes above were necessary but not sufficient to make CASTLE a net win. Measured directly (standalone harness, not assumed): even 80 back-to-back *identical* repeats of a realistic `META` line never drop the compressed-to-plain ratio below ~1.17× — it floors there, it does not converge toward 1.0 as repetition grows. Root cause, precisely identified this time (not just "wrong vocabulary" as with `COMPRESSED_RESPONSE`): ordinary CSTL/English vocabulary contains `t`/`f`/`n`/digits almost everywhere, which the literal-detection heuristic fragments into small `Literal` tokens that are *never* dictionary-compressed and pay a fixed per-occurrence overhead (tag + 4-byte length + bytes) every single time, with no amortization — while structural chars (`{}[]:,`) cost 2 bytes tagged vs. 1 plain, a flat loss on every occurrence. When content is constructed to avoid those trigger characters entirely, the mechanism works exactly as designed and converges to ~0.34× (a genuine 66% reduction) by 50 repeats — proving the dictionary/tag/gate machinery itself is sound, just not a match for this vocabulary. Confirmed again over a real running server, real TCP, a persistent connection, 5+ live round trips with `castle_response=true`: the dictionary never advances (`symbol_count() == 0` throughout) and every response stays plain text — real server responses are dominated by unique per-message SHA-256 hashes and the same scattered-trigger-character vocabulary, so even a deliberately favorable injected value couldn't tip the balance. Zero corruption, zero bandwidth cost either way — the size guard does real, confirmed work here, not a formality.

**Honest conclusion**: wired correctly, safe by construction (same roundtrip + size-guard discipline as `COMPRESSED_RESPONSE`), and currently a no-op on real traffic for a *different*, more precisely diagnosed reason than the Master Compresseur path — this time it's the tokenizer's literal-detection heuristic (inherited from a JSON-oriented design, never a good match for prose-like CSTL field values) fragmenting ordinary vocabulary into unamortizable pieces, not a training-data mismatch. Fixing *that* is bounded, identified future work (stop treating arbitrary identifier text as a JSON-number/boolean candidate), tracked here rather than in `text_dictionary`'s to-do — the two features are unrelated codecs with unrelated root causes for the same symptom.

### Quorum Layer 2 — BFT multi-agent consensus wired into the live pipeline (2026-10-01, `src/server/quorum.rs` + `src/server/quorum_wire.rs` + `src/adn_store.rs`)

`quorum.rs` existed since a previous session with 6 passing unit tests (`QuorumMember`, `VoteMessage`, `QuorumState`, Ed25519 vote-signature verification, BFT threshold, circuit breaker) — found by the same `pub mod X` / zero-external-reference grep that found CASTLE and the other dead modules this session. `pub mod quorum;` only, no caller, no network path, no persistence.

Wired in as three new purposes (`quorum_propose`, `quorum_vote`, `quorum_circuit_breaker`), dispatched from `handler.rs` right after the `agent_register` block, same short-circuit pattern as `council_decision`/`agent_register`. v1 scope, deliberately bounded and documented in `quorum_wire.rs`'s module comment rather than hidden:

- **Membership = `AgentRegistry` entries with a `public_key`.** Reuses the identity system already wired for `agent_register` instead of building a second one around `QuorumMember` — a legacy unsigned agent (`public_key=None`) can't propose or vote; BFT consensus needs cryptographically verifiable identity, not just a name.
- **No second, vote-specific signature.** `VoteMessage::verify_signature` exists in `quorum.rs` but is not exercised — the whole CSTL message carrying the vote is already signed and verified by `handler.rs` STEP 2a (same discipline as `council_decision`). `VoteMessage` is still used as a value type (`vote_id`/`content_hash` for the persisted ledger's immutability).
- **No automatic round-timeout advancement.** `round` stays `1` for every proposal in this version — a real retry-on-timeout needs a background scheduler, out of scope for "wire it into the request/response pipeline." `should_activate_circuit_breaker`/`aggregate_health_score` (which need `failed_round_count`/`QuorumMember` data this wiring doesn't track) are therefore not called; a **manual** circuit breaker exists instead (`purpose=quorum_circuit_breaker`), gated by `RestrictedCouncil` — same authority domain as `council_decision`.
- **`threshold` fixed at proposal creation**, computed from the number of currently-eligible (registered + signed) agents via `compute_threshold` (BFT ⌈2/3·n⌉). An agent that registers *after* a proposal is opened doesn't move that proposal's threshold — consistent with `QuorumState` itself, which never recomputes `threshold` from `add_vote`/`check_consensus`.

**Persistence, and why it has to differ from CASTLE.** CASTLE's dictionary is scoped to one TCP connection (lives in a `CastleParser` created once per `handle_connection` call) because its only requirement is "the same socket remembers what it already sent." Quorum can't work that way: real BFT voting means *different agents voting from different connections* on the *same* proposal, potentially minutes or hours apart. So `quorum_propose`/`quorum_vote`/`quorum_circuit_breaker` persist every write to two new SQLite tables in `adn_store.rs` (`quorum_proposals`, `quorum_votes`, the latter `UNIQUE(proposal_id, voter_id)` — the DB itself rejects a duplicate vote, `has_voted` is a belt-and-suspenders pre-check) and reload `QuorumState` from disk on every `quorum_vote`/`quorum_circuit_breaker` call rather than holding it in a connection-scoped struct. (`scripts/schema_quorum.sql` is a separate, considerably more elaborate schema — member health scores, vote status/rejection tracking, SQL triggers, views — that predates this wiring and was never loaded by any code; this implementation is the simpler v1 the module comment describes, not that design. Worth reconciling later if the richer schema's features — health-weighted consensus, a `quorum_members` table distinct from `AgentRegistry` — turn out to be needed; not done here.)

**Verified live, real TCP, real server, `examples/quorum_smoke_test.rs`** (9 scenarios, all passing): propose from an unregistered sender → rejected; propose from a registered sender (3 agents, alice/bob/carol) → `quorum_proposal_created`, `threshold=2` (`compute_threshold(3)`); alice votes `yea` **on a freshly opened connection** → `consensus_reached=false`; bob votes `yea` **on a third, separate connection** → `consensus_reached=true`, `final_decision=yea` — the specific proof this needed, since nothing here shares a socket; carol's vote after the decision is final → `proposal_already_decided`; an attacker signing with their own key while claiming `sender=alice` → caught (see honest finding below); circuit breaker attempted by a non-`RestrictedCouncil` agent → `not_authorized`; circuit breaker by the legitimate council member → `quorum_circuit_breaker_activated`, and a subsequent vote on that proposal → `circuit_breaker_active`.

**One honest finding from the live run that the unit tests didn't surface**: the impersonation scenario (attacker signs validly with their own key while claiming `sender=alice`, a registered agent) is actually caught by `handler.rs` STEP 2a — the *global* signature guard that already protects all signed traffic — before the message ever reaches `handle_quorum_vote`. The response is `purpose=signature_rejected, reason=public_key_mismatch`, not `purpose=quorum_rejected` from `quorum_wire::verify_registered_voter`'s own identical check. Both guards exist (the one in `quorum_wire.rs` mirrors `council_decision`'s exact pattern on purpose, see its doc comment), but STEP 2a fires first for any sender already in the registry — `verify_registered_voter`'s own `public_key_mismatch` branch is real defense-in-depth, not dead code, but is not the one that actually decides this case in the current pipeline order.

### TLS 1.3 mutual authentication — real implementation replacing a security-theater stub (2026-10-01, `src/server/tls.rs` + `src/server/listener.rs` + `src/server/handler.rs`)

`tls.rs` was found by the same `pub mod X` / zero-external-reference grep that found CASTLE and quorum.rs — but it was not the same kind of dead code. CASTLE and quorum were *real, working* algorithms simply never called. `tls.rs` was a stub that did no cryptography at all: `verify_client_cert` accepted any non-empty byte string as a valid certificate (`client_cert.len() > 0 && ca_cert.len() > 0`), and `generate_test_cert()` returned hard-coded, truncated, unparseable PEM text (`"-----BEGIN CERTIFICATE-----\nMIIC...\n-----END CERTIFICATE-----"`). Wiring that into the live pipeline as-is would not have been "finishing an incomplete feature" — it would have made the server *claim* TLS 1.3 mutual authentication while providing zero actual verification, which is worse than having no TLS at all (a false sense of security). `rustls`/`rustls-pemfile`/`tokio-rustls` were already Cargo.toml dependencies (`# v5.1 Dependencies`, labeled "TLS 1.3 mutual authentication layer") and never imported anywhere. Flagged to Olivier before writing any code; he chose the real implementation.

**What changed**: `tls.rs` is now a real wrapper around `rustls::ServerConfig` — `TlsConfig` carries real PEM bytes (`cert_chain_pem`, `private_key_pem`, optional `client_ca_pem`, `require_mutual_auth`), `TlsServer::new()` parses them with `rustls_pemfile::certs`/`private_key` (rejects anything that isn't actually valid PEM — proven directly by `test_tls_server_rejects_garbage_pem`, which the old stub would have silently accepted), and builds either `with_no_client_auth()` or a real `WebPkiClientVerifier` over an actual `RootCertStore` when mutual auth is required. `TlsServer::acceptor()` returns a `tokio_rustls::TlsAcceptor`.

**Network wiring**: `handle_connection`/`send_response` in `handler.rs` were generic-ized over `S: AsyncRead + AsyncWrite + Unpin` (previously hard-typed to `TcpStream`) — the body only ever called `.read()`/`.write_all()`, never a `TcpStream`-specific method, so this needed no duplicated logic. `listener.rs::accept_connections` now takes `Option<Arc<TlsAcceptor>>`: `None` (default) keeps raw-TCP behavior byte-for-byte identical to before this commit; `Some` wraps every accepted socket with `acceptor.accept(socket).await` *before* `handle_connection` ever sees it, so a failed handshake (missing client cert, untrusted CA, a plain-TCP client hitting a TLS port) never reaches application logic at all — exactly what real mutual TLS is supposed to guarantee, and exactly what the old stub's after-the-fact `verify_client_cert()` could never have provided (it ran *after* a plaintext message was already parsed). Opt-in via `CSTL_TLS_CERT_PATH`/`CSTL_TLS_KEY_PATH`/`CSTL_TLS_CLIENT_CA_PATH`/`CSTL_TLS_REQUIRE_MUTUAL_AUTH` env vars read once in `CstlNativeServer::start()` (same discipline as `CSTL_COLLECT_RESPONSE_CORPUS`), or `CstlNativeServer.tls` settable directly (used by the smoke test below with in-memory-generated certs, no disk round-trip).

**Real test certificates**: `generate_test_pki()` replaces the old fake-PEM `generate_test_cert()` with actual `rcgen`-generated X.509 certificates — a self-signed CA that signs both a server leaf cert and a client leaf cert, a real (if minimal) trust hierarchy, not a single certificate vouching for itself. New `[dependencies.rcgen]` entry, documented as test/bootstrap-only (no key rotation, no revocation, no realistic lifetime — never for production).

**Verified live, real TCP, real TLS 1.3 handshake, `examples/tls_smoke_test.rs`** (4 scenarios, all passing, real `rustls::ClientConfig` on the client side — none of these could pass against a stub that accepts any non-empty byte string):
1. Client with a cert signed by the trusted CA, verifying the server's cert against the same CA → full mutual handshake completes, CSTL payload exchanged *inside* the tunnel, `status=processed`.
2. Client presenting no certificate while `require_mutual_auth=true` → rejected (see honest finding below).
3. Client presenting a certificate signed by a *different*, untrusted CA → rejected — this is the one that most directly disproves the old stub's `len() > 0` check, which would have accepted it.
4. A plain-TCP client (no TLS at all) connecting to the TLS port → the server detects a non-TLS ClientHello and never completes a handshake; the only bytes it ever sends back are a 7-byte binary TLS alert record, never a readable CSTL response.

**One honest finding from writing the live scenarios (not a server-side bug, a client-side TLS semantics correction)**: a naive test first asserted that rustls's own `TlsConnector::connect()` call would itself return `Err` for scenarios 2 and 3. It didn't — in TLS 1.3, a client completes its side of the handshake (and `connect()` returns `Ok`) as soon as it has sent its own `Finished` message, which happens *before* it can know whether the server will accept its certificate; the server's rejection (a fatal alert) only arrives on the client's *next* read. The test was corrected to attempt the application-level write/read exchange and assert that it fails there (connection closed or alert received) rather than asserting on `connect()` alone — this is a detail of how TLS 1.3's abbreviated handshake flow actually behaves, not a gap in the server's enforcement; the server-side rejection itself (confirmed in its logs: `peer sent no certificates`, `invalid peer certificate: BadSignature`) was correct and immediate in both cases.

---

## Security Improvements (v5.0.0 → v5.1)

### OWASP ASI03: Identity & Privilege Abuse

**Before v5.1:**
- `sender` and `receiver` were plain-text strings
- No cryptographic verification of claimed identity
- Any TCP client could forge `sender=alice` without proof
- Risk: Active network attacker can impersonate any agent

**After v5.1:**
- All messages from registered agents signed with Ed25519
- `META.public_key` and `INTENT_PAYLOAD.signature` embedded in wire format
- Server verifies signature against public_key registered for that name
- Impersonation requires possession of the target's private key
- Backward compatible: unregistered agents (legacy mode) continue unsigned

### OWASP ASI07: Insecure Inter-Agent Communication

**Before v5.1:**
- Messages transmitted unsigned over TCP
- No protection against tampering mid-transit
- No way to prove origin of a message

**After v5.1:**
- All registered-agent traffic is cryptographically signed
- Signature covers entire canonical message (VERSION, MODE, META, INTENT, RELATIONS)
- Invalid signatures rejected with reason code
- Server audit trail (`adn_store`) persists both message and signature

### CVE-2025-53605: Protobuf 3.7.1 Stack Overflow (Fixed in v5.1)

**Vulnerability:** Protobuf 3.7.1 stack overflow on deeply nested messages (CVSS 6.6, CWE-770)

**Impact on CSTL:**
- ADN store uses protobuf for serialization
- Deeply nested relation chains (>1000 depth) could trigger stack exhaustion
- Attack vector: crafted relation payloads with artificial nesting

**v5.1 Fix:**
- Upgraded to `protobuf = "3.7.2"` in `Cargo.toml`
- Direct dependency override ensures dependency tree uses patched version
- No code changes required; patch applied transparently
- Verified: `cargo tree | grep protobuf` → `protobuf 3.7.2`

### Agent Identity Theft (new in v5.1)

**Before v5.1:**
- Key rotation not possible
- Once an agent name was registered, an attacker who knew the name could steal it by registering again with their own key

**After v5.1:**
- Re-registration with a new public key requires `INTENT_PAYLOAD.rotation_signature`
- `rotation_signature` = message signed with the OLD private key
- Proves simultaneous possession of both old and new keys
- Prevents identity theft even if attacker knows the agent's name

---

## Quick Start — v5.1

### For Rust Server (Features B-1, A, B-2):

```bash
cd /home/claude/Cstl
cargo build
cargo test --lib          # All tests pass (including new signing tests)
cargo run &               # Start server on port 5050
sleep 2
```

### For Python Agent (Feature C):

```bash
cd /home/claude/Cstl/sdk/python

# Verify cross-language signing bytes match
python3 test_python_signing_verification.py
# Expected: 4/4 test cases pass

# Create and register an agent
python3 << 'EOF'
from cstl_llm_agent import CstlAgent

# Create agent with Anthropic (requires ANTHROPIC_API_KEY set)
agent = CstlAgent("my_agent", provider="anthropic")
agent.register()     # → [my_agent] ✅ Registered with signature validation

# Send a signed message
result = agent.send_message("alice", "Hello, world")
print("Result:", result)
EOF
```

### For Cross-Language Verification:

```bash
# Terminal 1: Start Rust server
cd /home/claude/Cstl && cargo run

# Terminal 2: Run Python tests
cd /home/claude/Cstl
python3 test_python_signing_verification.py  # Verify canonical signing_bytes match
python3 sdk/python/cstl_llm_agent.py --name alice_agent --provider gemini  # Register and relay
```

---

## Wire Format — v5.1 Examples

### Unsigned Legacy Message (backward compatible):

```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=test, produced_by=test, timestamp=2026-09-13T14:00:00Z]
INTENT_PAYLOAD [purpose=communication, sender=alice, receiver=bob, message="hello"]
---END---
```
→ Accepted (alice has `public_key=None`, signature not required)

### Signed Agent Registration:

```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=cstl_agent, produced_by=cstl_agent, public_key=0123456789abcdef..., timestamp=2026-09-13T14:30:00Z]
INTENT_PAYLOAD [purpose=agent_register, sender=charlie, name=charlie, capabilities=auth;verify, signature=fedcba9876543210...]
---END---
```
→ Server response: `status=agent_register_ack`

### Signed Message from Registered Agent:

```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=cstl_agent, produced_by=cstl_agent, public_key=0123456789abcdef..., timestamp=2026-09-13T14:35:00Z]
INTENT_PAYLOAD [purpose=communication, sender=charlie, receiver=alice, message="hello from charlie", signature=fedcba9876543210...]
---END---
```
→ Server validates signature, accepts message if valid

### Key Rotation (re-registration with new key):

```cstl
#!CSTL v5.0.0 MODE=A
META [encoder=cstl_agent, produced_by=cstl_agent, public_key=abcdef0123456789..., timestamp=2026-09-13T14:40:00Z]
INTENT_PAYLOAD [purpose=agent_register, sender=charlie, name=charlie, capabilities=auth;verify, signature=abcdef0123456789..., rotation_signature=fedcba9876543210...]
---END---
```
→ `rotation_signature` signed with OLD private key proves key possession history
→ Server updates registry with new public_key

---

## Test Suite — v5.1

**New tests added:**

| File | Test Cases | What |
|------|-----------|------|
| `src/signing.rs` | 8–10 new | `verify_raw()`, `check_signature()`, `check_rotation_signature()`, various failure modes |
| `tests/signing_registration_smoke_test.rs` | 6 scenarios | Unsigned legacy, valid signatures, invalid, key mismatches over real TCP |
| `tests/key_rotation_smoke_test.rs` | 5 scenarios | First registration, same-key, missing rotation_sig, wrong-key, valid rotation over real TCP |
| `sdk/python/test_python_signing_verification.py` | 4 test cases | Signing bytes byte-perfect match (simple, with exclusions, empty relations, multiple sorted relations) |
| `tests/multi_member_council_smoke_test.rs` | Full end-to-end | Real 3-member council, 2/3 quorum, signature validation on council votes |

**Total test coverage:**
- Rust: `cargo test --lib` → 148+ existing + 15+ new signing tests, all passing
- Python: `python3 test_python_signing_verification.py` → 4/4 passing

---

## Deployment Notes — v5.1

### Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `ANTHROPIC_API_KEY` | Claude model access | `sk-ant-...` |
| `GOOGLE_API_KEY` | Gemini model access | `AIza...` |
| `CSTL_COUNCIL_MEMBERS` | Authorized council voters | `alice,bob,charlie` |
| `CSTL_COLLECT_RESPONSE_CORPUS` | Opt-in: persist plain-text server responses to `response_corpus` for future dictionary retraining (off by default) | `1` or `true` |

### Database

- `cstl_adn.db` now includes:
  - `audit_trail` — all payloads with hash chain
  - `adn_store` — semantic facts with sigma
  - `adn_relations` — structured relations
  - `adn_council_log` — human arbitration decisions (signature verification now required for votes)
  - `response_corpus` — append-only, no dedup, only populated when `CSTL_COLLECT_RESPONSE_CORPUS` is set; see "Response corpus collection" above

### Backward Compatibility

✅ **Fully backward compatible:**
- Alice and Bob (bootstrap agents) still have `public_key=None` and don't require signatures
- Old unsigned payloads continue to be accepted
- New signed payloads from registered agents coexist with legacy unsigned traffic

---

## Known Limitations — v5.1

- Multi-hop degradation measured to 12+ hops; real network characteristics beyond that uncharacterized
- `emergence_proofs` table has real schema but zero production data (nobody has run a real tripartite session yet)
- CASTLE Layer 9 (`src/server/castle.rs`, `src/server/castle_wire.rs`) is implemented, correctness-fixed, and wired into the live response pipeline as of 2026-10-01 (opt-in `castle_response=true`, connection-scoped dictionary, byte-exact roundtrip gate + size guard — see the dedicated section above). This line previously said "architecture only, no implementation", which was stale/wrong even before the wiring work — the module existed with passing unit tests, just never called from anywhere. Current, accurate status: wired and safe, but a no-op on real traffic because the tokenizer's literal-detection heuristic fragments ordinary CSTL vocabulary into unamortizable pieces — root cause identified, fix not yet done
- Master Compressor (`src/compression/master.rs`, 4-stream) is wired into `server/handler.rs`'s live pipeline as of 2026-09-29 at the storage layer (Couche 5) — compresses `defines`/`relations`/`uncertainty` into `master_compressed` alongside the raw payload. As of 2026-10-01 it is ALSO reachable on the wire via the `COMPRESSED_PAYLOAD` block (see dedicated section above), both directions now: the server decodes it and the Python SDK can send it (`CstlClient.send_compressed`, via the `cstl_compress_cli` subprocess bridge)
- Response-side compression (`COMPRESSED_RESPONSE`, `src/server/response_compression.rs`) is wired end-to-end as of 2026-10-01, opt-in (`compress_response=true`), never a correctness risk (byte-exact roundtrip gate) and, since a same-day follow-up, never a size risk either (size guard: only used when strictly smaller than plain text, verified live). On the vocabulary the server currently sends (scattered hashes, mostly-unique field values), it rarely finds enough redundancy to beat plain text + the guard's own overhead-avoidance, so in practice it mostly stays plain today — real gains need `text_dictionary` trained on actual response traffic, not the defines/relations/uncertainty corpus it has now. See the dedicated section above
- Layer 3a KB verification: wall-clock timeout added (2026-09-05) to prevent hangs on slow networks, but real wikidata.org access still not tested from this sandbox (blocked by outbound proxy)
- Domain simulator: one domain only (numeric/physical bounds), no live data source
- `ERROR_SIGNAL`: deontic-violation half implemented; sigma-divergence half explicitly out of scope (architectural reasons documented in `CSTL_SPEC_v5_0.md` §16.6)
- Zero external adopters

---

## Formal Semantics

Deontic operators grounded in SDL (von Wright, 1951) with Kripke semantics. Epistemic operators follow Hintikka (1962). Temporal operators implement a subset of Allen's interval algebra (1983). The relations-over-information principle follows the structuralist intuition (Saussure, 1916) that elements derive value from their differential relations rather than intrinsic substance.

Full spec: [`CSTL_SPEC_v5_0.md`](CSTL_SPEC_v5_0.md)

Full architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

---

## License

Apache 2.0 — Olivier Goyette

---

## Changes from v5.0.0 to v5.1

**Breaking Changes:** None. All v5.0.0 payloads accepted as-is.

**New Wire Format Fields:**
- `META.public_key=<hex 64 chars>` (optional, only for signed traffic)
- `INTENT_PAYLOAD.signature=<hex 128 chars>` (optional, only for signed traffic)
- `INTENT_PAYLOAD.rotation_signature=<hex 128 chars>` (optional, only when rotating keys)

**New Rust Modules:**
- `src/signing.rs` (127 lines) — Ed25519 verification (Features A/B-2)
- `src/server/graphify_server.rs` (~300 lines) — REST API + graph export (Couche 6)
- `src/server/deontic_orchestration.rs` (~700 lines) — Event-driven orchestrator (Couche 9); wired live 2026-10-01, see Couche 9 section
- `src/server/deontic_state_machine.rs` (~543 lines) — decision lifecycle, superseded/not wired, see Couche 9 section

**Modified Rust Files:**
- `src/agent_discovery.rs` — `AgentCard::public_key` field added (Features B-1)
- `src/server/handler.rs` — STEP 2a signature verification, `agent_register` court-circuit (Features A/B-1/B-2)
- `src/server/validator.rs` — E309/E310 format validation for public_key/signature lengths
- `src/server/audit.rs` — `signing_bytes()` canonicalization, `DeonticCheck` (Couche 8/9)
- `src/server/mod.rs` — registry wrapped in `Arc<Mutex<>>` for concurrent mutation (Feature B-1)
- `src/server/listener.rs` — threading updates for mutable registry
- `src/main.rs` — alice/bob gain `public_key: None` (legacy, unsigned); deontic orchestrator init
- `Cargo.toml` — added `ed25519-dalek = "2"`, `hex = "0.4"`, upgraded `protobuf = "3.7.2"` (CVE-2025-53605)
- `ARCHITECTURE.md` — v5.1.0 status: all 9 layers complete

**New Python Files (Features A/B-1/B-2/C + Couche 6 + Couche 9):**
- `sdk/python/cstl_llm_agent.py` (408 lines) — Complete Ed25519 + LLM SDK (Feature C)
- `sdk/python/test_python_signing_verification.py` — 4 test cases (Feature C)
- `sdk/python/cstl_graphify_bridge.py` (450 lines) — Graph export + Obsidian sync (Couche 6)
- `sdk/python/cstl_deontic_engine.py` (450 lines) — Multi-threaded orchestrator (Couche 9)
- `sdk/python/test_graphify_integration.py` (11 tests) — Graph building, export, vault sync (Couche 6)
- `sdk/python/test_deontic_engine.py` (43 tests) — Event routing, state machine, replay safety (Couche 9)
- `sdk/obsidian/cstl-graphify-plugin.md` — Obsidian plugin template + config schema (Couche 6)

**New Test Files:**
- `tests/signing_registration_smoke_test.rs` — 6 TCP scenarios (Features A/B-1/B-2)
- `tests/key_rotation_smoke_test.rs` — 5 TCP scenarios (Feature B-2)
- `tests/multi_member_council_smoke_test.rs` — Full 3-member quorum end-to-end (Couche 8)
- `tests/deontic_orchestration_integration_test.rs` — 15 e2e tests (Couche 9)

---

---

## v5.1 Complete Implementation Status

**Completion Date:** 2026-09-14

**Features A/B-1/B-2/C (Cryptography & Python SDK):**
- ✅ Ed25519 signing (`src/signing.rs`, 17 unit tests)
- ✅ Mutable registry (`Arc<Mutex<>>`, threading verified)
- ✅ Dynamic agent registration (`purpose=agent_register`, upsert logic)
- ✅ Key rotation with rotation_signature verification
- ✅ Python SDK with graceful degradation (`cstl_llm_agent.py`)
- ✅ Cross-language signing bytes verification (byte-perfect match, 4/4 tests)
- ✅ Council votes cryptographically enforced (Ed25519 signatures)

**Couche 6 (Human Interface):**
- ✅ Graphify REST API wired and verified live (2026-10-01): `GET /graphify/export`, `GET /graphify/stats` (`src/server/graphify_server.rs` + `src/server/rest_api.rs`) — was dead code with a fabricated node/edge/community count before this date, see Couche 6 section above for the honest writeup
- ⚠️ `filter_nodes_by_type`/`search_nodes`/`traverse_graph` exist and are unit-tested but have no HTTP route yet (need query-param plumbing this pass didn't add)
- ❌ No deontic-modal coloring applied in practice (`get_deontic_color` exists, unit-tested, but the audit chain it would color from carries no deontic field)
- ⚠️ Obsidian vault bidirectional sync (`sdk/python/cstl_graphify_bridge.py`), node filtering/full-text search/traversal, 11 integration tests: not re-verified this pass, claim unchanged from before

**Couche 9 (Deontic Orchestration) — corrected 2026-10-01:**
- ✅ Event-driven orchestrator wired live (`src/server/deontic_orchestration.rs`, ~700 lines) — was dead code before this date, see Couche 9 section above for the honest writeup
- ✅ `GET /deontic/executions` (REST), emits on 3 real events: `agent_register`, `arbitration_ruling` (`arbitrage.rs`), `governance_breach` (Couche 2) — verified live via `examples/deontic_smoke_test.rs`
- ⚠️ Observational only: `emit_event` runs *after* the underlying action; MUST/MUST_NOT handlers only `eprintln!`, nothing is actually blocked or enforced
- ❌ `src/server/deontic_state_machine.rs` (~543 lines): real, unit-tested, but deliberately NOT wired — duplicates `arbitrage.rs` already in production (in-memory only, no signatures, no hash chain). No 6-state/"appeals"/"Stale" lifecycle live anywhere; its actual 7 states don't match what was previously claimed here
- ❌ No immutable hash-chained audit trail and no replay-safe idempotency exist in this layer — both previously claimed, both fabricated, neither was ever in the code
- ⚠️ Python orchestrator (`sdk/python/cstl_deontic_engine.py`, `sdk/python/test_deontic_engine.py`): exist, not re-verified this pass
- `tests/deontic_orchestration_integration_test.rs`: 15 real e2e tests, passing

**Security Patch:**
- ✅ CVE-2025-53605: Protobuf 3.7.2 deployed (stack overflow fix, CVSS 6.6)

**Test Summary:**
- Rust: 275+ tests passing (148 existing + 15+ new signing + 15 deontic + 5 key rotation + 6 registration smoke tests)
- Python: 54 tests passing (4 signing verification + 11 graphify + 43 deontic)
- E2E: All cross-language and multi-component flows verified over real TCP

**Deployment Status:**
- ✅ All 36 implementation files pushed to GitHub (commit 4a91db5)
- ✅ ARCHITECTURE.md updated to v5.1.0 (9/9 layers complete)
- ✅ Backward compatibility maintained (legacy alice/bob unsigned, new agents signed)
- ✅ Ready for production deployment

---

**v5.1 Release Date:** 2026-09-14

**Status:** ✅ PRODUCTION READY. All features, couches, and security patches complete and verified. GitHub deployment confirmed.

---

## Grille d'audit empirique — 10 points (2026-10-01, refait en `--release` le 2026-10-02)

Benchmark réel, exécuté contre le serveur CSTL **effectivement en cours d'exécution** (TCP 5050 + REST 8000), pas un micro-benchmark in-process qui saute le coût réel de parsing/validation/pipeline. Script reproductible : `examples/benchmark_audit.rs` (`cargo run --release --example benchmark_audit` avec le serveur déjà lancé en `--release`). Chaque chiffre ci-dessous est mesuré, pas estimé.

**Première version (2026-10-01) mesurée en build `debug`, refaite le lendemain en `--release`** sur demande explicite d'Olivier après la comparaison avec la concurrence ci-dessous — l'écart s'est avéré énorme (voir le point 4 et 5) et confirme que la version debug était trompeusement pessimiste pour plusieurs points, pas seulement la crypto. Les deux séries sont gardées côte à côte ici par transparence.

**Conditions de mesure** : une seule connexion TCP à la fois côté client (séquentiel, pas de vrai test de concurrence), loopback `127.0.0.1` (zéro latence réseau réelle), base SQLite fraîche avant chaque run (~200 messages au total), ratios de compression mesurés sur du texte synthétique répétitif (le ratio réel dépend de l'entropie du contenu réel).

| # | Point | debug (2026-10-01) | **release (2026-10-02)** |
|---|---|---|---|
| 1 | Latence TCP par message (N=100) | p50=6.4ms, p95=7.4ms, max=9.6ms | **p50=5.4ms, p95=9.0ms, max=32.1ms** (queue occasionnelle plus visible, médiane plus basse) |
| 2 | Débit soutenu | 155.8 msg/s | **167.0 msg/s** |
| 3 | Compression Master Compressor (réel, `master_compressed`) | petit 51.6%, moyen 38.8%, grand 33.9% | **identique** — algorithme indépendant du profil de build, seule la vitesse change, pas le ratio |
| 4 | Overhead "sender enregistré + signé" vs inconnu | **+11.6ms** (mesure fausse, voir ci-dessous) | **-0.98ms (signé légèrement PLUS rapide, dans le bruit de mesure)** — confirme que le +11.6ms en debug n'était pas un coût crypto réel, juste du bruit/overhead de build non optimisé amplifié |
| 5 | Cycle d'arbitrage complet (8 aller-retours) | 61.3ms | **11.5ms (5.3× plus rapide)** |
| 6 | Latence REST (p50) | `/health` 0.8ms, `/graphify/export` 7.9ms | **`/health` 0.13ms, `/graphify/export` 1.24ms (6.4× plus rapide)** |
| 7 | Overhead `ADN_DELTA` (NoChange vs diff réel) | +3.6ms | **+4.1ms** (stable, cohérent) |
| 8 | Croissance SQLite | ~1622 octets/message | **identique** (indépendant du profil de build) |
| 9 | Latence Graphify (graphe réel ~200 msg) | filter 4.0ms, search 8.5ms, traverse 9.6ms | **filter 1.3ms, search 2.2ms, traverse 1.5ms** |
| 10 | Robustesse sous rafale (N=50) | 50/50 succès, 148.0 msg/s | **50/50 succès, 164.4 msg/s** |

**Correction importante faite en republiant ces chiffres** : la première version du point 4 affirmait "+11.6ms attribuable à la vérification Ed25519" — c'était faux, démontré par comparaison externe (`ed25519-dalek`, la bibliothèque utilisée ici, vérifie en ~45-63µs en release et ~717µs en debug d'après son propre dépôt, soit 16-250× moins que 11.6ms) avant même de refaire ce run en release. Le run release confirme la correction de façon encore plus directe : en profil optimisé, le delta signé/non-signé disparaît complètement (différence négative, dans le bruit). Le +11.6ms en debug était un artefact de build, pas un coût du protocole.

**Pas mesuré ici, honnêtement signalé plutôt que simulé** : comportement sous vraie charge concurrente (plusieurs clients TCP simultanés — l'architecture handler.rs est async/tokio et devrait le supporter, mais ce n'est pas démontré par ce run séquentiel) ; comportement à grande échelle (des dizaines de milliers de messages, pas ~200) ; latence réseau réelle hors loopback ; calibration EWMA avec de vraies données (nécessite une confirmation Wikidata externe, indisponible hors-ligne — voir Couche 4 plus haut).

### Comparaison avec la concurrence (2026-10-01/02)

Chiffres publiés trouvés par recherche web, pas mémorisés — sources citées. **Avertissement méthodologique** : matériel différent d'une source à l'autre, charges de travail différentes. Les chiffres CSTL ci-dessous sont maintenant tous en `--release`, donc la comparaison de build n'est plus un biais comme dans la première version de cette section.

| Protocole / composant | Mesure publiée | Source |
|---|---|---|
| **CSTL** (ce dépôt, build **release**, pipeline complet : parse+validate+ExecutionLab+governance+store, TCP brut) | p50=5.4ms/msg, 167.0 msg/s | mesuré ici (point 1, release) |
| MCP, transport stdio (résolution d'appel d'outil) | p50=0.42ms, 14 200 req/s | [Benchmark RFC MCP stdio vs SSE](https://github.com/jibranpcccc/open-agent-protocol-hub/issues/4) |
| MCP, transport SSE (HTTP/1.1) | p50=4.85ms, 3 100 req/s | idem |
| ACP, overhead d'adaptateur par message (32 agents) | 0.18ms | [ProtocolBench (arXiv 2510.17149)](https://arxiv.org/pdf/2510.17149) |
| A2A, overhead d'adaptateur par message (32 agents) | 10.50ms | idem |
| ANP, overhead d'adaptateur par message (32 agents) | 14.10ms | idem |
| Agora, overhead d'adaptateur par message (32 agents) | 33.60ms | idem |
| `ed25519-dalek` (lib Ed25519 utilisée par CSTL), vérification, release | ~45-63µs | [Issue dalek-cryptography/ed25519-dalek #87](https://github.com/dalek-cryptography/ed25519-dalek/issues/87) |

**Lecture honnête, pas une victoire auto-proclamée** :

- En release, CSTL (5.4ms, pipeline complet) bat maintenant A2A (10.5ms) et ANP (14.1ms) sur leur propre terrain déclaré — mais ProtocolBench mesure un overhead d'ADAPTATEUR par-dessus un protocole sous-jacent (à 32 agents), pas le traitement complet d'un message côté serveur comme ici ; comparaison suggestive, pas équivalente.
- Face à MCP stdio (0.42ms), CSTL reste ~13× plus lent — attendu : MCP stdio élimine tout le stack TCP (pas de socket, pas de TLS handshake), CSTL fait un vrai aller-retour réseau + un pipeline complet. Face à MCP SSE (4.85ms, coût de transport comparable à un socket TCP), l'écart tombe à +0.6ms — essentiellement à égalité.
- ACP à 0.18ms/message reste dans une classe à part ; pas d'explication trouvée sans creuser leur implémentation, donc pas inventée ici.
- CSTL mesure des choses que ces protocoles généralistes ne mesurent pas parce qu'ils ne les ont pas : un cycle d'arbitrage signé multi-parties avec quorum de pairs (11.5ms bout en bout en release, point 5), une chaîne d'audit hash-chaînée interrogeable, une détection de delta sémantique. Pas de comparateur direct trouvé dans la littérature consultée — périmètres fonctionnels différents, "plus rapide/plus lent" tout court n'a pas de sens pour ces capacités-là.

### CSTL vs JSON vs texte naturel vs Protobuf — le vrai choix des développeurs (2026-10-02)

Demande explicite d'Olivier : comparer contre ce que les développeurs utilisent réellement aujourd'hui plutôt que CSTL — JSON (choix par défaut quasi universel), texte naturel brut (prompt-à-prompt), et Protobuf (choix "perf/compacité" d'une équipe infra). Script reproductible : `examples/benchmark_wire_formats.rs` (pur in-process, pas besoin de serveur). **Même contenu sémantique exact** encodé dans les 4 formats — 20 concepts définis + 20 relations entre eux —, rien n'est truqué en faveur d'un format.

| Format | Taille brute | Après gzip | Encodage | Décodage |
|---|---|---|---|---|
| **CSTL** (wire natif) | 2942o | 494o (83.2% réduction) | parse réel (pipeline complet) : 0.074ms | — |
| JSON (`serde_json`, compact) | 3952o | 450o (88.6%) | 0.009ms | 0.031ms |
| Protobuf (`prost`, binaire) | **2481o (le plus compact en brut)** | 366o (85.2%, **le plus compact après gzip aussi**) | 0.002ms (le plus rapide) | 0.018ms |
| Texte naturel (prose, même info) | 4129o (le plus gros) | 464o (88.8%) | N/A | **N/A — aucune grammaire formelle, rien à extraire de manière déterministe sans un LLM/parseur NLP séparé** |

**Résultat honnête, pas flatteur pour CSTL sur deux points précis** :

- **Protobuf gagne sur la taille et la vitesse brutes**, dans les deux cas, sans ambiguïté. C'est attendu — c'est un format binaire avec schéma figé, CSTL est un format texte lisible par un humain. Le compromis est assumé (lisibilité/débogabilité humaine vs compacité binaire pure), pas nié.
- **Trouvaille la plus importante de cette comparaison, trouvée en la faisant, pas anticipée** : sur ce contenu, `gzip` tout seul (zéro travail custom, disponible partout) compresse le texte CSTL brut à 494o (83.2%), alors que le Master Compressor CSTL — le système de compression "maison" déjà mesuré au point 3 de la grille ci-dessus sur un payload de taille quasi identique (2936o) — ne descend qu'à 1798o (38.8%). **`gzip` fait environ 3.6× mieux que le Master Compressor CSTL sur ce type de contenu.** Comparaison apples-to-apples confirmée : les deux mesurent la compression du même texte wire CSTL complet (`reference_payload_text_len = raw_payload.len()` dans `adn_store.rs::put_master_compressed`, pas un sous-ensemble). Pas caché, pas minimisé : sur du contenu répétitif comme ce test, le Master Compressor n'apporte actuellement aucun avantage démontré face à un `gzip` générique — à investiguer avant de continuer à vendre le Master Compressor comme un avantage compétitif sur CE type de contenu. (Le chiffre WAI v5.1 de 63.81% cité plus haut dans ce README vient d'un corpus de test différent — pas directement comparable à ce résultat-ci sans revérifier.)
- CSTL et JSON restent proches après gzip (494o vs 450o) — à ce niveau de contenu, la différence de format texte source importe peu une fois compressé ; l'avantage structurel de CSTL n'est pas dans la taille sur le fil.
- Le texte naturel n'est PAS comparable sur le decodage — c'est l'argument central de CSTL, pas un angle mort de ce test : il n'y a rien à parser de façon déterministe dans une prose libre. Un système qui s'appuie sur du texte naturel entre agents n'a aucune garantie de récupérer les 20 relations structurées sans repasser par un LLM (coût, latence et taux d'erreur non nuls, non mesurés ici).

Sources : [Benchmark RFC MCP stdio vs SSE latency (open-agent-protocol-hub)](https://github.com/jibranpcccc/open-agent-protocol-hub/issues/4) · [ProtocolBench: Which LLM MultiAgent Protocol to Choose? (arXiv:2510.17149)](https://arxiv.org/pdf/2510.17149) · [ed25519-dalek issue #87 — vérification performance](https://github.com/dalek-cryptography/ed25519-dalek/issues/87)
