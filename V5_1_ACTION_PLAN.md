# CSTL v5.1 — Plan d'Action Détaillé

## Status: PLANIFICATION PRÉ-IMPLÉMENTATION

**Version actuelle:** v5.0.0 (production-ready pour réseaux internes)  
**Timeline v5.1:** Q4 2026 (estimation)  
**Priorité:** Critique (sécurité + stabilité)

---

## 1. FIXE IMMÉDIATE (Avant v5.1) — 1-2 heures

### 1.1 Update Model Names en `sdk/python/cstl_llm_agent.py`

**Fichier:** `sdk/python/cstl_llm_agent.py`

**Changements requis:**

```python
# Line ~185 (AnthropicAgentBrain)
- model="claude-3-5-sonnet-20241022",
+ model="claude-3-5-sonnet-20250515",  # Updated 2026-09 available model

# Line ~204 (GeminiAgentBrain)  
- self.model = genai.GenerativeModel("gemini-pro")
+ self.model = genai.GenerativeModel("gemini-1.5-pro")  # gemini-pro deprecated
```

**Verification post-fix:**
```bash
cd /home/claude/Cstl
python3 sdk/python/cstl_llm_agent.py --name alice --provider anthropic --turns 1
# Expected: ✅ Anthropic response generated (no 404)

python3 sdk/python/cstl_llm_agent.py --name charlie --provider gemini --turns 1
# Expected: ✅ Gemini response generated (no 404)
```

**Impact:** Immediate (agents will respond properly after fix)

---

## 2. LAYER 6 — TLS 1.3 Mutual Authentication (2-3 weeks)

### 2.1 Architecture Design

**Goal:** Mutual TLS authentication between all agents + server

**Changes:**

- Add `tokio-rustls` dependency to `Cargo.toml`
- Modify `src/server/listener.rs`:
  - Replace raw TCP listener with TLS listener
  - Load server certificate + private key from `~/.cstl/server_*.pem`
  - Configure client certificate validation (require all agents present valid cert)

- Modify agent communication in `sdk/python/cstl_llm_agent.py`:
  - Use `ssl.SSLContext` to load agent certificate + key
  - Connect to `tls://127.0.0.1:5051` instead of TCP `127.0.0.1:5050`

**Certificate Generation (local dev):**
```bash
# Generate self-signed certs for testing
openssl genrsa -out ~/.cstl/server_key.pem 2048
openssl req -new -x509 -key ~/.cstl/server_key.pem -out ~/.cstl/server_cert.pem -days 365

# Agent certificates  
openssl genrsa -out ~/.cstl/agent_alice_key.pem 2048
openssl req -new -x509 -key ~/.cstl/agent_alice_key.pem -out ~/.cstl/agent_alice_cert.pem -days 365
# Repeat for bob, charlie
```

**Test Plan:**
- ✅ TLS handshake succeeds (mutual auth verified)
- ✅ Unsigned payloads still accepted (backward compatibility)
- ✅ Signature verification works over TLS (STEP 2a unchanged)
- ✅ All 260 unit tests pass

**Risk:** Medium (async complexity, TLS config errors)  
**Effort:** 80-120 hours

---

## 3. LAYER 8 — AES-256-GCM Message Encryption (1-2 weeks)

### 3.1 Architecture

**Goal:** Application-level encryption (orthogonal to TLS, defense-in-depth)

**Design:**
- Each message encrypted with AES-256-GCM after serialization
- Encryption key derived from shared agent/server secret (TLS session key)
- Nonce: timestamp (Unix ms) + random 64-bit suffix
- Included in INTENT_PAYLOAD: `encryption_nonce`, `ciphertext` (base64)

**Format v5.1:**
```
#!CSTL v5.0.0 MODE=A
META [sender=alice, receiver=bob, public_key=abc123...]
INTENT_PAYLOAD [
  purpose=communication,
  message_ciphertext=<base64 AES-256-GCM ciphertext>,
  encryption_nonce=<hex timestamp+random>,
  signature=<Ed25519 signature of message before encryption>
]
```

**Implementation:**
- Add `aes-gcm` crate to `Cargo.toml`
- New function: `encrypt_intent(plaintext, key, nonce) -> (ciphertext, nonce)`
- Modify handler STEP 2 to decrypt message before validation

**Test Plan:**
- ✅ Encryption round-trip (plaintext → ciphertext → plaintext)
- ✅ Signature verification on decrypted payload
- ✅ Nonce uniqueness (prevent replay with same ciphertext)
- ✅ Backward compatibility (unencrypted messages still accepted v5.0.1)

**Risk:** Low (crypto crate well-audited)  
**Effort:** 40-60 hours

---

## 4. Rate Limiting + Proof-of-Work (1-2 weeks)

### 4.1 Per-IP Rate Limiting

