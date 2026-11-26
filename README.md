# CSTL — a control layer for AI agents

CSTL decides what an AI agent may **do**, proves who asked, and keeps a
record nobody can quietly rewrite.

- An agent asks the CSTL server **before** it acts.
- The server decides from rules that only designated authors may write.
- If the action is allowed, the server returns a **token signed with its own
  key**.
- The system that runs the action (a database gate, an MCP or A2A proxy, your
  own code) checks that token itself and refuses anything else.
- Every decision goes into a hash-chained audit log, signed by the server.

> **Are you an AI agent reading this repo?** [`AGENTS.md`](AGENTS.md) is
> written for you: commands you can run right now, with real outputs.

> *Les relations sont plus importantes que l'information.* —
> [Principes fondateurs](PRINCIPES.md)

---

## Try it in two minutes

```bash
cargo build --bin cstl_parser
pip install ./sdk/python        # the "cstl" package: SDK + 8 commands
cstl-demo                       # prints "Tout est conforme : 7/7."
```

The demo runs against the real server, in a temporary folder:
- two agents register;
- an allowed action gets a signed token;
- a forbidden action is denied with its rule;
- an amount cap applies;
- forged, mismatched, missing and reused tokens are refused;
- another key cannot speak for an agent;
- after one row of the audit is edited by hand, the server refuses to
  restart.

## What it looks like in code

```python
from cstl_llm_agent import CstlAgent
from cstl_guard import CstlGuard

bot = CstlAgent("agent_support", "none", port=5050)
bot.register()
token = bot.authorize("refund_order", amount=80)        # ActionDenied if refused

# In the system that executes the refund, which trusts only the server's public key:
guard = CstlGuard(open("cstl_adn.db.server_key.pub").read())
guard.run(token, "refund_order", do_refund, amount=80)  # Unauthorized otherwise
```

A policy author writes the rules, signed with their own key:

```python
owner.publish_policy([
    {"type": "PERFORM", "subject": "agent_support", "object": "refund_order",
     "modality": "MUST_NOT", "amount_above": "100", "reason": "above 100: a human decides"},
], server_key)   # raises PolicyNotConfirmed unless the server signs a receipt for this exact message
```

The same exists in TypeScript (`sdk/typescript`, no runtime dependency), and
from any JSON client (`POST /v1/message`).

---

## What it does

### Identity
| | |
|---|---|
| **Ed25519 signatures** | Every message is signed by the agent's registered key; signatures are required by default. |
| **Key rotation** | A new key must be proven by the old one. |
| **Revocation** | A stolen key stops working at once. What it signed no longer counts. The name comes back only with a key enrolled by a policy author. |
| **OIDC** | Registration can require an ID token (RS256 or ES256) from the company's identity provider, whose `sub` must be the agent's name. |
| **SPIFFE** | In mutual TLS, the SPIFFE ID of the client certificate fixes which agent the connection may speak for. |
| **Federation** | Two organizations' servers trust each other through short-lived signed identity assertions. |

### Policy
| | |
|---|---|
| **Policy authors** | Only rules from `CSTL_POLICY_AUTHORS`, signed with their registered key, count. |
| **Conditions enforced by the server** | Roles, validity periods, amount limits, cumulative totals, facts (true, false or unknown), priorities for exceptions. |
| **Prudence** | A condition the server cannot evaluate keeps a prohibition in force. |
| **Closed modes** | `CSTL_ACTION_DEFAULT=deny`, or deny only for listed risky actions with no rule (`CSTL_GOVERNED_ACTIONS`). |
| **Order and receipts** | `policy_seq` refuses an older message from the same author. A signed receipt tells the author that its exact message is in force. |
| **Effective policy** | `agent.policy()` returns what the server will decide, with reasons. A compact form is available for models. |

### Authorization and enforcement
| | |
|---|---|
| **Signed tokens** | Bound to the agent, the action, the amount and a time window, and usable once. |
| **Live check** | The guard can ask the server whether a token still holds (not revoked, not stopped), with a signed answer. |
| **Human approval** | Listed actions wait for a person's signed approval, valid once, for the same amount (`cstl-approve`). |
| **Emergency stop** | It wins over every rule, for all actions, one agent or one action pattern. The operator also has a file switch that no message can lift. |
| **Abnormal behavior** | A burst of requests stops the agent automatically. |
| **Enforcement points** | `cstl-gate` (named SQL, shell and HTTP operations; it alone holds the credentials), `cstl-mcp-proxy` (in front of any MCP server), `cstl-a2a-proxy` (in front of any A2A 1.0 agent, batches included), and `cstl-mcp-server` (CSTL as MCP tools). |

