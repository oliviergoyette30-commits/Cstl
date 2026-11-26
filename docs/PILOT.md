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

With Docker (the Dockerfile has not been built yet; registry limits blocked
the development environment on 2026-11-26, so check that `docker build`
works on your machine first):

```bash
docker build -t cstl .
docker run -d --name cstl -p 5050:5050 -v cstl-data:/data \
    -e CSTL_POLICY_AUTHORS=owner_ops \
    -e CSTL_APPROVAL_ACTIONS="refund_*" -e CSTL_APPROVERS="human_*" cstl
docker exec cstl cat /data/cstl_adn.db.server_key.pub    # the server public key
docker exec -it cstl cstl-demo                           # 7 checks, "Tout est conforme : 7/7."
```

Without Docker:

```bash
cargo build --release --bin cstl_parser
pip install ./sdk/python
CSTL_POLICY_AUTHORS=owner_ops ./target/release/cstl_parser
```

Keep the `/data` volume, or the folder you started from. It holds the audit
and the server key.

## 3. Write the rules (day 1)

The policy author, here `owner_ops`, signs the rules with its own key:

```python
from cstl_llm_agent import CstlAgent
owner = CstlAgent("owner_ops", "none", port=5050); owner.register()
owner.publish_policy([
    {"type": "PERFORM", "subject": "agent_support", "object": "refund_order",
     "modality": "MAY", "amount_at_most": "100"},
    {"type": "PERFORM", "subject": "agent_support", "object": "refund_order",
     "modality": "MUST_NOT", "if_fact": "incident_open", "reason": "no refunds during an incident"},
], server_key=open("server_key.pub").read())
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

Watch the dashboard. It runs on the server machine, on 127.0.0.1: use
`docker run --network host` on Linux, or `docker exec`.

```
http://127.0.0.1:8000/v1/dashboard
```

Keep these at hand:
- `cstl-approve --agent human_alice list | approve <id> | reject <id>` for
  human approvals;
- `owner.emergency_stop("all")`, or `touch /data/cstl_adn.db.emergency_stop`,
  to stop everything;
- `owner.revoke_key("agent_support")` if a key leaks.

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
  They do not read the other arguments.
- The server stores everything in SQLite on one machine. There is no
  PostgreSQL backend yet.
- The JSON gateway and the dashboard listen on 127.0.0.1 only.
- No external security audit has been done yet.

## 8. Feedback

Write down what blocked you, what you had to work around, and what you
would need to keep it in production. That is the most useful outcome of a
pilot, even more than the numbers.
