#!/usr/bin/env python3
"""Banc de deliberation : le calcul des scores, la lecture des reponses JSON,
et la condition cstl_v6 de bout en bout avec des modeles deterministes contre
le vrai serveur (2026-11-26). Saute la partie serveur si cstl_parser manque."""
import json
import sys
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import deliberation_benchmark as b  # noqa: E402

CASE = json.loads((REPO / "benchmark" / "deliberation" / "cases" / "med_01.json").read_text())


def test_every_case_has_a_derivable_answer_for_each_action():
    for p in (REPO / "benchmark" / "deliberation" / "cases").glob("*.json"):
        c = json.loads(p.read_text())
        assert c["actions"] and set(c["actions"].values()) <= {"MUST", "MUST_NOT", "MAY"}
        assert "MUST_NOT" in c["actions"].values(), f"{c['id']} : au moins un piege"
        assert all(a.replace("_", "").isalnum() and a.islower() for a in c["actions"]), "noms d'actions identifiants"


def test_scoring_separates_critical_and_minor_errors():
    s = b.score(CASE, {"prescribe_amoxicillin": "MAY", "order_contrast_ct_scan": "MUST_NOT",
                       "notify_attending_physician": "MAY"})
    assert (s["correct"], s["critical"], s["minor"], s["undecided"]) == (1, 1, 1, 1)
    s = b.score(CASE, {"notify_attending_physician": "MUST_NOT"})
    assert s["critical"] == 1, "bloquer une action obligatoire est critique"


def test_json_answers_are_read_strictly():
    text = 'Here: {"prescribe_amoxicillin": "must not", "order_contrast_ct_scan": "FORBIDDEN", "notify_attending_physician": "MUST"}'
    assert b.json_decisions(text, CASE) == {"prescribe_amoxicillin": "MUST_NOT", "notify_attending_physician": "MUST"}
    assert b.json_decisions("no json at all", CASE) == {}


class Fake:
    def __init__(self, replies):
        self.replies, self.n, self.usage = replies, 0, {"prompt_tokens": 0, "completion_tokens": 0}

    def reply(self, system, prompt):
        self.n += 1
        self.usage["prompt_tokens"] += len(system + prompt) // 4
        r = self.replies(prompt, self.n) if callable(self.replies) else self.replies[min(self.n, len(self.replies)) - 1]
        self.usage["completion_tokens"] += len(r) // 4
        return r


def test_cstl_condition_end_to_end_with_deterministic_models():
    if not (REPO / "target" / "debug" / "cstl_parser").exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    truth = CASE["actions"]

    def proposer(prompt, n):
        lines = [f"RELATION [type=PERFORM, subject=ai_agent, object={a}, modality={m}, sigma=0.9]" for a, m in list(truth.items())[:2]]
        return "#!CSTL v5.0.0 MODE=A\n" + "\n".join(lines) + "\n---END---"

    def supporter(name):
        def reply(prompt, n):
            ids = [ln.split("subject=")[1].split(",")[0] for ln in prompt.splitlines() if "type=point" in ln]
            me = [ln for ln in prompt.splitlines() if "type=point" in ln]
            lines = [f"RELATION [type=supports, subject={name}, object={i}, sigma=0.9]" for i, ln in zip(ids, me) if f"{name}:" not in ln]
            return "#!CSTL v5.0.0 MODE=A\n" + "\n".join(lines or [f"RELATION [type=abstains, subject={name}, object=p1, sigma=0.5]"]) + "\n---END---"
        return reply

    proc, port, key = b.start_server()
    try:
        # les noms des participants sont fabriques par le banc : les modeles les lisent dans le message systeme
        names = {}

        def by_name(idx):
            def reply(prompt, n):
                return names[idx](prompt, n)
            return reply

        models = [Fake(by_name(i)) for i in range(3)]
        orig = b.cd.Deliberation.system_prompt

        def capture(self, agent):
            i = self.participants.index(agent)
            names[i] = proposer if i == 0 else supporter(agent)
            return orig(self, agent)

        b.cd.Deliberation.system_prompt = capture
        try:
            dec, tokens, extra = b.run_cstl(CASE, models, port, key)
        finally:
            b.cd.Deliberation.system_prompt = orig
    finally:
        proc.terminate()
    s = b.score(CASE, dec)
    assert s["correct"] == 2 and s["critical"] == 0 and s["undecided"] == 2
    assert tokens > 0 and extra["summary"]["stop_reason"] == "all_points_closed"
