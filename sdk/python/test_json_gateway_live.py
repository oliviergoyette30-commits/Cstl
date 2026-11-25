#!/usr/bin/env python3
"""
test_json_gateway_live.py -- entree JSON du serveur (POST /v1/message),
contre le VRAI serveur (2026-11-25).

Le client de ce test ne connait PAS le format texte CSTL : il envoie du JSON
par HTTP (bibliotheque standard) et signe le dictionnaire JSON avec
`cstl_signing.signing_bytes`. Il recoit exactement la protection d'un client
CSTL : enregistrement signe, interdiction appliquee, signature falsifiee
refusee, message non signe refuse, trace d'audit.

Saute si target/debug/cstl_parser n'est pas compile.
"""
import json
import os
import socket
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
sys.path.insert(0, str(SDK))
import cstl_signing  # noqa: E402

PORT, REST = 5656, 8686


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST)}
    env.pop("CSTL_DEV_MODE", None)
    proc = subprocess.Popen([str(BINARY)], cwd=tempfile.mkdtemp(prefix="cstl_json_"), env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", REST), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    else:
        proc.terminate()
        pytest.fail("API REST injoignable")
    yield
    proc.terminate()
    proc.wait(timeout=10)


def post(msg):
    req = urllib.request.Request(f"http://127.0.0.1:{REST}/v1/message", data=json.dumps(msg).encode(),
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


class JsonAgent:
    """Un agent qui ne parle que JSON."""

    def __init__(self, name):
        _, self.pub, self.priv = cstl_signing.generate_keypair()
        self.name = name

    def message(self, purpose, relations=(), sign=True, **intent):
        msg = {"version": "v5.0.0", "mode": "A",
               "meta": {"encoder": "json_client", "produced_by": "json_client", "public_key": self.pub},
               "intent": {"purpose": purpose, "sender": self.name, "receiver": "server", **intent},
               "relations": list(relations)}
        if sign:
            msg["intent"]["signature"] = cstl_signing.sign_intent(msg, self.priv)
        return msg

    def send(self, purpose, relations=(), **intent):
        return post(self.message(purpose, relations, **intent))


def test_json_agent_gets_the_same_protection(server):
    a = JsonAgent(f"json_{uuid.uuid4().hex[:8]}")
    status, r = a.send("agent_register", name=a.name, capabilities="communication")
    assert status == 200 and r["intent"]["purpose"] == "agent_register_ack", r["raw"]

    # Une regle d'interdiction, envoyee en JSON.
    status, r = a.send("inform", [{"type": "PERFORM", "subject": a.name, "object": "wipe_db",
                                   "modality": "MUST_NOT", "reason": "production, jamais"}])
    assert status == 200, r

    status, r = a.send("action_request", action="wipe_db", request_id=uuid.uuid4().hex)
    assert r["intent"]["decision"] == "denied" and r["intent"]["reason"] == "forbidden", r["raw"]
    assert any(b["block"] == "AUDIT" for b in r["blocks"]), r["raw"]

    status, r = a.send("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    assert r["intent"]["decision"] == "allowed", r["raw"]
    assert r["intent"].get("authorization"), "jeton signe par le serveur attendu"


def test_tampered_or_unsigned_json_is_refused(server):
    a = JsonAgent(f"json_{uuid.uuid4().hex[:8]}")
    assert a.send("agent_register", name=a.name, capabilities="communication")[1]["intent"]["purpose"] == "agent_register_ack"

    # Signature calculee, puis action changee apres coup : refuse.
    msg = a.message("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    msg["intent"]["action"] = "wipe_db"
    _, r = post(msg)
    assert r["intent"].get("decision") != "allowed", r["raw"]
    assert r["intent"]["purpose"] == "signature_rejected", r["raw"]

    # Non signe : refuse (signatures obligatoires par defaut).
    _, r = post(a.message("action_request", sign=False, action="read_logs", request_id=uuid.uuid4().hex))
    assert r["intent"].get("decision") != "allowed", r["raw"]

    # Un autre agent ne peut pas parler au nom du premier.
    b = JsonAgent(a.name)
    _, r = b.send("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    assert r["intent"].get("decision") != "allowed", r["raw"]


def test_values_the_wire_cannot_carry_are_refused_before_sending(server):
    a = JsonAgent("json_x")
    for bad in ['avec "guillemets"', "deux\nlignes", " espace", 3]:
        status, r = post(a.message("inform", sign=False, note=bad))
        assert status == 400 and r["error"] == "message_refuse", r
    status, r = post({**a.message("inform"), "extra": "x"})
    assert status == 400 and "champ inconnu" in r["detail"]
