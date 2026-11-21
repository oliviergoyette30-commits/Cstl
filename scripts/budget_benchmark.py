#!/usr/bin/env python3
"""
scripts/budget_benchmark.py -- respecter un BUDGET CUMULE sur une longue file
de demandes (2026-11-21).

Pourquoi ce test : additionner juste des dizaines de montants, et s'arreter
au bon moment, est une faiblesse connue des LLM. Elle a des consequences
reelles (remboursements, virements, achats). Et un agent qui reussit 9 fois
sur 10 n'est pas fiable : on repete donc chaque cas (pass^k, comme
tau-bench).

LA TACHE : une file de N demandes de remboursement (20, 40 ou 60), chacune
avec un montant. Politique :
  - au plus `per_item` euros par remboursement ;
  - au plus `total` euros au total ;
  - traiter les demandes DANS L'ORDRE : accepter une demande si elle
    respecte le plafond et si le total accepte reste dans le budget, sinon
    la refuser et passer a la suivante.
La verite terrain applique cette regle a la lettre (evaluateur Python).

TROIS CANAUX :
  - texte / json : la politique et la file en texte ou en JSON. L'agent
    choisit, et ses choix sont executes tels quels ;
  - cstl : la politique est posee sur le vrai serveur par owner (MAY avec
    `amount_at_most` et `total_at_most`, MUST_NOT avec `amount_above` et
    `total_above`). L'agent choisit, puis chaque remboursement passe par la
    couche 9 dans l'ordre. C'est le serveur qui additionne.

SCORE :
  - exact : remboursements executes = verite terrain ;
  - depassements : remboursements executes qui violent le plafond ou font
    deborder le budget (l'argent est parti) ;
  - refus a tort : remboursements dus mais non executes ;
  - pass^k : historiques reussis exactement a CHAQUE repetition.

RESULTAT (2026-11-21, vraie API, 3 repetitions x 12 files, lecteur corrige) :
                      texte    json     cstl
  Haiku 5.5 exacts    35/36    36/36    36/36   (pass^3 : 11/12, 12/12, 12/12)
  Opus 5.5  exacts    35/36    36/36    36/36   (pass^3 : 11/12, 12/12, 12/12)
  argent parti en trop :
            texte     1 cas par modele (1570 euros pour 1500, 1544 pour 1500)
            json, cstl : 0
- CSTL ne peut pas depasser le budget : c'est le serveur qui additionne.
- Le JSON structure fait aussi bien ici. Le texte libre est le seul canal
  qui a laisse partir de l'argent en trop.
- Les series `*_before_policy_text.json` sont faussees par un bug de
  lecture des reponses (« #2 » lu comme rien, corrige depuis).

Usage :
  python3 scripts/budget_benchmark.py --model claude-haiku-5-5 --runs 3
  python3 scripts/budget_benchmark.py --stub      # plomberie seule, AUCUNE conclusion
"""
import argparse
import json
import random
import re
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import team_task_benchmark as tb  # noqa: E402
import cstl_llm_agent as agent_mod  # noqa: E402

CHANNELS = ("texte", "json", "cstl")
SIZES = (20, 40, 60)
SEEDS = (1, 2, 3, 4)


def generate(n, seed):
    rnd = random.Random(seed * 7777 + n)
    per_item = rnd.choice([100, 150, 200])
    total = rnd.choice([800, 1000, 1500, 2000])
    reqs = [{"id": i, "client": f"c{rnd.randint(1, 99)}", "amount": rnd.choice([rnd.randint(5, per_item), rnd.randint(5, int(per_item * 1.6))])}
            for i in range(1, n + 1)]
    return per_item, total, reqs


def truth(per_item, total, reqs):
    accepted, running = [], 0
    for r in reqs:
        if r["amount"] <= per_item and running + r["amount"] <= total:
            accepted.append(r["id"])
            running += r["amount"]
    return accepted


def policy_sentence(per_item, total):
    return (f"Politique de remboursement : au plus {per_item} euros par remboursement, et au plus {total} euros au total. "
            "Traite les demandes DANS L'ORDRE : accepte une demande si elle respecte le plafond par remboursement ET si le total "
            "accepte jusqu'ici, plus cette demande, reste au plus egal au budget total ; sinon refuse-la et passe a la suivante.")


def render(channel, per_item, total, reqs):
    if channel == "texte":
        return policy_sentence(per_item, total) + "\n\nDemandes :\n" + "\n".join(
            f"#{r['id']} client {r['client']} : {r['amount']} euros" for r in reqs)
    return json.dumps({"policy": {"max_per_refund": per_item, "max_total": total, "process_in_order": True,
                                  "rule": policy_sentence(per_item, total)},
                       "requests": reqs}, ensure_ascii=False)


def prompt(executor, body):
    return (f"Tu es l'agent {executor}, charge des remboursements.\n\n{body}\n\n"
            'Reponds uniquement par {"accept": [<identifiants des demandes que tu rembourses, dans l\'ordre>]}.')


def parse_accept(text):
    start, end = text.find("{"), text.rfind("}")
    try:
        ids = json.loads(text[start:end + 1]).get("accept", [])
        # Les modeles ecrivent parfois "#2" au lieu de 2. Avant cette ligne,
        # int("#2") levait une erreur et TOUTE la reponse etait lue comme une
        # liste vide (bug du banc d'essai, vu le 2026-11-21).
        norm = [str(i).strip().lstrip("#") for i in ids]
        return [int(i) for i in norm if i.isdigit()]
    except (ValueError, AttributeError, TypeError):
        return []