**Implementation in `src/server/handler.rs`:**

```rust
struct RateLimitState {
    by_ip: HashMap<IpAddr, RateBucket>,
    by_agent: HashMap<String, RateBucket>,
    by_purpose: HashMap<(String, String), RateBucket>, // (sender, purpose)
}

struct RateBucket {
    count: u32,
    window_start: Instant,
    limit: u32,
    window_duration: Duration,
}

fn check_rate_limit(state: &RateLimitState, ip: IpAddr, agent: &str, purpose: &str) -> Result<(), RateLimitError> {
    // Limits:
    // - Per IP: 1000 msgs/min
    // - Per agent: 500 msgs/min  
    // - Per (agent, purpose=agent_register): 1 per hour
    // - Per (agent, purpose=governance_vote): 100 per day
}
```

### 4.2 Proof-of-Work on agent_register

**Design:**
- New field in INTENT_PAYLOAD: `pow_nonce` (32-bit unsigned)
- Requirement: `SHA-256(canonical_bytes + pow_nonce)` must have leading 24 zero bits
- Client tries nonces until PoW satisfied (expected ~16M attempts, ~100ms on modern CPU)
- Server verifies PoW before processing registration

**Implementation:**
```rust
fn verify_pow(payload: &CstlPayload, difficulty: u32) -> bool {
    let msg = signing_bytes(payload); // Without pow_nonce
    let nonce = payload.intent.get("pow_nonce").unwrap_or("0").parse::<u32>()?;
    let hash = sha256(&format!("{}{}", String::from_utf8(msg), nonce));
    let leading_zeros = hash.leading_zeros();
    leading_zeros >= difficulty // difficulty=24 for v5.1
}
```

**Test Plan:**
- ✅ PoW generation completes in <500ms
- ✅ PoW verification fast (<1ms)
- ✅ agent_register rejected without PoW
- ✅ Legitimate messages (non-register) unaffected by rate limits

**Risk:** Low  
**Effort:** 20-30 hours

---

## 5. Encrypted Key Storage (1 week)

### 5.1 Store Agent Keys Encrypted at Rest

**Current state (v5.0.0):**
```
~/.cstl/agent_alice.key → 32-byte Ed25519 private key in plaintext
Risk: File theft = key compromise (100% loss)
```

**New state (v5.1):**
```
~/.cstl/agent_alice.key → AES-256-GCM encrypted
Requirement: User password (or hardware security module)
Risk: File theft + password needed to compromise (mitigation)
```

**Implementation:**
- Add `libsodium` bindings (or `argon2` + `aes-gcm` for KDF)
- Modify `load_or_create_keypair()`:
  - If key file encrypted: prompt for password
  - Decrypt with Argon2(password) as KDF key
  - Return plaintext key to process (in RAM)

**Password prompt (CLI):**
```bash
$ cargo run --release
[alice] Enter password to unlock agent key: ████
[alice] ✅ Key loaded and decrypted
```

**Test Plan:**
- ✅ Encrypted key loads with correct password
- ✅ Encrypted key rejected with wrong password
- ✅ Legacy plaintext keys auto-migrated on first load
- ✅ No performance impact (decryption happens once at startup)

**Risk:** Low (password UX, but crypto standard)  
**Effort:** 15-25 hours

---

## 6. Database Checkpointing + Pruning (2 weeks)

### 6.1 Problem Statement

**Current v5.0.0:**
- ADN Store (SQLite) append-only, no pruning
- After 1M messages: ~100GB database (unacceptable)
- No backup/recovery strategy

**Solution v5.1:**
- Snapshot checkpoints every 100k messages
- Keep last 1M messages in live DB
- Archive older messages to compressed snapshots
- Replay from checkpoint + delta logs on restart

### 6.2 Implementation

**Schema changes:**
```sql
-- New table: checkpoints
CREATE TABLE checkpoints (
    id INTEGER PRIMARY KEY,
    checkpoint_number INTEGER,
    message_count INTEGER,
    timestamp TIMESTAMP,
    merkle_root BLOB,  -- Root hash of messages in checkpoint
    archive_path TEXT  -- Path to /var/lib/cstl/archive/checkpoint_*.tar.gz
);

-- Modified: audit_entries (add index for pruning)
CREATE INDEX idx_audit_entries_seq ON audit_entries(sequence_num);
```

**Pruning logic:**
```rust
fn maybe_checkpoint(adn_store: &mut AdnStore) {
    if adn_store.total_messages() % 100_000 == 0 {
        let checkpoint = create_checkpoint(&adn_store);  // Merkle tree
        archive_messages(&adn_store, checkpoint.message_count - 1_000_000, checkpoint.message_count);
        delete_archived_messages(&adn_store);  // Keep last 1M only
        adn_store.insert_checkpoint(checkpoint);
    }
}
```

