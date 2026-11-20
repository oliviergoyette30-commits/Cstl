#!/usr/bin/env python3
"""
test_strict_mode_live.py -- CSTL_REQUIRE_SIGNATURES=true contre le VRAI
serveur (2026-11-20).

Sans ce mode, seuls les agents deja enregistres avec une cle doivent signer :
n'importe qui peut envoyer des messages non signes sous un nom libre. Avec ce
mode, tout expediteur doit etre enregistre ET signer. Tourne sur ses propres
ports (CSTL_PORT/CSTL_REST_PORT) pour ne pas gener test_llm_agent_live.py.
Saute si target/debug/cstl_parser n'est pas compile.
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
PORT, REST_PORT = 5252, 8282
REL = [{"type": "STATE", "subject": "door", "object": "open"}]


@pytest.fixture(scope="module")
def strict_server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST_PORT),
           "CSTL_REQUIRE_SIGNATURES": "true"}
    proc = subprocess.Popen([str(BINARY)], cwd=tempfile.mkdtemp(prefix="cstl_strict_"), env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    else:
        proc.terminate()
        pytest.fail("serveur strict injoignable")
    yield
    proc.terminate()
    proc.wait(timeout=10)


def test_strict_mode_requires_registered_signed_senders(strict_server, monkeypatch, tmp_path):
    import cstl_llm_agent as m
    import cstl_signing as cs
    from cstl_client import CstlClient

    client = CstlClient(port=PORT, timeout=20.0)

    # 1. Non signe -> rejete (accepte en mode normal, cf. AGENTS.md Option 2).
    r = client.send_relation("alice", "bob", "inform", REL, encoder="t", produced_by="t")
    assert r.purpose == "signature_rejected" and r.fields.get("reason") == "signature_required_strict_mode", r.raw

    # 2. Signature VALIDE mais cle jetable, nom jamais enregistre -> rejete.
    _, pub, priv = cs.generate_keypair()
    meta = {"encoder": "t", "produced_by": "t", "public_key": pub}
    intent = {"purpose": "inform", "sender": "ghost", "receiver": "bob"}
    sig = cs.sign_intent({"version": "v5.0.0", "mode": "A", "meta": meta, "intent": intent, "relations": REL}, priv)
    payload = client.build_payload(encoder="t", produced_by="t", purpose="inform", sender="ghost",
                                   receiver="bob", relations=REL, extra_meta={"public_key": pub},
                                   extra_intent={"signature": sig})
    r = client.send_raw(payload)
    assert r.fields.get("reason") == "sender_not_registered_strict_mode", r.raw

    # 3. Agent enregistre qui signe -> accepte (l'enregistrement reste possible).
    class Stub(m.LLMProvider):
        def generate(self, prompt, max_tokens=500):
            return ""

    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(m.HermesAgentBrain, "from_env", classmethod(lambda cls: Stub()))
    agent = m.CstlAgent(f"strict_{uuid.uuid4().hex[:10]}", "hermes", port=PORT)
    assert agent.register()
    resp = agent.send_relations("bob", REL)
    assert not resp.is_error, m.describe(resp)
