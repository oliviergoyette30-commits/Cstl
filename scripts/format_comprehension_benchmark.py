#!/usr/bin/env python3
"""
scripts/format_comprehension_benchmark.py -- un modele comprend-il aussi
bien du JSON compact que du JSON indente ou du CSTL ? (2026-11-21)

Pourquoi : pour une machine, JSON compact et JSON indente sont
identiques. Pour un LLM, ce n'est pas certain : un texte sans espaces est
plus dense. Si le modele fait plus d'erreurs sur le JSON compact, CSTL
aurait un vrai avantage : presque aussi court, et peut-etre plus lisible.

LA TACHE : une politique (10, 30 ou 60 regles) avec des agents, des roles,
des faits vrais ou faux et des conditions. Le modele repond a 3 questions
« cette action est-elle permise ? » et donne les obligations d'un agent.
La verite terrain est calculee par un evaluateur Python, avec les regles
ecrites dans la consigne (identique pour les trois formats).

LES TROIS FORMATS portent EXACTEMENT les memes donnees, converties par
`wire_size_benchmark.to_json` (verifiee dans les deux sens) :
  cstl          message CSTL : lignes RELATION [k=v, ...]
  json_compact  le meme message en JSON, sans aucun espace
  json_indente  le meme message en JSON, indente de 2 espaces

SCORE : reponses justes sur les questions de permission, et listes
d'obligations exactes. Chaque politique est posee 3 fois (pass^3).

Usage :
  python3 scripts/format_comprehension_benchmark.py --model claude-haiku-5-5 --runs 3
  python3 scripts/format_comprehension_benchmark.py --stub      # plomberie seule, AUCUNE conclusion
"""
import argparse
import json
import random
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import wire_size_benchmark as ws  # noqa: E402
import relay_tamper_benchmark as rt  # noqa: E402

FORMATS = ("cstl", "json_compact", "json_indente")
SIZES = (10, 30, 60)
SEEDS = (1, 2, 3, 4)
AGENTS = [f"agent_{c}" for c in "abcdef"]
ROLES = ["finance", "support", "ops"]
ACTIONS = ["refund_order", "export_customer_data", "wipe_db", "grant_admin", "share_medical_record",
           "close_account", "send_receipt", "log_transaction", "notify_regulator", "backup_db"]
FACTS = ["patient_consent", "audit_open", "incident_active", "contract_signed", "kyc_done", "freeze_period"]

RULES_TEXT = (
    "Regles d'interpretation (a appliquer a la lettre) :\n"
    "- Une relation PERFORM avec modality=MUST_NOT interdit l'action (object) a son subject ; avec modality=MUST, "
    "elle l'oblige. Tout ce qui n'est pas interdit est permis.\n"
    "- subject=role:<nom> vise tous les agents qui ont ce role. Un agent a un role si une relation POSSESSES a "
    "subject=<agent> et object=role:<nom>.\n"
    "- Un fait est vrai si une relation STATE a subject=<fait> et object=true ; il est faux s'il a object=false ou "
    "s'il n'apparait pas.\n"
    "- Conditions : amount_above=X -> la regle ne vaut que si le montant demande est strictement superieur a X ; "
    "if_fact=F -> seulement si F est vrai ; unless_fact=F -> seulement si F n'est pas vrai. Une regle sans "
    "condition vaut toujours. Une regle vaut si TOUTES ses conditions sont remplies.\n"
    "- Une action est interdite si au moins une regle MUST_NOT qui vaut la vise. Sinon elle est permise."
)


