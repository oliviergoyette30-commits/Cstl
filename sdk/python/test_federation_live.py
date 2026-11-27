#!/usr/bin/env python3
"""
test_federation_live.py -- deux VRAIS serveurs qui se font confiance
(2026-11-25). Le serveur « acme » atteste qu'un de ses agents a telle cle ;
le serveur « partner », qui connait la cle publique d'acme, l'enregistre sous
acme.<agent> et lui applique SES regles.
  - une attestation modifiee, pour une autre cle ou d'un serveur inconnu est
    refusee ; un agent local ne peut pas prendre un nom acme.* ;
  - une cle revoquee chez acme n'obtient plus d'attestation, et partner refuse
    l'agent des que la derniere attestation expire.
Saute si target/debug/cstl_parser n'est pas compile.
"""
import contextlib
import io
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
A_PORT, B_PORT = 6070, 6071
TTL = 6


def start(port, rest, env_extra):
    work = Path(tempfile.mkdtemp(prefix=f"cstl_fed_{port}_"))
    env = {**os.environ, "CSTL_PORT": str(port), "CSTL_REST_PORT": str(rest), "CSTL_KB_VERIFY": "off", **env_extra}
    env.pop("CSTL_DEV_MODE", None)
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    return proc, work


@pytest.fixture(scope="module")
def servers():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    a, a_work = start(A_PORT, 9070, {"CSTL_SERVER_NAME": "acme", "CSTL_FEDERATION_TTL_SECS": str(TTL)})
    acme_key = (a_work / "cstl_adn.db.server_key.pub").read_text().strip()
    b, b_work = start(B_PORT, 9071, {"CSTL_SERVER_NAME": "partner", "CSTL_FEDERATION_PEERS": f"acme:{acme_key}",
                                      "CSTL_POLICY_AUTHORS": "owner_*"})
    yield a_work
    for p in (a, b):
        p.terminate()
        p.wait(timeout=10)


@pytest.fixture
def m(servers, monkeypatch):
    monkeypatch.setenv("HOME", str(servers))
    import cstl_llm_agent
    return cstl_llm_agent


def agent(m, name, port):
    with contextlib.redirect_stdout(io.StringIO()):
        a = m.CstlAgent(name, "none", port=port)
        assert a.register()
    return a


def test_peer_agent_registers_at_the_partner_and_its_rules_apply(m):
    bot = agent(m, f"bot_{uuid.uuid4().hex[:6]}", A_PORT)
    fed = bot.federated_at("127.0.0.1", B_PORT)
    assert fed.name == f"acme.{bot.name}" and fed.pub_key == bot.pub_key
    assert fed.request_action("read_logs")[0]
    owner = agent(m, f"owner_{uuid.uuid4().hex[:6]}", B_PORT)
    owner.send_relations("server", [{"type": "PERFORM", "subject": fed.name, "object": "wipe_db", "modality": "MUST_NOT"}])
    ok, r = fed.request_action("wipe_db")
    assert not ok and r.fields["reason"] == "forbidden"
    assert bot.request_action("wipe_db")[0], "chez acme, la regle de partner ne s'applique pas"


def test_forged_foreign_or_mismatched_assertions_are_refused(m):
    bot = agent(m, f"bot_{uuid.uuid4().hex[:6]}", A_PORT)
    a = bot.identity_assertion()
    for change, reason in [({"agent": "admin"}, "federation_signature_invalid"),
                           ({"issuer": "evil"}, "federation_unknown_peer"),
                           ({"expires_at": str(int(a["expires_at"]) + 99999)}, "federation_signature_invalid")]:
        with pytest.raises(m.ControlDenied) as e:
            bot.federated_at("127.0.0.1", B_PORT, {**a, **change})
        assert e.value.reason == reason, change
    # Un voleur de l'attestation, avec SA propre cle : refuse.
    thief = agent(m, f"thief_{uuid.uuid4().hex[:6]}", A_PORT)
    with pytest.raises(m.ControlDenied) as e:
        thief.federated_at("127.0.0.1", B_PORT, a)
    assert e.value.reason == "federation_key_mismatch"
    # Un agent local de partner ne peut pas prendre un nom acme.*.
    with contextlib.redirect_stdout(io.StringIO()):
        squat = m.CstlAgent(f"acme.{bot.name}", "none", port=B_PORT)
        assert not squat.register()
    assert squat.last_register_response.fields["reason"] == "federation_name_reserved"


def test_revocation_at_the_issuer_reaches_the_partner_when_the_assertion_expires(m):
    bot = agent(m, f"bot_{uuid.uuid4().hex[:6]}", A_PORT)
    fed = bot.federated_at("127.0.0.1", B_PORT)
    assert fed.request_action("read_logs")[0]
    bot.revoke_key(reason="volee")
    with pytest.raises(m.ControlDenied):
        bot.identity_assertion()
    time.sleep(TTL + 1.5)
    ok, r = fed.request_action("read_logs")
    assert not ok and r.fields.get("reason") == "federation_expired", r.fields
