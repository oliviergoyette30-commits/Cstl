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
   cargo run --bin cstl_parser &
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
cargo run --bin cstl_parser &
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

`alice` is one of two agents the server bootstraps itself (`src/main.rs`),
pre-registered without a key, so this needs no signing step. Real
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

Run any of them the same way: `cargo run --example <name>`. Each file's
header comment explains what it's actually testing and why, in the same
style as this one.

## If something here doesn't match what you observe

Trust the live output over any written claim, including this file and
`README.md` -- both are maintained by hand and can drift from the code.
If you find a gap, that's a legitimate finding worth reporting, not a
sign you did something wrong.
