# CSTL pilot guide

This guide is for a team that wants to try CSTL on a real agent for two to
four weeks. The goal is to put **one** risky action of **one** agent under
control, measure what happens, and decide whether to keep it.

## 1. Pick the action (day 1)

Choose an action that an agent already takes, or is about to take, and that
you would not let it take blindly. Examples:
- refunding an order;
- running SQL against a production database;
- calling an internal API that sends money, email or data outside;
- deleting or exporting data;
- an MCP tool that writes files or runs commands.

Write down the rules you want in plain words, for example "refunds up to
100 EUR, above that a human decides, never during an incident".

## 2. Start the server (day 1, 15 minutes)

With Docker:

```bash
CSTL_APPROVAL_ACTIONS="refund_*" CSTL_APPROVERS="human_*" docker compose up -d
docker compose exec cstl cstl demo          # 7 checks, "Tout est conforme : 7/7."
docker compose exec cstl cstl status        # reachable, server public key, authors enforced
```

Without Docker:

```bash
cargo build --release --bin cstl_parser
pip install ./sdk/python
cstl init                                   # cstl.json, the author's key, an example policy.cstl
cstl serve                                  # data in ./cstl-data
```

Keep the `cstl-data` volume (Docker) or folder. It holds the audit and the
server key. The server public key, for your gates, is printed by
`cstl status`.

## 3. Write the rules (day 1)

Write the rules in `policy.cstl`, one per line, then publish them. The
author (`owner_ops` by default) signs them, and the command checks the
server's signed receipt:

```
agent_support MAY refund_order <=100
agent_support MUST_NOT refund_order if:incident_open "no refunds during an incident"
```

```bash
cstl policy apply policy.cstl                                   # without Docker
docker compose exec -T cstl cstl policy apply - < policy.cstl   # with Docker
cstl agent add agent_support
cstl check agent_support refund_order --amount 80               # what the server decides
```

`AGENTS.md`, section "Conditions the server enforces", lists every
condition: roles, periods, amounts, totals, facts and priorities.

## 4. Put the action behind a gate (days 2 to 3)

Pick **one** of these, depending on how the action runs today:

| The action is... | Use | The agent no longer has... |
|---|---|---|
| SQL, a shell command, an HTTP call | `cstl-gate` with a named operation | the database password, the command, the API key |
| a tool of an MCP server | `cstl-mcp-proxy` in front of that server | direct access to the MCP server |
| a call to another A2A agent | `cstl-a2a-proxy` in front of that agent | direct access to the agent |
| your own code | `CstlGuard(...).run(token, ...)` around the function | a way to run it without a token |

**The important part:** the gate is mandatory only if it alone holds the
privilege. Remove the agent's direct access, so the gate becomes the only
way in.

Add `CSTL_GUARD_ONLINE=host:5050` to the gate. A revocation or an emergency
stop then also stops the tokens already issued.

## 5. Run for two to four weeks

Watch the dashboard. It listens on 127.0.0.1 of the server machine (inside
the container with Docker: read it with `docker compose exec`, see
`compose.yaml`).

```
http://127.0.0.1:8000/v1/dashboard
```

Keep these at hand:
- `cstl approve --agent human_alice list | approve <id> | reject <id>` for
  human approvals;
- `cstl stop all` (and `cstl stop all --off`), or
  `touch cstl-data/cstl_adn.db.emergency_stop`, to stop everything;
- `cstl revoke agent_support` if a key leaks.

## 6. What to measure

At the end of the pilot, fill this in:

| Question | How to measure |
|---|---|
| How many actions went through? How many were denied, and why? | Dashboard, "Refus par raison (24 h)", or `/v1/dashboard.json`. |
| Were any denials wrong (a legitimate action blocked)? | Review the denied decisions with the team. |
| Did a forbidden action ever run? | It should be impossible behind a gate. Check the system's own logs against the audit. |
| Latency added per action | Time `authorize` plus the gate, against the action alone. |
| Time spent by humans on approvals | Number of approvals multiplied by the time each one took. |
| Could you answer "who did what, under which rule" for an incident? | Take one real decision and trace it in the audit. |

## 7. Known limits to keep in mind

- The proxies control which tool or method is called and with which amount.
  Give the MCP proxy an `--args-schema` to bound the other arguments
  (`AGENTS.md`); the A2A proxy does not read the message content.
- The server stores everything in SQLite on one machine. There is no
  PostgreSQL backend yet.
- The JSON gateway and the dashboard listen on 127.0.0.1 only.
- No external security audit has been done yet.

## 8. Feedback

Write down what blocked you, what you had to work around, and what you
would need to keep it in production. That is the most useful outcome of a
pilot, even more than the numbers.
