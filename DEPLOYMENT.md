# CSTL v5.0.0 — Production Deployment Guide

**Complete Operational & Configuration Reference**

## Table of Contents

1. [Deployment Architecture](#deployment-architecture)
2. [Pre-Deployment Checklist](#pre-deployment-checklist)
3. [Installation & Setup](#installation--setup)
4. [Configuration Management](#configuration-management)
5. [Running the CSTL Server](#running-the-cstl-server)
6. [Deploying Python LLM Agents](#deploying-python-llm-agents)
7. [Network Security & Access Control](#network-security--access-control)
8. [Monitoring & Observability](#monitoring--observability)
9. [Operational Procedures](#operational-procedures)
10. [Scaling & Performance Tuning](#scaling--performance-tuning)
11. [Troubleshooting Guide](#troubleshooting-guide)
12. [Disaster Recovery](#disaster-recovery)

---

## Deployment Architecture

### Target Environment

**Recommended Context:**
- Internal corporate network (VPN/IPsec-wrapped)
- Byzantine-resilient multi-agent systems
- Deterministic LLM orchestration with cryptographic signing
- 3-100 agents per deployment

**NOT Recommended For:**
- Public internet (add TLS 1.3 mutual auth from v5.1)
- Sensitive data (add encryption from v5.1)
- Unlimited-sequence payloads (add FSE from v5.1)

### Deployment Topology

```
┌─────────────────────────────────────────────────────────────┐
│                    Corporate Internal Network               │
│                      (VPN/IPsec Protected)                  │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │         CSTL Server (Rust Binary)                     │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  Listener (TCP:9000)                            │  │  │
│  │  │  ├─ Accept connections                          │  │  │
│  │  │  ├─ Dispatch to handler                         │  │  │
│  │  │  └─ Maintain connection pool                    │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  Handler (Request/Response)                     │  │  │
│  │  │  ├─ Parser: Wire format → internal               │  │  │
│  │  │  ├─ Validator: Semantic checks                   │  │  │
│  │  │  ├─ Signing (Feature A): Verify Ed25519         │  │  │
│  │  │  ├─ Registry (Feature B-1): Agent lookup        │  │  │
│  │  │  ├─ Register (Feature B-2): Dynamic enrollment  │  │  │
│  │  │  └─ Response: Internal → Wire format             │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  Shared State                                   │  │  │
│  │  │  ├─ Arc<Mutex<AgentRegistry>>                  │  │  │
│  │  │  ├─ Arc<Mutex<AuditLog>>                       │  │  │
│  │  │  └─ Arc<Mutex<GovernanceState>>                │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
│           ▲ TCP:9000              ▲ TCP:9000              │
│           │                       │                        │
│  ┌────────┴──────────┐  ┌────────┴──────────────┐        │
│  │  Agent A (Python) │  │  Agent B (Python)     │        │
│  │  ┌──────────────┐ │  │ ┌──────────────────┐ │        │
│  │  │ LLMProvider  │ │  │ │ LLMProvider      │ │        │
│  │  │ (Anthropic)  │ │  │ │ (Ollama Hermes)  │ │        │
│  │  └──────────────┘ │  │ └──────────────────┘ │        │
│  │ send_message()   │  │ send_message()       │        │
│  │ register()       │  │ register()           │        │
│  └────────┬─────────┘  └────────┬──────────────┘        │
│           │                     │                        │
│  (Optional: External LLM APIs)                           │
│  ┌────────┴─────────────────────────┐                    │
│  │ Anthropic API / Ollama / Gemini  │                    │
│  └────────────────────────────────────┘                    │
└─────────────────────────────────────────────────────────────┘
```

---

## Pre-Deployment Checklist

### Code Quality Verification

- [ ] Clone repo: `git clone https://github.com/oliviergoyette30-commits/Cstl.git`
- [ ] Build: `cargo build --release`
- [ ] All tests pass: `cargo test --lib` (285/285) ✅
- [ ] No compilation errors (only 18 non-blocking warnings)
- [ ] Review Cargo.toml dependencies
  - [ ] `ed25519-dalek = "2"` ✅ (RFC 8032)
  - [ ] `hex = "0.4"` ✅ (hex encoding)
  - [ ] `tokio = "1"` ✅ (async runtime)
  - [ ] `serde = "1"` ✅ (serialization)

### Security Audit

- [ ] Ed25519 keypair generation tested (sdk/python/cstl_signing.py)
- [ ] Cross-platform signing_bytes validation (Rust ↔ Python roundtrip)
- [ ] Signature verification working end-to-end
- [ ] Agent registration protocol tested with council voting
- [ ] No hardcoded API keys in source code
- [ ] Review ARCHITECTURE.md for threat model coverage

### Infrastructure Readiness

- [ ] Internal network available (VPN/IPsec)
- [ ] TCP port 9000 available (configurable, see Configuration section)
- [ ] Rust toolchain installed (1.70+)
- [ ] Python 3.8+ installed
- [ ] Optional: Ollama running (if using Hermes LLM)
- [ ] File system: ~1GB disk for logs + audit trail

### Documentation Review

- [ ] README.md reviewed (overview, quick start)
- [ ] ARCHITECTURE.md reviewed (design + features)
- [ ] LIMITATIONS.md reviewed (acceptable constraints)
- [ ] Known issues acknowledged (ANS 5-10 symbol limit, no TLS v5.1, no encryption v5.1)

---

## Installation & Setup

### Step 1: Clone Repository

```bash
git clone https://github.com/oliviergoyette30-commits/Cstl.git
cd Cstl
git checkout main
```

### Step 2: Verify Rust Installation

```bash
rustc --version  # Should be 1.70+
cargo --version
```

If not installed, use rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### Step 3: Build Release Binary

```bash
cargo build --release
# Output: target/release/cstl_server
```

**Size:** ~15-20 MB

**Duration:** ~60 seconds (cold build)

### Step 4: Run Tests Locally

```bash
cargo test --lib --release
# Expected: test result: ok. 285 passed; 0 failed
```

### Step 5: Setup Python SDK

```bash
cd sdk/python

# Create virtual environment (recommended)
python3 -m venv venv
source venv/bin/activate

# Install cryptography library
pip install cryptography

# Optional: Install LLM providers
pip install anthropic        # For Anthropic Claude
pip install google-generativeai  # For Google Gemini
pip install ollama           # For Ollama client
```

### Step 6: Verify Python SDK

```bash
python3 cstl_signing.py
# Expected: All 7 tests passed!
```

---

## Configuration Management

### Rust Server Configuration

#### Option A: Environment Variables

```bash
# Network
export CSTL_PORT=9000              # Server port (default 9000)
export CSTL_HOST=127.0.0.1          # Bind address (default 127.0.0.1)
export CSTL_BACKLOG=128             # TCP backlog (default 128)

# Logging
export CSTL_LOG_LEVEL=info          # debug | info | warn | error
export CSTL_LOG_FILE=/var/log/cstl.log  # Optional

# Governance
export CSTL_QUORUM_SIZE=3           # Min voters for approval
export CSTL_QUORUM_THRESHOLD=2      # Min approvals (2/3)

# Audit
export CSTL_AUDIT_PATH=/var/lib/cstl/audit.db
export CSTL_AUDIT_RETENTION_DAYS=90

# Agent Registry
export CSTL_AGENT_SEED_FILE=/etc/cstl/agents.json
```

#### Option B: Configuration File (src/config.toml)

```toml
[server]
port = 9000
host = "127.0.0.1"
backlog = 128

[logging]
level = "info"
file = "/var/log/cstl.log"
format = "json"

[governance]
quorum_size = 3
quorum_threshold = 2
voting_timeout_secs = 30

[audit]
path = "/var/lib/cstl/audit.db"
retention_days = 90

[registry]
seed_file = "/etc/cstl/agents.json"
max_agents = 100
```

**Usage in code:**
```rust
let config = Config::from_env()
    .or_else(|_| Config::from_file("src/config.toml"))?;
```

### Python Agent Configuration

#### Option A: Environment Variables

```bash
# Agent Identity
export CSTL_AGENT_NAME=alice
export CSTL_AGENT_KEYPAIR=/home/alice/.cstl/keypair.json

# Server Connection
export CSTL_SERVER_HOST=localhost
export CSTL_SERVER_PORT=9000

# LLM Provider Selection
export CSTL_LLM_PROVIDER=anthropic  # anthropic | ollama | gemini
export CSTL_LLM_MODEL=claude-3-5-sonnet-20250515

# API Keys (keep confidential!)
export ANTHROPIC_API_KEY=sk-...
export GOOGLE_API_KEY=...
export OLLAMA_HOST=http://localhost:11434

# Agent Behavior
export CSTL_CAPABILITIES=reasoning;planning;verification
export CSTL_TRUST_SCORE=0.8
```

#### Option B: YAML Config

```yaml
# ~/.cstl/agent.yaml
agent:
  name: alice
  keypair_path: /home/alice/.cstl/keypair.json

server:
  host: localhost
  port: 9000

llm:
  provider: anthropic
  model: claude-3-5-sonnet-20250515
  api_key_env: ANTHROPIC_API_KEY

capabilities:
  - reasoning
  - planning
  - verification

trust_score: 0.8
```

**Usage:**
```python
config = Config.from_yaml('/home/alice/.cstl/agent.yaml')
agent = CstlAgent.from_config(config)
```

---

## Running the CSTL Server

### Method 1: Direct Binary Execution

```bash
# Navigate to project root
cd Cstl

# Run server
./target/release/cstl_server

# Output:
# CSTL Server v5.0.0 starting...
# Listening on 127.0.0.1:9000
# Registry loaded with 2 seed agents (alice, bob)
# Ready for connections.
```

**Foreground (Development):**
```bash
./target/release/cstl_server --log-level debug
```

**Background (Production):**
```bash
nohup ./target/release/cstl_server > /var/log/cstl.log 2>&1 &
echo $! > /var/run/cstl.pid
```

### Method 2: Systemd Service (Recommended for Production)

#### Create service file: `/etc/systemd/system/cstl.service`

```ini
[Unit]
Description=CSTL Server v5.0.0
After=network.target
Documentation=https://github.com/oliviergoyette30-commits/Cstl

[Service]
Type=simple
User=cstl
Group=cstl
WorkingDirectory=/opt/cstl

# Environment variables
Environment="CSTL_PORT=9000"
Environment="CSTL_LOG_LEVEL=info"
Environment="CSTL_AUDIT_PATH=/var/lib/cstl/audit.db"

# Start command
ExecStart=/opt/cstl/target/release/cstl_server

# Restart policy
Restart=on-failure
RestartSec=10
StartLimitInterval=60s
StartLimitBurst=3

# Process management
KillMode=process
KillSignal=SIGTERM

# Resource limits
LimitNOFILE=65535
LimitNPROC=4096

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=cstl

[Install]
WantedBy=multi-user.target
```

#### Enable and start service:

```bash
# Copy binary
sudo cp target/release/cstl_server /opt/cstl/

# Create cstl user
sudo useradd --system --home /var/lib/cstl --shell /bin/false cstl

# Create directories
sudo mkdir -p /var/lib/cstl
sudo chown -R cstl:cstl /var/lib/cstl

# Enable service
sudo systemctl daemon-reload
sudo systemctl enable cstl.service
sudo systemctl start cstl.service

# Verify
sudo systemctl status cstl.service
sudo journalctl -u cstl.service -f  # Follow logs
```

### Method 3: Docker Container (Optional)

#### Dockerfile

```dockerfile
FROM rust:1.75 as builder
WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/cstl_server /usr/local/bin/

# Create cstl user
RUN useradd --system --home /var/lib/cstl --shell /bin/false cstl

# Setup working directory
WORKDIR /var/lib/cstl
RUN chown -R cstl:cstl /var/lib/cstl

USER cstl
EXPOSE 9000

ENTRYPOINT ["cstl_server"]
```

#### Build and run:

```bash
docker build -t cstl:v5.0.0 .
docker run -d \
  -p 9000:9000 \
  -e CSTL_LOG_LEVEL=info \
  -v /var/lib/cstl:/var/lib/cstl \
  --name cstl-server \
  cstl:v5.0.0
```

---

## Deploying Python LLM Agents

### Agent 1: Local Ollama (Hermes)

**Prerequisites:**
```bash
# Install Ollama
curl https://ollama.ai/install.sh | sh

# Download model
ollama pull hermes3:8b

# Verify
ollama list
# hermes3:8b  4.1 GB
```

**Deploy Agent:**

```bash
cd sdk/python
source venv/bin/activate

python3 cstl_llm_agent.py \
  --name alice \
  --provider ollama \
  --server-host localhost \
  --server-port 9000 \
  --capabilities reasoning planning
```

**Output:**
```
Agent: alice
Provider: Ollama (hermes3:8b)
Server: localhost:9000
Registering... ✓ Registered as alice
Ready for messages.
```

### Agent 2: Anthropic Claude

**Prerequisites:**
```bash
# Get API key
export ANTHROPIC_API_KEY=sk-... # From claude.ai dashboard

# Verify
pip install anthropic
python3 -c "import anthropic; print('OK')"
```

**Deploy Agent:**

```bash
export ANTHROPIC_API_KEY=sk-...

python3 cstl_llm_agent.py \
  --name bob \
  --provider anthropic \
  --server-host localhost \
  --server-port 9000 \
  --capabilities reasoning verification
```

**Output:**
```
Agent: bob
Provider: Anthropic (claude-3-5-sonnet-20250515)
Server: localhost:9000
Registering... ✓ Registered as bob
Ready for messages.
```

### Agent 3: Google Gemini

**Prerequisites:**
```bash
# Get API key from Google AI Studio
export GOOGLE_API_KEY=... # From https://ai.google.dev

# Verify
pip install google-generativeai
python3 -c "import google.generativeai; print('OK')"
```

**Deploy Agent:**

```bash
export GOOGLE_API_KEY=...

python3 cstl_llm_agent.py \
  --name charlie \
  --provider gemini \
  --server-host localhost \
  --server-port 9000 \
  --capabilities planning verification
```

### Multi-Agent Dialogue

```bash
# Terminal 1: Start server
cd Cstl
./target/release/cstl_server

# Terminal 2: Agent A (Anthropic)
cd Cstl/sdk/python
export ANTHROPIC_API_KEY=sk-...
python3 cstl_llm_agent.py --name alice --provider anthropic

# Terminal 3: Agent B (Ollama)
cd Cstl/sdk/python
python3 cstl_llm_agent.py --name bob --provider ollama

# Watch agent conversation (check server logs)
tail -f /var/log/cstl.log
```

---

## Network Security & Access Control

### TCP Access Control

**Recommended: Internal VPN Only**

```bash
# Firewall rule (iptables)
sudo iptables -A INPUT -p tcp --dport 9000 -i tun0 -j ACCEPT  # VPN interface
sudo iptables -A INPUT -p tcp --dport 9000 -j DROP             # All others

# Or via UFW
sudo ufw allow in on tun0 to any port 9000
sudo ufw deny 9000
```

**Alternative: Agent-by-agent ACL**

```rust
// src/server/listener.rs
fn is_allowed_client(peer_addr: SocketAddr) -> bool {
    // Whitelist known agents
    ALLOWED_AGENTS.contains(&peer_addr.ip())
}
```

### TLS & Encryption (v5.1 Roadmap)

**Current (v5.0.0):** TCP cleartext, messages cryptographically signed

**Interim Mitigation:** Wrap connection in VPN/IPsec at OS level

```bash
# Example: VPN tunnel
sudo openssl s_client -connect localhost:9000 \
  -cert /etc/cstl/client.crt \
  -key /etc/cstl/client.key \
  -CAfile /etc/cstl/ca.crt
```

**v5.1 Plan:** TLS 1.3 + mutual certificate authentication

---

## Monitoring & Observability

### Logging Strategy

**Three Log Levels:**

1. **DEBUG** (development only)
   ```
   [DEBUG] Handler: Processing message from alice
   [DEBUG] Signature: Verifying Ed25519 signature...
   [DEBUG] Registry: Routing to 3 agents with capability=reasoning
   ```

2. **INFO** (production default)
   ```
   [INFO] Server listening on 127.0.0.1:9000
   [INFO] Agent registered: alice (trust_score=0.8)
   [INFO] Message delivered: alice → bob (purpose=communication)
   ```

3. **WARN** (anomalies)
   ```
   [WARN] Invalid signature from unknown sender (IP: 10.0.0.15)
   [WARN] Registration rejected: Council vote failed
   [WARN] Compression ratio degraded (1.8× vs expected 2.5×)
   ```

**Log Output:**

```bash
# Console (JSON format recommended for parsing)
{"timestamp": "2026-09-13T10:00:00Z", "level": "INFO", "event": "agent_register", "agent": "alice", "status": "ok"}
{"timestamp": "2026-09-13T10:00:01Z", "level": "INFO", "event": "message_delivered", "sender": "alice", "receiver": "bob"}

# File
export CSTL_LOG_FILE=/var/log/cstl.log
tail -f /var/log/cstl.log | grep -i error
```

### Prometheus Metrics (Optional)

```rust
// Instrumentation (v5.1 roadmap)
prometheus::Counter!("cstl_messages_total", "Total messages processed")
prometheus::Histogram!("cstl_signature_verify_duration_seconds")
prometheus::Gauge!("cstl_agents_registered", "Current agent count")
prometheus::Histogram!("cstl_compression_ratio", "Compression ratio achieved")
```

**Scrape endpoint:** `http://localhost:9090/metrics`

### Health Checks

```bash
# Liveness probe
curl -f http://localhost:9000/health || exit 1

# Readiness probe
curl -f http://localhost:9000/ready || exit 1

# Detailed metrics
curl http://localhost:9000/metrics | head -20
```

---

## Operational Procedures

### Graceful Shutdown

```bash
# Systemd
sudo systemctl stop cstl.service
# Waits up to 30 seconds for in-flight requests

# Manual
kill -SIGTERM $(cat /var/run/cstl.pid)
# Server: Closing connections... (audit trail flushed)
```

### Agent Registration Management

**List registered agents:**

```bash
# Query registry (via debug endpoint, future)
curl http://localhost:9000/debug/agents | jq '.agents[]'

# Output:
# {
#   "name": "alice",
#   "capabilities": ["reasoning", "planning"],
#   "trust_score": 0.8,
#   "public_key": "a1b2c3..."
# }
```

**Deregister agent:**

```bash
# Send purpose=agent_deregister (future; not in v5.0.0)
# Interim: Restart server to reset registry
```

**Update trust score:**

```bash
# Council votes to adjust trust_score
# (governance voting protocol)
```

### Audit Trail Management

**Maintain audit integrity:**

```bash
# Backup audit database
sudo cp /var/lib/cstl/audit.db /backup/audit.db.$(date +%Y%m%d)

# Verify chain integrity (parent_hash)
python3 /opt/cstl/audit_verify.py /var/lib/cstl/audit.db

# Retention policy (90 days default)
find /var/lib/cstl -name "audit.*.gz" -mtime +90 -delete
```

---

## Scaling & Performance Tuning

### Connection Pooling

```rust
// Tokio task-per-connection (current)
let listener = TcpListener::bind(addr).await?;
loop {
    let (socket, _) = listener.accept().await?;
    tokio::spawn(handle_connection(socket));  // New task
}

// Future: Connection pool with max_connections limit
const MAX_CONNECTIONS: usize = 1000;
let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));
```

### Buffer Sizing

```rust
// Payload buffer (default 64KB)
const MAX_PAYLOAD_SIZE: usize = 64 * 1024;

// Bitstream buffer (ANS Stage 5)
const MAX_BITSTREAM_LEN: u16 = 16384;  // ~16KB

// Agent registry (default 100)
const MAX_AGENTS: usize = 100;
```

### Compression Tuning

**Use ANS Stage 5 for:**
- High-frequency messages (>1000/sec)
- Network-constrained environments
- Deterministic roundtrip requirement

**Skip ANS for:**
- One-off messages
- Large payloads (>10KB) where fixed u64 limit applies
- Cases where ZSTD is available and determinism not required

### Benchmark Baseline

```bash
# Compression performance
cargo bench --release

# Expected:
# ans_encode_500b:     0.8ms  (target <1ms ✓)
# ans_decode_500b:     0.4ms  (target <0.5ms ✓)
# signature_verify:    1.2ms  (Ed25519 crypto)
# registry_lookup:     0.05ms (agent discovery)
```

---

## Troubleshooting Guide

### Server Won't Start

```bash
# Error: "Address already in use"
lsof -i :9000
# Kill existing process
kill -9 <PID>

# Error: "Permission denied" (on port < 1024)
sudo setcap cap_net_bind_service=ep ./target/release/cstl_server
```

### Agent Registration Fails

```bash
# Error: "Invalid signature"
→ Check keypair generation: python3 cstl_signing.py
→ Verify private key matches public_key in payload

# Error: "Council vote failed"
→ Check governance quorum: CSTL_QUORUM_SIZE, CSTL_QUORUM_THRESHOLD
→ Ensure sufficient council members registered

# Error: "Missing field: public_key"
→ Verify agent sends purpose=agent_register with:
  - name (String)
  - public_key (64 hex chars)
  - capabilities (semicolon-separated List)
```

### Message Not Delivered

```bash
# Error: "Route to receiver failed"
→ Check receiver is registered: curl localhost:9000/agents | grep receiver_name
→ Verify receiver capabilities: does receiver have required capability?

# Error: "Signature verification failed"
→ Check signing_bytes canonicalization (Python/Rust must match)
→ Verify signature not corrupted in transit

# Error: "Compression ratio poor"
→ Check payload size: ANS suitable for < 10 symbols (~5-20 bytes)
→ Consider disabling compression for large payloads
```

### Performance Degradation

```bash
# Symptom: High latency (>100ms per message)
→ Check CPU usage: top (should be < 50% per core)
→ Check memory: free (should be > 1GB available)
→ Check agent count: too many agents in registry?

# Symptom: Memory leak (steadily growing)
→ Check for dropped connections: netstat -an | grep TIME_WAIT
→ Restart server: sudo systemctl restart cstl.service
→ Review audit log retention: is it growing unbounded?
```

---

## Disaster Recovery

### Backup Strategy

**Daily Backup:**

```bash
#!/bin/bash
# backup_cstl.sh
DATE=$(date +%Y%m%d)
BACKUP_DIR=/backup/cstl

# Audit database
sudo cp /var/lib/cstl/audit.db $BACKUP_DIR/audit.db.$DATE
sudo cp /var/lib/cstl/registry.json $BACKUP_DIR/registry.$DATE.json

# Configuration
sudo cp /etc/cstl/agents.json $BACKUP_DIR/agents.$DATE.json

# Compress
tar -czf $BACKUP_DIR/cstl_$DATE.tar.gz $BACKUP_DIR/*.json $BACKUP_DIR/*.db

# Verify
tar -tzf $BACKUP_DIR/cstl_$DATE.tar.gz | head -10
```

**Retention:** 30-day rolling backup (minimum 3 copies)

### Recovery Procedure

**Step 1: Restore from backup**

```bash
sudo systemctl stop cstl.service

tar -xzf /backup/cstl/cstl_20260913.tar.gz -C /var/lib/cstl/

sudo chown -R cstl:cstl /var/lib/cstl/
```

**Step 2: Verify audit trail integrity**

```bash
python3 /opt/cstl/audit_verify.py /var/lib/cstl/audit.db
# Expected: ✓ All 5000 messages verify OK (parent_hash chains intact)
```

**Step 3: Restart service**

```bash
sudo systemctl start cstl.service
sudo systemctl status cstl.service
```

### Corruption Detection

```bash
# Periodically verify audit chain
0 2 * * * python3 /opt/cstl/audit_verify.py /var/lib/cstl/audit.db

# Output on corruption:
# ERROR: Message 1234 hash mismatch: expected deadbeef, got cafebabe
# → Restore from backup immediately
```

---

## Conclusion

CSTL v5.0.0 deployment is straightforward for small-to-medium deployments (3-100 agents). The systemd service method is recommended for production. Monitor via logs and metrics; graceful degradation is built in (e.g., LLM provider unavailable → fallback provider or error response).

For large-scale deployments (>1000 agents), plan for v5.1 scaling improvements: per-shard mutexes, connection pooling, metrics aggregation.

---

**Document Version:** 1.0  
**Last Updated:** 2026-09-13  
**Status:** ✅ COMPLETE
