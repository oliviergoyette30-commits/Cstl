#!/usr/bin/env python3
"""
scripts/gemini_tokens.py -- les tokens de chaque format, comptes par le
tokenizer de GEMINI (2026-11-22).

Pourquoi : toutes les mesures de tokens precedentes utilisaient le tokenizer
de Claude. Un ecart entre formats peut dependre du tokenizer ; le recompter
avec celui de Google verifie qu'il tient. L'API countTokens de Gemini est
gratuite et n'appelle aucun modele.

Trois jeux de donnees, les memes que les bancs d'essai :
  politiques   12 politiques de 10 a 60 regles (`format_short_benchmark.py`)
  donnees      20 jeux de 5 types : taches, faits, commandes, evenements,
               negociation (`data_types_benchmark.py`)
  domaines     les 18 domaines de src/domains.rs (`domains_benchmark.py`)

RESULTAT (2026-11-22, tokenizer de gemini-3.5-flash, totaux) :
                  politiques   donnees   domaines
  cstl_symboles      5 540      20 743     20 284
  cstl_court         6 269        -          -
  cstl_compact         -        26 052     21 479
  cstl_tableau       7 575      20 813     22 210
  toon               9 788      24 817     27 399
  json_court         9 236        -          -
  json (compact)    15 064      32 358     30 968
  cstl complet      16 828      38 169     37 127
- Le CSTL sans mot-cle est le plus court partout : 43 % de moins que TOON
  sur les politiques, 16 % sur les donnees, 26 % sur les domaines.
- Avec le tokenizer de Claude (format_short_benchmark.py,
  data_types_benchmark.py), TOON battait le CSTL compact EN LIGNES sur les
  donnees regulieres ; avec celui de Gemini, c'est aussi le cas (24 817
  contre 26 052), mais le mode tableau et le mode sans mot-cle le battent.
  Les modes tableau et sans mot-cle restent a compter avec Claude.

Usage : GEMINI_API_KEY=... python3 scripts/gemini_tokens.py [--model gemini-3.5-flash]
"""
import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import format_comprehension_benchmark as fc  # noqa: E402
import format_short_benchmark as fs  # noqa: E402
import data_types_benchmark as dt  # noqa: E402
import domains_benchmark as dm  # noqa: E402

CACHE = {}


def count(model, key, text):
    if text in CACHE:
        return CACHE[text]
    body = json.dumps({"contents": [{"parts": [{"text": text}]}]}).encode()
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:countTokens"
    for attempt in range(6):
        req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json", "x-goog-api-key": key})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                CACHE[text] = json.load(r)["totalTokens"]
                return CACHE[text]
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 503) and attempt < 5:
                time.sleep(5 * (attempt + 1))
                continue
            raise


def table(title, rows, formats):
    tot = {f: sum(r[f] for r in rows) for f in formats}
    best = min(formats, key=lambda f: tot[f])
    print(f"\n{title} (tokens Gemini, total) :")
    for f in sorted(formats, key=lambda f: tot[f]):
        print(f"  {f:16} {tot[f]:>8}  ({100 * (tot[f] - tot[best]) / tot[best]:+.0f} % par rapport au plus court)")
    return tot


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="gemini-3.5-flash")
    args = ap.parse_args()
    key = os.environ["GEMINI_API_KEY"]
    out = {"model": args.model}

    pol_formats = ("cstl", "json_compact", "cstl_court", "json_court", "toon", "cstl_tableau", "cstl_symboles")
    rows = []
    for size in fc.SIZES:
        for seed in fc.SEEDS:
            rels, _ = fc.generate(size, seed)
            rows.append({f: count(args.model, key, fs.render(f, rels)) for f in pol_formats})
    out["politiques"] = table("Politiques (12)", rows, pol_formats)

    data_formats = ("json", "toon", "cstl", "cstl_compact", "cstl_tableau", "cstl_symboles")
    rows = []
    for kind in dt.TYPES:
        for n in dt.SIZES:
            for seed in dt.SEEDS:
                recs, rtype, subj, obj, _, _ = dt.gen(kind, n, seed)
                rows.append({f: count(args.model, key, dt.render(f, kind, recs, rtype, subj, obj)) for f in data_formats})
    out["donnees"] = table("Donnees (20 jeux, 5 types)", rows, data_formats)

    rows = []
    for name, ops in dm.domains().items():
        rels = dm.scenario(name, ops)
        rows.append({f: count(args.model, key, dm.render(f, rels)) for f in dm.FORMATS})
    out["domaines"] = table("Domaines (18)", rows, dm.FORMATS)

    dest = REPO / "scripts" / "gemini_tokens.json"
    dest.write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n")
    print(f"\nResultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
