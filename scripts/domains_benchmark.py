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


# ------------------------------------------------------------ comprehension

NOTES = {
    "json": "Format : JSON. roles = [agent, role] ; facts = {fait: vrai/faux} ; rules = regles PERFORM ; records = relations du domaine.",
    "toon": ("Format : TOON. « nom[N]{champs}: » puis une ligne par element, valeurs dans l'ordre des champs ; null = absent. "
             "roles, facts et rules forment la politique ; records = relations du domaine."),
    "cstl_compact": ("Format : CSTL compact. ROLE <agent> <role> ; FACT <fait> true|false ; regle : <qui> <MUST|MUST_NOT|MAY> "
                     "<action> [>X] [if:F] [unless:F] ; relation du domaine : REL <type> <subject> <object> cle=valeur."),
    "cstl_symboles": ("Format : CSTL compact sans mot-cle. <agent> @<role> ; <fait>=1|0 ; regle : <qui> *<action> (MUST), "
                      "!<action> (MUST_NOT), ?<action> (MAY), avec >X, +F (si F vrai), -F (sauf si F vrai) ; relations du "
                      "domaine : « <TYPE>[N]: col1 col2 ... » puis une ligne par relation : subject object puis les colonnes."),
}
RULES = ("Regles : une regle PERFORM interdit (MUST_NOT), oblige (MUST) ou permet (MAY) l'action a son subject ; "
         "subject=role:<nom> vise les agents qui ont ce role. Conditions : amount_above=X (>X) : seulement si le montant "
         "demande depasse X ; if_fact=F : seulement si F est vrai ; unless_fact=F : sauf si F est vrai. Un fait absent "
         "est faux. Une action est interdite si au moins une regle MUST_NOT qui vaut la vise ; sinon elle est permise.")


def holds(r, facts, amount):
    if "amount_above" in r and not (amount is not None and amount > float(r["amount_above"])):
        return False
    return ("if_fact" not in r or r["if_fact"] in facts) and ("unless_fact" not in r or r["unless_fact"] not in facts)


def questions(name, rels):
    rnd = random.Random(zlib.crc32(("q/" + name).encode()))
    facts = {r["subject"] for r in rels if r["type"] == "STATE" and r["object"] == "true"}
    roles = lambda a: {r["object"][5:] for r in rels if r["type"] == "POSSESSES" and r["subject"] == a}  # noqa: E731
    targets = lambda r, a: r["subject"] == a or (r["subject"].startswith("role:") and r["subject"][5:] in roles(a))  # noqa: E731
    rules = [r for r in rels if r["type"] == "PERFORM"]
    qs = []
    for r in rnd.sample([x for x in rules if x["modality"] == "MUST_NOT"], 3):
        agent = r["subject"] if not r["subject"].startswith("role:") else next(
            (a for a in [f"agent_{c}" for c in "abcdef"] if r["subject"][5:] in roles(a)), "agent_a")
        amount = int(float(r.get("amount_above", 300)) + rnd.choice([-1, 1, 50]))
        ok = not any(x["modality"] == "MUST_NOT" and x["object"] == r["object"] and targets(x, agent) and holds(x, facts, amount)
                     for x in rules)
        qs.append((f"{agent} veut lancer {r['object']} pour {amount} : est-ce permis ? (true ou false)", ok))
    agent = max([f"agent_{c}" for c in "abcdef"], key=lambda a: sum(x["modality"] == "MUST" and targets(x, a) for x in rules))
    must = sorted({x["object"] for x in rules if x["modality"] == "MUST" and targets(x, agent) and holds(x, facts, None)})
    qs.append((f"Quelles actions {agent} est-il OBLIGE de lancer ? (liste)", must))
    recs = [r for r in rels if r["type"] not in ("POSSESSES", "STATE", "PERFORM")]
    t = rnd.choice(sorted({r["type"] for r in recs}))
    qs.append((f"Combien de relations du domaine ont le type {t} ?", sum(r["type"] == t for r in recs)))
    a = rnd.choice(sorted({r["subject"] for r in recs}))
    qs.append((f"Quelle est la somme des amount des relations du domaine dont le subject est {a} ? (0 si aucune)",
               sum(int(r["amount"]) for r in recs if r["subject"] == a and "amount" in r)))
    return qs


def same(answer, truth):
    if isinstance(truth, bool):
        return str(answer).strip().lower() == str(truth).lower()
    if isinstance(truth, list):
        return isinstance(answer, list) and sorted(str(x).strip() for x in answer) == truth
    try:
        return abs(float(answer) - float(truth)) < 0.006
    except (TypeError, ValueError):
        return False


def comprehension(model, runs, formats, workers):
    from concurrent.futures import ThreadPoolExecutor

    def one(job):
        name, ops, fmt, run = job
        rels = scenario(name, ops)
        qs = questions(name, rels)
        lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(qs))
        prompt = (f"Voici un echange entre agents, domaine {name}.\n{NOTES[fmt]}\n{RULES}\n\nDonnees :\n{render(fmt, rels)}\n\n"
                  f"Questions :\n{lines}\n\nReponds uniquement par un objet JSON "
                  + "{" + ", ".join(f'"r{i + 1}": <reponse>' for i in range(len(qs))) + "}.")
        raw = model.generate(prompt, max_tokens=4000)
        start, end = raw.find("{"), raw.rfind("}")
        try:
            ans = json.loads(raw[start:end + 1])
        except ValueError:
            ans = {}
        ok = [same(ans.get(f"r{i + 1}"), t) for i, (_, t) in enumerate(qs)]
        return {"domain": name, "format": fmt, "run": run, "ok": ok, "raw": raw[:800]}

    jobs = [(n, o, f, r) for r in range(runs) for n, o in domains().items() for f in formats]
    with ThreadPoolExecutor(max_workers=workers) as pool:
        rows = list(pool.map(one, jobs))
    print(f"\n  Comprehension, {runs} passe(s) x {len(domains())} domaines x 6 questions :")
    for f in formats:
        r = [x for x in rows if x["format"] == f]
        pol = sum(sum(x["ok"][:4]) for x in r)
        dat = sum(sum(x["ok"][4:]) for x in r)
        print(f"    {f:13} | politique {pol:>3}/{4 * len(r):<3} | donnees {dat:>3}/{2 * len(r):<3} | "
              f"tout juste {sum(all(x['ok']) for x in r):>3}/{len(r)}")
    return rows


def same_relations(a, b):
    key = lambda r: sorted(r.items())  # noqa: E731
    return sorted(map(key, a)) == sorted(map(key, b))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tokens", action="store_true", help="compter aussi les tokens (API Anthropic requise)")
    ap.add_argument("--comprehension", metavar="MODELE", help="poser les questions a ce modele (ex. claude-haiku-5-5), ou « stub »")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--formats", default="json,toon,cstl_compact,cstl_symboles")
    args = ap.parse_args()
    if args.comprehension:
        if args.comprehension == "stub":
            class Stub:
                def generate(self, prompt, max_tokens=0):
                    return '{"r1": true, "r2": true, "r3": true, "r4": [], "r5": 0, "r6": 0}'
            model = Stub()
        else:
            import team_task_benchmark as tb
            model = tb.make_brain(args.comprehension)
        rows = comprehension(model, args.runs, [f for f in args.formats.split(",") if f], int(os.environ.get("BENCH_WORKERS", "8")))
        dest = REPO / "scripts" / f"domains_comprehension_{args.comprehension}.json"
        dest.write_text(json.dumps({"rows": rows}, indent=2, ensure_ascii=False) + "\n")
        print(f"  Resultats bruts : {dest.relative_to(REPO)}")
        return
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
