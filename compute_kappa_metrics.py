#!/usr/bin/env python3
"""Calcule Fleiss' kappa + deux metriques qui ne degenerent pas sous
prevalence extreme (Gwet's AC1, PABAK multi-juges) sur le meme vote brut.

Motif (2026-10-06, demande explicite d'Olivier) : Fleiss' kappa seul
mentirait par defaut pres du plafond de preservation (paradoxe de
prevalence, Feinstein & Cicchetti 1990, deja cite dans
CSTL_SPEC_v5_0.md). La pratique professionnelle standard face a ce
probleme est de rapporter plusieurs metriques en parallele, pas d'en
choisir une seule. Ce script prend le meme tableau de votes bruts
(PRESERVED/NOT_PRESERVED, N juges) et calcule les trois cote a cote.

Usage : remplir VOTES ci-dessous (ou passer un JSON {item: [v1, v2, v3]}
en argument), lancer `python3 compute_kappa_metrics.py [fichier.json]`.

Validation : les votes du kappa diagnostique #2 (voir
claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md) sont inclus comme
exemple par defaut -- doit reproduire kappa=0.1793 exactement.
"""
import json
import sys
from pathlib import Path

P, NP = "P", "NP"

# Votes bruts du kappa #2 (13 items unanimes P omis pour la lisibilite,
# ajoutes programmatiquement ci-dessous -- voir build_default_votes()).
DISAGREEMENTS_K2 = {
    "medium_003":  [NP, P, P],
    "medium_005":  [NP, P, P],
    "complex_003": [NP, P, NP],
    "edge_003":    [NP, P, P],
    "edge_004":    [NP, NP, P],
}
N_UNANIMOUS_K2 = 13  # easy_001-005, medium_001/002/004, complex_001/004/005, edge_002/005


def build_default_votes():
    votes = dict(DISAGREEMENTS_K2)
    for i in range(N_UNANIMOUS_K2):
        votes[f"_unanimous_{i}"] = [P, P, P]
    return votes


def fleiss_kappa(votes: dict):
    """votes: {item: [vote_juge_1, vote_juge_2, ..., vote_juge_n]}, 2 categories."""
    n_items = len(votes)
    n_raters = len(next(iter(votes.values())))
    categories = sorted({v for vs in votes.values() for v in vs})
    assert len(categories) == 2, "cette implementation suppose exactement 2 categories"

    # n_ij : nombre de juges ayant assigne l'item i a la categorie j
    counts = {}
    for item, vs in votes.items():
        counts[item] = {c: vs.count(c) for c in categories}

    total_assignments = n_items * n_raters
    p_j = {c: sum(counts[i][c] for i in votes) / total_assignments for c in categories}

    P_i = {}
    for item in votes:
        s = sum(counts[item][c] ** 2 for c in categories)
        P_i[item] = (s - n_raters) / (n_raters * (n_raters - 1))
    P_bar = sum(P_i.values()) / n_items

    P_e_bar = sum(p_j[c] ** 2 for c in categories)

    kappa = (P_bar - P_e_bar) / (1 - P_e_bar) if P_e_bar != 1 else float("nan")
    return {
        "p_j": p_j,
        "P_bar": P_bar,
        "P_e_bar": P_e_bar,
        "kappa": kappa,
        "n_items": n_items,
        "n_raters": n_raters,
        "unanimous": sum(1 for i in votes if len(set(votes[i])) == 1),
    }


def gwet_ac1(votes: dict):
    """Gwet's AC1 (2008) -- memes P_bar/p_j que Fleiss, mais le terme de
    hasard P_e est fonde sur une difficulte de classification supposee
    uniforme entre juges, pas sur la distribution marginale observee.
    Pour q=2 categories : P_e(gwet) = 2 * p1 * (1 - p1).
    Ne degenere pas (ne tend pas vers 1) quand p1 -> 1 ou p1 -> 0 aussi
    violemment que Fleiss' P_e_bar = p1^2 + p2^2."""
    f = fleiss_kappa(votes)
    p_vals = list(f["p_j"].values())
    assert len(p_vals) == 2
    p1 = p_vals[0]
    P_e_gwet = 2 * p1 * (1 - p1)
    P_a = f["P_bar"]  # meme accord observe moyen que Fleiss
    ac1 = (P_a - P_e_gwet) / (1 - P_e_gwet) if P_e_gwet != 1 else float("nan")
    return {"P_a": P_a, "P_e_gwet": P_e_gwet, "AC1": ac1}


def pabak_multi(votes: dict):
    """PABAK (Byrt, Bishop & Carlin 1993), generalise a N juges par
    accord pairwise moyen (identique au P_bar de Fleiss) : PABAK = 2*P_a - 1.
    Ajuste le biais de prevalence en figeant le terme de hasard a 0.5,
    au lieu de le recalculer sur la distribution observee -- comportement
    oppose a Fleiss (jamais penalise par une prevalence extreme, mais ne
    distingue pas non plus un vrai desaccord residuel d'un hasard a 50/50)."""
    f = fleiss_kappa(votes)
    return {"P_a": f["P_bar"], "PABAK": 2 * f["P_bar"] - 1}


def report(votes: dict, label: str):
    f = fleiss_kappa(votes)
    g = gwet_ac1(votes)
    b = pabak_multi(votes)
    print(f"=== {label} ({f['n_items']} items, {f['n_raters']} juges) ===")
    print(f"Items unanimes            : {f['unanimous']} / {f['n_items']}")
    print(f"p_j (proportions)         : {f['p_j']}")
    print(f"Accord observe (P_bar)    : {f['P_bar']:.4f}")
    print(f"Fleiss kappa              : {f['kappa']:.4f}  (P_e_bar={f['P_e_bar']:.4f})")
    print(f"Gwet's AC1                : {g['AC1']:.4f}  (P_e_gwet={g['P_e_gwet']:.4f})")
    print(f"PABAK                     : {b['PABAK']:.4f}")
    print()


def main():
    if len(sys.argv) > 1:
        votes = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
        report(votes, sys.argv[1])
    else:
        votes = build_default_votes()
        report(votes, "kappa #2 (validation -- doit donner Fleiss kappa=0.1793)")


if __name__ == "__main__":
    main()