### Deliberation (v6, draft)
| | |
|---|---|
| **Engine** | `cstl-deliberate`: several models debate on numbered points. Each call receives only the current state (−65 % estimated tokens on a long debate). Every reply is signed, checked by the server and audited; errors go back to the model. |
| **Closure** | CONSENSUS or DEADLOCK computed from positions, with caps on new points, amendment depth and rounds. |
| **Execution** | Decisions in consensus become signed rules that the control layer enforces. Deadlocks go to a human through the approval flow. |

### Audit
| | |
|---|---|
| **Hash chain signed by the server** | Startup is refused if a link was changed, deleted or stripped of its signature. |
| **External witness** | A witness on another machine catches a rollback to an old copy of the database (`cstl-witness`). |
| **Dashboard** | `/v1/dashboard` shows decisions, refusals by reason, stops, pending approvals and revoked keys. |

### Data for models
| | |
|---|---|
| **Compact CSTL** | Policies and relations in few tokens. Nothing is guessed when reading: an unreadable line is an error, never a silent default. |
| **Trees** | Nested JSON documents, signed and audited. The server checks completeness, and a source can commit to a document that someone else transmits. |
| **CSTL strict** | An indented form with JSON values, the most reliable for a model to write on irregular data. |
| **Unknown values** | `?` is distinct from `null` and from an absent field. |

---

## Measured results

Every figure below comes from a script in this repository; the raw results
are committed next to it.

**Attacked policy channel, deterministic** (`benchmark/control_benchmark.py`):
- 12 attack cases, 20 requests;
- 5 other mechanisms: no control, a rule engine, a signed policy, a signed
  and versioned policy, and simplified CSTL.

| Violations / false refusals | no control | signed | signed + versioned | **CSTL** |
|---|---|---|---|---|
| Relay compromised (T1) | 13 / 0 | 3 / 5 | 1 / 0 | **0 / 0** |
| Author link compromised (T2) | 13 / 0 | 3 / 5 | 1 / 0 | **0 / 0** |

**With real models** (`scripts/competition_benchmark.py`, GPT-5 mini,
Llama 4, Claude Haiku 5.5, Mistral):
- Mechanisms that judge the received message (rule engine, LLM guard) are
  compromised often.
- **CSTL and the signed, versioned policy are tied.**
  - Open mode: 73 against 78 perfect cases, McNemar p = 0.18. CSTL also
    had 6 compromised cases, against 0. All came from an author policy that
    was missing or contradictory, and so refused, while the server allows
    by default.
  - With governed actions: 56 against 58 out of 59, p = 0.50, and 0
    compromised cases for both.
- So CSTL is **not yet proven better** than a well-built signed policy with
  models. The benchmark was stopped when the API credits ran out; see
  `benchmark/README.md`.

**Formats** (`scripts/real_data_benchmark.py`, irregular data):
- **Writing:**
  - with a short instruction, CSTL strict is written correctly 48/48 times,
    against 36/48 for JSON;
  - with 3 examples, they are tied (46 against 47);
  - CSTL reports its own errors, while JSON fails silently.
- **Reading:** the indented tree scored 36/36, against 35/36 for JSON, on
  Gemma 4 31B, with 39 % fewer tokens.

**Conformance**:
- `conformance/run_server_conformance.py` was written from the spec alone;
  this server passes 18/18.
- Python, Rust and TypeScript produce identical signing bytes, tokens, tree
  hashes and compact texts.

---

## Specification and interoperability

- [`docs/CSTL_CONTROLE_SPEC_v1.md`](docs/CSTL_CONTROLE_SPEC_v1.md) covers the
  control layer:
  - wire syntax and canonical signing bytes;
  - identity (registration, revocation, OIDC, SPIFFE, federation);
  - rules, authorization and tokens;
  - live status, approval and emergency stop;
  - audit and errors.
- `conformance/vectors.json` holds exact inputs and outputs.
- SDKs:
  - Python: `sdk/python`, the `cstl` package;
  - TypeScript: `sdk/typescript`, Node 20+, no runtime dependency;
  - Rust: the server crate itself.
- [`CSTL_SPEC_v5_0.md`](CSTL_SPEC_v5_0.md) is the original language
  specification (modalities, uncertainty, provenance).

## Main settings

