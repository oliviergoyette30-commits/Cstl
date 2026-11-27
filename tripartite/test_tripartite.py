#!/usr/bin/env python3
"""Verificateur du test tripartite, sur un faux transcript avec les erreurs
typiques d'un chatbot, contre le VRAI serveur CSTL (2026-11-26).
Saute si target/debug/cstl_parser n'est pas compile."""
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import tripartite as t  # noqa: E402

RULES = "\n".join(t.rules())


def msg(sender, receiver, rels, rules=RULES, agree=True, extra=""):
    blocks = "AGREEMENT_BLOCK [\nSTRENGTH faster_results [sigma=0.8]\n]\n" if agree else ""
    return (f"#!CSTL v5.0.0 MODE=A\n{rules}\nMETA [encoder={sender}, produced_by={sender}, turn=1]\n"
            f"INTENT_PAYLOAD [purpose=propose, sender={sender}, receiver={receiver}]\n{rels}\n{blocks}{extra}---END---")


GOOD = ("RELATION [type=BELIEVES, subject=chatgpt, object=pilot_is_useful, sigma=0.7]\n"
        "RELATION [type=PERFORM, subject=ai_agent, object=order_lab_test, modality=MUST_NOT, sigma=0.8]\n"
        "RELATION [type=DOUBTS, subject=chatgpt, object=liability_is_clear, sigma=0.3]")


def test_checker_reports_each_typical_mistake(tmp_path):
    if not (t.REPO / "target" / "debug" / "cstl_parser").exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    transcript = "\n=====\n".join([
        (HERE / "1_sujet.cstl").read_text(),
        "Voici ma reponse :\n```\n" + msg("chatgpt", "gemini", GOOD) + "\n```",
        msg("gemini", "claude", GOOD.replace("sigma=0.7", "sigma=1.5"), rules="\n".join(t.rules()[:5])),
        msg("claude", "chatgpt",
            "RELATION [type=PERFORM, subject=ai_agent, object=order_lab_test, modality=MUST, sigma=0.6]\n"
            "RELATION [type=LOVES, subject=claude, object=debate, sigma=0.5]\n"
            "RELATION [type=agrees_with, subject=claude, object=gemini, sigma=0.5]"),
        msg("chatgpt", "gemini", GOOD, agree=False, extra="Je pense que c'est tout.\n"),
    ])
    path = tmp_path / "transcript.txt"
    path.write_text(transcript)
    rows = t.check(str(path), out=str(tmp_path / "rapport.md"))
    by = {i: (sender, checks) for i, sender, _, _, checks, _, _ in rows}
    assert len(rows) == 4, "le message du moderateur n'est pas note"
    s, c = by[2]
    assert s == "chatgpt" and not c["aucune prose"] and c["serveur PASS"] and c["regles recopiees"]
    s, c = by[3]
    assert not c["sigma 0-1 partout"] and not c["regles recopiees"]
    assert not c["serveur PASS"], "depuis le 2026-11-26, le serveur refuse sigma=1.5 (invalid_sigma)"
    s, c = by[4]
    assert not c["operateurs officiels"] and not c["serveur PASS"], "operateur invente : refuse par le serveur"
    s, c = by[5]
    assert not c["aucune prose"] and not c["accord/desaccord"]
    assert not c["serveur PASS"], "depuis le 2026-11-26, le serveur refuse une ligne de prose (unrecognized_line)"
    report = (tmp_path / "rapport.md").read_text()
    assert "order_lab_test" in report and "MUST par claude" in report and "MUST_NOT par chatgpt" in report
