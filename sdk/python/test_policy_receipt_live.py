#!/usr/bin/env python3
"""
test_policy_receipt_live.py -- ordre de l'auteur (policy_seq) et recu signe
du serveur, contre le VRAI serveur (2026-11-25). Corrige les deux faiblesses
mesurees par benchmark/control_benchmark.py (modele T2) :
  - deux messages authentiques livres dans le desordre : l'ancien est refuse
    (stale_policy) au lieu de gagner ;
  - un message refuse ou modifie en route n'a pas de recu valide : l'auteur
    le sait (PolicyNotConfirmed) et renvoie.
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
sys.path.insert(0, str(REPO / "conformance"))
PORT = 6464


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_receipt_"))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": "9494", "CSTL_KB_VERIFY": "off",
           "CSTL_POLICY_AUTHORS": "owner_*"}
    env.pop("CSTL_DEV_MODE", None)
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


def test_publish_policy_returns_a_valid_receipt_and_the_rule_applies(server, monkeypatch):
    monkeypatch.setenv("HOME", str(server))
    import cstl_llm_agent as m
    owner = m.CstlAgent(f"owner_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    bot = m.CstlAgent(f"bot_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert owner.register() and bot.register()
    key = (server / "cstl_adn.db.server_key.pub").read_text().strip()
    receipt = owner.publish_policy([{"type": "PERFORM", "subject": bot.name, "object": "wipe_db", "modality": "MUST_NOT"}], key)
    assert receipt["sender"] == owner.name and receipt["audit_hash"].startswith("sha256:")
    assert bot.request_action("wipe_db")[0] is False

    # Mauvaise cle de serveur : aucun recu ne se verifie -> PolicyNotConfirmed.
    import cstl_signing
    _, other, _ = cstl_signing.generate_keypair()
    with pytest.raises(m.PolicyNotConfirmed):
        owner.publish_policy([{"type": "PERFORM", "subject": bot.name, "object": "read_logs", "modality": "MAY"}], other, attempts=1)


def test_out_of_order_message_is_refused_and_forged_receipts_fail(server):
    import run_server_conformance as wire
    import cstl_receipt
    c = wire.Client("127.0.0.1", PORT)
    owner, bot = wire.Agent(c, name=f"owner_{uuid.uuid4().hex[:6]}"), wire.Agent(c, name=f"bot_{uuid.uuid4().hex[:6]}")
    for a in (owner, bot):
        a.send("agent_register", name=a.name, capabilities="communication")
    key = (server / "cstl_adn.db.server_key.pub").read_text().strip()
    rule = lambda mod: [{"type": "PERFORM", "subject": bot.name, "object": "delete_user", "modality": mod}]  # noqa: E731
    v1 = owner.signed(owner.message("inform", rule("MAY"), policy_seq="1"))
    v2 = owner.signed(owner.message("inform", rule("MUST_NOT"), policy_seq="2"))

    raw2 = c.send(wire.render(v2))                          # v2 arrive d'abord
    assert cstl_receipt.verify_receipt(raw2, key, owner.name, 2, wire.canonical(v2))
    raw1 = c.send(wire.render(v1))                          # puis v1, plus ancien
    assert "purpose=stale_policy" in raw1 and "last_seq=2" in raw1
    assert wire.first(bot.ask("delete_user"), "INTENT_PAYLOAD")["decision"] == "denied"   # v2 reste en vigueur

    # Un recu valide pour v2 ne vaut pas pour v1, ni sous une autre cle.
    assert cstl_receipt.verify_receipt(raw2, key, owner.name, 1, wire.canonical(v1)) is None
    assert cstl_receipt.verify_receipt(raw2.replace("policy_seq=2", "policy_seq=3"), key, owner.name, 3, wire.canonical(v2)) is None

    # policy_seq non entier : refuse.
    bad = owner.signed(owner.message("inform", rule("MAY"), policy_seq="abc"))
    assert "purpose=policy_seq_invalid" in c.send(wire.render(bad))
