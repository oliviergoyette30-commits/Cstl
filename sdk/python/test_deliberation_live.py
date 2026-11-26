#!/usr/bin/env python3
"""
test_deliberation_live.py -- moteur de deliberation v6, avec des modeles
deterministes (ScriptedModel), contre le VRAI serveur CSTL (2026-11-26) :
  - consensus -> regle signee -> l'agent vise obtient ou non son jeton ;
  - impasse -> accord humain -> regle ;
  - erreur du serveur (operateur invente) -> renvoyee au modele, corrigee ;
  - participant defaillant -> essais, puis exclusion, sans bloquer les autres ;
  - amendements limites en profondeur ; « politesse structuree » bornee ;
  - deux modalites adoptees pour la meme action -> rien n'est compile ;
  - l'etat envoye reste petit, contrairement a la relecture de l'historique.
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
PORT, REST = 6868, 9868


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_delib_"))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_KB_VERIFY": "off",
           "CSTL_POLICY_AUTHORS": "moderateur*", "CSTL_APPROVERS": "human_*",
           "CSTL_APPROVAL_ACTIONS": "deliberate.*", "CSTL_RATE_LIMIT_PER_IP_PER_MIN": "100000"}
    for k in ("CSTL_DEV_MODE", "CSTL_GOVERNED_ACTIONS", "CSTL_ANOMALY_MAX_PER_MIN"):
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
def d(server, monkeypatch):
    monkeypatch.setenv("HOME", str(server))
    import cstl_deliberation
    return cstl_deliberation


def R(*lines):
    return "#!CSTL v5.0.0 MODE=A\n" + "\n".join(f"RELATION [{ln}]" for ln in lines) + "\n---END---"


def names():
    u = uuid.uuid4().hex[:5]
    return [f"gpt_{u}", f"gem_{u}", f"cla_{u}"], f"bot_{u}", f"moderateur_{u}"


def make(d, server, participants, scripts, moderator, **kw):
    key = (server / "cstl_adn.db.server_key.pub").read_text().strip()
    models = {p: d.ScriptedModel(s) for p, s in zip(participants, scripts)}
    with contextlib.redirect_stdout(io.StringIO()):
        return d.Deliberation(f"t_{uuid.uuid4().hex[:6]}", "should_ai_order_lab_tests", participants, models,
                              port=PORT, moderator=moderator, server_key=key, **kw), models


def agent(d, name):
    import cstl_llm_agent as m
    with contextlib.redirect_stdout(io.StringIO()):
        a = m.CstlAgent(name, "none", port=PORT)
        assert a.register()
    return a


def test_consensus_compiles_to_a_signed_rule_that_the_control_layer_enforces(d, server):
    (a, b, c), bot, mod = names()
    s_a = [R(f"type=PERFORM, subject={bot}, object=order_routine_tests, modality=MAY, sigma=0.8",
             f"type=PERFORM, subject={bot}, object=order_invasive_tests, modality=MUST_NOT, sigma=0.9")]
    s_b = [R(f"type=supports, subject={b}, object=p1, sigma=0.8", f"type=supports, subject={b}, object=p2, sigma=0.9")]
    s_c = [R(f"type=supports, subject={c}, object=p1, sigma=0.7", f"type=supports, subject={c}, object=p2, sigma=0.95")]
    delib, _ = make(d, server, [a, b, c], [s_a, s_b, s_c], mod)
    target = agent(d, bot)
    assert target.request_action("order_invasive_tests")[0], "avant compilation : aucune regle (proposition non adoptee)"
    summary = delib.run()
    assert summary["stop_reason"] == "all_points_closed" and summary["status"] == {"consensus": ["p1", "p2"]}
    assert summary["rounds"] == 1
    out = delib.compile()
    assert out["receipt"]["sender"] == mod and len(out["rules"]) == 2
    ok, r = target.request_action("order_invasive_tests")
    assert not ok and r.fields["reason"] == "forbidden"
    assert target.request_action("order_routine_tests")[0]
    st = delib.moderator.fetch_tree(f"delib_{delib.topic}")
    assert [p["status"] for p in st["points"]] == ["consensus", "consensus"], "etat signe et audite"


def test_deadlock_goes_to_a_human_whose_approval_becomes_the_rule(d, server):
    (a, b, c), bot, mod = names()
    s_a = [R(f"type=PERFORM, subject={bot}, object=order_routine_tests, modality=MAY, sigma=0.8"),
           R(f"type=supports, subject={a}, object=p1, sigma=0.8")]
    s_b = [R(f"type=supports, subject={b}, object=p1, sigma=0.8")]
    s_c = [R(f"type=rejects, subject={c}, object=p1, sigma=0.7, reason=\"signoff on every order\"")]
    delib, _ = make(d, server, [a, b, c], [s_a, s_b, s_c], mod)
    summary = delib.run()
    assert summary["status"] == {"deadlock": ["p1"]}
    assert summary["stop_reason"] in ("no_change_in_a_full_round", "max_rounds")
    assert delib.compile()["rules"] == [], "une impasse n'est jamais compilee seule"
    esc = delib.escalate()
    assert esc[0]["reason"] == "approval_required" and esc[0]["approval_id"]
    human = agent(d, f"human_{uuid.uuid4().hex[:5]}")
    assert any(r["approval_id"] == esc[0]["approval_id"] for r in human.pending_approvals())
    human.approve(esc[0]["approval_id"], reason="pilote valide")
    delib.resolve_escalations()
    assert delib.points["p1"].status == "resolved_by_human"
    rules = delib.compile()["rules"]
    assert rules[0]["reason"].startswith("human_decision")
    assert agent(d, bot).request_action("order_routine_tests")[0]


def test_server_error_is_sent_back_and_the_model_fixes_it(d, server):
    (a, b, c), bot, mod = names()
    bad = R(f"type=AGREES, subject={a}, object=pilot_is_useful, sigma=0.7")
    good = R(f"type=BELIEVES, subject={a}, object=pilot_is_useful, sigma=0.7")
    delib, models = make(d, server, [a, b, c],
                         [[bad, good], [R(f"type=supports, subject={b}, object=p1, sigma=0.8")],
                          [R(f"type=supports, subject={c}, object=p1, sigma=0.8")]], mod)
    summary = delib.run()
    assert summary["status"] == {"consensus": ["p1"]} and summary["retries"] == 1
    first = delib.turns[0]["attempts"]
    assert "E101" in first[0]["error"] and first[1]["ok"]
    assert "type=error" in models[a].calls[1] and "E101" in models[a].calls[1], "l'erreur exacte est renvoyee"


def test_defective_participant_is_excluded_without_blocking_the_others(d, server):
    (a, b, c), bot, mod = names()
    prose = "Je pense que c'est une bonne idee, mais il faut nuancer."
    delib, models = make(d, server, [a, b, c],
                         [[R(f"type=PERFORM, subject={bot}, object=order_routine_tests, modality=MAY, sigma=0.8")],
                          [R(f"type=supports, subject={b}, object=p1, sigma=0.8")], [prose]], mod)
    summary = delib.run()
    assert c in summary["excluded"] and summary["failed_turns"] == 2
    assert len(models[c].calls) == 6, "1 essai + 2 corrections, sur 2 tours"
    assert summary["status"] == {"consensus": ["p1"]}, "les participants actifs concluent"
    assert all(at["error"].startswith(("E_FORMAT", "E_PROSE")) for t in delib.turns if t["agent"] == c for at in t["attempts"])


def test_amendments_are_depth_limited_and_politeness_loops_are_bounded(d, server):
    (a, b, c), bot, mod = names()
    s_a = [R(f"type=PERFORM, subject={bot}, object=order_tests, modality=MAY, sigma=0.8"),
           R(f"type=PERFORM, subject={bot}, object=order_tests_v2, modality=MAY, sigma=0.7, amends=p1"),
           R(f"type=PERFORM, subject={bot}, object=order_tests_v3, modality=MAY, sigma=0.7, amends=p1.1"),
           R(f"type=PERFORM, subject={bot}, object=order_tests_v4, modality=MAY, sigma=0.7, amends=p1.1.1"),
           R(f"type=abstains, subject={a}, object=p1, sigma=0.5")]
    delib, _ = make(d, server, [a, b, c],
                    [s_a, [R(f"type=abstains, subject={b}, object=p1, sigma=0.5")],
                     [R(f"type=abstains, subject={c}, object=p1, sigma=0.5")]], mod, max_rounds=5)
    delib.run()
    assert "p1.1.1" in delib.points and "p1.1.1.1" not in delib.points
    assert any("E_DEPTH" in at.get("error", "") for t in delib.turns for at in t["attempts"])

    # Politesse structuree : chacun soutient tout et ajoute 2 propositions a chaque tour.
    (a, b, c), bot, mod = names()

    def polite(me):
        def reply(prompt, n):
            ids = [ln.split("subject=")[1].split(",")[0] for ln in prompt.splitlines() if "type=point" in ln]
            lines = [f"type=supports, subject={me}, object={i}, sigma=0.9" for i in ids]
            lines += [f"type=BELIEVES, subject={me}, object=idea_{me}_{n}_{k}, sigma=0.6" for k in range(2)]
            return R(*lines)
        return reply

    delib, _ = make(d, server, [a, b, c], [polite(a), polite(b), polite(c)], mod, max_rounds=4, max_points=8)
    summary = delib.run()
    assert len(delib.points) <= 8 and summary["rounds"] <= 4
    assert any(e[1] == "point_limit" for e in delib.events)
    assert summary["tokens"]["prompts"] < summary["tokens"]["full_history_baseline"]


def test_two_adopted_modalities_for_the_same_action_are_not_compiled(d, server):
    (a, b, c), bot, mod = names()
    s_a = [R(f"type=PERFORM, subject={bot}, object=wipe_cache, modality=MAY, sigma=0.8",
             f"type=PERFORM, subject={bot}, object=wipe_cache_nightly, modality=MUST, sigma=0.8")]
    delib, _ = make(d, server, [a, b, c],
                    [s_a, [R(f"type=supports, subject={b}, object=p1, sigma=0.8", f"type=supports, subject={b}, object=p2, sigma=0.8")],
                     [R(f"type=supports, subject={c}, object=p1, sigma=0.8", f"type=supports, subject={c}, object=p2, sigma=0.8")]], mod)
    delib.run()
    # Meme action, deux modalites : on fabrique le conflit en adoptant aussi MUST_NOT wipe_cache.
    delib._add_point({"type": "PERFORM", "subject": bot, "object": "wipe_cache", "modality": "MUST_NOT", "sigma": "0.9"}, a, 1)
    delib.points["p3"].status = "consensus"
    out = delib.compile()
    assert out["conflicts"] and all(r["object"] != "wipe_cache" for r in out["rules"])


def test_refuses_to_run_without_policy_authors(d, tmp_path, monkeypatch):
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    env = {**os.environ, "CSTL_PORT": "6869", "CSTL_REST_PORT": "9869", "CSTL_KB_VERIFY": "off"}
    for k in ("CSTL_POLICY_AUTHORS", "CSTL_DEV_MODE"):
        env.pop(k, None)
    proc = subprocess.Popen([str(BINARY)], cwd=tmp_path, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", 6869), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        monkeypatch.setenv("HOME", str(tmp_path))
        with pytest.raises(RuntimeError, match="CSTL_POLICY_AUTHORS"):
            with contextlib.redirect_stdout(io.StringIO()):
                d.Deliberation("t", "q", ["a1", "a2"], {"a1": d.ScriptedModel(["x"]), "a2": d.ScriptedModel(["x"])}, port=6869)
    finally:
        proc.terminate()
        proc.wait(timeout=10)


def test_manual_mode_prints_the_state_to_paste_and_reads_the_reply(d, server):
    """ManualModel : le moteur affiche quoi coller dans le chatbot et lit sa
    reponse jusqu'a ---END--- (meme flux que le test tripartite, mais avec
    l'etat calcule par le moteur au lieu de tout l'historique)."""
    (a, b, c), bot, mod = names()
    shown = []
    reply = R(f"type=PERFORM, subject={bot}, object=order_routine_tests, modality=MAY, sigma=0.8").splitlines()
    feed = iter(reply)
    manual = d.ManualModel(a, read=lambda: next(feed), write=shown.append)
    key = (server / "cstl_adn.db.server_key.pub").read_text().strip()
    with contextlib.redirect_stdout(io.StringIO()):
        delib = d.Deliberation(f"t_{uuid.uuid4().hex[:6]}", "q", [a, b, c],
                               {a: manual, b: d.ScriptedModel([R(f"type=supports, subject={b}, object=p1, sigma=0.8")]),
                                c: d.ScriptedModel([R(f"type=supports, subject={c}, object=p1, sigma=0.8")])},
                               port=PORT, moderator=mod, server_key=key, max_rounds=1)
    assert delib.run()["status"] == {"consensus": ["p1"]}
    assert "A COLLER DANS" in shown[0] and "deliberation CSTL" in shown[0] and "type=ASK" in shown[0]


def test_decision_mode_ignores_claims_and_accepts_an_empty_reply(d, server):
    """claims=False (deliberation de decision) : une affirmation ne garde pas
    le debat ouvert ; une reponse sans relation = rien a changer, pas une
    erreur (constate avec de vrais modeles le 2026-11-26 : 9 essais inutiles)."""
    (a, b, c), bot, mod = names()
    s_a = [R(f"type=PERFORM, subject={bot}, object=act_one, modality=MAY, sigma=0.8",
             f"type=PERFORM, subject={bot}, object=act_two, modality=MUST_NOT, sigma=0.8",
             f"type=PERFORM, subject={bot}, object=act_three, modality=MUST, sigma=0.8")]
    s_b = [R(*[f"type=supports, subject={b}, object=p{i}, sigma=0.8" for i in (1, 2, 3)],
             f"type=STATE, subject={b}, object=all_positions_given, sigma=0.9")]
    s_c = ["#!CSTL v5.0.0 MODE=A\n---END---"]
    delib, _ = make(d, server, [a, b, c], [s_a, s_b, s_c], mod, claims=False, max_new_points=3)
    summary = delib.run()
    assert summary["retries"] == 0 and summary["failed_turns"] == 0
    assert set(delib.points) == {"p1", "p2", "p3"}, "l'affirmation n'est pas un point"
    assert summary["status"] == {"deadlock": ["p1", "p2", "p3"]}, "c n'a soutenu aucun point : pas de consensus"
    assert any(e[1] == "claim_ignored" for e in delib.events)
