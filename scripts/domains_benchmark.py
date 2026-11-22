#!/usr/bin/env python3
"""
scripts/domains_benchmark.py -- les formats sur les 18 domaines de CSTL
(src/domains.rs), hors ligne (2026-11-22).

Pour chaque domaine, un echange realiste et reproductible :
  - une politique : 6 agents, des roles, des faits vrais ou faux, 15 regles
    dont les actions sont les OPERATEURS DU DOMAINE (prescribe, liquidate,
    patch...), avec des conditions de montant et de fait ;
  - 30 relations du domaine : type = un operateur, subject et object, et des
    attributs facultatifs (date, confidence, source, ref, amount), presents
    ou non selon la ligne.
Les operateurs sont lus dans src/domains.rs (seule source de verite).

Six formats, memes donnees, aller-retour verifie pour les formats CSTL :
  json, toon, cstl (complet), cstl_compact (lignes), cstl_tableau, cstl_symboles.

MESURE : caracteres. Les tokens et la comprehension demandent l'API (limite
de depense atteinte au moment de l'ecriture) : `--tokens` les compte quand
elle est disponible.

RESULTAT (2026-11-22, caracteres, 18 domaines) :
  total : json 87 405, toon 52 920, cstl 89 108, cstl_compact 49 675,
          cstl_tableau 48 549, cstl_symboles 40 753
- Le mode sans mot-cle est le plus court dans les 18 domaines : 23 % de
  moins que TOON, 53 % de moins que le JSON compact.
- Ici, meme le mode ligne bat TOON (49 675 contre 52 920) : les donnees
  melangent beaucoup de types de relations (un par operateur), et TOON doit
  les mettre dans un seul tableau, avec des null pour les attributs absents.
- Allers-retours sans perte pour tous les formats CSTL, dans tous les domaines.
- Tokens et comprehension non mesures (limite de depense de l'API).

Usage : python3 scripts/domains_benchmark.py [--tokens]
"""
import argparse
import json
import os
import random
import re
import sys
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import relay_tamper_benchmark as rt  # noqa: E402
import toon_lite  # noqa: E402
import cstl_compact  # noqa: E402

FORMATS = ("json", "toon", "cstl", "cstl_compact", "cstl_tableau", "cstl_symboles")


def domains():
    src = (REPO / "src" / "domains.rs").read_text()
    out = {}
    for m in re.finditer(r'"([^"]+)"(?:\s*\|\s*"[^"]+")*\s*=>\s*&\[(.*?)\]', src, re.S):
        ops = re.findall(r'"([A-Z_]+)"', m.group(2))
        if ops:
            out[m.group(1)] = ops
    return out


def scenario(name, ops):
    rnd = random.Random(zlib.crc32(name.encode()))
    agents = [f"agent_{c}" for c in "abcdef"]
    roles = ["lead", "operator", "auditor"]
    facts = [f"{w}_{i}" for i, w in enumerate(["approval", "incident", "consent", "deadline"])]
    rels = [{"type": "POSSESSES", "subject": a, "object": f"role:{r}"} for a in agents for r in roles if rnd.random() < 0.3]
    rels += [{"type": "STATE", "subject": f, "object": rnd.choice(["true", "false"])} for f in facts]
    for _ in range(15):
        r = {"type": "PERFORM", "subject": rnd.choice(agents + [f"role:{x}" for x in roles]),
             "object": rnd.choice(ops).lower(), "modality": rnd.choice(["MUST_NOT", "MUST_NOT", "MUST", "MAY"])}
        if r["modality"] != "MUST" and rnd.random() < 0.4:
            r["amount_above"] = str(rnd.choice([100, 500, 1000, 5000]))
        if rnd.random() < 0.35:
            r[rnd.choice(["if_fact", "unless_fact"])] = rnd.choice(facts)
        rels.append(r)
    entities = [f"{name[:4]}_obj_{i}" for i in range(10)]
    for i in range(30):
        r = {"type": rnd.choice(ops), "subject": rnd.choice(agents), "object": rnd.choice(entities), "id": f"r{i}"}
        for attr, p, val in (("date", 0.7, lambda: f"2026-11-{rnd.randint(1, 28):02d}"),
                             ("confidence", 0.4, lambda: str(rnd.choice([0.5, 0.8, 0.95]))),
                             ("source", 0.5, lambda: rnd.choice(["registry", "agent_report", "sensor"])),
                             ("ref", 0.3, lambda: f"r{rnd.randrange(max(1, i))}"),
                             ("amount", 0.3, lambda: str(rnd.randint(10, 9000)))):
            if rnd.random() < p:
                r[attr] = val()
        rels.append(r)
    return rels