def generate(n, seed):
    rnd = random.Random(seed * 1000 + n)
    roles = {a: rnd.sample(ROLES, rnd.randint(0, 2)) for a in AGENTS}
    facts = {f: rnd.random() < 0.5 for f in FACTS}
    rels = []
    for a, rs in roles.items():
        rels += [{"type": "POSSESSES", "subject": a, "object": f"role:{r}"} for r in rs]
    rels += [{"type": "STATE", "subject": f, "object": "true" if v else "false"} for f, v in facts.items() if rnd.random() < 0.85]
    rules = []
    for _ in range(n):
        subj = rnd.choice(AGENTS) if rnd.random() < 0.6 else f"role:{rnd.choice(ROLES)}"
        modality = "MUST_NOT" if rnd.random() < 0.7 else "MUST"
        r = {"type": "PERFORM", "subject": subj, "object": rnd.choice(ACTIONS), "modality": modality}
        if modality == "MUST_NOT" and rnd.random() < 0.4:
            r["amount_above"] = str(rnd.choice([50, 100, 200, 500, 1000]))
        c = rnd.random()
        if c < 0.25:
            r["if_fact"] = rnd.choice(FACTS)
        elif c < 0.5:
            r["unless_fact"] = rnd.choice(FACTS)
        rules.append(r)
    rels += rules
    rnd.shuffle(rels)
    return rels, rnd


def facts_of(rels):
    return {r["subject"] for r in rels if r["type"] == "STATE" and r["object"] == "true"}


def roles_of(rels, agent):
    return {r["object"][5:] for r in rels if r["type"] == "POSSESSES" and r["subject"] == agent}


def holds(r, facts, amount):
    if "amount_above" in r and not (amount is not None and amount > float(r["amount_above"])):
        return False
    if "if_fact" in r and r["if_fact"] not in facts:
        return False
    if "unless_fact" in r and r["unless_fact"] in facts:
        return False
    return True


def targets(r, agent, roles):
    return r["subject"] == agent or (r["subject"].startswith("role:") and r["subject"][5:] in roles)


def allowed(rels, agent, action, amount):
    facts, roles = facts_of(rels), roles_of(rels, agent)
    return not any(r["type"] == "PERFORM" and r["modality"] == "MUST_NOT" and r["object"] == action
                   and targets(r, agent, roles) and holds(r, facts, amount) for r in rels)


def obligations(rels, agent):
    facts, roles = facts_of(rels), roles_of(rels, agent)
    return sorted({r["object"] for r in rels if r["type"] == "PERFORM" and r["modality"] == "MUST"
                   and targets(r, agent, roles) and holds(r, facts, None)})


def questions(rels, rnd):
    """3 questions de permission sur des actions visees par des regles (avec
    un montant proche des seuils), et 1 agent dont on demande les obligations."""
    forb = [r for r in rels if r["type"] == "PERFORM" and r["modality"] == "MUST_NOT"]
    qs = []
    for r in rnd.sample(forb, min(3, len(forb))):
        if r["subject"].startswith("role:"):
            cands = [a for a in AGENTS if r["subject"][5:] in roles_of(rels, a)] or AGENTS
            agent = rnd.choice(cands)
        else:
            agent = r["subject"]
        t = float(r.get("amount_above", rnd.choice([100, 200])))
        amount = int(t + rnd.choice([-20, -1, 1, 30]))
        qs.append({"agent": agent, "action": r["object"], "amount": amount,
                   "truth": allowed(rels, agent, r["object"], amount)})
    agent = max(AGENTS, key=lambda a: (len(obligations(rels, a)), a))
    return qs, {"agent": agent, "truth": obligations(rels, agent)}


def render(fmt, rels):
    c = CLIENT.build_payload(encoder="policy_store", produced_by="policy_store", purpose="inform",
                             sender="policy_owner", receiver="server", relations=rels)
    if fmt == "cstl":
        return c
    obj = ws.to_json(c)
    assert ws.same(c, obj)
    if fmt == "json_compact":
        return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))
    return json.dumps(obj, ensure_ascii=False, indent=2)


def prompt(fmt, data, qs, ob):
    label = {"cstl": "message CSTL", "json_compact": "message JSON", "json_indente": "message JSON"}[fmt]
    lines = "\n".join(f"{i + 1}. {q['agent']} veut lancer {q['action']} pour {q['amount']} euros : est-ce permis ?"
                      for i, q in enumerate(qs))
    return (f"Voici la politique d'un systeme d'agents ({label}).\n\n{RULES_TEXT}\n\nPolitique :\n{data}\n\n"
            f"Questions :\n{lines}\n{len(qs) + 1}. Quelles actions {ob['agent']} est-il OBLIGE de lancer ?\n\n"
            'Reponds uniquement par {"permis": [true ou false pour chaque question 1 a '
            f'{len(qs)}], "obligations": ["<action>", ...]}}.')


