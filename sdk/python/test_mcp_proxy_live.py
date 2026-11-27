#!/usr/bin/env python3
"""
test_mcp_proxy_live.py -- cstl_mcp_proxy.py devant un vrai serveur MCP
(stdio), contre le VRAI serveur CSTL (2026-11-25).

Le serveur MCP de test (ecrit ci-dessous) note dans un fichier chaque outil
qu'il execute : le test verifie ainsi qu'un outil refuse n'est JAMAIS
execute, pas seulement que le client recoit un refus.

Saute si target/debug/cstl_parser n'est pas compile.
"""
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
PROXY = SDK / "cstl_mcp_proxy.py"
sys.path.insert(0, str(SDK))
PORT, REST = 5757, 8787

FAKE_MCP = r'''
import json, os, sys
log = open(os.environ["FAKE_MCP_LOG"], "a")
TOOLS = [{"name": n, "description": n, "inputSchema": {"type": "object"}} for n in ("read_logs", "wipe_db", "refund")]
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m:
        continue
    if m["method"] == "initialize":
        r = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif m["method"] == "tools/list":
        r = {"tools": TOOLS}
    elif m["method"] == "tools/call":
        log.write(json.dumps(m["params"]) + "\n"); log.flush()
        r = {"content": [{"type": "text", "text": "fait : " + m["params"]["name"]}]}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32601, "message": "?"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
'''


@pytest.fixture(scope="module")
def cstl():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_mcp_proxy_"))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST)}
    env.pop("CSTL_DEV_MODE", None)
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
    (work / "fake_mcp.py").write_text(FAKE_MCP)
    yield work
    proc.terminate()
    proc.wait(timeout=10)


def set_rules(home, monkeypatch, subject, relations):
    """Un autre agent ecrit la politique (sans CSTL_POLICY_AUTHORS, toute regle compte)."""
    monkeypatch.setenv("HOME", str(home))
    import cstl_llm_agent as m
    owner = m.CstlAgent(f"owner_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert owner.register()
    resp = owner.send_relations("server", [{"type": "PERFORM", "subject": subject, **r} for r in relations])
    assert not resp.is_error, resp.raw


class Session:
    def __init__(self, work, agent, extra=(), port=PORT):
        self.log = work / f"calls_{agent}.jsonl"
        self.log.touch()
        env = {**os.environ, "CSTL_AGENT": agent, "CSTL_PORT": str(port), "HOME": str(work / "home_proxy"),
               "FAKE_MCP_LOG": str(self.log)}
        env.pop("CSTL_SERVER_PUBLIC_KEY", None)
        self.p = subprocess.Popen([sys.executable, str(PROXY), *extra, "--", sys.executable, str(work / "fake_mcp.py")],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  text=True, env=env, bufsize=1)
        self.n = 0

    def ask(self, method, params=None):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params or {}}) + "\n")
        self.p.stdin.flush()
        reply = json.loads(self.p.stdout.readline())
        assert reply["id"] == self.n
        return reply

    def call(self, tool, **arguments):
        return self.ask("tools/call", {"name": tool, "arguments": arguments})["result"]

    def executed(self):
        return [json.loads(x)["name"] for x in self.log.read_text().splitlines()]

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=20)


def test_forbidden_tool_is_never_executed(cstl, monkeypatch):
    agent = f"proxy_{uuid.uuid4().hex[:6]}"
    set_rules(cstl / "home_owner", monkeypatch, agent, [
        {"object": "wipe_db", "modality": "MUST_NOT", "reason": "production"},
        {"object": "refund", "modality": "MUST_NOT", "amount_above": "100"}])
    key = (cstl / "cstl_adn.db.server_key.pub").read_text().strip()
    s = Session(cstl, agent, ["--server-key", key, "--amount-arg", "amount"])
    try:
        assert s.ask("initialize", {"protocolVersion": "2025-06-18"})["result"]["serverInfo"]["name"] == "fake"
        assert [t["name"] for t in s.ask("tools/list")["result"]["tools"]] == ["read_logs", "wipe_db", "refund"]

        assert s.call("read_logs")["content"][0]["text"] == "fait : read_logs"
        r = s.call("wipe_db")
        assert r["isError"] and "forbidden" in r["content"][0]["text"] and "MUST_NOT" in r["content"][0]["text"]
        assert s.call("refund", amount=500)["isError"]
        assert s.call("refund", amount=50)["content"][0]["text"] == "fait : refund"
        assert s.executed() == ["read_logs", "refund"]   # ni wipe_db, ni le remboursement de 500
    finally:
        s.close()


