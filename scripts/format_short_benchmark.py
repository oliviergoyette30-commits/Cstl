#!/usr/bin/env python3
"""
scripts/format_short_benchmark.py -- un format ABREGE, qui n'ecrit pas ce
qui est previsible, fait-il gagner des tokens sans faire perdre de
comprehension ? (2026-11-21)

L'idee (Olivier) : l'agent qui recoit n'a pas besoin qu'on lui ecrive ce
qu'il peut prevoir. On ne lui envoie que l'imprevisible : qui, quoi, quelle
regle, quel montant. Un programme reconstruit le message complet pour le
serveur (signatures, audit).

QUATRE FORMATS, memes politiques et memes questions que
`format_comprehension_benchmark.py` (10, 30 et 60 regles, roles, faits,
conditions) :
  cstl         message CSTL complet (RELATION [type=..., subject=..., ...])
  json_compact le meme message en JSON compact (`wire_size_benchmark.to_json`)
  cstl_court   une ligne par element, sans nom de champ :
                 ROLE agent_a finance
                 FACT patient_consent true
                 role:ops MUST_NOT send_receipt >50 if:audit_open
  json_court   le meme contenu en JSON positionnel, sans nom de champ :
                 {"roles": [["agent_a", "finance"]], "facts": {"patient_consent": true},
                  "rules": [["role:ops", "MUST_NOT", "send_receipt", 50, "audit_open", null]]}

Les formats abreges ont besoin d'une convention de lecture : elle est
ajoutee a la consigne, et COMPTEE dans les tokens de la consigne complete.

MESURES : reponses justes (permissions, obligations, pass^3), tokens des
donnees seules et de la consigne complete (API count_tokens, gratuite).

RESULTAT (2026-11-21, vraie API, 3 passes x 12 politiques, tokens moyens) :
                 tokens donnees  tokens consigne  tout juste Haiku  tout juste Opus
  cstl                1 971          2 558           17/36             36/36
  json_compact        2 039          2 625           18/36             36/36
  cstl_court            782          1 569           23/36             36/36
  json_court          1 033          1 764           22/36             36/36
- Le CSTL abrege divise les tokens des donnees par 2,5 (-60 % contre CSTL
  complet, -62 % contre JSON compact) et la consigne complete, convention
  comprise, de 39 %. A 60 regles : 1 285 tokens de donnees contre 3 185.
- Aucune perte de comprehension : Opus fait tout juste dans les quatre
  formats. Haiku fait meme un peu mieux en abrege (23/36 contre 17/36 et
  18/36), sans doute parce que le texte est plus court, mais l'ecart reste
  petit sur 36 cas.
- Le JSON abrege fait aussi mieux que le JSON complet, mais il coute 32 %
  de tokens de plus que le CSTL abrege (1 033 contre 782) : les crochets,
  les guillemets et les null de chaque position se paient.
- Attention : les formats abreges regroupent les roles et les faits avant
  les regles, alors que les formats complets gardent l'ordre melange. Une
  partie du gain de Haiku peut venir de cet ordre.
- C'est le premier avantage MESURE du format CSTL face au JSON : sous forme
  abregee, il est le plus economique en tokens a comprehension egale.

Usage :
  python3 scripts/format_short_benchmark.py --model claude-haiku-5-5 --runs 3
  python3 scripts/format_short_benchmark.py --stub      # plomberie seule, AUCUNE conclusion
"""
import argparse
import json
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
import format_comprehension_benchmark as fc  # noqa: E402
import toon_lite  # noqa: E402
import cstl_compact  # noqa: E402

FORMATS = ("cstl", "json_compact", "cstl_court", "json_court", "toon", "cstl_tableau", "cstl_symboles")

