#!/usr/bin/env python3
"""
scripts/multi_model_summary.py -- tableau des trois bancs, modele par modele
(2026-11-23).

Lit les resultats deja enregistres (aucun appel de modele) :
  lecture        scripts/nested_comprehension_<modele>.json  (36 questions par format)
  ecriture       scripts/nested_writing_<modele>.json         (12 documents par format)
  vide qui parle scripts/unknown_benchmark.json               (24 questions par format)
Les jeux en erreur d'API sont exclus du denominateur (x/y).

Usage : python3 scripts/multi_model_summary.py
"""
import json
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
S = REPO / "scripts"


def score(rows, fmt):
    r = [x for x in rows if x.get("format") == fmt and x.get("ok") is not None]
    return sum(sum(x["ok"]) for x in r), sum(len(x["ok"]) for x in r)


def models():
    found = {p.name[len("nested_comprehension_"):-5] for p in S.glob("nested_comprehension_*.json")}
    found |= {p.name[len("nested_writing_"):-5] for p in S.glob("nested_writing_*.json") if "passe" not in p.name}
    return sorted(found)


def main():
    unknown = json.loads((S / "unknown_benchmark.json").read_text()) if (S / "unknown_benchmark.json").exists() else {}
    fmt_cell = lambda a, b: f"{a}/{b}" if b else "-"  # noqa: E731
    print("LECTURE (questions justes)            json      toon      cstl_indent")
    for m in models():
        p = S / f"nested_comprehension_{m}.json"
        if p.exists():
            rows = json.loads(p.read_text())["rows"]
            print(f"  {m:34} " + "  ".join(f"{fmt_cell(*score(rows, f)):>8}" for f in ("json", "toon", "cstl_indent")))
    print("\nECRITURE (documents identiques)       json      toon      cstl_indent_ligne (strict)")
    for m in models():
        p = S / f"nested_writing_{m}.json"
        if not p.exists():
            continue
        rows = json.loads(p.read_text())["rows"]
        cells = []
        for f in ("json", "toon", "cstl_indent_ligne"):
            r = [x for x in rows if x["format"] == f and x.get("ok") is not None]
            cells.append(fmt_cell(sum(x["exact"] for x in r), len(r)))
        print(f"  {m:34} " + "  ".join(f"{c:>8}" for c in cells))
    print("\nVIDE QUI PARLE (questions justes)    json_naif   json   toon   cstl_indent")
    for key, rows in sorted(unknown.items()):
        if not key.startswith("comprehension_"):
            continue
        m = key[len("comprehension_"):]
        print(f"  {m:34} " + "  ".join(f"{fmt_cell(*score(rows, f)):>7}" for f in ("json_naif", "json", "toon", "cstl_indent")))


if __name__ == "__main__":
    main()
