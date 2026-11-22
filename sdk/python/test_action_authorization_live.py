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
    workdir = tempfile.mkdtemp(prefix="cstl_action_")
    proc = subprocess.Popen([str(BINARY)], cwd=workdir, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    proc.workdir = Path(workdir)
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
    # Depuis le 2026-11-21 (signatures obligatoires par defaut), le message
    # non signe est refuse avant meme la couche 9.
    assert r.fields.get("decision") != "allowed", r.raw
    assert r.purpose == "signature_rejected" or r.fields.get("reason") == "sender_not_registered", r.raw


def test_whitelist_policy_allows_only_explicit_obligations(servers, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5454)
    bot.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "backup_db", "modality": "MUST"}])
    allowed, resp = bot.request_action("read_logs")
    assert not allowed and resp.fields.get("reason") == "default_deny_no_explicit_permission"
    assert bot.request_action("backup_db")[0]


# -- Autorite et revision (CSTL_POLICY_AUTHORS, 2026-11-20) ------------------
# Constate par scripts/team_task_benchmark.py : sans auteur de politique, la
# couche 9 refusait des actions correctes (interdiction non levable, regle
# d'un agent non autorise prise en compte).

@pytest.fixture(scope="module")
def authority_server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    proc = start(5555, 8585, {"CSTL_POLICY_AUTHORS": "owner_*"})
    yield proc.workdir
    proc.terminate()
    proc.wait(timeout=10)


def named_agent(m, name, port):
    a = m.CstlAgent(name, "hermes", port=port)
    assert a.register()
    return a


def test_policy_owner_can_lift_a_prohibition(authority_server, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    rule = {"type": "PERFORM", "subject": bot.name, "object": "rotate_keys"}
    owner.send_relations("server", [{**rule, "modality": "MUST_NOT"}])
    allowed, resp = bot.request_action("rotate_keys")
    assert not allowed and resp.fields.get("reason") == "forbidden", resp.raw
    owner.send_relations("server", [{**rule, "modality": "MUST"}])
    allowed, resp = bot.request_action("rotate_keys")
    assert allowed and resp.fields.get("superseded") == "true", resp.raw


def test_rules_from_non_authors_and_forged_senders_are_ignored(authority_server, monkeypatch, tmp_path):
    from cstl_client import CstlClient
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    ops = named_agent(m, f"ops_{uuid.uuid4().hex[:8]}", 5555)
    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "check_backups", "modality": "MUST"}])
    # Agent enregistre et signe, mais pas auteur de politique : ignore.
    ops.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "check_backups", "modality": "MUST_NOT"}])
    allowed, resp = bot.request_action("check_backups")
    assert allowed and resp.fields.get("ignored_rules") == "1", resp.raw
    # Message NON signe qui pretend venir d'un auteur (nom jamais enregistre) : ignore.
    c = CstlClient(port=5555)
    forged = c.send_raw(c.build_payload(
        encoder="t", produced_by="t", purpose="inform", sender=f"owner_{uuid.uuid4().hex[:8]}", receiver="server",
        relations=[{"type": "PERFORM", "subject": bot.name, "object": "check_backups", "modality": "MUST_NOT"}]))
    if forged.is_error:
        # Signatures obligatoires par defaut (2026-11-21) : le faux auteur est
        # refuse a l'entree, sa regle n'est meme pas enregistree.
        assert forged.purpose == "signature_rejected", forged.raw
        expected_ignored = "1"
    else:
        expected_ignored = "2"
    allowed, resp = bot.request_action("check_backups")
    assert allowed and resp.fields.get("ignored_rules") == expected_ignored, resp.raw


# -- Politique effective (2026-11-21) ----------------------------------------
# Banc d'equipe avec la vraie API : un agent obeissait a l'interdiction d'un
# agent sans autorite (que le serveur ignorait), oubliait les obligations
# absentes de sa tache, et perdait la raison des regles (MUST_NOT nu).

def test_policy_query_gives_obligations_reasons_and_ignored_rules(authority_server, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    ops = named_agent(m, f"ops_{uuid.uuid4().hex[:8]}", 5555)
    owner.send_relations("server", [
        {"type": "PERFORM", "subject": bot.name, "object": "check_backups", "modality": "MUST"},
        {"type": "PERFORM", "subject": bot.name, "object": "refund_order", "modality": "MUST_NOT",
         "reason": "pas sans accord du client, et l'accord n'a pas ete donne"},
    ])
    ops.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "check_backups", "modality": "MUST_NOT"}])
    p = bot.policy()
    assert [r["action"] for r in p["required"]] == ["check_backups"], p
    assert [r["action"] for r in p["forbidden"]] == ["refund_order"], p
    assert p["forbidden"][0]["reason"] == "pas sans accord du client, et l'accord n'a pas ete donne", p
    assert p["forbidden"][0]["author"] == owner.name
    assert [(r["action"], r["author"], r["why"]) for r in p["ignored"]] == [("check_backups", ops.name, "not_policy_author")], p
    # Lecture seule : la chaine d'audit n'avance pas.
    _, before = bot.request_action("read_logs")
    bot.policy()
    _, after = bot.request_action("read_logs")
    assert after.audit_seq == before.audit_seq + 1


def test_policy_query_requires_a_proven_identity(authority_server):
    from cstl_client import CstlClient
    c = CstlClient(port=5555)
    r = c.send_raw(c.build_payload(encoder="t", produced_by="t", purpose="policy_query", sender="alice", receiver="server"))
    assert r.purpose in ("policy_denied", "signature_rejected"), r.raw


