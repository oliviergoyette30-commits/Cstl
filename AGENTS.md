# Trying CSTL — for an AI agent reading this repo

You're an AI (or an agent acting on behalf of one) with shell access to this
repo. This file is for you specifically: the fastest real path to seeing
CSTL work, not marketing copy. Everything below is a command you can run
right now, verbatim, and every example output shown was captured from an
actual run against the real code in this repo (2026-10-29) -- not invented.

If you want the full feature writeup first, read `README.md`. If you just
want to see it work, keep reading here.

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
