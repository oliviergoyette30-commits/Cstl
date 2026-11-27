#!/usr/bin/env python3
"""
test_mcp_remote_live.py -- le serveur MCP distant (connecteurs claude.ai)
contre le VRAI serveur CSTL (2026-11-27), par HTTP, comme un client MCP
« Streamable HTTP » : jeton (chemin et Bearer), origine, sessions, outils,
une action refusee et une permise, HTTPS, et le jeton jamais dans le journal.

Saute si target/debug/cstl_parser n'est pas compile.
"""
import json
import os
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
PORT, REST, MCP, MCPS = 5888, 8888, 8766, 8767
TOKEN = "t" * 8 + uuid.uuid4().hex + uuid.uuid4().hex[:8]


def wait(port):
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            return True
        except OSError:
            time.sleep(0.25)
    return False


@pytest.fixture(scope="module")
def remote():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_mcp_remote_"))
    (work / "home").mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith("CSTL_")}
    env.update({"HOME": str(work / "home"), "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST),
                "CSTL_KB_VERIFY": "off"})
    srv = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    assert wait(PORT), "serveur CSTL injoignable"
    # Une regle : agent_claude ne peut pas supprimer de depot.
    sys.path.insert(0, str(SDK))
    old_home = os.environ.get("HOME")
    os.environ["HOME"] = str(work / "home")
    import cstl_llm_agent as m
    owner = m.CstlAgent(f"owner_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert owner.register()
    r = owner.send_relations("server", [{"type": "PERFORM", "subject": "agent_claude", "object": "delete_repo",
                                         "modality": "MUST_NOT", "reason": "irreversible"}])
    assert not r.is_error
    if old_home is not None:
        os.environ["HOME"] = old_home
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=localhost",
                    "-addext", "subjectAltName=DNS:localhost", "-keyout", str(work / "k.pem"),
                    "-out", str(work / "c.pem")], check=True, capture_output=True)
    menv = {**env, "CSTL_AGENT": "agent_claude", "CSTL_MCP_TOKEN": TOKEN}
    logs = [work / "mcp.log", work / "mcps.log"]
    p1 = subprocess.Popen([sys.executable, str(SDK / "cstl_mcp_remote.py"), "--listen", f"127.0.0.1:{MCP}"],
                          env=menv, stdout=subprocess.DEVNULL, stderr=open(logs[0], "w"))
    p2 = subprocess.Popen([sys.executable, str(SDK / "cstl_mcp_remote.py"), "--listen", f"127.0.0.1:{MCPS}",
                           "--tls-cert", str(work / "c.pem"), "--tls-key", str(work / "k.pem")],
                          env=menv, stdout=subprocess.DEVNULL, stderr=open(logs[1], "w"))
    assert wait(MCP) and wait(MCPS), "serveur MCP distant injoignable"
    yield work, logs
    for p in (p1, p2, srv):
        p.terminate()
        p.wait(timeout=10)


def post(url, body, headers=None, ctx=None):
    data = json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method="POST",
                                 headers={"Content-Type": "application/json",
                                          "Accept": "application/json, text/event-stream", **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=30, context=ctx) as r:
            raw = r.read()
            return r.status, dict(r.headers), json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        raw = e.read()
        return e.code, dict(e.headers), json.loads(raw) if raw else None


def init(url, headers=None, ctx=None):
    st, h, r = post(url, {"jsonrpc": "2.0", "id": 1, "method": "initialize",
                          "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                                     "clientInfo": {"name": "test", "version": "1"}}}, headers, ctx)
    assert st == 200, (st, r)
    return h["Mcp-Session-Id"], r


def test_access_is_refused_without_the_token(remote):
    base = f"http://127.0.0.1:{MCP}"
    hello = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}
    assert post(f"{base}/mcp", hello)[0] == 401
    assert post(f"{base}/mcp/{'x' * len(TOKEN)}", hello)[0] == 401
    assert post(f"{base}/mcp", hello, {"Authorization": "Bearer wrong"})[0] == 401
    assert post(f"{base}/mcp/{TOKEN}", hello, {"Origin": "https://evil.example"})[0] == 403
    assert post(f"{base}/mcp/{TOKEN}", hello, {"Origin": "https://claude.ai"})[0] == 200


def test_a_claude_ai_style_session(remote):
    url = f"http://127.0.0.1:{MCP}/mcp/{TOKEN}"
    sid, r = init(url)
    assert r["result"]["protocolVersion"] == "2025-06-18" and "cstl_authorize" in r["result"]["instructions"]
    h = {"Mcp-Session-Id": sid, "MCP-Protocol-Version": "2025-06-18"}
    assert post(url, {"jsonrpc": "2.0", "method": "notifications/initialized"}, h)[0] == 202
    assert post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})[0] == 400, "session obligatoire"
    assert post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}, {"Mcp-Session-Id": "inconnue"})[0] == 404
    st, _, r = post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}, h)
    assert st == 200 and {t["name"] for t in r["result"]["tools"]} == {
        "cstl_policy", "cstl_authorize", "cstl_send_compact", "cstl_send_relations", "cstl_compact_guide"}

    def call(i, name, **args):
        st, _, r = post(url, {"jsonrpc": "2.0", "id": i, "method": "tools/call",
                              "params": {"name": name, "arguments": args}}, h)
        assert st == 200, r
        return r["result"]

    denied = json.loads(call(3, "cstl_authorize", action="delete_repo")["content"][0]["text"])
    assert denied["allowed"] is False and denied["reason"] == "forbidden" and "token" not in denied
    ok = json.loads(call(4, "cstl_authorize", action="read_issues")["content"][0]["text"])
    assert ok["allowed"] is True and ok["token"]["authorization"], ok
    pol = call(5, "cstl_policy", format="compact")["content"][0]["text"]
    assert "delete_repo" in pol

    assert post(url, {"jsonrpc": "2.0", "id": 6, "method": "ping"}, {**h, "MCP-Protocol-Version": "1999-01-01"})[0] == 400
    req = urllib.request.Request(url, method="DELETE", headers={"Mcp-Session-Id": sid})
    with urllib.request.urlopen(req, timeout=10) as resp:
        assert resp.status == 200
    assert post(url, {"jsonrpc": "2.0", "id": 7, "method": "ping"}, h)[0] == 404, "session fermee"


def test_bearer_header_and_https(remote):
    work, _ = remote
    sid, _ = init(f"http://127.0.0.1:{MCP}/mcp", {"Authorization": f"Bearer {TOKEN}"})
    assert sid
    ctx = ssl.create_default_context(cafile=str(work / "c.pem"))
    url = f"https://localhost:{MCPS}/mcp/{TOKEN}"
    sid, _ = init(url, ctx=ctx)
    st, _, r = post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}, {"Mcp-Session-Id": sid}, ctx)
    assert st == 200 and len(r["result"]["tools"]) == 5


def test_the_token_never_reaches_the_log(remote):
    _, logs = remote
    text = "".join(p.read_text() for p in logs)
    assert "POST" in text and TOKEN not in text