NOTES = {
    "cstl_court": (
        "Lecture du format abrege (une ligne par element) :\n"
        "- « ROLE <agent> <role> » : l'agent a ce role (comme POSSESSES subject=<agent> object=role:<role>).\n"
        "- « FACT <fait> true|false » : le fait est vrai ou faux (comme STATE). Un fait absent est faux.\n"
        "- « <subject> <MUST|MUST_NOT> <action> [>X] [if:F] [unless:F] » : une regle PERFORM ; « >X » est "
        "amount_above=X, « if:F » est if_fact=F, « unless:F » est unless_fact=F."),
    "json_court": (
        "Lecture du format abrege (JSON sans noms de champs) :\n"
        "- \"roles\" : liste de [agent, role] (comme POSSESSES subject=<agent> object=role:<role>).\n"
        "- \"facts\" : {fait: true|false} (comme STATE). Un fait absent est faux.\n"
        "- \"rules\" : liste de [subject, modality, action, amount_above, if_fact, unless_fact] ; null quand la "
        "condition n'existe pas."),
    "cstl_symboles": (
        "Format CSTL compact sans mot-cle (une ligne par element) :\n"
        "- « <agent> @<role> » : l'agent a ce role.\n"
        "- « <fait>=1 » ou « <fait>=0 » : le fait est vrai ou faux. Un fait absent est faux.\n"
        "- « <qui> *<action> » = obligation (MUST), « <qui> !<action> » = interdiction (MUST_NOT), « <qui> ?<action> » = permission (MAY). Conditions : >X (seulement au-dela du montant X), +F (seulement si le fait F est vrai), -F (sauf si le fait F est vrai)."),
    "cstl_tableau": (
        "Lecture du format CSTL compact en tableaux :\n"
        "- \"ROLES[N] agent role\" puis une ligne par role : l'agent a ce role (comme POSSESSES).\n"
        "- \"FACTS[N] fait valeur\" puis une ligne par fait, true ou false (comme STATE). Un fait absent est faux.\n"
        "- \"RULES[N] qui modalite action au_dela si sauf_si\" puis une ligne par regle PERFORM : au_dela est "
        "amount_above, si est if_fact, sauf_si est unless_fact ; « - » quand la condition n'existe pas."),
    "toon": (
        "Lecture du format TOON :\n"
        "- \"roles[N]{agent,role}:\" puis une ligne par role : l'agent a ce role (comme POSSESSES).\n"
        "- \"facts:\" puis \"<fait>: true|false\" (comme STATE). Un fait absent est faux.\n"
        "- \"rules[N]{subject,modality,action,amount_above,if_fact,unless_fact}:\" puis une ligne par regle PERFORM ; "
        "null quand la condition n'existe pas."),
}


def render(fmt, rels):
    if fmt == "cstl_symboles":
        return cstl_compact.encode_symbols(rels)
    if fmt == "cstl_tableau":
        return cstl_compact.encode(rels, table=True)
    if fmt == "toon":
        return toon_lite.encode(toon_lite.policy_object(rels))
    if fmt in ("cstl", "json_compact"):
        return fc.render(fmt, rels)
    roles = [(r["subject"], r["object"][5:]) for r in rels if r["type"] == "POSSESSES"]
    facts = [(r["subject"], r["object"] == "true") for r in rels if r["type"] == "STATE"]
    rules = [r for r in rels if r["type"] == "PERFORM"]
    if fmt == "json_court":
        return json.dumps({"roles": [list(x) for x in roles], "facts": dict(facts),
                           "rules": [[r["subject"], r["modality"], r["object"],
                                      int(r["amount_above"]) if "amount_above" in r else None,
                                      r.get("if_fact"), r.get("unless_fact")] for r in rules]},
                          ensure_ascii=False, separators=(",", ":"))
    lines = [f"ROLE {a} {r}" for a, r in roles] + [f"FACT {f} {'true' if v else 'false'}" for f, v in facts]
    for r in rules:
        parts = [r["subject"], r["modality"], r["object"]]
        if "amount_above" in r:
            parts.append(f">{r['amount_above']}")
        if r.get("if_fact"):
            parts.append(f"if:{r['if_fact']}")
        if r.get("unless_fact"):
            parts.append(f"unless:{r['unless_fact']}")
        lines.append(" ".join(parts))
    return "\n".join(lines)


