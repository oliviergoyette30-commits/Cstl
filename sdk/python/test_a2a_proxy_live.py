#!/usr/bin/env python3
"""
test_a2a_proxy_live.py -- cstl_a2a.py devant un agent A2A (JSON-RPC sur
HTTP), contre le VRAI serveur CSTL (2026-11-25).

L'agent A2A de test note chaque appel qu'il recoit, avec l'en-tete
X-CSTL-Agent : le test prouve ainsi qu'un appel refuse ne lui parvient
jamais, et qu'un appel accepte lui dit qui appelle.
Saute si target/debug/cstl_parser n'est pas compile.
"""
import json
import os
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
import uuid
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT, REST = 6161, 9191
CALLS = []


class FakeA2A(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        card = {"name": "Billing", "version": "1.0.0", "capabilities": {"streaming": True},
                "supportedInterfaces": [{"url": "http://127.0.0.1:1/a2a", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}],
                "securitySchemes": {}, "skills": [{"id": "refund", "name": "Refund"}]}
        data = json.dumps(card).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        msg = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if isinstance(msg, list):                      # lot JSON-RPC
            out = []
            for c in msg:
                CALLS.append((c["method"], self.headers.get("X-CSTL-Agent"), self.headers.get("X-CSTL-Authorization")))
                out.append({"jsonrpc": "2.0", "id": c["id"], "result": {"ok": c["method"]}})
            data = json.dumps(out).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        CALLS.append((msg["method"], self.headers.get("X-CSTL-Agent"), self.headers.get("X-CSTL-Authorization")))
        if msg["method"] == "SendStreamingMessage":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            for state in ("TASK_STATE_WORKING", "TASK_STATE_COMPLETED"):
                ev = {"jsonrpc": "2.0", "id": msg["id"], "result": {"statusUpdate": {"taskId": "t1", "status": {"state": state}}}}
                self.wfile.write(f"data: {json.dumps(ev)}\n\n".encode())
                self.wfile.flush()
            return
        result = {"message": {"role": "ROLE_AGENT", "messageId": "m2", "parts": [{"text": "ok " + msg["method"]}]}}
        data = json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture(scope="module")
def env():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_a2a_"))
    e = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_KB_VERIFY": "off"}
    e.pop("CSTL_DEV_MODE", None)
    e.pop("CSTL_POLICY_AUTHORS", None)
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=e, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    up_port, px_port = free_port(), free_port()
    upstream = ThreadingHTTPServer(("127.0.0.1", up_port), FakeA2A)
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    import cstl_a2a
    proxy = cstl_a2a.serve(f"127.0.0.1:{px_port}", f"http://127.0.0.1:{up_port}/a2a", "billing",
                           (work / "cstl_adn.db.server_key.pub").read_text().strip())
    threading.Thread(target=proxy.serve_forever, daemon=True).start()
    yield {"work": work, "url": f"http://127.0.0.1:{px_port}/"}
    proxy.shutdown()
    upstream.shutdown()
    proc.terminate()
    proc.wait(timeout=10)


def make_agent(env, monkeypatch, name):
    monkeypatch.setenv("HOME", str(env["work"]))
    import cstl_llm_agent as m
    a = m.CstlAgent(name, "none", port=PORT)
    assert a.register()
    return m, a


def post(url, msg, headers=None):
    req = urllib.request.Request(url, data=json.dumps(msg).encode(),
                                 headers={"Content-Type": "application/json", **(headers or {})}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


SEND = {"message": {"role": "ROLE_USER", "messageId": "m1", "parts": [{"text": "rembourse la commande 42"}]}}


def test_agent_card_points_to_the_proxy_and_requires_cstl(env):
    with urllib.request.urlopen(env["url"] + ".well-known/agent-card.json") as r:
        card = json.loads(r.read())
    assert card["supportedInterfaces"][0]["url"] == env["url"]
    assert card["securitySchemes"]["cstl"]["apiKeySecurityScheme"]["name"] == "X-CSTL-Authorization"
    assert card["securityRequirements"] == [{"schemes": {"cstl": {"list": []}}}]


def test_allowed_call_passes_and_names_the_caller(env, monkeypatch):
    import cstl_a2a
    _, alice = make_agent(env, monkeypatch, f"alice_{uuid.uuid4().hex[:6]}")
    CALLS.clear()
    status, _, body = cstl_a2a.call(alice, env["url"], "billing", "SendMessage", SEND)
    assert status == 200 and json.loads(body)["result"]["message"]["parts"][0]["text"] == "ok SendMessage"
    assert CALLS == [("SendMessage", alice.name, None)]   # le jeton ne va pas plus loin que le proxy

    # Nom 0.3 : meme action que le nom 1.0.
    status, _, body = cstl_a2a.call(alice, env["url"], "billing", "message/send", SEND, version="")
    assert status == 200 and CALLS[-1][0] == "message/send"

    # Flux SSE recopie tel quel.
    status, headers, body = cstl_a2a.call(alice, env["url"], "billing", "SendStreamingMessage", SEND)
    events = [json.loads(x[6:]) for x in body.decode().split("\n\n") if x.startswith("data: ")]
    assert status == 200 and headers["Content-Type"] == "text/event-stream"
    assert [e["result"]["statusUpdate"]["status"]["state"] for e in events] == ["TASK_STATE_WORKING", "TASK_STATE_COMPLETED"]


def test_refused_calls_never_reach_the_agent(env, monkeypatch):
    import cstl_a2a
    m, owner = make_agent(env, monkeypatch, f"owner_{uuid.uuid4().hex[:6]}")
    _, eve = make_agent(env, monkeypatch, f"eve_{uuid.uuid4().hex[:6]}")
    owner.send_relations("server", [{"type": "PERFORM", "subject": eve.name, "object": "a2a.billing.SendMessage",
                                     "modality": "MUST_NOT", "reason": "pas de remboursement"}])
    CALLS.clear()

    # 1. Interdit : le serveur CSTL refuse le jeton, rien n'est envoye.
    with pytest.raises(m.ActionDenied):
        cstl_a2a.call(eve, env["url"], "billing", "SendMessage", SEND)

    # 2. Sans jeton : 401.
    status, r = post(env["url"], {"jsonrpc": "2.0", "id": 7, "method": "SendMessage", "params": SEND})
    assert status == 401 and r["error"]["code"] == cstl_a2a.ERROR_CODE and r["id"] == 7

    # 3. Jeton permis pour GetTask, presente pour SendMessage : 403 wrong_action.
    tok = cstl_a2a.encode_token(eve.authorize(cstl_a2a.action_for("billing", "GetTask")))
    status, r = post(env["url"], {"jsonrpc": "2.0", "id": 8, "method": "SendMessage", "params": SEND},
                     {"X-CSTL-Authorization": tok})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_WRONG_ACTION"

    # 4. Jeton falsifie (action reecrite) : 403 bad_signature.
    import base64
    d = json.loads(base64.urlsafe_b64decode(tok + "=" * (-len(tok) % 4)))
    d["action"] = "a2a.billing.SendMessage"
    forged = base64.urlsafe_b64encode(json.dumps(d).encode()).decode()
    status, r = post(env["url"], {"jsonrpc": "2.0", "id": 9, "method": "SendMessage", "params": SEND},
                     {"X-CSTL-Authorization": forged})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_BAD_SIGNATURE"

    # 5. Jeton valide rejoue : la premiere fois passe, la deuxieme 403 already_used.
    good = cstl_a2a.encode_token(eve.authorize(cstl_a2a.action_for("billing", "GetTask")))
    get = {"jsonrpc": "2.0", "id": 10, "method": "GetTask", "params": {"id": "t1"}}
    assert post(env["url"], get, {"X-CSTL-Authorization": good})[0] == 200
    status, r = post(env["url"], get, {"X-CSTL-Authorization": good})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_ALREADY_USED"

    assert CALLS == [("GetTask", eve.name, None)]          # seul l'appel autorise est arrive


def test_batch_needs_one_token_per_call_and_is_all_or_nothing(env, monkeypatch):
    import cstl_a2a
    m, owner = make_agent(env, monkeypatch, f"owner_{uuid.uuid4().hex[:6]}")
    _, bob = make_agent(env, monkeypatch, f"bob_{uuid.uuid4().hex[:6]}")
    CALLS.clear()
    status, _, body = cstl_a2a.call_batch(bob, env["url"], "billing", [("GetTask", {"id": "t1"}), ("SendMessage", SEND)])
    assert status == 200 and [r["result"]["ok"] for r in json.loads(body)] == ["GetTask", "SendMessage"]
    assert CALLS == [("GetTask", bob.name, None), ("SendMessage", bob.name, None)]

    batch = [{"jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {}},
             {"jsonrpc": "2.0", "id": 2, "method": "CancelTask", "params": {}}]
    t_get = cstl_a2a.encode_token(bob.authorize(cstl_a2a.action_for("billing", "GetTask")))
    t_get2 = cstl_a2a.encode_token(bob.authorize(cstl_a2a.action_for("billing", "GetTask")))
    CALLS.clear()
    # Un jeton par appel : il en manque un.
    status, r = post(env["url"], batch, {"X-CSTL-Authorization": t_get})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_TOKEN_COUNT"
    # Le second jeton est pour GetTask, pas CancelTask : tout le lot est refuse,
    # et le premier jeton n'est pas consomme (tout est verifie avant).
    status, r = post(env["url"], batch, {"X-CSTL-Authorization": f"{t_get},{t_get2}"})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_WRONG_ACTION"
    # Le meme jeton deux fois dans le lot : refuse.
    status, r = post(env["url"], [batch[0], batch[0]], {"X-CSTL-Authorization": f"{t_get},{t_get}"})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_ALREADY_USED"
    # Jetons de deux agents differents : refuse.
    _, eve = make_agent(env, monkeypatch, f"eve_{uuid.uuid4().hex[:6]}")
    t_eve = cstl_a2a.encode_token(eve.authorize(cstl_a2a.action_for("billing", "GetTask")))
    status, r = post(env["url"], [batch[0], batch[0]], {"X-CSTL-Authorization": f"{t_get2},{t_eve}"})
    assert status == 403 and r["error"]["data"][0]["reason"] == "CSTL_MIXED_AGENTS"
    assert CALLS == [], "aucun appel d'un lot refuse n'arrive"

    # Une methode interdite dans le lot : le client leve ActionDenied et n'envoie rien.
    owner.send_relations("server", [{"type": "PERFORM", "subject": bob.name, "object": "a2a.billing.CancelTask", "modality": "MUST_NOT"}])
    with pytest.raises(m.ActionDenied):
        cstl_a2a.call_batch(bob, env["url"], "billing", [("GetTask", {}), ("CancelTask", {})])
    assert CALLS == []