# -- Conditions verifiees par la couche 9 (2026-11-21) -----------------------
# Roles, periode de validite, limites chiffrees, conditions de faits. Avant,
# "interdit de rembourser sans accord" ne pouvait etre applique que comme
# "interdit de rembourser, point".

def test_amount_limit_is_enforced_by_server_and_guard(authority_server, monkeypatch, tmp_path):
    from cstl_guard import CstlGuard, Unauthorized
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "refund_order",
                                     "modality": "MUST_NOT", "amount_above": "100",
                                     "reason": "au-dela de 100, accord du manager requis"}])
    assert bot.request_action("refund_order", 80)[0]
    allowed, resp = bot.request_action("refund_order", 250)
    assert not allowed and resp.fields.get("reason") == "forbidden", resp.raw
    allowed, resp = bot.request_action("refund_order")       # montant non declare : prudence
    assert not allowed, resp.raw
    allowed, resp = bot.request_action("refund_order", "beaucoup")
    assert not allowed and resp.fields.get("reason") == "invalid_amount", resp.raw
    # Le jeton porte le montant : le systeme vise ne peut pas en executer un autre.
    token = bot.authorize("refund_order", 80)
    assert token.amount == "80", token
    guard = CstlGuard((authority_server / "cstl_adn.db.server_key.pub").read_text())
    with pytest.raises(Unauthorized, match="wrong_amount"):
        guard.run(token, "refund_order", lambda: "rembourse", amount=5000)
    assert guard.run(token, "refund_order", lambda: "rembourse", amount=80) == "rembourse"
    p = bot.policy()
    rule = [r for r in p["forbidden"] if r["action"] == "refund_order"][0]
    assert rule["amount_above"] == "100" and "manager" in rule["reason"], p


def test_fact_condition_lifts_a_prohibition_only_when_an_author_records_it(authority_server, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    fact = f"consent_{uuid.uuid4().hex[:6]}"
    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "share_data",
                                     "modality": "MUST_NOT", "unless_fact": fact}])
    assert not bot.request_action("share_data")[0]
    # L'agent ne peut pas declarer lui-meme l'accord qui leverait son interdiction.
    bot.send_relations("server", [{"type": "STATE", "subject": fact, "object": "true"}])
    assert not bot.request_action("share_data")[0]
    owner.send_relations("server", [{"type": "STATE", "subject": fact, "object": "true"}])
    assert bot.request_action("share_data")[0]
    owner.send_relations("server", [{"type": "STATE", "subject": fact, "object": "false"}])
    assert not bot.request_action("share_data")[0], "fait retire : l'interdiction revient"


def test_unknown_fact_keeps_a_prohibition(authority_server, monkeypatch, tmp_path):
    """2026-11-22 : un fait jamais declare est INCONNU, pas faux. « Interdit de
    liquider s'il y a un incident » reste en place tant que personne n'a dit
    s'il y a un incident ; la politique effective donne l'etat du fait."""
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    fact = f"incident_{uuid.uuid4().hex[:6]}"
    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "liquidate",
                                     "modality": "MUST_NOT", "if_fact": fact}])
    assert not bot.request_action("liquidate")[0], "fait inconnu : l'interdiction doit tenir"
    rule = [r for r in bot.policy()["forbidden"] if r["action"] == "liquidate"][0]
    assert rule["if_fact_now"] == "unknown", rule
    owner.send_relations("server", [{"type": "STATE", "subject": fact, "object": "false"}])
    assert bot.request_action("liquidate")[0], "pas d'incident declare : permis"
    owner.send_relations("server", [{"type": "STATE", "subject": fact, "object": "true"}])
    assert not bot.request_action("liquidate")[0], "incident declare : interdit"


def test_role_rule_and_validity_period(authority_server, monkeypatch, tmp_path):
    m, bot = agent(monkeypatch, tmp_path, 5555)
    owner = named_agent(m, f"owner_{uuid.uuid4().hex[:8]}", 5555)
    role = f"exec_{uuid.uuid4().hex[:6]}"
    owner.send_relations("server", [
        {"type": "PERFORM", "subject": f"role:{role}", "object": "drop_table", "modality": "MUST_NOT"},
        {"type": "PERFORM", "subject": bot.name, "object": "old_freeze", "modality": "MUST_NOT", "valid_until": "2020-01-01"},
        {"type": "PERFORM", "subject": bot.name, "object": "future_freeze", "modality": "MUST_NOT", "valid_from": "2099-01-01"},
    ])
    assert bot.request_action("drop_table")[0], "pas encore membre du role"
    bot.send_relations("server", [{"type": "POSSESSES", "subject": bot.name, "object": f"role:{role}"}])
    assert bot.request_action("drop_table")[0], "un agent ne peut pas se donner un role lui-meme"
    owner.send_relations("server", [{"type": "POSSESSES", "subject": bot.name, "object": f"role:{role}"}])
    allowed, resp = bot.request_action("drop_table")
    assert not allowed and resp.fields.get("reason") == "forbidden", resp.raw
    p = bot.policy()
    assert role in p["roles"] and any(r.get("via_role") == role for r in p["forbidden"]), p
    assert bot.request_action("old_freeze")[0], "regle expiree"
    assert bot.request_action("future_freeze")[0], "regle pas encore en vigueur"