def prompt(fmt, data, qs, ob):
    p = fc.prompt("cstl" if fmt.startswith("cstl") else "json_compact", data, qs, ob)
    if fmt in NOTES:
        p = p.replace("\n\nPolitique :\n", f"\n\n{NOTES[fmt]}\n\nPolitique :\n", 1)
    return p


TOKENS = {}


def count(model, text):
    client = getattr(model, "client", None)
    if client is None:
        return len(text) // 4
    if text not in TOKENS:
        TOKENS[text] = client.messages.count_tokens(model=model.model, messages=[{"role": "user", "content": text}]).input_tokens
    return TOKENS[text]


def run_case(model, size, seed, fmt, run):
    rels, rnd = fc.generate(size, seed)
    qs, ob = fc.questions(rels, rnd)
    data = render(fmt, rels)
    full = prompt(fmt, data, qs, ob)
    raw = model.generate(full, max_tokens=4000)
    perm, obl = fc.parse(raw)
    perm_ok = [i < len(perm) and perm[i] == q["truth"] for i, q in enumerate(qs)]
    return {"size": size, "seed": seed, "format": fmt, "run": run, "chars": len(data),
            "tokens_data": count(model, data), "tokens_prompt": count(model, full),
            "perm_ok": perm_ok, "obl_ok": obl == ob["truth"], "obl_truth": ob["truth"], "obl_answer": obl,
            "raw": raw[:600], "all_ok": all(perm_ok) and obl == ob["truth"]}


def summarize(rows, size=None):
    out = {}
    for f in FORMATS:
        r = [x for x in rows if x["format"] == f and (size is None or x["size"] == size)]
        groups = {}
        for x in r:
            groups.setdefault((x["size"], x["seed"]), []).append(x["all_ok"])
        n = max(1, len(r))
        out[f] = {"cases": len(r), "perm_ok": sum(sum(x["perm_ok"]) for x in r), "perm_total": sum(len(x["perm_ok"]) for x in r),
                  "obl_ok": sum(x["obl_ok"] for x in r), "all_ok": sum(x["all_ok"] for x in r),
                  "pass_k": sum(all(v) for v in groups.values()), "policies": len(groups),
                  "tokens_data": round(sum(x["tokens_data"] for x in r) / n),
                  "tokens_prompt": round(sum(x["tokens_prompt"] for x in r) / n)}
    return out


def main():
    global FORMATS
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-haiku-5-5")
    ap.add_argument("--stub", action="store_true")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--formats", default=",".join(FORMATS), help="formats a jouer, separes par des virgules")
    args = ap.parse_args()
    FORMATS = tuple(f for f in args.formats.split(",") if f)
    if args.stub:
        model = fc.StubBrain()
    else:
        import team_task_benchmark as tb
        model = tb.make_brain(args.model)
    jobs = [(size, seed, f, run) for run in range(args.runs) for size in fc.SIZES for seed in fc.SEEDS for f in FORMATS]
    with ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
        rows = list(pool.map(lambda j: run_case(model, *j), jobs))
    label = "stub [plomberie seule, aucune conclusion]" if args.stub else args.model
    print(f"\nModele : {label}, {args.runs} passe(s) x {len(fc.SIZES) * len(fc.SEEDS)} politiques")
    for size in (None, *fc.SIZES):
        print(f"\n  {'TOUTES' if size is None else f'{size} regles'} :")
        print(f"    {'format':12} | permissions | obligations | tout juste | pass^{args.runs} | tokens donnees | tokens consigne")
        for f, s in summarize(rows, size).items():
            print(f"    {f:12} | {s['perm_ok']:>4}/{s['perm_total']:<6} | {s['obl_ok']:>4}/{s['cases']:<6} | "
                  f"{s['all_ok']:>4}/{s['cases']:<5} | {s['pass_k']:>3}/{s['policies']:<3} | {s['tokens_data']:>14} | {s['tokens_prompt']:>15}")
    dest = REPO / "scripts" / f"format_short_benchmark_{'stub' if args.stub else args.model.replace('/', '__')}.json"
    dest.write_text(json.dumps({"summary": summarize(rows), "rows": rows}, indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
