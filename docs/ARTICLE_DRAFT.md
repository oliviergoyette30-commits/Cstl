# Ask before you act: a signed control layer for AI agents

*Draft, 2026-11-26. Every number below comes from a script in the CSTL
repository, and the raw results are committed next to it.*

AI agents increasingly *do* things: they refund orders, run SQL, call APIs,
delete files. The usual safeguards live inside the agent: a system prompt,
a guardrail model, a rule engine that reads the policy. They share one
weakness: the agent, or whatever sits between the policy and the agent,
can be wrong or compromised, and nothing outside it checks.

CSTL takes another route. The decision to act and the action itself are
separated by a cryptographic token.

## How it works

1. **The agent asks before it acts.** It sends a signed request to the CSTL
   server: "agent_support wants to refund_order, amount 80".
2. **The server decides from signed rules.** Only designated authors can
   write rules, each signed with their own key. The server checks
   conditions itself: amounts, cumulative totals, periods, roles, facts. A
   condition it cannot evaluate keeps a prohibition in force.
3. **If allowed, the server returns a token signed with its own key.** The
   token is bound to that agent, that action and that amount, for a short
   time window. It can be used only once.
4. **The system that executes checks the token itself.** It trusts only the
   server's public key. In practice this is a database gate, an MCP proxy,
   an A2A proxy, or a wrapper in your code. Without a valid token, nothing
   runs.
5. **Everything goes into an audit chain signed by the server.** If one row
   is edited by hand, the server refuses to restart and says which link
   broke.

Around this core:
- revocation of stolen keys;
- an emergency stop that no message can lift;
- human approval for risky actions;
- a live token check;
- OIDC and SPIFFE identities;
- federation between organizations;
- a dashboard.

## What we measured

### An attacked policy channel (deterministic, no model)

`benchmark/control_benchmark.py` has 12 attack cases: replay, reordering,
deletion, insider, substitution. It compares CSTL with no control, a rule
engine, a signed policy, and a signed, versioned policy.

| Violations / false refusals | no control | signed | signed + versioned | CSTL |
|---|---|---|---|---|
| Relay compromised | 13 / 0 | 3 / 5 | 1 / 0 | **0 / 0** |
| Author link compromised | 13 / 0 | 3 / 5 | 1 / 0 | **0 / 0** |

Our first measurement was worse: CSTL was compromised in 7 cases on the
author link. Two server features fixed that the same day:
- per-author sequence numbers;
- a signed receipt, so the author knows its exact message is in force.

### With real models (GPT-5 mini, Llama 4, Claude Haiku 5.5, Mistral)

Real models write the policy and act on it, while a relay is compromised.

- Guardrails that judge the received message were compromised often.
- **CSTL and the signed, versioned policy are statistically tied:** 73
  against 78 perfect cases, McNemar p = 0.18.
- With governed actions: 56 against 58 out of 59, p = 0.50, and zero
  compromised cases for both.

So we do **not** claim that CSTL beats a well-built signed policy with
models. It is as good, and it adds what that policy does not have:
- tokens checked at execution;
- revocation;
- an emergency stop;
- human approval;
- audit.

### Deliberation between models

We built a deliberation engine: numbered points, state instead of history,
every reply checked by the server, closure by consensus or deadlock, and
decisions compiled into signed rules. Then we tested it on 26 cases with a
known answer, 6 of them adversarial:
- an injected "system notice" in a customer message;
- a sanctioned alias hidden among 60 payments;
- an allergy conflict hidden among 80 prescriptions;
- an outdated wiki contradicting the official policy;
- a daily cap exceeded by earlier refunds.

| | correct | critical errors | tokens per case |
|---|---|---|---|
| one agent alone | 100 % | 0 | ~700 |
| free debate (AutoGen-style) | 100 % | 0 | ~18,000 |
| CSTL deliberation | 100 % | 0 | ~4,600 |

The honest reading:
- **Deliberation did not reduce errors here, because a single agent made
  none.** Today's models apply clearly written rules well, even under
  these traps.
- CSTL deliberation costs **4 times fewer tokens** than a free debate.
- But a single agent remains the cheapest.

So the case for CSTL is not "debate makes better decisions". It is
**controlled execution**, plus cheaper deliberation when a decision process
is mandated anyway: a committee, or four-eyes review.

### Data formats for models

- For irregular data written by a model, the strict CSTL form was correct
  48/48 times with a short instruction, against 36/48 for JSON. With
  three examples, the two are tied (46 against 47).
- CSTL reports its own errors, while JSON fails silently.
- For reading, the indented tree scored 36/36 against 35/36 for JSON, with
  39 % fewer tokens.

## Limits

- **A token makes an action mandatory only where the executing system
  demands it.** The gate must be the only path to the privilege.
- **The proxies check the tool or method and the amount**, not the other
  arguments.
- **One server on SQLite.** No external security audit yet.
- **Our model benchmarks** use a single pass per case for deliberation,
  free models, and cases written by one team.

## Try it

```bash
cargo build --bin cstl_parser && pip install ./sdk/python && cstl-demo
```

Or with Docker, see `docs/PILOT.md`. We are looking for teams willing to put
one real risky action of one agent behind CSTL for a few weeks, and to tell
us what broke.
