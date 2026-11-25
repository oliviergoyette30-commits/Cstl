# Trying CSTL — for an AI agent reading this repo

You're an AI (or an agent acting on behalf of one) with shell access to this
repo. This file is for you specifically: the fastest real path to seeing
CSTL work, not marketing copy. Everything below is a command you can run
right now, verbatim, and every example output shown was captured from an
actual run against the real code in this repo (2026-10-29) -- not invented.

If you want the full feature writeup first, read `README.md`. If you just
want to see it work, keep reading here.

## Fastest — the whole control layer in one command (~2 s)

```bash
cargo build --bin cstl_parser
pip install ./sdk/python        # the "cstl" package: SDK + 7 commands
cstl-demo                       # or: python3 sdk/python/cstl_demo.py
```

`cstl-demo` runs against the real server, in a temporary folder that it
deletes afterwards:
1. two agents register with their keys, and a policy is written;
2. an allowed action gets a server-signed token, which the executing system
   checks itself;
3. a forbidden action is denied, with its rule;
4. an amount cap applies;
5. a forged, mismatched, missing or reused token is refused;
6. another key cannot speak for the agent;
7. after one SQLite row of the audit is edited by hand, the server refuses
   to restart and says why.

It prints `Tout est conforme : 7/7.` Live test: `sdk/python/test_demo_live.py`.

The other installed commands are `cstl-mcp-proxy`, `cstl-a2a-proxy`,
`cstl-gate`, `cstl-approve`, `cstl-mcp-server` and `cstl-witness`. Server binaries for Linux, macOS and Windows, plus the
wheel, are built by `.github/workflows/release.yml` (on a `v*` tag or by
hand). Publishing them is a human decision.

## Writing your own client or server — the frozen spec

`docs/CSTL_CONTROLE_SPEC_v1.md` (frozen 2026-11-25) is enough to write a
compatible client or server for the control layer: wire syntax, canonical
signing bytes, registration, `action_request`, the signed token and how to
check it, `policy_query`, audit and errors.

- **Vectors:** `conformance/vectors.json` holds exact inputs and outputs.
  Python and Rust produce the same bytes and the same verdicts.
- **Server test:** `conformance/run_server_conformance.py --port 5050
  --server-key cstl_adn.db.server_key.pub` is a black-box tester written
  from the spec alone, without the SDK. This repo's server passes 18/18.

## From TypeScript / Node.js

`sdk/typescript` is a client with no runtime dependency (Ed25519, TCP and
HTTP come from Node 20+). It produces the conformance vectors exactly, like
Python and Rust, and uses the same key file as the Python SDK
(`~/.cstl/agent_<name>.key`), so an agent keeps one identity in both
languages.

```ts
const bot = new CstlAgent("agent_support", { port: 5050 });
await bot.register();
const token = await bot.authorize("refund_order", 80);          // ActionDenied if refused
await new CstlGuard(serverKeyHex).run(token, "refund_order", doIt, { amount: 80 });
```

Tests: `cd sdk/typescript && npm install && npm test` (vectors, then live
tests against `target/debug/cstl_parser`).

## Public benchmark — attacked policy channel

`benchmark/control_benchmark.py` is deterministic, needs no API key, and
tests CSTL on the real server against four other mechanisms: no control,
a rule engine on the received policy, a signed policy, and a signed policy
with versions and refetch. The 12 attack cases are JSON files in
`benchmark/cases/`; add your own (format in `benchmark/README.md`).

Results (2026-11-25):
- **First measure:** on a compromised author-to-server link, CSTL was
  compromised in 7 cases, against 1 for the signed and versioned policy.
- **Fixed the same day,** with two server features:
  - `policy_seq`: the server refuses a message older than the author's
    last accepted one (`stale_policy`);
  - a **signed receipt** (`RECEIPT` block, spec section 6.4): the author
    checks that its exact message is in force, and republishes otherwise.
    Python: `agent.publish_policy(relations, server_key)`; TypeScript:
    `await agent.publishPolicy(relations, serverKey)`. Both raise
    `PolicyNotConfirmed` without a valid receipt and share the sequence
    file `~/.cstl/agent_<name>.seq`.
- **Now:** CSTL is the only mechanism with 0 violations and 0 false
  refusals under both threat models.

## Option 0 — you have NO code execution at all (a chat-window AI)

