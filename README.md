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
> [Principes fondateurs](docs/PRINCIPES.md)

---

## Get started

**With Docker** (nothing else to install):

```bash
docker compose up -d                                           # builds and starts the server
docker compose exec cstl cstl demo                             # 7 checks: "Tout est conforme : 7/7."
docker compose exec -T cstl cstl policy apply - < policy.cstl  # your rules (example below)
docker compose exec cstl cstl agent add agent_support
docker compose exec cstl cstl check agent_support refund_order --amount 80
```

**Without Docker** (Rust and Python 3.10+):

```bash
cargo build --release --bin cstl_parser
pip install ./sdk/python
cstl init                                  # cstl.json, the policy author's key, an example policy.cstl
cstl serve                                 # in another terminal; data in ./cstl-data
cstl policy apply policy.cstl              # signed, and confirmed by a receipt from the server
cstl agent add agent_support
cstl check agent_support refund_order --amount 80     # AUTORISE
cstl check agent_support refund_order --amount 500    # REFUSE, with the rule
```

A policy file is compact CSTL, one rule per line:

```
agent_support MAY refund_order <=100
agent_support MUST_NOT refund_order >100 "above 100: a human decides"
agent_support MUST_NOT wipe_db "production data"
```

Everything goes through one command, `cstl`:

| Command | What it does |
|---|---|
| `cstl init` / `cstl serve` / `cstl status` | Set up, start, check the server. |
| `cstl policy apply <file>` / `cstl policy show <agent>` | Publish the rules; see what the server will decide for an agent. |
| `cstl agent add <name>` / `cstl check <agent> <action>` | Create an agent's key; ask for an authorization as that agent. |
| `cstl stop [all\|agent:X\|action:Y] [--off]` / `cstl revoke <agent>` | Emergency stop; revoke a stolen key. |
| `cstl approve`, `cstl gate`, `cstl mcp-proxy`, `cstl a2a-proxy`, `cstl mcp-server`, `cstl witness`, `cstl deliberate`, `cstl demo` | Human approvals, enforcement points, witness, deliberation, demo. |

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

## How the server works

One Rust binary, `cstl_parser`, with one SQLite file next to it. Every
message goes through the same stages:

```
 agent ──TCP / TLS 1.3 (optional mTLS, SPIFFE)──▶ listener   rate limit per IP
                                                     │
 1. security scan       dangerous characters and structures refused
 2. strict parser       header, blocks; prose outside a block, a sigma outside
                        0–1 → parse_error (nothing is silently dropped)
 3. identity            Ed25519 signature against the registered key; revoked
                        keys, SPIFFE identity, federation expiry
 4. validation          operators, deontic contradictions (MUST + MUST_NOT),
                        SCHEMA types, tree completeness and source commitments,
                        policy_seq (replayed or older policy refused)
 5. purpose             register · revoke · emergency stop · approval ·
                        action_request → layer 9 decides from the authors'
                        rules → signed token · policy_query · tree_query ·
                        token_status · deliberation messages
 6. audit               every accepted message and decision is chained by hash
                        and signed with the server key
 7. response            CSTL text (or JSON through the gateway), with the
                        AUDIT line
```

Around the pipeline:
- **Storage:** SQLite (`cstl_adn.db`): messages, agents and keys, revoked
  keys, emergency stops, approvals, issued tokens, decisions, audit chain.
- **Server key** (`<db>.server_key`): signs tokens, receipts, token status
  and every audit link. Startup re-verifies the whole chain and refuses to
  start on any change, deletion or missing signature.
- **REST port** (127.0.0.1 only): the JSON gateway `/v1/message` and the
  dashboard `/v1/dashboard`.
- **Optional outside pieces:** the witness (`cstl-witness`, on another
  machine, against rollback), Telegram for approvals, Wikidata fact checks
  (off with `CSTL_KB_VERIFY=off`).
- **Executing side** (not in the server): `cstl-gate`, the MCP and A2A
  proxies, or `CstlGuard` in your code. They trust only the server's public
  key and check each token themselves.

