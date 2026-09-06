#!/usr/bin/env python3
"""
test_multi_hop_fidelity_curve.py -- verification automatisee de la logique
de scoring/stub de multi_hop_fidelity_curve.py (pas d'appel reseau ici,
donc pas de dependance a un serveur compile -- voir
test_real_server_end_to_end pour la partie qui en a besoin et se saute
proprement si le binaire n'est pas construit).

Lancer avec: pytest scripts/test_multi_hop_fidelity_curve.py -v
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from multi_hop_fidelity_curve import (  # noqa: E402
    REFERENCE_RELATIONS,
    HOP_DEPTHS,
    rel_key,
    score_content_fidelity,
    stub_relay_hop,
    _seeded_unit_interval,
)


def test_reference_relations_nonempty_and_unique():
    assert len(REFERENCE_RELATIONS) >= 8
    keys = [rel_key(r) for r in REFERENCE_RELATIONS]
    assert len(keys) == len(set(keys)), "des relations de reference dupliquees fausseraient le score"


def test_score_identity_is_perfect():
    assert score_content_fidelity(REFERENCE_RELATIONS, REFERENCE_RELATIONS) == 1.0


def test_score_empty_current_is_zero():
    assert score_content_fidelity(REFERENCE_RELATIONS, []) == 0.0


def test_score_partial_drop():
    current = REFERENCE_RELATIONS[:-1]
    expected = (len(REFERENCE_RELATIONS) - 1) / len(REFERENCE_RELATIONS)
    assert score_content_fidelity(REFERENCE_RELATIONS, current) == expected


def test_regle_stricte_never_drops_or_mutates():
    """La condition 'regle_stricte' doit reproduire une fidelite de 100% --
    c'est la reproduction du chiffre reellement mesure dans
    CSTL_SPEC_v5_0_COMPLETE.md §5, pas une extrapolation. Verifie sur
    plusieurs runs/hops pour couvrir l'espace de seed."""
    current = [dict(r) for r in REFERENCE_RELATIONS]
    for run_id in range(5):
        current = [dict(r) for r in REFERENCE_RELATIONS]
        for hop in range(1, max(HOP_DEPTHS) + 1):
            result = stub_relay_hop(current, "regle_stricte", run_id, hop)
            assert result.dropped == []
            assert result.mutated == []
            current = result.relations
            fidelity = score_content_fidelity(REFERENCE_RELATIONS, current)
            assert fidelity == 1.0


def test_sans_regle_can_drop_or_mutate_over_enough_hops():
    """La condition 'sans_regle' doit, sur assez de runs/hops, produire au
    moins une perte ou mutation -- sinon la calibration serait cassee et le
    rapport pretendrait faussement une degradation qui n'existe jamais."""
    any_event = False
    for run_id in range(30):
        current = [dict(r) for r in REFERENCE_RELATIONS]
        for hop in range(1, max(HOP_DEPTHS) + 1):
            result = stub_relay_hop(current, "sans_regle", run_id, hop)
            if result.dropped or result.mutated:
                any_event = True
            current = result.relations
    assert any_event, "calibration de sans_regle: aucun drop/mutation observe sur 30 runs x 5 hops"


def test_sans_regle_content_fidelity_degrades_or_stays_with_hop_depth():
    """La fidelite moyenne a hop=5 ne doit jamais depasser celle a hop=1
    (le stub ne peut pas 'reparer' une relation perdue a un hop anterieur --
    monotonie attendue de la simulation, pas une propriete d'un vrai LLM)."""
    n_runs = 20
    fidelity_by_hop = {1: [], 5: []}
    for run_id in range(n_runs):
        current = [dict(r) for r in REFERENCE_RELATIONS]
        for hop in range(1, 6):
            result = stub_relay_hop(current, "sans_regle", run_id, hop)
            current = result.relations
            if hop in fidelity_by_hop:
                fidelity_by_hop[hop].append(score_content_fidelity(REFERENCE_RELATIONS, current))
    mean_hop1 = sum(fidelity_by_hop[1]) / n_runs
    mean_hop5 = sum(fidelity_by_hop[5]) / n_runs
    assert mean_hop5 <= mean_hop1


def test_seeded_unit_interval_is_deterministic_and_reproducible():
    a = _seeded_unit_interval("x", 1, 2, 3)
    b = _seeded_unit_interval("x", 1, 2, 3)
    c = _seeded_unit_interval("x", 1, 2, 4)
    assert a == b
    assert 0.0 <= a < 1.0
    assert a != c


def test_stub_always_adds_at_least_one_new_relation():
    current = [dict(r) for r in REFERENCE_RELATIONS]
    result = stub_relay_hop(current, "sans_regle", run_id=0, hop_index=1)
    assert result.added == 1
    assert len(result.relations) >= len(current) - len(result.dropped) + 1