def parse(text):
    start, end = text.find("{"), text.rfind("}")
    try:
        o = json.loads(text[start:end + 1])
        return [bool(x) for x in o.get("permis", [])], sorted({str(x).strip() for x in o.get("obligations", [])})
    except (ValueError, AttributeError, TypeError):
        return [], None


def run_case(model, size, seed, fmt, run):
    rels, rnd = generate(size, seed)
    qs, ob = questions(rels, rnd)
    data = render(fmt, rels)
    raw = model.generate(prompt(fmt, data, qs, ob), max_tokens=4000)
    perm, obl = parse(raw)
    perm_ok = [i < len(perm) and perm[i] == q["truth"] for i, q in enumerate(qs)]
    return {"size": size, "seed": seed, "format": fmt, "run": run, "chars": len(data),
            "perm_ok": perm_ok, "obl_ok": obl == ob["truth"], "obl_truth": ob["truth"], "obl_answer": obl,
            "truth": [q["truth"] for q in qs], "answer": perm, "raw": raw[:600],
            "all_ok": all(perm_ok) and obl == ob["truth"]}


class StubBrain:
    """Plomberie seule : repond toujours « permis » et aucune obligation."""

    def generate(self, prompt, max_tokens=500):
        return json.dumps({"permis": [True, True, True], "obligations": []})


def summarize(rows, size=None):
    out = {}
    for f in FORMATS:
        r = [x for x in rows if x["format"] == f and (size is None or x["size"] == size)]
        groups = {}
        for x in r:
            groups.setdefault((x["size"], x["seed"]), []).append(x["all_ok"])
        out[f] = {"cases": len(r), "perm_ok": sum(sum(x["perm_ok"]) for x in r), "perm_total": sum(len(x["perm_ok"]) for x in r),
                  "obl_ok": sum(x["obl_ok"] for x in r), "all_ok": sum(x["all_ok"] for x in r),
                  "pass_k": sum(all(v) for v in groups.values()), "policies": len(groups),
                  "chars": round(sum(x["chars"] for x in r) / max(1, len(r)))}
    return out


CLIENT = ws.CstlClient(port=1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-haiku-5-5")
    ap.add_argument("--stub", action="store_true")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--workers", type=int, default=8)
    args = ap.parse_args()
    if args.stub:
        model = StubBrain()
    else:
        import team_task_benchmark as tb
        model = tb.ClaudeBrain(args.model)
    jobs = [(size, seed, f, run) for run in range(args.runs) for size in SIZES for seed in SEEDS for f in FORMATS]
    with ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
        rows = list(pool.map(lambda j: run_case(model, *j), jobs))
    label = "stub [plomberie seule, aucune conclusion]" if args.stub else args.model
    print(f"\nModele : {label}, {args.runs} passe(s) x {len(SIZES) * len(SEEDS)} politiques (3 questions de permission + 1 liste d'obligations)")
    for size in (None, *SIZES):
        print(f"\n  {'TOUTES' if size is None else f'{size} regles'} :")
        print(f"    {'format':13} | permissions justes | obligations exactes | tout juste | pass^{args.runs} | caracteres")
        for f, s in summarize(rows, size).items():
            print(f"    {f:13} | {s['perm_ok']:>8}/{s['perm_total']:<9} | {s['obl_ok']:>9}/{s['cases']:<9} | "
                  f"{s['all_ok']:>4}/{s['cases']:<5} | {s['pass_k']:>3}/{s['policies']:<3} | {s['chars']:>9}")
    dest = REPO / "scripts" / f"format_comprehension_benchmark_{'stub' if args.stub else args.model}.json"
    dest.write_text(json.dumps({"summary": summarize(rows), "rows": rows}, indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