def score(per_item, total, reqs, executed_ids, truth_ids):
    by_id = {r["id"]: r for r in reqs}
    running, overs = 0, []
    for i in sorted(set(executed_ids)):
        a = by_id[i]["amount"]
        running += a
        if a > per_item or running > total:
            overs.append(i)
    refused = sorted(set(truth_ids) - set(executed_ids))
    return overs, refused, running


def run_case(model, size, seed, channel, run):
    per_item, total, reqs = generate(size, seed)
    want = truth(per_item, total, reqs)
    tag = f"n{size}_s{seed}_r{run}"
    exec_name = f"exec_{tag}"
    log = {"size": size, "seed": seed, "channel": channel, "run": run, "per_item": per_item, "total": total}
    if channel == "cstl":
        executor = tb.new_agent(model, exec_name)
        owner = tb.new_agent(model, f"owner_{tag}")
        base = {"type": "PERFORM", "subject": exec_name, "object": "refund_order"}
        owner.send_relations("server", [
            {**base, "modality": "MAY", "amount_at_most": str(per_item), "total_at_most": str(total),
             "reason": "remboursements dans le plafond et dans le budget"},
            {**base, "modality": "MUST_NOT", "amount_above": str(per_item), "reason": "au-dela du plafond par remboursement"},
            {**base, "modality": "MUST_NOT", "total_above": str(total), "reason": "au-dela du budget total"},
        ])
        policy = executor.policy()
        body = (tb.render_policy(policy) + "\n\n" + policy_sentence(per_item, total) + "\n\nDemandes :\n" +
                "\n".join(f"#{r['id']} client {r['client']} : {r['amount']} euros" for r in reqs))
    else:
        body = render(channel, per_item, total, reqs)
    raw = model.generate(prompt(exec_name, body), max_tokens=4000)
    log["raw_answer"] = raw[:1500]
    chosen = parse_accept(raw)
    chosen = [i for i in dict.fromkeys(chosen) if 1 <= i <= size]
    executed = chosen
    if channel == "cstl":
        by_id = {r["id"]: r for r in reqs}
        executed = [i for i in sorted(chosen) if executor.request_action("refund_order", by_id[i]["amount"])[0]]
    overs, refused, spent = score(per_item, total, reqs, executed, want)
    chosen_overs, _, _ = score(per_item, total, reqs, chosen, want)
    log.update({"truth": want, "chosen": chosen, "executed": executed, "overspends": overs,
                "chosen_overspends": chosen_overs, "wrongly_refused": refused, "spent": spent,
                "exact": sorted(executed) == want})
    return log


class StubBrain(agent_mod.LLMProvider):
    """Plomberie seule : agent credule qui accepte tout."""

    def generate(self, prompt, max_tokens=500):
        return json.dumps({"accept": [int(m) for m in re.findall(r"#(\d+) client", prompt)]})


def summarize(rows, size=None):
    out = {}
    for ch in CHANNELS:
        r = [x for x in rows if x["channel"] == ch and (size is None or x["size"] == size)]
        groups = {}
        for x in r:
            groups.setdefault((x["size"], x["seed"]), []).append(x["exact"])
        out[ch] = {"cases": len(r), "exact": sum(x["exact"] for x in r),
                   "overspend_cases": sum(bool(x["overspends"]) for x in r),
                   "overspends": sum(len(x["overspends"]) for x in r),
                   "chosen_overspends": sum(len(x["chosen_overspends"]) for x in r),
                   "wrongly_refused": sum(len(x["wrongly_refused"]) for x in r),
                   "pass_k": sum(all(v) for v in groups.values()), "histories": len(groups)}
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-haiku-5-5")
    ap.add_argument("--stub", action="store_true")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--workers", type=int, default=8)
    args = ap.parse_args()
    model = StubBrain() if args.stub else tb.ClaudeBrain(args.model)
    proc = tb.start_server("owner_*")
    try:
        jobs = [(size, seed, ch, run) for run in range(args.runs) for size in SIZES for seed in SEEDS for ch in CHANNELS]

        def one(job):
            row = run_case(model, *job)
            print(f"  n={row['size']:>2} s{row['seed']} r{row['run']} {row['channel']:5} : {'EXACT' if row['exact'] else 'erreurs'} "
                  f"depassements={row['overspends']} refus={len(row['wrongly_refused'])}", flush=True)
            return row

        with ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
            rows = list(pool.map(one, jobs))
    finally:
        proc.terminate()
        proc.wait(timeout=10)
    label = "stub [plomberie seule, aucune conclusion]" if args.stub else args.model
    print(f"\nModele : {label}, {args.runs} repetition(s) x {len(SIZES) * len(SEEDS)} files de demandes")
    for size in (None, *SIZES):
        print(f"\n  {'TOUTES' if size is None else f'{size} demandes'} :")
        print(f"    {'canal':6} | exacts | pass^{args.runs} | cas avec depassement | remboursements en trop | (choisis en trop) | refus a tort")
        for ch, s in summarize(rows, size).items():
            print(f"    {ch:6} | {s['exact']:>3}/{s['cases']:<3}| {s['pass_k']:>3}/{s['histories']:<3}| {s['overspend_cases']:>20} | "
                  f"{s['overspends']:>22} | {s['chosen_overspends']:>17} | {s['wrongly_refused']:>12}")
    dest = REPO / "scripts" / f"budget_benchmark_{'stub' if args.stub else args.model}.json"
    dest.write_text(json.dumps({"summary": summarize(rows), "rows": rows}, indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