**Recovery on startup:**
```rust
fn replay_from_checkpoint() {
    let latest = adn_store.latest_checkpoint();
    if latest.exists() {
        load_checkpoint_messages(latest);  // Restore hash-chain from checkpoint merkle root
        load_delta_messages(latest.message_count..);  // Load messages since checkpoint
    } else {
        replay_from_genesis();  // Fallback: full replay (slow but correct)
    }
}
```

**Test Plan:**
- ✅ Checkpoint creation and archiving works
- ✅ Replay from checkpoint recovers full state
- ✅ Database size bounded after pruning
- ✅ Hash-chain integrity preserved across checkpoint boundary

**Risk:** Medium (data loss risk if checkpoint corrupted)  
**Effort:** 60-90 hours

---

## 7. Test & Integration (1-2 weeks)

### 7.1 End-to-End Test Suite for v5.1

```bash
# TLS connectivity
cargo test --release -- tests/tls_handshake
cargo test --release -- tests/mutual_auth

# Encryption
cargo test --release -- tests/aes256gcm_roundtrip
cargo test --release -- tests/signature_over_encrypted

# Rate limiting
cargo test --release -- tests/rate_limit_per_ip
cargo test --release -- tests/pow_generation_verification

# Database checkpointing
cargo test --release -- tests/checkpoint_creation
cargo test --release -- tests/recovery_from_checkpoint

# All existing 260 tests still pass
cargo test --release -- --test-threads=1
```

### 7.2 Integration Test (Alice ↔ Bob over v5.1)

```python
# Test: alice sends message to bob over TLS+encryption+signatures
python3 integration_test_v5_1.py
  1. Start CSTL server with TLS + encryption
  2. alice: generate PoW, sign, encrypt message
  3. Transmit over TLS
  4. bob: receive, decrypt, verify signature
  5. bob: send response (sign, encrypt)
  6. alice: receive, decrypt, verify
  7. Assert: 100% message integrity preserved
```

---

## 8. Documentation Updates

### 8.1 README.md Updates

- Add TLS setup instructions (certificate generation)
- Add password management section
- Update threat model (TLS mitigates Man-in-the-Middle)
- Add performance benchmarks (v5.1 vs v5.0.0)

### 8.2 ARCHITECTURE.md Updates

- Document Layer 6 TLS + Layer 8 AES-256-GCM
- Add threat model STRIDE analysis (post-TLS)
- Update deployment scenarios (now supports public internet with v5.1)

### 8.3 Security Audit Report

- Generate OWASP Top 10 Agentic 2026 compliance matrix
- Document residual risks
- Recommend external audit timeline

---

## 9. Release Checklist for v5.1

- [ ] All 260 existing unit tests pass
- [ ] All new v5.1 tests pass (50+ new tests expected)
- [ ] Security audit (internal + external)
- [ ] Performance benchmarks (throughput, latency)
- [ ] Documentation complete (README, ARCHITECTURE, Security)
- [ ] Git tag: `v5.1.0`
- [ ] Build macOS release binary (arm64)
- [ ] Build Linux release binary (x86_64)
- [ ] Publish to GitHub releases
- [ ] Update project documentation in claude/

---

## 10. Estimation Summary

| Component | Hours | Risk | Priority |
|-----------|-------|------|----------|
| API model fixes (immediate) | 1 | Low | 🔴 URGENT |
| Layer 6 TLS | 100 | Medium | 🔴 CRITICAL |
| Layer 8 AES-256-GCM | 50 | Low | 🟠 HIGH |
| Rate Limiting + PoW | 25 | Low | 🟠 HIGH |
| Key Encryption | 20 | Low | 🟡 MEDIUM |
| DB Checkpointing | 75 | Medium | 🟡 MEDIUM |
| Testing | 40 | Low | 🔴 CRITICAL |
| Documentation | 20 | Low | 🟡 MEDIUM |
| **TOTAL** | **331** | | |

**Calendar estimate:** 6-8 weeks (parallel work on Rust infrastructure + Python SDK)

---

## 11. Next Steps (Immediately After v5.1)

### v5.2 (Q1 2027)
- HashMap registry (O(1) instead O(n))
- Async/Tokio migration (handle 10k concurrent)
- Formal Byzantine proof (Coq/Isabelle)
- Leader election fallback (if alice crashes)

### v5.3 (Q3 2027)
- Post-quantum: Ed25519 + Dilithium
- Post-quantum KDF: Argon2 + ML-KEM

### v6.0 (2028)
- Distributed consensus (HotStuff or Tendermint)
- Remove single-server dependency (SPoF)

---

**Status:** Document achevé pour planification v5.1  
**Next action:** Commencer par fix immédiate des model names, puis Layer 6 (TLS)  
**Review:** Avant d'engager ressources, faire revue sécurité externe des plans v5.1