The server is the only judge: models write and read CSTL, but no decision
depends on a model. Policy evaluation, signatures and the audit are
deterministic Rust code.

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
| **Built-in connector** | For your own MCP server or A2A agent (`cstl_connect`): `@guarded("refund_order", amount_arg="amount")` on an MCP tool, or `A2AGuard(app, name="billing")` around an ASGI A2A app. Same checks as the proxies, in the same process, closed by default. |

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
CSTL has several written forms, described in [the next section](#the-cstl-formats).
The server checks and audits all of them the same way.

---

## The CSTL formats

All forms carry the same relations and convert into each other with no loss
(`sdk/python/cstl_compact.py`, `cstl_formes.py`). Reading never guesses: an
unreadable line, a wrong count or a badly typed value is an error, never a
silent default.

**1. The wire format**: what the server receives and signs.
```
#!CSTL v5.0.0 MODE=A
META [encoder=agent_support, produced_by=agent_support]
INTENT_PAYLOAD [purpose=action_request, sender=agent_support, receiver=server, action=refund_order, amount=80, request_id=r1, signature=...]
---END---
```

**2. Compact CSTL, in lines**: policies and relations for a model.
```
agent_support MAY refund_order <=100
agent_support MUST_NOT refund_order >100 "above 100: a human decides"
agent_support MUST_NOT liquidate if:incident_open
agent_support MUST log_transaction
```

**3. Without keywords (`symboles`)**: the same policy, in the shortest form
(`?` may, `!` must not, `*` must, `+fact` if the fact holds).
```
agent_support ?refund_order <=100
agent_support !refund_order >100 "above 100: a human decides"
agent_support !liquidate +incident_open
agent_support *log_transaction
```
Both are real encoder output (`encode`, `encode_symbols`), and both decode
back to the exact same four relations.

**4. The indented tree**: nested documents. Children sit under their
parent, and an optional field is written only when present.
```
@@ service=billing
@orders[2]: id client
@lines[3]^orders: item qty
o1 client_3 note=fragile
  gpu_rack 2
  lic 1
o2 client_1
  router 3
```
With `counts="line"`, each element ends its line with `/N`, its number of
children, so a model's counting mistake names the faulty line.

**5. Strict**: indented keys, with every value written as JSON. This is the
form to ask a model to **write** on irregular data.

**6. Unknown values**: `?` or `?{sigma:0.3,pending:next_checkup}`. They
are distinct from `null` (known to be empty) and from an absent field (not
applicable).

### What each form saves (measured)

| Form | Data | Tokens vs JSON compact | Model accuracy |
|---|---|---|---|
| Indented tree | Nested tabular data (orders → lines → options) | **−39 %** (10,792 against 17,577) | Reading 36/36, against 35/36 for JSON (Gemma 4 31B) |
| Indented tree with `/N` counts | Same | −35 % | Writing 12/12 identical, in strict mode |
| Indented tree | Real API responses (PyPI, npm, crates.io, Wikidata), 6 tokenizers | −39 to −49 % on tabular parts, −8 to −13 % on object-rich data | Reading on par with JSON |
| Compact CSTL in lines | Effective policies | **−62 %** (782 against 2,039) | Reading: Opus 36/36 |
| Without keywords | Policies, Gemini tokenizer | **−63 %** (5,540 against 15,064); −43 % against TOON | Not yet measured with a model |
| Strict | Irregular real data | +7 to +18 % (the cost of reliability) | Writing 48/48, against 42/48 for JSON |
| Full wire format | 16 real server messages | −3 % (−7 % in bytes) | — |

What these numbers mean:
- **The saving is in tokens for models,** about 40 % on nested data and
  more than 60 % on policies. On plain wire bytes, CSTL saves almost
  nothing (see compression below).
- **Strong models read every form as well as JSON.** Across 10 models and 5
  providers: 312/360 for the indented tree, 315 for JSON, 319 for TOON.
  Weak models make a few more mistakes on the indented tree.
- **To have a model write irregular data, use strict.** The compact forms
  are for reading, and for passing data between programs.
- **TOON is shorter on flat, regular data** (compact CSTL in lines costs
  33 % more there). The table and keyword-free modes close that gap offline,
  but no model has been measured on them yet.

Sources: `scripts/nested_benchmark.py`, `scripts/real_data_benchmark.py`,
`scripts/format_short_benchmark.py`, `scripts/gemini_tokens.py`,
`scripts/multi_model_summary.py`, `scripts/wire_size_benchmark.py`. Each is
detailed in [`ROADMAP.md`](ROADMAP.md).

### Compression: CSTL against gzip and zstd

Measured on the same 16 real messages (`scripts/compression_benchmark.py`):

| | Bytes |
|---|---|
| Best for JSON: zstd with a trained dictionary | 2,076 |
| Best for CSTL: zstd with a trained dictionary | 2,070 |
| CSTL's own compressors (WAI codec, master 4-stream compressor) | about 10,000 |

- Once compressed with the best tool, CSTL and JSON are the same size.
- CSTL's own compressors do about **5 times worse** than zstd.
- Compressed server responses (`COMPRESSED_RESPONSE`) came out **larger**
  than plain text (971 → 1,327 bytes), because of base64 and a dictionary
  that does not know the response vocabulary.
- Uncompressed, the CSTL wire format is 7 % smaller than compact JSON, but
  3 % larger after gzip.

**The indented tree, compressed** (`scripts/tree_compression_benchmark.py`,
2026-11-27): the data themselves, not wire messages. Totals in bytes, every
form read back identical and every compression checked.

| | raw | gzip -9 | zstd -19 | brotli -11 | zstd + trained dictionary |
|---|---|---|---|---|---|
| Nested data, JSON compact (12 docs) | 50,262 | 7,424 | 6,916 | 6,224 | 4,202 |
| Nested data, indented tree | 23,789 | 6,507 | 6,216 | 5,581 | **3,578** |
| | **−53 %** | **−12 %** | **−10 %** | **−10 %** | **−15 %** |
| Real API responses, JSON compact (12 docs) | 31,653 | 9,144 | 9,117 | 7,715 | 6,352 |
| Real API responses, indented tree | 26,120 | 8,916 | 8,922 | 7,599 | 6,595 |
| | −17 % | −2 % | −2 % | −2 % | +4 % |

- On nested, tabular data, the indented tree **stays 10 to 15 % smaller
  than JSON after every compressor**. It removes repetition (field names
  written once, no ids, no brackets) that compressors only partly recover.
- On irregular real data, the gain almost disappears once compressed, and
  zstd with a dictionary does slightly better on JSON. That dictionary was
  trained on only 11 very different documents, so this point is weak.
- TOON and JSON indented are larger than compact JSON after compression;
  strict costs 0 to 5 % more.

So **compression alone is not a reason to use CSTL**, except for nested
tabular data in the indented tree. The old compression modules
stay in the code, opt-in, and are not part of the control layer. Earlier
figures for them (such as a "63.81 % compression ratio") are superseded by
these measurements on real messages.

---

## Measured results

Every figure below comes from a script in this repository; the raw results
are committed next to it.

**Attacked policy channel, deterministic** (`benchmark/control_benchmark.py`):
- 12 attack cases, 20 requests;
- the other mechanisms: no control, a rule engine, a signed policy, a
  signed and versioned policy, and **the real OPA 1.23.0** (`--opa`), with
  unsigned and with signed bundles (`opa build --signing-key`, verified by
  OPA: the practice the OPA project recommends).

| Violations / false refusals | no control | OPA | OPA, signed bundles | signed + versioned | **CSTL** |
|---|---|---|---|---|---|
| Relay compromised (T1) | 13 / 0 | 10 / 0 | 4 / 2 | 1 / 0 | **0 / 0** |
| Author link compromised (T2) | 13 / 0 | 10 / 0 | 4 / 2 | 1 / 0 | **0 / 0** |
| Compromised cases (of 12) | 12 | 10 | 4 | 1 | **0** |

OPA's signature check works: a modified bundle, or one signed with another
key, is refused. But OPA does not protect the order: a replayed old signed
bundle is activated again, a dropped bundle leaves the old policy in force,
and bundles delivered out of order end on the last delivery. CSTL closes
these with `policy_seq` and the signed receipt. Cedar is not in the table:
it evaluates policies but does not distribute or verify them (that role is
a cloud service, Amazon Verified Permissions, which cannot be tested here).
Details: [`benchmark/README.md`](benchmark/README.md).

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

**Formats:** see [The CSTL formats](#the-cstl-formats): about −40 % tokens on
nested data, more than −60 % on policies, reading on par with JSON, and
strict written correctly 48/48 against 36/48 for JSON with one example
(46 against 47 with three). CSTL reports its own errors, while JSON fails
silently.

**Deliberation** (`benchmark/deliberation/`, 26 cases, 3 free models):
100 % correct in every condition, so deliberating did not reduce errors
here. CSTL deliberation used about 4,600 tokens per case, against 18,000 for
a free debate and 700 for a single agent.

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
- [`docs/CSTL_SPEC_v5_0.md`](docs/CSTL_SPEC_v5_0.md) is the original language
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

## Known limitations (2026-11-27)

**Security model**
- A token makes an action mandatory only where the executing system demands
  it. A gate protects only what it alone can do; removing the same access
  from the agent is part of the setup.
- The MCP and A2A proxies control which tool or method is called, and the
  amount. The MCP proxy also bounds the top-level arguments when given an
  `--args-schema`; the A2A proxy does not read the message content.
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
- There are no external users yet.

## Older modules (still in the code, secondary)

CSTL started as a compact language for model-to-model messages, and several
earlier components remain:
- **Compression:** the WAI codec, the 4-stream master compressor, the
  `COMPRESSED_PAYLOAD` and `COMPRESSED_RESPONSE` blocks and the CASTLE
  session dictionary. They are opt-in and verified byte-exact, but they
  lose to zstd (see [Compression](#compression-cstl-against-gzip-and-zstd)).
  They are not part of the control layer.
- Wikidata fact verification (off with `CSTL_KB_VERIFY=off`), a signed
  2/3 quorum, an arbitration channel, calibration, and the Graphify export.

Their history, audit notes and measurements are in
[`docs/HISTORIQUE_README_v5_1.md`](docs/HISTORIQUE_README_v5_1.md) and
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

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
| `docs/` | Specifications, architecture, pilot guide. |
| `archive/` | Old reports, experiments and prototypes, kept for reference. Not used by the build. |
| `tripartite/` | Manual test: ChatGPT, Gemini and Claude talking in CSTL, checked by the real server (`GUIDE.md`, in French). |
| [`ROADMAP.md`](ROADMAP.md) | What is done, what remains, known weaknesses. |

## Tests

```bash
cargo test                                             # Rust: unit and integration
python3 -m pytest -q sdk/python/test_*_live.py         # Python, against the real server
cd sdk/typescript && npm install && npm test           # TypeScript
```

## License

Apache License 2.0, by Olivier Goyette. See [LICENSE](LICENSE).
