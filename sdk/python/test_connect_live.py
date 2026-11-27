#!/usr/bin/env python3
"""
test_connect_live.py -- le connecteur embarque (cstl_connect) contre le VRAI
serveur CSTL (2026-11-27).

- `@guarded` : un outil permis s'execute ; un outil interdit, un montant
  au-dessus du plafond, un serveur injoignable ou une mauvaise cle publique
  l'empechent de s'executer (le test compte les executions reelles) ;
  fonctions synchrones et async ; signature conservee pour le SDK MCP.
- Un vrai serveur MCP minimal (stdio, JSON-RPC) dont les outils portent
  `@guarded` : le refus revient au client comme erreur d'outil.
- `A2AGuard` : middleware ASGI appele comme le ferait un serveur ASGI ; sans
  jeton, jeton reutilise, mauvaise methode ou lot mixte -> l'application ne
  recoit rien ; avec jeton -> elle recoit X-CSTL-Agent.

Saute si target/debug/cstl_parser n'est pas compile.
"""
import asyncio
import inspect
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT, REST = 5878, 8878


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_connect_"))
    home = work / "home"
    home.mkdir()
    old_home = os.environ.get("HOME")
    os.environ["HOME"] = str(home)
    env = {k: v for k, v in os.environ.items() if not k.startswith("CSTL_")}
    env.update({"CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_KB_VERIFY": "off"})
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    else:
        proc.terminate()
        pytest.fail("serveur CSTL injoignable")
    key = (work / "cstl_adn.db.server_key.pub").read_text().strip()
    yield work, key
    proc.terminate()
    proc.wait(timeout=10)
    if old_home is not None:
        os.environ["HOME"] = old_home


def rules(subject, relations):
    """Sans CSTL_POLICY_AUTHORS, la regle d'un agent enregistre compte."""
    import cstl_llm_agent as m
    owner = m.CstlAgent(f"owner_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert owner.register()
    resp = owner.send_relations("server", [{"type": "PERFORM", "subject": subject, **r} for r in relations])
    assert not resp.is_error, resp.raw


def test_guarded_tools_run_only_with_a_verified_authorization(server):
    import cstl_connect as cc
    _, key = server
    name = f"mcp_{uuid.uuid4().hex[:6]}"
    rules(name, [{"object": "wipe_db", "modality": "MUST_NOT", "reason": "production"},
                 {"object": "refund_order", "modality": "MUST_NOT", "amount_above": "100"}])
    conn = cc.Connector(agent=name, server_key=key, port=PORT)
    ran = []

    @conn.guarded("refund_order", amount_arg="amount")
    def refund_order(order_id: int, amount: float = 10.0) -> str:
        """Rembourse une commande."""
        ran.append(("refund", order_id, amount))
        return f"rembourse {amount}"

    @conn.guarded()
    def wipe_db() -> str:
        ran.append(("wipe",))
        return "efface"

    @conn.guarded("refund_order", amount_arg="amount", on_deny="message")
    async def refund_async(order_id: int, amount: float) -> str:
        ran.append(("refund_async", order_id, amount))
        return "ok async"

    # La signature reste celle de l'outil : un SDK MCP en tire le meme schema.
    assert str(inspect.signature(refund_order)) == "(order_id: int, amount: float = 10.0) -> str"
    assert refund_order.__doc__ == "Rembourse une commande." and refund_order.cstl_action == "refund_order"

    assert refund_order(1, 80) == "rembourse 80"
    assert refund_order(order_id=2) == "rembourse 10.0", "le montant par defaut est celui qui est autorise"
    with pytest.raises(cc.ActionDenied) as e:
        refund_order(3, amount=500)
    assert e.value.reason == "forbidden"
    with pytest.raises(cc.ActionDenied):
        wipe_db()
    assert asyncio.run(refund_async(4, 50)) == "ok async"
    msg = asyncio.run(refund_async(5, 900))
    assert msg.startswith("Refuse par CSTL") and "forbidden" in msg
    assert ran == [("refund", 1, 80), ("refund", 2, 10.0), ("refund_async", 4, 50)], ran


def test_closed_by_default(server):
    import cstl_connect as cc
    import cstl_signing
    _, key = server
    ran = []

    down = cc.Connector(agent=f"mcp_{uuid.uuid4().hex[:6]}", server_key=key, port=1)

    @down.guarded("read_logs")
    def read_logs():
        ran.append(1)

    with pytest.raises(cc.ActionDenied) as e:
        read_logs()
    assert e.value.reason.startswith("cstl_indisponible")

    _, other_key, _ = cstl_signing.generate_keypair()
    fake = cc.Connector(agent=f"mcp_{uuid.uuid4().hex[:6]}", server_key=other_key, port=PORT)

    @fake.guarded("read_logs")
    def read_logs2():
        ran.append(2)

    with pytest.raises(cc.ActionDenied) as e:
        read_logs2()
    assert e.value.reason == "jeton_invalide:bad_signature"
    assert ran == [], "rien ne s'execute sans jeton verifie"

    with pytest.raises(ValueError):
        cc.Connector(agent="x", server_key=None)       # sans cle publique : refuse de demarrer
    with pytest.raises(ValueError):
        down.guarded("x", amount_arg="nope")(lambda a: a)


MCP_SERVER = r'''
import json, os, sys
sys.path.insert(0, os.environ["CSTL_SDK"])
from cstl_connect import guarded, ActionDenied

@guarded("send_email")
def send_email(to: str) -> str:
    open(os.environ["RAN_LOG"], "a").write("send_email\n")
    return "envoye a " + to

@guarded("delete_repo")
def delete_repo(name: str) -> str:
    open(os.environ["RAN_LOG"], "a").write("delete_repo\n")
    return "supprime"

TOOLS = {"send_email": send_email, "delete_repo": delete_repo}
for line in sys.stdin:
    m = json.loads(line)
    if m["method"] == "tools/call":
        p = m["params"]
        try:
            r = {"content": [{"type": "text", "text": TOOLS[p["name"]](**p["arguments"])}]}
        except ActionDenied as e:      # ce que fait un SDK MCP : une erreur d'outil lisible par le modele
            r = {"content": [{"type": "text", "text": str(e)}], "isError": True}
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
'''


def test_an_mcp_server_with_guarded_tools(server):
    work, key = server
    name = f"mcp_{uuid.uuid4().hex[:6]}"
    rules(name, [{"object": "delete_repo", "modality": "MUST_NOT", "reason": "irreversible"}])
    script, log = work / "mcp_server.py", work / f"ran_{name}.log"
    script.write_text(MCP_SERVER)
    log.touch()
    env = {**os.environ, "CSTL_AGENT": name, "CSTL_PORT": str(PORT), "CSTL_SERVER_PUBLIC_KEY": key,
           "CSTL_SDK": str(SDK), "RAN_LOG": str(log)}
    p = subprocess.Popen([sys.executable, str(script)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         text=True, env=env, bufsize=1)
    try:
        def call(i, tool, **args):
            p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": i, "method": "tools/call",
                                      "params": {"name": tool, "arguments": args}}) + "\n")
            p.stdin.flush()
            return json.loads(p.stdout.readline())["result"]
        assert call(1, "send_email", to="a@example.com")["content"][0]["text"] == "envoye a a@example.com"
        r = call(2, "delete_repo", name="prod")
        assert r["isError"] and "forbidden" in r["content"][0]["text"]
    finally:
        p.stdin.close()
        p.wait(timeout=20)
    assert log.read_text().split() == ["send_email"], "delete_repo n'a jamais tourne"


