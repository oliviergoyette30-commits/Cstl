#!/usr/bin/env python3
"""
test_action_authorization_live.py -- Couche 9, autorisation AVANT action,
contre le VRAI serveur (2026-11-20).

L'agent demande au serveur s'il a le droit d'agir ; CstlAgent.act()
n'execute l'action que si elle est autorisee. Deux serveurs sur des ports
dedies : politique par defaut (allow) et liste blanche
(CSTL_ACTION_DEFAULT=deny). Saute si target/debug/cstl_parser n'est pas
compile.
"""
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


def start(port, rest_port, extra_env=None):
    env = {**os.environ, "CSTL_PORT": str(port), "CSTL_REST_PORT": str(rest_port), **(extra_env or {})}
    proc = subprocess.Popen([str(BINARY)], cwd=tempfile.mkdtemp(prefix="cstl_action_"), env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            return proc
        except OSError:
            time.sleep(0.25)
    proc.terminate()
    pytest.fail(f"serveur injoignable sur {port}")


@pytest.fixture(scope="module")
def servers():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    allow = start(5353, 8383)
    deny = start(5454, 8484, {"CSTL_ACTION_DEFAULT": "deny"})
    yield
    for p in (allow, deny):
        p.terminate()
        p.wait(timeout=10)


def agent(monkeypatch, tmp_path, port):
    import cstl_llm_agent as m

    class Stub(m.LLMProvider):
        def generate(self, prompt, max_tokens=500):
            return ""

    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(m.HermesAgentBrain, "from_env", classmethod(lambda cls: Stub()))
    a = m.CstlAgent(f"bot_{uuid.uuid4().hex[:10]}", "hermes", port=port)
    assert a.register()
    return m, a


def test_prohibited_action_is_denied_and_never_executed(servers, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5353)
    bot.send_relations("server", [
        {"type": "PERFORM", "subject": bot.name, "object": "wipe_db", "modality": "MUST_NOT"},
        {"type": "PERFORM", "subject": bot.name, "object": "backup_db", "modality": "MUST"},
    ])
    ran = []
    with pytest.raises(m.ActionDenied) as exc:
        bot.act("wipe_db", lambda: ran.append("wipe"))
    assert ran == [] and exc.value.reason == "forbidden" and exc.value.rule == "MUST_NOT"

    assert bot.act("backup_db", lambda: "ok") == "ok"
    allowed, resp = bot.request_action("read_logs")
    assert allowed and resp.fields.get("reason") == "no_prohibition"

    # Chaque demande laisse SA trace : deux demandes identiques -> deux maillons.
    _, r1 = bot.request_action("read_logs")
    _, r2 = bot.request_action("read_logs")
    assert r1.audit_seq is not None and r2.audit_seq == r1.audit_seq + 1


def test_unsigned_or_unregistered_requests_are_denied(servers):
    from cstl_client import CstlClient
    c = CstlClient(port=5353)
    r = c.send_raw(c.build_payload(encoder="t", produced_by="t", purpose="action_request",
                                   sender="alice", receiver="server", extra_intent={"action": "read_logs"}))
    assert r.fields.get("decision") == "denied" and r.fields.get("reason") == "sender_not_registered", r.raw


def test_whitelist_policy_allows_only_explicit_obligations(servers, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5454)
    bot.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "backup_db", "modality": "MUST"}])
    allowed, resp = bot.request_action("read_logs")
    assert not allowed and resp.fields.get("reason") == "default_deny_no_explicit_permission"
    assert bot.request_action("backup_db")[0]
