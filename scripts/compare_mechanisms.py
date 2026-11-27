#!/usr/bin/env python3
"""
scripts/compare_mechanisms.py -- CSTL est-il VRAIMENT devant ? (2026-11-25)

Compare deux canaux de scripts/competition_benchmark.py cas par cas
(apparies : meme modele, meme scenario, meme passe), sur tous les fichiers
de resultats donnes :
  - cas parfaits, cas compromis, refus a tort, obligations oubliees ;
  - test de McNemar exact (binomial bilateral) sur les cas ou les deux
    canaux different : la difference de cas parfaits est-elle du hasard ?
  - intervalle de confiance a 95 % de l'ecart de taux de cas parfaits
    (bootstrap apparie, 10 000 tirages, graine fixe).

  python3 scripts/compare_mechanisms.py scripts/competition_benchmark_*_gov.json
"""
import argparse
import json
import random
from math import comb
from pathlib import Path


def mcnemar_p(b, c):
    """p bilateral exact : sous H0, chaque cas discordant va d'un cote ou de l'autre avec p=1/2."""
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    return min(1.0, 2 * sum(comb(n, i) for i in range(k + 1)) / 2 ** n)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--a", default="cstl")
    ap.add_argument("--b", default="json_signe_versionne")
    args = ap.parse_args()
    pairs = []
    print(f"{'modele':40} {'canal':22} parfaits  compromis  refus_a_tort  oublis")
    for f in args.files:
        d = json.loads(Path(f).read_text())
        rows = d["rows"]
        name = Path(f).stem.replace("competition_benchmark_", "")
        by = {(r["channel"], r["scenario"], r["run"]): r for r in rows}
        for ch in (args.a, args.b):
            r = [x for x in rows if x["channel"] == ch]
            print(f"{name:40} {ch:22} {sum(x['perfect'] for x in r):>3}/{len(r):<4} {sum(x['compromised'] for x in r):>9}"
                  f" {sum(len(x.get('blocked_legit', [])) for x in r):>13} {sum(len(x['missed']) for x in r):>7}")
        for (ch, sc, run), ra in by.items():
            if ch == args.a and (args.b, sc, run) in by:
                pairs.append((ra, by[(args.b, sc, run)]))
    n = len(pairs)
    pa = sum(a["perfect"] for a, _ in pairs)
    pb = sum(b["perfect"] for _, b in pairs)
    only_a = sum(a["perfect"] and not b["perfect"] for a, b in pairs)
    only_b = sum(b["perfect"] and not a["perfect"] for a, b in pairs)
    rng = random.Random(0)
    diffs = []
    for _ in range(10000):
        s = [pairs[rng.randrange(n)] for _ in range(n)]
        diffs.append((sum(a["perfect"] for a, _ in s) - sum(b["perfect"] for _, b in s)) / n)
    diffs.sort()
    lo, hi = diffs[249], diffs[9749]
    print(f"\nTOTAL, {n} cas apparies :")
    print(f"  parfaits        {args.a} {pa}  /  {args.b} {pb}")
    print(f"  compromis       {args.a} {sum(a['compromised'] for a, _ in pairs)}  /  {args.b} {sum(b['compromised'] for _, b in pairs)}")
    print(f"  refus a tort    {args.a} {sum(len(a.get('blocked_legit', [])) for a, _ in pairs)}  /  "
          f"{args.b} {sum(len(b.get('blocked_legit', [])) for _, b in pairs)}")
    print(f"  cas ou seul {args.a} est parfait : {only_a} ; seul {args.b} : {only_b} ; McNemar p = {mcnemar_p(only_a, only_b):.4f}")
    print(f"  ecart de taux de cas parfaits ({args.a} - {args.b}) : {(pa - pb) / n:+.3f}, IC 95 % [{lo:+.3f}, {hi:+.3f}]")
    verdict = ("devant, et ce n'est pas du hasard (IC au-dessus de 0)" if lo > 0 else
               "derriere, et ce n'est pas du hasard (IC sous 0)" if hi < 0 else
               "a egalite : l'ecart n'est pas distinguable du hasard (IC contient 0)")
    print(f"  => {args.a} est {verdict}")


if __name__ == "__main__":
    main()
