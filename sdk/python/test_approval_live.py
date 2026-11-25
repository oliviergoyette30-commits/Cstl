#!/usr/bin/env python3
"""
test_approval_live.py -- escalade vers un humain et verification des jetons
en direct, contre le VRAI serveur (2026-11-25).
  - une action a accord n'a son jeton qu'apres l'accord SIGNE d'un
    approbateur, pour la meme action et le meme montant, une seule fois ;
  - l'agent ne s'approuve pas lui-meme ; une interdiction reste une
    interdiction ; l'arret d'urgence l'emporte sur un accord ;
  - le systeme qui execute demande au serveur si le jeton vaut encore :
    refus apres une revocation ou un arret d'urgence, meme avant
    l'expiration ; une reponse non signee ou absente fait refuser.
Saute si target/debug/cstl_parser n'est pas compile.
"""
import io
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from contextlib import redirect_stdout
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
REPO = SDK.parent.parent
BINARY = REPO / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT = 6666


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_approval_"))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": "9696", "CSTL_KB_VERIFY": "off",
           "CSTL_POLICY_AUTHORS": "owner_*", "CSTL_APPROVERS": "human_*",
           "CSTL_APPROVAL_ACTIONS": "wipe_*,refund_*", "CSTL_ANOMALY_MAX_PER_MIN": "30"}
    for k in ("CSTL_DEV_MODE", "CSTL_EMERGENCY_STOP_FILE", "CSTL_GOVERNED_ACTIONS"):
        env.pop(k, None)
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
    with redirect_stdout(io.StringIO()):
        a = m.CstlAgent(f"{prefix}_{uuid.uuid4().hex[:6]}", "none", port=PORT)
        assert a.register()
    return a


def guard(server, **kw):
    from cstl_guard import CstlGuard
    return CstlGuard((server / "cstl_adn.db.server_key.pub").read_text(), **kw)


def test_risky_action_needs_a_signed_human_approval_once(m, server):
    bot, human = agent(m, "bot"), agent(m, "human")
    with pytest.raises(m.ApprovalRequired) as e:
        bot.authorize("wipe_db")
    aid = e.value.approval_id
    assert aid and e.value.reason == "approval_required"
    assert bot.authorize("read_logs")  # sans accord requis : direct

    with pytest.raises(m.ControlDenied) as d:
        bot.approve(aid)  # l'agent n'est pas approbateur
    assert d.value.reason == "not_authorized"
    with pytest.raises(m.ApprovalRequired) as e2:
        bot.authorize("wipe_db", approval_id=aid)
    assert e2.value.reason == "approval_pending"

    assert any(r["approval_id"] == aid and r["agent"] == bot.name for r in human.pending_approvals())
    human.approve(aid, reason="verifie")
    token = bot.authorize("wipe_db", approval_id=aid)
    assert guard(server).run(token, "wipe_db", lambda: "fait", agent=bot.name) == "fait"
    with pytest.raises(m.ActionDenied) as again:
        bot.authorize("wipe_db", approval_id=aid)
    assert again.value.reason == "approval_used"
    with pytest.raises(m.ControlDenied) as twice:
        human.approve(aid)
    assert twice.value.reason == "approval_not_pending"


def test_approval_is_for_the_same_amount_and_can_be_rejected(m):
    bot, human = agent(m, "bot"), agent(m, "human")
    with pytest.raises(m.ApprovalRequired) as e:
        bot.authorize("refund_order", 50)
    aid = e.value.approval_id
    human.approve(aid)
    with pytest.raises(m.ActionDenied) as wrong:
        bot.authorize("refund_order", 5000, approval_id=aid)
    assert wrong.value.reason == "approval_mismatch"
    with pytest.raises(m.ActionDenied) as other:
        bot.authorize("refund_invoice", 50, approval_id=aid)
    assert other.value.reason == "approval_mismatch"
    assert bot.authorize("refund_order", 50, approval_id=aid).amount == "50"

    with pytest.raises(m.ApprovalRequired) as e:
        bot.authorize("refund_order", 70)
    human.reject(e.value.approval_id, reason="non")
    with pytest.raises(m.ActionDenied) as rej:
        bot.authorize("refund_order", 70, approval_id=e.value.approval_id)
    assert rej.value.reason == "approval_rejected"


def test_no_self_approval_prohibitions_and_stops_still_win(m):
    owner, bot, human = agent(m, "owner"), agent(m, "bot"), agent(m, "human")
    with pytest.raises(m.ApprovalRequired) as e:
        human.authorize("wipe_logs")
    with pytest.raises(m.ControlDenied) as d:
        human.approve(e.value.approval_id)
    assert d.value.reason == "self_approval"

    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "wipe_db", "modality": "MUST_NOT"}])
    with pytest.raises(m.ActionDenied) as f:
        bot.authorize("wipe_db")
    assert f.value.reason == "forbidden" and not isinstance(f.value, m.ApprovalRequired)

    with pytest.raises(m.ApprovalRequired) as e:
        bot.authorize("refund_order", 10)
    human.approve(e.value.approval_id)
    owner.emergency_stop(f"agent:{bot.name}")
    with pytest.raises(m.ActionDenied) as s:
        bot.authorize("refund_order", 10, approval_id=e.value.approval_id)
    assert s.value.reason == "emergency_stop"
    owner.emergency_stop(f"agent:{bot.name}", on=False)
    assert bot.authorize("refund_order", 10, approval_id=e.value.approval_id)