def asgi_post(app, body, headers):
    out = {}

    async def go():
        chunks = [{"type": "http.request", "body": body, "more_body": False}]

        async def receive():
            return chunks.pop(0) if chunks else {"type": "http.disconnect"}

        async def send(msg):
            if msg["type"] == "http.response.start":
                out["status"] = msg["status"]
            elif msg["type"] == "http.response.body":
                out["body"] = out.get("body", b"") + msg.get("body", b"")

        scope = {"type": "http", "method": "POST", "path": "/a2a",
                 "headers": [(k.lower().encode(), v.encode()) for k, v in headers.items()]}
        await app(scope, receive, send)
    asyncio.run(go())
    return out["status"], json.loads(out["body"])


def test_a2a_guard_middleware(server):
    import cstl_a2a
    import cstl_connect as cc
    import cstl_llm_agent as m
    _, key = server
    seen = []

    async def agent_app(scope, receive, send):           # l'agent A2A, qui note ce qu'il recoit
        msg = await receive()
        hdrs = dict(scope["headers"])
        seen.append((json.loads(msg["body"]), hdrs.get(b"x-cstl-agent", b"").decode(),
                     b"x-cstl-authorization" in hdrs))
        data = json.dumps({"jsonrpc": "2.0", "id": 1, "result": {"ok": True}}).encode()
        await send({"type": "http.response.start", "status": 200, "headers": [(b"content-type", b"application/json")]})
        await send({"type": "http.response.body", "body": data})

    app = cc.A2AGuard(agent_app, name="billing", server_key=key)
    caller = m.CstlAgent(f"caller_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert caller.register()
    rules(caller.name, [{"object": "a2a.billing.CancelTask", "modality": "MUST_NOT"}])
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendMessage", "params": {}}).encode()

    status, r = asgi_post(app, body, {})
    assert status == 401 and r["error"]["code"] == -32050

    tok = cstl_a2a.encode_token(caller.authorize(cstl_a2a.action_for("billing", "SendMessage")))
    status, r = asgi_post(app, body, {"X-CSTL-Authorization": tok, "X-CSTL-Agent": "usurpateur"})
    assert status == 200 and r["result"] == {"ok": True}
    assert seen[-1] == (json.loads(body), caller.name, False), "appelant verifie ; jeton et faux en-tete retires"

    status, r = asgi_post(app, body, {"X-CSTL-Authorization": tok})
    assert status == 403 and "CSTL_" in r["error"]["data"][0]["reason"], "jeton deja utilise"

    other = json.dumps({"jsonrpc": "2.0", "id": 2, "method": "GetTask", "params": {}}).encode()
    tok2 = cstl_a2a.encode_token(caller.authorize(cstl_a2a.action_for("billing", "SendMessage")))
    status, r = asgi_post(app, other, {"X-CSTL-Authorization": tok2})
    assert status == 403, "un jeton pour SendMessage ne vaut pas pour GetTask"

    with pytest.raises(m.ActionDenied):
        caller.authorize(cstl_a2a.action_for("billing", "CancelTask"))
    assert len(seen) == 1, "seul l'appel autorise a atteint l'agent"