def to_objects(rels):
    pol = [r for r in rels if r["type"] in ("POSSESSES", "STATE", "PERFORM")]
    recs = [r for r in rels if r["type"] not in ("POSSESSES", "STATE", "PERFORM")]
    return toon_lite.policy_object(pol), recs


def render(fmt, rels):
    if fmt in ("json", "toon"):
        policy, recs = to_objects(rels)
        if fmt == "json":
            rules = [{k: v for k, v in r.items() if v is not None} for r in policy["rules"]]
            return json.dumps({"roles": policy["roles"], "facts": policy["facts"], "rules": rules, "records": recs},
                              ensure_ascii=False, separators=(",", ":"))
        cols = []
        for r in recs:
            cols += [k for k in r if k not in cols]
        return toon_lite.encode({**policy, "records": [{c: r.get(c) for c in cols} for r in recs]})
    if fmt == "cstl":
        return rt.render_relations(rels)
    if fmt == "cstl_compact":
        return cstl_compact.encode(rels)
    if fmt == "cstl_tableau":
        return cstl_compact.encode(rels, table=True)
    return cstl_compact.encode_symbols(rels)


def same_relations(a, b):
    key = lambda r: sorted(r.items())  # noqa: E731
    return sorted(map(key, a)) == sorted(map(key, b))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tokens", action="store_true", help="compter aussi les tokens (API Anthropic requise)")
    args = ap.parse_args()
    count = None
    if args.tokens:
        import anthropic
        client = anthropic.Anthropic(api_key=os.environ["ANTHROPIC_API_KEY"], base_url="https://api.anthropic.com")

        def count(text):
            return client.messages.count_tokens(model="claude-opus-5-5", messages=[{"role": "user", "content": text}]).input_tokens
    rows, totals, broken = [], {f: 0 for f in FORMATS}, []
    for name, ops in domains().items():
        rels = scenario(name, ops)
        row = {"domain": name, "operators": len(ops)}
        for f in FORMATS:
            text = render(f, rels)
            row[f] = count(text) if count else len(text)
            totals[f] += row[f]
        for f, dec in (("cstl", rt.relations_in_text), ("cstl_compact", cstl_compact.decode),
                       ("cstl_tableau", cstl_compact.decode), ("cstl_symboles", cstl_compact.decode)):
            if not same_relations(dec(render(f, rels)), rels):
                broken.append(f"{name}/{f}")
        if not toon_lite.decode(render("toon", rels))[1] == []:
            broken.append(f"{name}/toon")
        rows.append(row)
    unit = "tokens" if count else "caracteres"
    print(f"{len(rows)} domaines, 15 regles + 30 relations chacun ({unit})\n")
    print(f"  {'domaine':16} | " + " | ".join(f"{f:>13}" for f in FORMATS))
    for r in rows:
        best = min(FORMATS, key=lambda f: r[f])
        print(f"  {r['domain']:16} | " + " | ".join(f"{r[f]:>12}{'*' if f == best else ' '}" for f in FORMATS))
    print(f"  {'TOTAL':16} | " + " | ".join(f"{totals[f]:>13}" for f in FORMATS))
    print(f"\n  * = le plus court du domaine. Allers-retours en echec : {broken or 'aucun'}")
    wins = {f: sum(min(FORMATS, key=lambda g: r[g]) == f for r in rows) for f in FORMATS}
    print(f"  Domaines gagnes : {wins}")
    dest = REPO / "scripts" / f"domains_benchmark_{unit}.json"
    dest.write_text(json.dumps({"unit": unit, "totals": totals, "rows": rows, "broken": broken}, indent=2, ensure_ascii=False) + "\n")
    print(f"  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