This is for you if you are reading this because a human pasted it into your
chat and you cannot run a single command yourself — only compose text and
wait for the human to paste a result back. This was added on 2026-10-29
after exactly that situation: an AI in a chat window said so itself ("I
don't click Enter in your terminal") rather than quietly pretending it had
run code it hadn't. If that's you, don't guess at CSTL's behavior from this
file's prose — have the human get you a real answer in one copy-paste step:

1. Ask the human running your chat to do this once (it stays running):
   ```bash
   CSTL_DEV_MODE=1 cargo run --bin cstl_parser &
   cargo run --example http_bridge_for_ai &
   ```
2. Compose your own CSTL payload as plain text (see the format in Option 2
   below, or `CSTL_SPEC_v5_0.md`). Give the human exactly this to run,
   substituting your payload text:
   ```bash
   cat > /tmp/payload.cstl << 'EOF'
   <your CSTL payload text here>
   EOF
   curl -s --data-binary @/tmp/payload.cstl http://127.0.0.1:8089/try
   ```
3. Have them paste the raw output back to you. That text is the real
   server's real response — not simulated, not predicted by any LLM.
4. Repeat with a different payload for anything else you want to try.

`examples/http_bridge_for_ai.rs` is a plain HTTP-to-TCP relay: it does zero
CSTL interpretation itself, it just forwards your payload's bytes to the
real server and returns the real response's bytes. `curl` was chosen
because it's already installed on effectively everything, unlike the raw
`/dev/tcp` bash trick in Option 2, which needs a shell that understands it.

## Option 1 — the whole architecture, one command, zero setup

```bash
cargo run --example full_architecture_ai_to_ai_test
```

This starts a real `CstlNativeServer` over real TCP, creates two agents
(`alice`, `bob`) with real Ed25519 keypairs, and walks them through:
registration, a signed semantic message between them, a quorum
propose/vote with a simulated Sybil attack that gets rejected, an
intra-payload deontic contradiction (`MUST` + `MUST_NOT` on the same
subject/object) that gets hard-rejected before persistence, a simulated
server restart, and an identity-theft attempt after that restart that also
gets rejected. It uses a throwaway SQLite file under your temp directory
and deletes it when done -- it will not touch anything else in this repo.

Read `examples/full_architecture_ai_to_ai_test.rs` alongside the output;
the file comments explain what each assertion is actually proving and why.

## Option 2 — talk to the server yourself, raw wire format, no library

```bash
CSTL_DEV_MODE=1 cargo run --bin cstl_parser &
sleep 3

cat <<'EOF' > /tmp/try_cstl.cstl
#!CSTL v5.0.0 MODE=A
META [encoder=curious_ai, produced_by=curious_ai, timestamp=2026-10-29T00:00:00Z]
INTENT_PAYLOAD [purpose=inform, sender=alice, receiver=server]
RELATION [type=STATE, subject=cstl, object=alive]
---END---
EOF

exec 3<>/dev/tcp/127.0.0.1/5050
cat /tmp/try_cstl.cstl >&3
timeout 3 cat <&3
exec 3<&- 3>&-
```

`alice` is one of two demo agents the server creates itself
(`src/main.rs`), without a key, so this needs no signing step. That only
happens in **development mode** (`CSTL_DEV_MODE=1`). Development mode is
allowed only on a local listener. Since 2026-11-21, **signatures are
required by default**: without development mode, an unsigned message is
refused (`signature_rejected`). Real agents register with their own key
(`agent_register`, self-signed); see the Python SDK's `CstlAgent`. Real
response, captured live:

```
#!CSTL v5.0.0 MODE=A
META [encoder=CstlNativeServer, produced_by=Server, status=processed]
INTENT_PAYLOAD [purpose=acknowledgement, sender=server, receiver=alice]
RELATION [type=received, subject=inform, status=valid]
VERIFICATION [subject=cstl, predicate=STATE, object=alive, verified=unchallenged_unproven, source=none]
CONSISTENCY [consistent=true, contradictions=0, cycles=0, sigma=0.75]
PERFORMATIVE [type=INFORM, recognized=true, relations_attached=1]
GOVERNANCE [sender=alice, circuit=closed, breaker_trips=0, drift_ratio=-0.00, drift_flagged=false]
SIGMA_CALIBRATION [agent_sigma=0.500, server_sigma_effective=0.500, agent_trust_snapshot=1.000]
EXECUTION_TRACE [verdict=PASS, semantic_validation=PASS, kb_verification=RUN, kb_relations_checked=1, consistency_check=PASS, deontic_audit=PASS, scope=SERVER_LOCAL_THIS_PAYLOAD]
ADN_DELTA [...]
AUDIT [hash=sha256:..., parent_hash=sha256:..., seq=N]
---END---
```

That's the actual shape of a CSTL response: a signal-level acknowledgement
(`RELATION [type=received]`), a provenance/consistency audit of what you
sent, and a tamper-evident hash chain entry -- all computed by the
deterministic Rust parser, not inferred by an LLM.

This writes to `./cstl_adn.db` in whatever directory you ran it from
(`.gitignore`'d, safe to delete). Kill the server when done:
`pkill -f target/debug/cstl_parser`.

## Option 3 — a specific behavior, not the whole thing

`examples/` has ~40 other live, runnable scenarios, each one verifying one
specific claim against the real server rather than describing it. A few
worth knowing about if you're checking something specific:

- `examples/quorum_sybil_test.rs` -- demonstrates (and the current code
  now rejects) a Sybil attack on quorum voting.
- `examples/deontic_audit_smoke_test.rs` -- the deontic modality system
  (`MUST`/`MUST_NOT`), intra-payload vs. cross-history contradictions.
- `examples/key_rotation_smoke_test.rs` -- proving key possession across
  a legitimate identity rotation.
- `examples/agent_registry_persistence_test.rs` -- identity surviving a
  server restart.
- `examples/trust_score_poisoning_test.rs` -- trust-weighted routing and
  its [0,1] clamp.
- `examples/strict_deontic_history_smoke_test.rs` -- the opt-in hard
  rejection of cross-history deontic contradictions
  (`INTENT_PAYLOAD[strict_deontic_history=true]`, 2026-10-30): default
  behavior stays informative-only (unchanged, tested since 2026-09-04),
  but a client can now ask for a real rejection instead.

Run any of them the same way: `cargo run --example <name>`. Each file's
header comment explains what it's actually testing and why, in the same
style as this one.

## If something here doesn't match what you observe

Trust the live output over any written claim, including this file and
`README.md` -- both are maintained by hand and can drift from the code.
If you find a gap, that's a legitimate finding worth reporting, not a
sign you did something wrong.

## Ask before you act — authorization (Layer 9)

If your agent is about to *do* something (not just say something), ask the
server first. Signed request, from a registered agent:

```
INTENT_PAYLOAD [purpose=action_request, sender=<you>, receiver=server, action=wipe_db, request_id=<unique>]
```

Answer: `INTENT_PAYLOAD [purpose=action_authorization, ..., decision=allowed|denied, reason=..., rule=...]`
plus an `AUDIT` line. A `MUST_NOT` on (you, that action) in the history
denies it.

Who may set those rules: with `CSTL_POLICY_AUTHORS=policy_owner,ops_lead_*`
on the server, only rules from those agents count. Each rule must come in a
message signed with the author's registered key. The **most recent** of
those rules decides, so an authority can lift a prohibition. Rules from
anyone else are ignored, and the response reports how many were ignored
(`ignored_rules=N`). Without that variable, any rule counts and a
prohibition can never be lifted. That was the original behavior, and it
refused 3 correct actions in `scripts/team_task_benchmark.py`.

**Ask for your effective policy.** Send `purpose=policy_query`, signed,
or call `agent.policy()` in the Python SDK. The server answers with what
layer 9 will decide for you:
- your obligations (`decision=required`), to perform even when the task
  does not mention them;
- your prohibitions, with the rule's `reason` (set
  `RELATION [..., reason="..."]` when you write a rule, so that its
  conditions survive);
- the rules it ignores, because their author has no authority to set
  policy (`POLICY_IGNORED`). Do not follow those.

Agents that read this view instead of raw messages scored 48/48 with
both Haiku 5.5 and Opus 5.5 in `scripts/team_task_benchmark.py`.

**Compact views for a model.** `agent.policy_text(fmt)` returns the same
effective policy written for a model: `"phrases"` (sentences, the default),
`"compact"` or `"symboles"` (compact CSTL without keywords, the shortest:
`*log_transaction`, `!liquidate +incident_open etat=incident_open=inconnu`,
`?refund_order <=100`). The other way round, `agent.send_compact(receiver,
text)` takes relations a model wrote in compact CSTL (with or without
keywords, lines or tables), rebuilds the full relations, signs and sends
them. Nothing is guessed: an unreadable line, a wrong row count or a badly
typed value raises `cstl_compact.CompactError`. Format and tests:
`sdk/python/cstl_compact.py`, `sdk/python/test_cstl_compact.py`.

**Nested data (trees).** `agent.send_tree(receiver, tree)` sends any JSON
document: an object, a list or a single value, nested to any depth. It also
takes the same document written in compact CSTL tree mode.

The recommended text form is the **indented tree**
(`cstl_compact.encode_tree_indented`). The id-based form
(`cstl_compact.encode_tree`) is also accepted.

```
@@ service=billing tags=[prod,eu]
@orders[2]: id client
@lines[3]^orders: item qty
o1 client_3 note=fragile
  gpu_rack 2 dims=[[1,2],[3,4]]
  lic 1
o2 client_1 desc=|2
  | first line
  | second line
  router 3
```

How to read it:
- `@name[N]^parent: cols` declares each list once: `N` elements, which are
  children of `parent`, with these columns.
- Children are written under their parent, indented 2 spaces per level.
- A field that only some elements have is written `name=value`, and only
  when it is present.
- Lists and objects are written inline (`[a,b]`, `{lat:1}`).
- A multiline text is written `|N`, then `N` lines that start with `| `.
- `@@ ...` holds the plain fields of the root. `@@list` marks a root that is
  a list of objects. `@@= value` marks any other root.

`[N]` is optional; when present, it is checked. For a tree written by a
model, use `decode_tree_indented(text, lenient=True)` or
`send_tree(..., lenient=True)`. This relaxes three rules, so nothing is
guessed:
- a wrong `[N]` is only a warning;
- a row may omit its last columns (those fields are absent);
- any consistent indentation is accepted.

A model that wrote `@orders[8]: id client date note` and `@options[7]`
(instead of 10) is read back identical this way.

Models count long lists badly. Gemma 4 31B got a `[N]` wrong in 3 of 12
large documents, and a count placed at the end (`@end lines=21`) did no
better. Small local counts work: with `encode_tree_indented(data,
counts="line")`, each element that has children ends its line with `/N`,
its number of direct children (`ctr_1 party_9 /3`), and the document ends
with `@end`. Gemma wrote 12/12 documents identical in **strict** mode this
way. A wrong `/N` names the faulty line. Cost: +6 % tokens.

Round trips are lossless: tested on 30,000 random JSON documents with trap
values. In a comprehension test on Gemma 4 31B (36 questions), the indented
tree scored 36/36, against 35/36 for JSON, and used 39 % fewer tokens.

Each element becomes a relation marked `tree=<id>`, so the server signs,
checks (`SCHEMA` types) and audits it like any other. A list, an object or a
text the wire cannot carry (`"`, `[`, `]`, a newline) is sent as base64url
JSON, with the type `json`. `agent.fetch_tree(tree_id)` reads the document
back, identical, through the signed `purpose=tree_query`. Sending again with
the same id replaces it.

A long value (a text, a large list) is split into chunks of 250
characters: the first under its own name, the next ones under
`tree_c<i>_<name>`, with its type noting the number of chunks
(`body:str/12`). It is reassembled on reading. A message can hold up to
1 MiB. A field name outside `[A-Za-z0-9_.-]` is refused before sending.

**Unknown values (the "void that speaks").** An unknown value is distinct
from `null` (known: no value) and from an absent field (not applicable).
In JSON it is `{"$unknown": {}}`, or `{"$unknown": {"sigma": 0.3,
"pending": "next_checkup"}}` with its uncertainty and its plan
(`cstl_compact.unknown(sigma=0.3)`). The indented tree writes it `?` or
`?{sigma:0.3,pending:next_checkup}`:

```
@patients[3]: id
p1 renal=?{sigma:0.3,pending:next_checkup}
p2 renal=null
p3
```

**The server checks that a tree is complete.** Each element that has
children carries `tree_n=<number of direct children>`, and a `tree_root`
relation carries the number of root elements. On arrival, the server
counts them itself (`src/server/tree_check.rs`). It refuses the whole
message, before storing anything, with `INTENT_PAYLOAD
[purpose=tree_rejected, detail=...]`, in three cases:
- a branch is missing;
- an element has no parent;
- an element appears twice.

The counts are then signed and audited with the rest, and `fetch_tree`
checks them again on reading.

So a client that bypasses the SDK cannot leave a hole unnoticed, and a
reader can later prove the tree is complete. Limit: the counts are the
author's. They prove the tree matches what the author announced, not
that the author tells the truth.

**The source commits, someone else transmits.** To stop a transmitter
from dropping, changing or adding anything, the agent that owns the data
commits to it first: `source.commit_tree(data, tree_id)`. This signs the
tree's canonical hash and records it in the audit
(`RELATION [type=tree_commit, subject=<tree_id>, object=sha256:...]`).

Any agent can then send the tree with `send_tree(..., tree_id=tree_id,
source="<source>")`, in any form, including an indented tree written by a
model. The server recomputes the hash of what it receives. It refuses the
tree (`tree_rejected`) in three cases:
- the source never committed;
- the hash differs from the source's latest commit;
- the commit is not signed with the source's registered key. A
  transmitter cannot commit in the source's place.

The hash is identical in Python (`cstl_compact.tree_hash`) and in Rust
(`tree_check::canonical_hash`), and a test pins the same value on both
sides. Live test: a transmitter that drops a line, changes a price or adds
an order is refused, and so is one that commits to its own altered version.

Hardening (2026-11-23), after a review of this mechanism:
- **Salt.** `commit_tree` returns `(salt, response)`. The hash covers a
  random 128-bit `tree_nonce`, so the hash of a small document cannot be
  guessed by trying plausible contents. The source gives the salt to the
  transmitter with the data: `send_tree(..., source=..., nonce=salt)`.
- **Replay.** A replayed message (same canonical hash) no longer stores its
  relations again. Before this fix, replaying an old signed `tree_commit`
  made the old tree valid again. This was confirmed live. A replayed policy
  rule would also have come back into force.
- **Name collisions.** `fetch_tree(tree_id, source=...)` returns only the
  latest version that claims that source, and was therefore checked against
  its commitment. A third party using the same tree name cannot shadow it.
- **No knowledge-base lookups on tree nodes.** Tree elements (`tree=`) and
  `tree_commit` are local identifiers. They are no longer sent to
  Wikidata, which used to cost one outgoing request per line.
- A model may omit the `/N` counts. The SDK computes `tree_n` from the
  structure itself, and the model's `/N` is only a check.

Limits: a tree sent without `source` is vouched for by its sender only.
Nobody can prove that a source tells the truth about its own data; it can
only be bound, by signature, to what it declared.

Any registered agent can read a tree by its id. Live test:
`test_tree_mode_goes_through_the_server`.

**In front of any MCP server.** `sdk/python/cstl_mcp_proxy.py` sits between
an MCP client and an existing MCP server, which it starts itself.
Everything passes through unchanged, except tool calls (`tools/call`).
Before each one, the proxy asks the CSTL server for a signed authorization:
`action_request`, with the tool name as the action. A denied call never
reaches the MCP server. The client gets a tool error (`isError: true`) with
the reason and the rule.

```json
{"mcpServers": {"files": {"command": "python3",
   "args": ["/path/Cstl/sdk/python/cstl_mcp_proxy.py",
            "--server-key", "/path/cstl_adn.db.server_key.pub", "--",
            "npx", "-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
   "env": {"CSTL_AGENT": "agent_files", "CSTL_PORT": "5050"}}}}
```

- **Closed by default:** if the CSTL server is unreachable, its answer is
  unreadable or the token is invalid, the tool is not called.
- **With `--server-key`** (or `CSTL_SERVER_PUBLIC_KEY`), every authorization
  must carry a token checked by `CstlGuard`: signed by that key, for this
  tool, this agent and this amount, and never used before. A fake server
  cannot authorize a tool. Without the key, the proxy takes the server's
  answer on trust; use that only with a trusted local server.
- **Options:** `--amount-arg amount` sends that tool argument as the amount
  (rules `amount_above` / `amount_at_most`). `--prefix mcp_` asks for
  `mcp_<tool>`.
- **Limit:** the proxy controls which tool is called and with which amount.
  It does not understand the other arguments.

Live test: `sdk/python/test_mcp_proxy_live.py`. A test MCP server records
every tool it actually runs, so the test proves that a forbidden tool is
never run, not only that the client sees a refusal.

**The gate that executes: SQL, shell, HTTP.** A token makes an action
mandatory only if the system that runs it demands one. So a gate is only
mandatory if **it alone** holds the privilege: the database password, the
API key, the right to run the command. `sdk/python/cstl_gate.py`
(`cstl-gate`) is that service.

```bash
cstl-gate --config gate.json --server-key cstl_adn.db.server_key.pub --listen 127.0.0.1:9300
```

It runs only **named operations** described in its config file. The agent
never supplies SQL, a command or a URL:

```json
{"databases": {"billing": {"driver": "sqlite", "path": "billing.db"},
               "pg": {"driver": "postgres", "dsn": "host=/run/pg dbname=app user=gate"}},
 "operations": {
   "refund_order": {"type": "sql", "db": "billing", "amount_param": "amount",
                    "sql": "UPDATE orders SET refunded = :amount WHERE id = :order_id",
                    "params": {"order_id": "int", "amount": "num"}},
   "backup": {"type": "shell", "argv": ["/usr/bin/pg_dump", "-f", "/backups/{name}.sql", "app"],
              "params": {"name": "word"}},
   "notify": {"type": "http", "url": "https://chat.example/hook", "json": {"text": "{message}"},
              "headers": {"Authorization": "env:CHAT_TOKEN"}, "params": {"message": "text"}}}}
```

- The agent calls `cstl_gate.call(agent, url, "refund_order", {"order_id":
  42, "amount": 80}, amount=80)`. This gets the token, then sends `POST
  /v1/run` with the token in `X-CSTL-Authorization`.
- **Before anything runs**, the gate checks the token itself
  (`CstlGuard`):
  - its signature under the server key;
  - the operation's `action`;
  - the allowed `agents`, if the operation lists them;
  - the amount, which must equal the declared `amount_param`;
  - its time window, and that it was never used.
- **Then it validates every parameter** against its type: `int`, `num`,
  `bool`, `word`, `text`, `{"enum": [...]}` or `{"regex": "..."}`. A
  missing, extra or mistyped parameter is refused.
- **Otherwise nothing runs.** The answer is 401 (no token), 403 (token
  refused), 400 (bad parameter) or 404 (unknown operation).
- **How each type runs:**
  - **SQL:** `:name` placeholders, always bound. SQLite goes through
    `sqlite3`. PostgreSQL goes through `psql`, where `:name` becomes
    `:'name'` and psql does the quoting, so no Python driver is needed.
  - **Shell:** an argument list, run with no shell, so a value can neither
    add an argument nor chain a command.
  - **HTTP:** `env:NAME` secrets come from the gate's environment, never
    from the file or the agent.
- `GET /v1/operations` lists the operations.
- **Limit:** the gate only protects what it alone can do. Removing the
  same access from the agent is part of the setup.

Live test: `sdk/python/test_gate_live.py`. It uses a real SQLite database
and a real throwaway PostgreSQL (skipped if not installed). It checks:
- wrong amount, wrong operation, missing token and reused token, with the
  database left unchanged;
- SQL and shell injection attempts refused, and a hostile text stored
  as-is in PostgreSQL, with the table intact;
- the HTTP secret coming from the gate;
- an operation restricted to other agents.

**In front of any A2A agent.** `sdk/python/cstl_a2a.py` (`cstl-a2a-proxy`)
sits in front of an existing A2A agent (Agent2Agent 1.0, JSON-RPC over
HTTP). It acts as the system that executes the call.

```bash
cstl-a2a-proxy --listen 127.0.0.1:9100 --upstream http://127.0.0.1:9000/a2a \
    --name billing --server-key cstl_adn.db.server_key.pub
```

- **Every JSON-RPC call** must carry a CSTL token in the
  `X-CSTL-Authorization` header, for the action `a2a.<name>.<Method>`
  (1.0 method names; 0.3 names such as `message/send` map to them).
- **The proxy checks the token itself** with `CstlGuard`: signature under
  the server key, action, time window, single use. Only then does it
  forward the call, with `X-CSTL-Agent: <verified caller>`, so the target
  knows who is calling. The token does not go further than the proxy.
- **Otherwise the target receives nothing.** The caller gets HTTP 401 (no
  token) or 403 (refused token), with JSON-RPC error `-32050` and
  `CSTL_<VERDICT>` as the reason.
- **The agent card** at `/.well-known/agent-card.json` is republished:
  `supportedInterfaces[].url` points to the proxy, and the card requires
  `securitySchemes.cstl` (`apiKeySecurityScheme`, that header).
- **SSE streams** (`SendStreamingMessage`) are copied through as they come.
- **Client:** `cstl_a2a.call(agent, url, "billing", "SendMessage", params)`
  gets the token first. If the call is forbidden, it raises `ActionDenied`
  and sends nothing. Rules are written as usual:
  `RELATION [type=PERFORM, subject=<caller>, object=a2a.billing.SendMessage,
  modality=MUST_NOT]`.
- **Batches** (2026-11-25): a JSON-RPC batch carries one token per call, in
  order, comma-separated in the same header (`cstl_a2a.call_batch(agent,
  url, "billing", [("GetTask", {...}), ("SendMessage", {...})])`). It is all
  or nothing: every token is checked before any is used, all must belong to
  the same agent, and one refused token refuses the whole batch. At most 50
  calls.
- **Limit:** the proxy controls which method is called on which agent, not
  the message content.

Live test: `sdk/python/test_a2a_proxy_live.py`. A test A2A agent records
every call it receives, which proves that refused calls never reach it.

**From any JSON client (no CSTL text at all).** `POST /v1/message` on the
REST port (`CSTL_REST_PORT`, default 8000, 127.0.0.1 only) takes the message
as JSON and returns the answer as JSON:

```json
{"version": "v5.0.0", "mode": "A",
 "meta": {"encoder": "a1", "produced_by": "a1", "public_key": "<hex>"},
 "intent": {"purpose": "action_request", "sender": "a1", "receiver": "server",
            "action": "wipe_db", "request_id": "r1", "signature": "<hex>"},
 "relations": []}
```

- The message goes through the TCP listener, so it gets exactly the same
  processing as a CSTL text message: signature, policy, authorization,
  audit.
- Sign the JSON dictionary itself with `cstl_signing.signing_bytes`. The
  server signs and checks the canonical form, not the syntax, so the
  signature is the same.
- The answer holds `intent` and `meta` (the first blocks), `blocks` (all of
  them, in order) and `raw` (the exact text, which is what counts).
- Values must be JSON strings. A value the CSTL wire cannot carry exactly
  (a `"`, a newline, spaces at either end, `---END---`) is refused with
  HTTP 400 and the reason, instead of being altered. Unknown fields are
  refused too.
- **TLS listener** (2026-11-25): when `CSTL_TLS_CERT_PATH` is set, the
  gateway connects in TLS. It trusts `CSTL_GATEWAY_TLS_CA_PATH` (by
  default the server certificate itself), expects the name
  `CSTL_GATEWAY_TLS_SERVER_NAME` (default `localhost`), and presents
  `CSTL_GATEWAY_CLIENT_CERT_PATH` / `CSTL_GATEWAY_CLIENT_KEY_PATH` when the
  server requires mutual authentication. Live test:
  `test_json_gateway_relays_to_a_mutual_tls_listener`.
- Limit: every request comes from 127.0.0.1, so they share one per-IP rate
  limit.

Live test: `sdk/python/test_json_gateway_live.py`. It uses a client that
speaks only JSON (standard library HTTP plus signing): a forbidden action is
denied, a tampered signature is refused, an unsigned message is refused, and
another key cannot speak for an agent.

**From any MCP client.** `sdk/python/cstl_mcp_server.py` is an MCP server
(stdio, JSON-RPC, no dependency beyond the SDK). An agent that speaks MCP
(Claude Desktop, Claude Code, other clients) uses CSTL through tools,
without learning the format: `cstl_policy` (its effective policy),
`cstl_authorize` (ask before acting; returns the decision and the signed
token), `cstl_send_compact`, `cstl_send_relations` and
`cstl_compact_guide`. Set `CSTL_AGENT` (the agent's name; its key is
created in `~/.cstl/`), and `CSTL_HOST` / `CSTL_PORT`. Client config:

```json
{"mcpServers": {"cstl": {"command": "python3",
  "args": ["/path/to/Cstl/sdk/python/cstl_mcp_server.py"],
  "env": {"CSTL_AGENT": "agent_support", "CSTL_PORT": "5050"}}}}
```

Live test: `test_mcp_server_exposes_policy_and_authorization` in
`sdk/python/test_action_authorization_live.py`.

**Conditions the server enforces.** A rule can carry extra attributes,
which layer 9 checks itself (not just their text):

| Attribute | The rule applies only... |
|---|---|
| `subject=role:<name>` | to the agents that have this role |
| `valid_from` / `valid_until` | inside this period (ISO 8601 date or unix seconds) |
| `amount_above=X` / `amount_at_most=X` | when the requested `amount` is above X / at most X |
| `if_fact=F` / `unless_fact=F` | when the fact F is true / is false |
| `priority=N` | (ordering, not a condition) see below |

- A **role** is given by `RELATION [type=POSSESSES, subject=<agent>,
  object=role:<name>]`, and taken back with `role=false`.
- A **fact** has three states: true (`RELATION [type=STATE, subject=F,
  object=true]`), false (`object=false`), or unknown (never declared).
  The most recent declaration counts. The effective policy reports each
  cited fact as `if_fact_now` / `unless_fact_now` = `true`, `false` or
  `unknown`.
- With `CSTL_POLICY_AUTHORS`, only roles and facts from a policy author
  count. So an agent can neither give itself a role nor declare the
  consent that would lift its own prohibition.
- **Prudence:** a condition the server cannot evaluate (no amount in the
  request, an unreadable value, an unknown fact) makes a prohibition apply,
  and an obligation or a permission not apply. So "forbidden to liquidate
  during an incident" (`if_fact=incident_open`) holds until a policy author
  declares `incident_open=false`. Before 2026-11-22, an unknown fact counted
  as false and lifted that prohibition.
- **Priority (exceptions):** with `CSTL_POLICY_AUTHORS`, rules are ranked
  by `priority` (integer from -1000 to 1000, default 0), then by date. So
  an author can write an exception that wins over a general rule, even a
  later one: `MAY refund_order amount_at_most=50 priority=10` beats a later
  `MUST_NOT refund_order`, for amounts up to 50 only. Without
  `CSTL_POLICY_AUTHORS`, priority is ignored and a prohibition always wins,
  since anyone could otherwise lift one with a high priority.
- **Typed relations:** a policy author declares the types of a relation
  type with `RELATION [type=SCHEMA, subject=order_record, object=v1,
  amount=num, date=date]` (types: `num`, `int`, `date`, `bool`, `str`). A
  later relation of that type with a badly typed attribute is refused,
  whole message included: `INTENT_PAYLOAD [purpose=schema_rejected,
  detail=...]`. The latest schema of a type wins; with
  `CSTL_POLICY_AUTHORS`, only schemas from a policy author count. The
  described type follows the usual rule: lowercase (`order_record`), or an
  official or domain operator in uppercase.
- **Amounts:** send `amount=` with `action_request`
  (`agent.authorize("refund_order", amount=80)`). The token then carries
  that amount, and the guard refuses to execute any other
  (`guard.run(token, "refund_order", do_it, amount=80)`).

With the Python SDK: `agent.act("wipe_db", do_it)` runs `do_it`
only if allowed, else raises `ActionDenied`. Live test:
`sdk/python/test_action_authorization_live.py`.

**Making it mandatory.** On its own, the check above is voluntary: an agent
that skips the request is not stopped. To close that gap, make the system
that performs the action (a database, an API, a tool) demand proof:

- Every `allowed` answer carries a token signed with the server's own
  Ed25519 key: `request_id`, `issued_at`, `expires_at`, `audit_hash` and
  `authorization=<signature>`. Tokens expire after `CSTL_AUTHZ_TTL_SECS`
  seconds (default 60).
- The server key lives next to the database, in `<db>.server_key` (mode
  0600, git-ignored), or at `CSTL_SERVER_KEY_PATH`. The public key is
  written to `<db>.server_key.pub`.
- Give that public key to the target system out of band. Never trust the
  `server_key=` echoed in a response: a fake server would put its own key
  there.

Then the agent and the target work like this:

```python
token = agent.authorize("wipe_db")                    # ActionDenied if refused
guard = CstlGuard(open("cstl_adn.db.server_key.pub").read())   # in the target system
guard.run(token, "wipe_db", do_it)                    # Unauthorized otherwise
```

The guard refuses a missing token, a forged token, a token from another
key, a token for another action or agent, an expired token and a reused
token. It can keep its record of used tokens in SQLite (`replay_db=`), so
that record survives a restart. The Rust equivalent is
`server_key::verify_authorization`. Live test:
`sdk/python/test_mandatory_authorization_live.py`.

The same server key also **signs every link of the audit chain**. Startup
re-verifies each signature and refuses to start if one is invalid,
missing or stripped. That catches a full rewrite of the database with
recomputed hashes. A signed marker next to the key
(`<db>.server_key.audit_since`) records the last link written. If the
chain now stops before that link, its last links were deleted, and
startup is refused. If the history is empty, the database was deleted or
reset: the server logs a warning and starts over, signed from `seq=0`.
Set `CSTL_SERVER_PUBLIC_KEY=<hex>` to pin the key, so that a swapped key
file is caught too. Tests: `tests/audit_integrity_test.rs`.

**Revoking a stolen agent key.** A key that leaked must stop working at
once, and the thief must not be able to keep the name. Spec section 5.5.

```python
owner.revoke_key("agent_support", reason="key leaked")   # a policy author, any agent
bot.revoke_key()                                          # or the agent itself, its own key
owner.enroll_key("agent_support", new_public_key_hex)     # then the agent registers again
```

- From then on, every message signed with that key is refused
  (`signature_rejected`, `reason=key_revoked`), including a new
  `agent_register`.
- **Rules, facts, roles, schemas and tree commits signed by that key no
  longer count**, even the ones written before the revocation, since nobody
  knows when the key was stolen.
- The usual rotation proof does not help a thief: it is signed by the old
  key, which the thief also has. A revoked name comes back only with a new
  key enrolled by a policy author (`key_enroll`, used once). Otherwise:
  `reason=key_revoked_enrollment_required`. A revoked key cannot be
  enrolled.
- The request carries `policy_seq`, like a policy (the SDK adds it). Only the
  agent itself, for its current key, or a policy author may revoke.
- TypeScript: `revokeKey`, `enrollKey`. Live test:
  `sdk/python/test_revocation_live.py`. A thief holding the key, and even a
  valid rotation proof, is refused.

**Emergency stop.** It wins over every rule. Spec section 7.4.

```python
owner.emergency_stop("all", reason="incident")      # or "agent:<name>", "action:refund_*"
owner.emergency_stop("action:refund_*", on=False)   # lift it
```

- While a stop covers a request, `action_request` answers `decision=denied,
  reason=emergency_stop, rule=<scope>`, whatever the rules say, and no
  amount is counted. `agent.policy()["emergency_stop"]` lists the stops that
  concern the agent.
- Only a policy author (`CSTL_POLICY_AUTHORS`) may set or lift a stop.
  `policy_seq` is required, so an old "off" message that is replayed or
  delayed is refused (`stale_policy`).
- **Operator switch:** `touch cstl_adn.db.emergency_stop`, next to the
  server key (or `CSTL_EMERGENCY_STOP_FILE`), stops everything, with
  `rule=operator`. No message can lift it, not even one signed with a
  stolen author key. Only removing the file does.
- **Limit:** a token already issued stays valid until it expires
  (`CSTL_AUTHZ_TTL_SECS`, 60 s by default). Lower that TTL when a stop must
  bite faster.

**Human approval for risky actions.** Some actions should not run until a
person says yes. Spec section 7.5.

```bash
CSTL_APPROVAL_ACTIONS="wipe_*,refund_*" CSTL_APPROVERS="human_*" cargo run --bin cstl_parser
```

```python
token = bot.authorize("wipe_db", wait=300)   # waits up to 5 min for a human
# or: catch ApprovalRequired, show e.approval_id, then authorize(..., approval_id=id)
```

```bash
cstl-approve --agent human_alice list
cstl-approve --agent human_alice approve <approval_id> --reason "checked"
```

- If the rules allow the action, the server answers
  `reason=approval_required` with an `approval_id`, and sends a Telegram
  message when Telegram is configured.
- The approval is signed by the approver's key and audited. It is valid
  **once**, for the same agent, action and amount, within
  `CSTL_APPROVAL_TTL_SECS` (1 h by default).
- The agent cannot approve itself (`self_approval`). A prohibition stays a
  prohibition. An emergency stop wins over an approval.
- With no approver configured, such an action is never allowed.

**Live token check.** A token stays valid for up to 60 s. A revocation or
an emergency stop must also stop the tokens already issued. Spec section
7.6.

- `CstlGuard(key, online=("127.0.0.1", 5050))` asks the server, before
  running, whether the token still holds (`purpose=token_status`). The
  answer is signed with the server key and carries the guard's fresh nonce.
- The guard refuses `revoked`, `stopped`, `expired`, `unknown`, and
  `status_unavailable` (no signed answer), so it is closed by default.
- `CSTL_GUARD_ONLINE=host:port` turns this on in `cstl-mcp-proxy`,
  `cstl-a2a-proxy` and `cstl-gate` without changing their config.

**Abnormal behavior.** With `CSTL_ANOMALY_MAX_PER_MIN=30`, an agent that
sends more than 30 action requests in 60 s is stopped automatically:
`emergency_stop` on `agent:<name>`, set by `anomaly_detector`. It stays
stopped until a policy author lifts the stop. Other agents carry on.

Live test for all three: `sdk/python/test_approval_live.py`. It includes a
fake server that answers `valid` without the real signature; the guard
refuses it.

**Dashboard.** `http://127.0.0.1:8000/v1/dashboard` (REST port,
127.0.0.1 only; JSON at `/v1/dashboard.json`). It is read-only and
refreshes every 10 s. It shows:
- decisions allowed and denied in the last 24 h, and refusals by reason;
- the emergency stops in force, and the operator stop;
- pending approvals;
- revoked keys;
- the last 100 decisions, with agent, action, amount, reason and rule.

**Changing the server key.** Run `cargo run --bin cstl_parser --
--rotate-server-key` with the server stopped. It first verifies the
history with the current key, then creates a new key. The handover is
signed by both keys and recorded in `<db>.server_key.rotations`. Links
already written stay valid under the old key, but only for the seq range
when it was in service, so a stolen old key cannot sign a new link. Then
update `CSTL_SERVER_PUBLIC_KEY`, the guards' key and the witness's
`--server-key`. Live test: `sdk/python/test_secure_defaults_live.py`.

**Rollback to an old copy.** Someone who restores an old copy of the
database *and* its marker together gets an old but valid, signed state.
Nothing on that machine can tell. To catch it, run a witness on **another
machine**. It keeps the last signed link it has seen and only ever moves
forward:

```bash
python3 sdk/python/cstl_witness.py --port 8090 --store witness.json \
    --server-key "$(cat cstl_adn.db.server_key.pub)"     # on the other machine
CSTL_WITNESS_URL=http://<witness-host>:8090 cargo run --bin cstl_parser
```

The server sends each new link to the witness. At startup it asks the
witness for the last link seen, and refuses to start on an older history.
It also refuses if the witness is unreachable. The witness cannot make the
server accept a fake state, because what it returns must carry the
server's signature. Limit: links are sent asynchronously, so links written
just before an abrupt stop may not have reached the witness yet. Live test:
`sdk/python/test_witness_rollback_live.py`.

## Navigating this repo without reading whole files — graphify

The repo is large (~70k lines across Rust, Python, docs). Before opening a
file to find something, query the code graph: it answers in a few hundred
bytes what reading a file costs thousands of tokens for.

```bash
pip install graphifyy          # the cloud environment's setup script does this
graphify update .              # build/refresh graphify-out/ (AST only, no LLM, ~10s)
graphify explain "fetch_json"                 # where a symbol is (file:line) + its links
graphify explain "src/kb_verify.rs::verify_relation()"   # disambiguate by path
graphify affected "parse_payload"             # what depends on it
graphify god-nodes                            # core abstractions (AdnStore, CstlNativeServer, ...)
graphify path "A" "B"                         # shortest link between two symbols
```

`graphify-out/` is git-ignored and goes stale after each commit: rerun
`graphify update .` after editing code.

**Known limit (checked 2026-11-20):** the Rust extraction misses method calls
made from *another file* through a value or a lock guard
(`kb_verifier.verify_relation(...)` in `handler.rs`,
`state.adn_store.lock().await.graph_relations()` in `rest_api.rs`) — the graph
then reports "no affected nodes" although callers exist. Calls inside the
same file, symbol locations and structure are reliable. For cross-file
callers, complete with a targeted `grep -rn "name(" src` before concluding
something is unused.
