#!/usr/bin/env python3
"""
test_revocation_live.py -- revocation des cles et arret d'urgence, contre le
VRAI serveur (2026-11-25).
  - une cle volee, revoquee, ne signe plus rien : ni action, ni rotation vers
    la cle du voleur ; l'agent revient avec une nouvelle cle inscrite par un
    auteur de politique ;
  - une regle signee par une cle d'auteur revoquee ne compte plus ;
  - l'arret d'urgence d'un auteur l'emporte sur toute regle, par portee
    (all, agent:, action:) ; seul un auteur le pose ou le leve ; un vieux
    message rejoue ne le leve pas ;
  - le fichier d'arret de l'operateur arrete tout, et aucun agent ne le leve.
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
REPO = SDK.parent.parent
BINARY = REPO / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT = 6565


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_revoke_"))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": "9595", "CSTL_KB_VERIFY": "off",
           "CSTL_POLICY_AUTHORS": "owner_*"}
    env.pop("CSTL_DEV_MODE", None)
    env.pop("CSTL_EMERGENCY_STOP_FILE", None)
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    yield work
    proc.terminate()
    proc.wait(timeout=10)


@pytest.fixture
def m(server, monkeypatch):
    monkeypatch.setenv("HOME", str(server))
    import cstl_llm_agent
    return cstl_llm_agent


def agent(m, prefix):
    a = m.CstlAgent(f"{prefix}_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert a.register()
    return a


def allowed(a, action, amount=None):
    ok, r = a.request_action(action, amount)
    return ok, r.fields.get("reason") or r.purpose


def rotate(m, a):
    """agent_register vers une NOUVELLE cle, avec la preuve de rotation signee
    par la cle actuelle de `a` (rotation_signature)."""
    import cstl_signing
    _, new_pub, new_priv = cstl_signing.generate_keypair()
    meta = {"encoder": m.ENCODER, "produced_by": m.ENCODER, "public_key": new_pub}
    intent = {"purpose": "agent_register", "sender": a.name, "receiver": "server",
              "name": a.name, "capabilities": "communication"}
    sig = m.sign_intent(bytes.fromhex(new_priv), new_pub, meta=meta, intent=intent, relations=[])
    proof = m.sign_intent(a.priv_bytes, a.pub_key, meta=meta, intent=intent, relations=[])
    payload = a.client.build_payload(
        encoder=m.ENCODER, produced_by=m.ENCODER, purpose="agent_register", sender=a.name,
        receiver="server", relations=[], extra_meta={"public_key": new_pub},
        extra_intent={"name": a.name, "capabilities": "communication", "signature": sig, "rotation_signature": proof})
    return a.client.send_raw(payload)


def test_rotation_proof_works_when_the_key_is_not_revoked(m):
    bot = agent(m, "bot")
    assert rotate(m, bot).purpose == "agent_register_ack"


def test_stolen_key_is_revoked_and_the_agent_comes_back_with_an_enrolled_key(m):
    owner, bot = agent(m, "owner"), agent(m, "bot")
    thief = m.CstlAgent(bot.name, "none", port=PORT)  # meme fichier de cle : la cle est volee
    thief.registered = True
    assert thief.pub_key == bot.pub_key and allowed(thief, "read_logs")[0]

    r = owner.revoke_key(bot.name, reason="cle volee")
    assert r["public_key"] == bot.pub_key
    for who in (bot, thief):
        ok, why = allowed(who, "read_logs")
        assert not ok and why == "key_revoked"

    # Le voleur tente de faire tourner la cle vers la sienne, avec une preuve
    # de rotation signee par l'ancienne cle (qu'il possede) : refuse.
    r = rotate(m, thief)
    assert r.purpose == "agent_register_rejected" and r.fields["reason"] == "key_revoked_enrollment_required"

    # L'agent legitime cree une nouvelle cle ; tant qu'aucun auteur ne l'a
    # inscrite, elle est refusee.
    bot.keyfile.unlink()
    fresh = m.CstlAgent(bot.name, "none", port=PORT)
    assert fresh.pub_key != bot.pub_key
    assert not fresh.register()
    with pytest.raises(m.ControlDenied) as e:
        bot2 = agent(m, "bot")
        bot2.enroll_key(fresh.name, fresh.pub_key)  # un agent ordinaire ne peut pas inscrire
    assert e.value.reason == "not_authorized"
    owner.enroll_key(fresh.name, fresh.pub_key)
    assert fresh.register()
    assert allowed(fresh, "read_logs")[0]
    assert not allowed(thief, "read_logs")[0]
    # La cle revoquee ne peut pas etre reinscrite.
    with pytest.raises(m.ControlDenied) as e:
        owner.enroll_key(fresh.name, bot.pub_key)
    assert e.value.reason == "key_revoked"


def test_rules_signed_by_a_revoked_author_key_no_longer_count(m):
    owner_a, owner_b, bot = agent(m, "owner"), agent(m, "owner"), agent(m, "bot")
    owner_a.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "wipe_db", "modality": "MUST_NOT"}])
    assert not allowed(bot, "wipe_db")[0]
    # Avec la cle volee de owner_b, une exception prioritaire leve l'interdiction.
    owner_b.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "wipe_db",
                                       "modality": "MAY", "priority": "100"}])
    assert allowed(bot, "wipe_db")[0]
    owner_a.revoke_key(owner_b.name, reason="cle volee")
    ok, why = allowed(bot, "wipe_db")
    assert not ok and why == "forbidden"
    assert any(r.get("why") == "identity_not_proven" for r in bot.policy()["ignored"])


def test_self_revocation_and_limits(m):
    bot, other = agent(m, "bot"), agent(m, "bot")
    with pytest.raises(m.ControlDenied) as e:
        bot.revoke_key(other.name)
    assert e.value.reason == "not_authorized"
    assert allowed(other, "read_logs")[0]
    bot.revoke_key(reason="perdue")  # sa propre cle
    assert allowed(bot, "read_logs") == (False, "key_revoked")


def test_emergency_stop_wins_over_every_rule_by_scope(m):
    owner, bot, other = agent(m, "owner"), agent(m, "bot"), agent(m, "bot")
    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "refund_order", "modality": "MAY"}])
    assert allowed(bot, "refund_order", 10)[0] and allowed(bot, "read_logs")[0]

    owner.emergency_stop(f"action:refund_*", reason="fraude")
    assert allowed(bot, "refund_order", 10) == (False, "emergency_stop")
    assert allowed(bot, "read_logs")[0]
    owner.emergency_stop(f"agent:{other.name}")
    assert allowed(other, "read_logs") == (False, "emergency_stop")
    assert allowed(bot, "read_logs")[0]
    assert "action:refund_*" in bot.policy()["emergency_stop"]

    with pytest.raises(m.ControlDenied) as e:
        bot.emergency_stop(f"action:refund_*", on=False)  # un agent ne leve pas un arret
    assert e.value.reason == "not_authorized"
    with pytest.raises(m.ControlDenied) as e:
        owner.emergency_stop("tout")
    assert e.value.reason == "invalid_scope"

    owner.emergency_stop(f"action:refund_*", on=False)
    owner.emergency_stop(f"agent:{other.name}", on=False)
    assert allowed(bot, "refund_order", 10)[0] and allowed(other, "read_logs")[0]


def test_old_stop_message_replayed_or_delayed_is_refused(m):
    owner, bot = agent(m, "owner"), agent(m, "bot")
    sent = []
    real = owner.client.send_raw
    owner.client.send_raw = lambda payload: (sent.append(payload), real(payload))[1]
    owner.emergency_stop("all", on=False)   # message 1 : "off"
    owner.emergency_stop("all", reason="incident")  # message 2 : "on"
    assert allowed(bot, "read_logs") == (False, "emergency_stop")
    replay = real(sent[0])  # l'attaquant renvoie le vieux "off"
    assert replay.purpose == "stale_policy"
    assert allowed(bot, "read_logs") == (False, "emergency_stop")
    owner.client.send_raw = real
    owner.emergency_stop("all", on=False)
    assert allowed(bot, "read_logs")[0]


def test_operator_stop_file_stops_everything_and_no_agent_lifts_it(m, server):
    owner, bot = agent(m, "owner"), agent(m, "bot")
    stop = server / "cstl_adn.db.emergency_stop"
    stop.write_text("maintenance\n")
    try:
        ok, r = bot.request_action("read_logs")
        assert not ok and r.fields["reason"] == "emergency_stop" and r.fields.get("rule") == "operator"
        owner.emergency_stop("all", on=False)
        assert allowed(bot, "read_logs") == (False, "emergency_stop")
        assert bot.policy()["emergency_stop"][0] == "operator"
    finally:
        stop.unlink()
    assert allowed(bot, "read_logs")[0]