def test_wrong_server_key_closes_the_gate(cstl):
    # Une cle qui n'est pas celle du serveur : les jetons ne se verifient
    # pas, donc rien ne passe, meme un outil permis.
    import cstl_signing
    _, other_key, _ = cstl_signing.generate_keypair()
    s = Session(cstl, f"proxy_{uuid.uuid4().hex[:6]}", ["--server-key", other_key])
    try:
        r = s.call("read_logs")
        assert r["isError"] and "jeton_invalide:bad_signature" in r["content"][0]["text"]
        assert s.executed() == []
    finally:
        s.close()


def test_unreachable_cstl_server_closes_the_gate(cstl):
    s = Session(cstl, f"proxy_{uuid.uuid4().hex[:6]}", port=1)
    try:
        r = s.call("read_logs")
        assert r["isError"], r
        assert s.executed() == []
    finally:
        s.close()


def test_args_schema_bounds_the_other_arguments(cstl):
    """--args-schema (2026-11-27) : un argument hors schema n'atteint jamais
    le serveur MCP, meme quand l'outil lui-meme est permis par CSTL."""
    schema = cstl / "schema.json"
    schema.write_text(json.dumps({
        "read_logs": {"path": {"regex": "/var/log/app/[a-z0-9_.-]+"}, "lines": "int",
                      "$required": ["path"], "$only": True},
        "refund": {"amount": "num"},
        "$default": "deny"}))
    key = (cstl / "cstl_adn.db.server_key.pub").read_text().strip()
    s = Session(cstl, f"proxy_{uuid.uuid4().hex[:6]}", ["--server-key", key, "--args-schema", str(schema)])
    try:
        assert s.call("read_logs", path="/var/log/app/today.log", lines=10)["content"][0]["text"] == "fait : read_logs"
        for bad, why in [({"path": "/etc/shadow"}, "invalide:path"),
                         ({"path": "/var/log/app/../../etc/shadow"}, "invalide:path"),
                         ({"lines": 5}, "manquant:path"),
                         ({"path": "/var/log/app/a.log", "lines": "beaucoup"}, "invalide:lines"),
                         ({"path": "/var/log/app/a.log", "follow": True}, "en_trop:follow")]:
            r = s.call("read_logs", **bad)
            assert r["isError"] and f"argument_refuse:{why}" in r["content"][0]["text"], (bad, r)
        r = s.call("wipe_db")
        assert r["isError"] and "argument_refuse:outil_hors_schema" in r["content"][0]["text"]
        assert s.call("refund", amount="1e9; DROP")["isError"]
        assert s.call("refund", amount=20)["content"][0]["text"] == "fait : refund"
        assert s.executed() == ["read_logs", "refund"], "seuls les appels conformes ont ete executes"
    finally:
        s.close()


def test_a_badly_written_args_schema_stops_the_proxy_at_start(cstl):
    schema = cstl / "bad_schema.json"
    schema.write_text(json.dumps({"read_logs": {"path": {"regex": "("}}}))
    p = subprocess.run([sys.executable, str(PROXY), "--args-schema", str(schema), "--", sys.executable, "-c", "pass"],
                       env={**os.environ, "CSTL_AGENT": "x", "CSTL_PORT": str(PORT)}, capture_output=True, text=True,
                       timeout=30)
    assert p.returncode != 0 and "mal ecrit" in p.stderr