def test_agent_waits_for_the_human_and_the_cli_approves(m, server):
    bot, human = agent(m, "bot"), agent(m, "human")
    import cstl_approve

    def person():
        for _ in range(50):
            rows = [r for r in human.pending_approvals() if r["agent"] == bot.name]
            if rows:
                out = io.StringIO()
                with redirect_stdout(out):
                    assert cstl_approve.main(["--agent", human.name, "--port", str(PORT), "list"]) == 0
                assert rows[0]["approval_id"] in out.getvalue()
                with redirect_stdout(io.StringIO()):
                    assert cstl_approve.main(["--agent", human.name, "--port", str(PORT), "approve", rows[0]["approval_id"]]) == 0
                return
            time.sleep(0.2)

    t = threading.Thread(target=person)
    t.start()
    token = bot.authorize("wipe_cache", wait=20, poll=0.3)
    t.join()
    assert token.action == "wipe_cache"


def test_live_token_status_catches_revocation_and_stop_before_expiry(m, server):
    owner, bot, other = agent(m, "owner"), agent(m, "bot"), agent(m, "bot")
    g = guard(server, online=("127.0.0.1", PORT))
    t = bot.authorize("read_logs")
    assert g.live_status(t) == "valid"
    assert g.run(t, "read_logs", lambda: 1) == 1

    t2 = bot.authorize("read_logs")
    owner.emergency_stop("action:read_logs")
    with pytest.raises(Exception) as e:
        g.run(t2, "read_logs", lambda: 1)
    assert e.value.reason == "stopped"
    owner.emergency_stop("action:read_logs", on=False)
    assert g.run(t2, "read_logs", lambda: 1) == 1

    t3 = other.authorize("read_logs")
    owner.revoke_key(other.name, reason="volee")
    with pytest.raises(Exception) as e:
        g.run(t3, "read_logs", lambda: 1)
    assert e.value.reason == "revoked"
    assert guard(server).run(t3, "read_logs", lambda: 1) == 1, "sans online : seulement l'expiration protege"


def test_live_status_fails_closed_on_forged_or_missing_answers(m, server):
    bot = agent(m, "bot")
    t = bot.authorize("read_logs")
    from cstl_guard import AuthorizationToken
    unknown = AuthorizationToken(**{**t.__dict__, "audit_hash": "sha256:" + "0" * 64})
    assert guard(server, online=("127.0.0.1", PORT)).live_status(unknown) == "unknown"

    # Un faux serveur qui repond "valid" sans la signature du vrai serveur.
    fake = socket.socket()
    fake.bind(("127.0.0.1", 0))
    fake.listen(1)

    def serve():
        c, _ = fake.accept()
        data = c.recv(65536).decode()
        nonce = data.split("nonce=")[1].split("]")[0]
        c.sendall(("#!CSTL v5.0.0 MODE=A\nINTENT_PAYLOAD [purpose=token_status, audit_hash=" + t.audit_hash +
                   f", status=valid, nonce={nonce}, checked_at=1, signature={'ab' * 64}]\n---END---\n").encode())
        c.close()

    threading.Thread(target=serve, daemon=True).start()
    assert guard(server, online=fake.getsockname()).live_status(t) == "status_unavailable"
    fake.close()
    with pytest.raises(Exception) as e:
        guard(server, online=("127.0.0.1", 1)).run(t, "read_logs", lambda: 1)
    assert e.value.reason == "status_unavailable"


def test_burst_of_requests_trips_an_automatic_stop_that_only_an_author_lifts(m):
    owner, bot, other = agent(m, "owner"), agent(m, "bot"), agent(m, "bot")
    results = [bot.request_action("read_logs")[1].fields.get("reason") for _ in range(31)]
    assert all(r != "emergency_stop" for r in results[:30]), results
    assert results[30] == "emergency_stop"
    ok, r = bot.request_action("read_logs")
    assert not ok and r.fields.get("rule") == f"agent:{bot.name}"
    assert f"agent:{bot.name}" in bot.policy()["emergency_stop"]
    assert other.request_action("read_logs")[0], "les autres agents continuent"
    with pytest.raises(m.ControlDenied):
        bot.emergency_stop(f"agent:{bot.name}", on=False)
    owner.emergency_stop(f"agent:{bot.name}", on=False)
    assert bot.request_action("read_logs")[0], "leve par un auteur : le compteur repart de zero"