| Variable | Effect |
|---|---|
| `CSTL_POLICY_AUTHORS` | Who may write rules (`owner,ops_*`). Strongly recommended. |
| `CSTL_ACTION_DEFAULT` / `CSTL_GOVERNED_ACTIONS` | Closed mode for everything, or only for listed risky actions. |
| `CSTL_APPROVAL_ACTIONS` / `CSTL_APPROVERS` | Actions that need a human's signed approval, and who may approve. |
| `CSTL_ANOMALY_MAX_PER_MIN` | Automatic stop of an agent above this rate. |
| `CSTL_AUTHZ_TTL_SECS` | Token validity (60 s). |
| `CSTL_TLS_CERT_PATH` / `CSTL_TLS_KEY_PATH` / `CSTL_TLS_REQUIRE_MUTUAL_AUTH` | TLS 1.3, optionally mutual. |
| `CSTL_SPIFFE_TRUST_DOMAIN` / `CSTL_OIDC_ISSUER` / `CSTL_FEDERATION_PEERS` | Standard identities and federation. |
| `CSTL_WITNESS_URL` / `CSTL_SERVER_PUBLIC_KEY` | External witness of the audit, pinned server key. |
| `CSTL_BIND_HOST` | Listen address (`127.0.0.1` by default). |
| `CSTL_DEV_MODE=1` | Local trials only: unsigned messages and demo agents. |

The full list, with each behavior explained, is in [`AGENTS.md`](AGENTS.md).

---

## Known limitations (2026-11-25)

**Security model**
- A token makes an action mandatory only where the executing system demands
  it. A gate protects only what it alone can do; removing the same access
  from the agent is part of the setup.
- The MCP and A2A proxies control which tool or method is called, and the
  amount. They do not understand the other arguments.
- Without the live check, a token stays valid until it expires (60 s by
  default), even after a revocation or a stop.
- Tree counts are the author's: they prove completeness against what the
  author announced, not that the author tells the truth.
- The external witness receives links asynchronously: the last links before
  an abrupt stop may be missing there.
- No external security audit has been done yet.

**Product**
- The server stores everything in SQLite. There is no PostgreSQL backend.
- The JSON gateway listens on 127.0.0.1 only. All its requests share one rate
  limit, and it speaks for one identity under SPIFFE enforcement.
- The TypeScript SDK has neither the indented trees nor the document forms.
- With models, CSTL is not yet proven better than a signed, versioned policy
  (see above).
- Nothing is published yet (PyPI, npm, binaries). The release workflow has
  never run.
- There is no LICENSE file yet. The intended license is Apache 2.0, to be
  confirmed by the author.
- There are no external users yet.

## Older modules (still in the code, secondary)

CSTL started as a compact language for model-to-model messages, and several
earlier components remain:
- **Compression:** the WAI codec, the 4-stream master compressor, the
  `COMPRESSED_PAYLOAD` and `COMPRESSED_RESPONSE` blocks and the CASTLE
  session dictionary. They are opt-in, verified byte-exact, and their gains
  on real traffic are small. They are not part of the control layer.
- Wikidata fact verification (off with `CSTL_KB_VERIFY=off`), a signed
  2/3 quorum, an arbitration channel, calibration, and the Graphify export.

Their history, audit notes and measurements are in
[`docs/HISTORIQUE_README_v5_1.md`](docs/HISTORIQUE_README_v5_1.md) and
[`ARCHITECTURE.md`](ARCHITECTURE.md).

## Repository map

| Path | Content |
|---|---|
| `src/server/` | The server: handler, authorization, approval, emergency stop, federation, OIDC, SPIFFE, dashboard, JSON gateway, TLS. |
| `src/adn_store.rs` | SQLite storage, audit chain. |
| `sdk/python/` | SDK, commands, live tests (`test_*_live.py`). |
| `sdk/typescript/` | TypeScript SDK and tests. |
| `conformance/` | Vectors and the black-box server tester. |
| `benchmark/` | Public attack benchmark and its cases. |
| `scripts/` | Model benchmarks and format measurements. |
| `docs/` | Specifications. |
| `tripartite/` | Manual test: ChatGPT, Gemini and Claude talking in CSTL, checked by the real server (`GUIDE.md`, in French). |
| [`ROADMAP.md`](ROADMAP.md) | What is done, what remains, known weaknesses. |

## Tests

```bash
cargo test                                             # Rust: unit and integration
python3 -m pytest -q sdk/python/test_*_live.py         # Python, against the real server
cd sdk/typescript && npm install && npm test           # TypeScript
```

## License

Intended: Apache 2.0, by Olivier Goyette. See the limitation above: the
LICENSE file is not in the repository yet.
