#!/usr/bin/env python3
"""
scripts/nested_writing_benchmark.py -- un modele sait-il ECRIRE le format ?
(2026-11-22)

Pourquoi : `nested_benchmark.py --comprehension` mesure la LECTURE (le
modele repond a des questions sur des donnees dans chaque format). Un format
d'echange doit aussi etre ECRIT par les modeles, et c'est la que les formats
inventes echouent souvent : en-tete mal compte, indentation decalee,
guillemets oublies.

Protocole, identique pour chaque format :
  - le modele recoit la description du format (`nested_benchmark.NOTES`) et un
    petit exemple d'une AUTRE famille de donnees (le JSON et sa forme dans le
    format, produits par l'encodeur), puis un document JSON a convertir ;
  - sa sortie est relue par le VRAI decodeur du format (rien n'est devine) ;
  - mesures : relu sans erreur, identique au JSON de depart, identique en
    mode tolerant (cstl_indent : `decode_tree_indented(lenient=True)`), et
    part des valeurs feuilles correctes (chemin + valeur ; 0 si illisible).

Jeux : les 12 jeux imbriques de `nested_benchmark.py` (commandes, organisation,
contrats ; 8 et 24 elements ; 2 graines).

RESULTATS (2026-11-22, Gemma 4 31B, 12 jeux) :
  passe 1 (nested_writing_gemma-4-31b-it_passe1.json) :
    json 10/10 identiques, toon 8/10 (98,0 % des valeurs), arbre indente 9/12
    (2 erreurs d'API pour json et toon)
  passe 2 (..._passe2.json), arbre indente seulement :
    comptes en tete  : 9/12 identiques en strict, 12/12 en mode tolerant
    comptes a la fin : 8/12 identiques en strict, 12/12 en mode tolerant
- TOUS les echecs de l'arbre indente sont des comptes faux sur les gros jeux
  (lines[52] pour 48, clauses[122] pour 129...) : le contenu est juste a
  100 %. TOON, lui, est toujours relu mais fait de vraies erreurs de valeurs.
- Compter A LA FIN n'aide pas : le modele se trompe autant en comptant ce
  qu'il vient d'ecrire. Les modeles ne savent pas compter de longues listes.
- Conclusion : un texte ecrit par un modele se relit en mode tolerant
  (`lenient=True`) ; les comptes restent utiles entre machines.

Usage :
  GEMINI_API_KEY=... python3 scripts/nested_writing_benchmark.py gemma-4-31b-it
  ... --formats json,toon,cstl_indent --runs 1 [--resume]
"""
import argparse
import json
import os
import re
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
import nested_benchmark as nb  # noqa: E402

LENIENT = {"cstl_indent": lambda t: nb.cstl_compact.decode_tree_indented(t, lenient=True),
           "cstl_indent_fin": lambda t: nb.cstl_compact.decode_tree_indented(t, lenient=True)}
DECODE = {"json": json.loads, "toon": nb.toon_lite.decode_nested,
          "cstl_arbre": nb.cstl_compact.decode_tree, "cstl_indent": nb.cstl_compact.decode_tree_indented,
          "cstl_indent_fin": nb.cstl_compact.decode_tree_indented}


def leaves(v, path=""):
    """Valeurs feuilles : {(chemin, valeur JSON)}."""
    if isinstance(v, dict):
        return set().union(*(leaves(x, f"{path}/{k}") for k, x in v.items())) if v else {(path, "{}")}
    if isinstance(v, list):
        return set().union(*(leaves(x, f"{path}[{i}]") for i, x in enumerate(v))) if v else {(path, "[]")}
    return {(path, json.dumps(v))}


def strip_fences(raw):
    m = re.search(r"```[a-zA-Z]*\n(.*?)```", raw, re.S)
    return (m.group(1) if m else raw).strip("\n")


def prompt(fmt, data, kind):
    ex_kind = nb.KINDS[(nb.KINDS.index(kind) + 1) % len(nb.KINDS)]
    ex = nb.gen(ex_kind, 3, 99)
    return (f"Convertis le document JSON ci-dessous dans le format decrit.\n{nb.NOTES[fmt]}\n\n"
            f"Exemple (autres donnees).\nJSON :\n{json.dumps(ex, ensure_ascii=False)}\n"
            f"Meme document dans le format :\n{nb.render(fmt, ex)}\n\n"
            f"Document a convertir :\n{json.dumps(data, ensure_ascii=False)}\n\n"
            "Reponds uniquement par le document converti, sans explication, sans bloc de code.")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("--formats", default="json,toon,cstl_indent")
    ap.add_argument("--runs", type=int, default=1)
    ap.add_argument("--resume", action="store_true", help="ne repose que les jeux en erreur d'API")
    args = ap.parse_args()
    import team_task_benchmark as tb
    model = tb.make_brain(args.model)
    formats = [f for f in args.formats.split(",") if f]
    datasets = [(k, n, s, nb.gen(k, n, s)) for k in nb.KINDS for n in nb.SIZES for s in nb.SEEDS]
    jobs = [(k, n, s, d, f, r) for r in range(args.runs) for k, n, s, d in datasets for f in formats]

    def one(job):
        k, n, s, d, f, run = job
        base = {"kind": k, "size": n, "seed": s, "format": f, "run": run}
        try:
            raw = model.generate(prompt(f, d, k), max_tokens=8000)
        except RuntimeError as e:
            return {**base, "ok": None, "error": str(e)[:200]}
        text = strip_fences(raw)
        try:
            back, err = DECODE[f](text), None
        except Exception as e:  # noqa: BLE001 -- toute erreur du decodeur compte comme illisible
            back, err = None, f"{type(e).__name__}: {e}"[:200]
        truth = leaves(d)
        got = leaves(back) if err is None else set()
        score = len(truth & got) / max(len(truth), len(got), 1)
        exact = err is None and back == d
        # Mode tolerant (cstl_indent) : [N] faux, colonnes finales omises,
        # autre largeur d'indentation. Sans objet pour les autres formats.
        try:
            soft = LENIENT.get(f, DECODE[f])(text) == d
        except Exception:  # noqa: BLE001
            soft = False
        print(f"    {k:13} n={n:<3} s{s} {f:12} : {'identique' if exact else 'lisible' if err is None else 'ILLISIBLE'}"
              f"  feuilles {100 * score:5.1f} %" + ("  tolerant: identique" if soft and not exact else "") + (f"  ({err[:70]})" if err else ""), flush=True)
        return {**base, "ok": True, "parsed": err is None, "exact": exact, "exact_lenient": soft, "leaf_score": round(score, 4),
                "error_decode": err, "chars_out": len(text), "raw": text}

    dest = REPO / "scripts" / f"nested_writing_{args.model}.json"
    rows = [None] * len(jobs)
    if args.resume and dest.exists():
        old = json.loads(dest.read_text())["rows"]
        if len(old) == len(jobs):
            rows = [r if r.get("ok") is not None else None for r in old]
    todo = [i for i, r in enumerate(rows) if r is None]
    with ThreadPoolExecutor(max_workers=int(os.environ.get("BENCH_WORKERS", "2"))) as pool:
        for i, r in zip(todo, pool.map(one, [jobs[i] for i in todo])):
            rows[i] = r
    print(f"\n  Ecriture ({args.model}) :")
    for f in formats:
        r = [x for x in rows if x["format"] == f and x["ok"] is not None]
        if not r:
            continue
        print(f"    {f:12} | relus {sum(x['parsed'] for x in r):>2}/{len(r)} | identiques {sum(x['exact'] for x in r):>2}/{len(r)}"
              f" | identiques en mode tolerant {sum(x.get('exact_lenient', x['exact']) for x in r):>2}/{len(r)}"
              f" | feuilles {100 * sum(x['leaf_score'] for x in r) / len(r):5.1f} %"
              + (f" | erreurs d'API {sum(1 for x in rows if x['format'] == f and x['ok'] is None)}"
                 if any(x["format"] == f and x["ok"] is None for x in rows) else ""))
    dest.write_text(json.dumps({"rows": rows}, indent=2, ensure_ascii=False) + "\n")
    print(f"\nResultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
