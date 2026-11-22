#!/usr/bin/env python3
"""
scripts/data_types_benchmark.py -- le CSTL compact reste-t-il le plus
court, et aussi bien compris, sur AUTRE CHOSE que des politiques ?
(2026-11-22)

Pourquoi : tous les bancs precedents portaient sur des politiques (regles
MUST/MUST_NOT avec conditions), le terrain pour lequel le CSTL compact a ete
concu. Les agents s'echangent bien d'autres choses. Ce test en couvre cinq :
  taches        delegation : qui fait quoi, statut, echeance, dependances
  faits         sujet, predicat, objet, source, confiance (la forme
                naturelle de CSTL : des relations)
  commandes     tableau regulier : id, client, montant, statut, date (le
                terrain favori de TOON)
  evenements    journal irregulier : horodatage, agent, evenement, champs
                qui varient d'une ligne a l'autre
  negociation   offres et contre-offres : prix, quantite, reponse a, condition

QUATRE FORMATS, memes donnees :
  json          JSON compact, champs absents omis
  toon          TOON (`toon_lite.py`, tableaux uniformes, null pour un champ
                absent : sa meilleure forme)
  cstl          CSTL complet, une RELATION [type=..., subject=..., object=..., ...]
                par element
  cstl_compact  CSTL compact (`sdk/python/cstl_compact.py`). Hors politiques,
                il n'a que sa forme generale : REL <type> <subject> <object> cle=valeur

Pour CSTL, chaque element devient une relation : `subject` et `object`
portent les deux champs principaux, le reste devient des attributs. La
legende de chaque jeu de donnees (identique pour tous les formats) dit a
quoi correspondent subject et object.

QUESTIONS : 3 par jeu de donnees (somme, filtre, dependance, derniere
offre...), verite calculee par programme. 5 types x 2 tailles (20 et 60
elements) x 2 graines = 20 jeux de donnees, 3 passes.

RESULTAT (2026-11-22, vraie API, 3 passes x 20 jeux de donnees, 180 questions) :
                 questions justes      tokens donnees (moyenne)
                 Haiku     Opus
  json           179       178          2 006
  toon           176       178          1 185
  cstl           178       174          2 146
  cstl_compact   177       177          1 571
  par type, tokens (json / toon / cstl / cstl_compact) :
    taches       1 788 /   973 / 1 922 / 1 362
    faits        1 958 / 1 054 / 2 009 / 1 478
    commandes    1 862 /   995 / 2 052 / 1 492
    evenements   1 962 / 1 540 / 2 177 / 1 585
    negociation  2 457 / 1 364 / 2 570 / 1 940
- Hors politiques, TOON est le plus court : le CSTL compact coute 33 % de
  tokens de plus (1 571 contre 1 185), car il repete le nom de chaque champ
  (status=, due=) a chaque ligne, alors que TOON l'ecrit une fois dans son
  en-tete. Seule exception, le journal irregulier : 1 585 contre 1 540.
- Le CSTL compact reste 22 % plus court que le JSON compact et 27 % plus
  court que le CSTL complet.
- La comprehension est la meme dans les quatre formats (174 a 179 sur 180).
- Sur les politiques (`format_short_benchmark.py`), c'etait l'inverse : le
  CSTL compact etait 25 % plus court que TOON, parce que les regles ont des
  conditions facultatives que TOON doit remplir de null.

Usage :
  python3 scripts/data_types_benchmark.py --model claude-haiku-5-5 --runs 3
  python3 scripts/data_types_benchmark.py --stub      # plomberie seule, AUCUNE conclusion
"""
import argparse
import json
import random
import sys
import zlib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import relay_tamper_benchmark as rt  # noqa: E402
import toon_lite  # noqa: E402
import cstl_compact  # noqa: E402

FORMATS = ("json", "toon", "cstl", "cstl_compact", "cstl_tableau", "cstl_symboles")
TYPES = ("taches", "faits", "commandes", "evenements", "negociation")
SIZES = (20, 60)
SEEDS = (1, 2)
AGENTS = [f"agent_{c}" for c in "abcdefgh"]

NOTES = {
    "json": "Format : JSON.",
    "toon": ("Format : TOON. « nom[N]{champ1,champ2,...}: » puis une ligne par element, valeurs dans l'ordre des "
             "champs ; null = champ absent."),
    "cstl": ("Format : CSTL. Une ligne RELATION [type=..., subject=..., object=..., cle=valeur, ...] par element."),
    "cstl_compact": ("Format : CSTL compact. Une ligne « REL <type> <subject> <object> cle=valeur ... » par element."),
    "cstl_symboles": ("Format : CSTL compact sans mot-cle. « <TYPE>[N]: col1 col2 ... » puis une ligne par element : "
                      "les 2 premieres valeurs sont subject et object, puis les colonnes annoncees dans l'ordre ; "
                      "« - » = champ absent."),
    "cstl_tableau": ("Format : CSTL compact en tableau. « <TYPE>[N] subject object col1 col2 ... » puis une ligne par "
                     "element, valeurs dans l'ordre des colonnes ; « - » = champ absent."),
}


# ---------------------------------------------------------------- donnees

def gen(kind, n, seed):
    """(enregistrements, nom du type de relation, champ subject, champ object, legende, questions)."""
    rnd = random.Random(zlib.crc32(f"{kind}/{n}/{seed}".encode()))   # meme graine a chaque execution
    if kind == "taches":
        recs = []
        for i in range(n):
            r = {"id": f"t{i}", "assignee": rnd.choice(AGENTS), "status": rnd.choice(["todo", "doing", "done", "blocked"]),
                 "due": f"2026-12-{rnd.randint(1, 28):02d}", "priority": rnd.randint(1, 3)}
            if i > 0 and rnd.random() < 0.5:
                r["depends_on"] = f"t{rnd.randrange(i)}"
            recs.append(r)
        legend = ("Taches : id, assignee (l'agent charge de la tache), status (todo, doing, done, blocked), due "
                  "(echeance), priority (1 = la plus haute), depends_on (tache dont elle depend, si elle existe). "
                  "En CSTL : type=TASK, subject = id, object = assignee.")
        a = rnd.choice(AGENTS)
        done = {r["id"] for r in recs if r["status"] == "done"}
        q = [(f"Combien de taches de priorite 1 ne sont pas au statut done ?",
              sum(r["priority"] == 1 and r["status"] != "done" for r in recs)),
             (f"Quelles taches de {a} dependent d'une tache qui n'est pas au statut done ? (liste d'id)",
              sorted(r["id"] for r in recs if r["assignee"] == a and r.get("depends_on") and r["depends_on"] not in done)),
             ("Quelle tache non terminee (statut different de done) a l'echeance la plus proche ? En cas d'egalite, "
              "la plus petite par ordre d'id numerique (t2 avant t10). (un id)",
              min((r for r in recs if r["status"] != "done"), key=lambda r: (r["due"], int(r["id"][1:])))["id"])]
        return recs, "TASK", "id", "assignee", legend, q
    if kind == "faits":
        subjects = [f"entity_{i}" for i in range(max(6, n // 4))]
        preds = ["located_in", "owned_by", "supplies", "competes_with", "founded_by"]
        sources = ["registry", "news", "agent_report", "web"]
        recs = [{"id": f"f{i}", "subject": rnd.choice(subjects), "predicate": rnd.choice(preds),
                 "object": rnd.choice(subjects + ["paris", "berlin", "acme_corp"]), "source": rnd.choice(sources),
                 "confidence": round(rnd.choice([0.3, 0.5, 0.7, 0.8, 0.9, 0.95]), 2)} for i in range(n)]
        legend = ("Faits : id, subject, predicate, object, source (d'ou vient le fait), confidence (0 a 1). "
                  "En CSTL : type = predicate, subject = subject, object = object.")
        e = rnd.choice(subjects)
        s = rnd.choice(sources)
        q = [(f"Combien de faits ont la source {s} et une confidence d'au moins 0.8 ?",
              sum(r["source"] == s and r["confidence"] >= 0.8 for r in recs)),
             (f"Quels faits ont {e} comme subject OU comme object ? (liste d'id)",
              sorted((r["id"] for r in recs if e in (r["subject"], r["object"])), key=lambda x: int(x[1:]))),
             ("Quelle est la confidence moyenne des faits de predicate supplies, arrondie a 2 decimales ? "
              "(0 s'il n'y en a pas)",
              round(sum(r["confidence"] for r in recs if r["predicate"] == "supplies") /
                    max(1, sum(r["predicate"] == "supplies" for r in recs)), 2))]
        for r in recs:   # pour CSTL : le predicat devient le type de relation
            r["_type"] = r["predicate"].upper()
        return recs, None, "subject", "object", legend, q
    if kind == "commandes":
        clients = [f"client_{i}" for i in range(max(5, n // 6))]
        recs = [{"id": f"o{i}", "client": rnd.choice(clients), "amount": rnd.randint(10, 900),
                 "currency": rnd.choice(["EUR", "EUR", "USD"]), "status": rnd.choice(["paid", "pending", "refunded"]),
                 "date": f"2026-11-{rnd.randint(1, 30):02d}"} for i in range(n)]
        legend = ("Commandes : id, client, amount, currency (EUR ou USD), status (paid, pending, refunded), date. "
                  "En CSTL : type=ORDER, subject = id, object = client.")
        c = rnd.choice(clients)
        q = [(f"Quel est le montant total, en EUR, des commandes paid de {c} ? (un nombre)",
              sum(r["amount"] for r in recs if r["client"] == c and r["status"] == "paid" and r["currency"] == "EUR")),
             ("Combien de commandes en USD ont un amount superieur a 500 ?",
              sum(r["currency"] == "USD" and r["amount"] > 500 for r in recs)),
             ("Quelles commandes refunded datent d'apres le 2026-11-20 (strictement) ? (liste d'id)",
              sorted((r["id"] for r in recs if r["status"] == "refunded" and r["date"] > "2026-11-20"), key=lambda x: int(x[1:])))]
        return recs, "ORDER", "id", "client", legend, q
    if kind == "evenements":
        recs = []
        for i in range(n):
            ev = rnd.choice(["login", "tool_call", "error", "handoff", "logout"])
            r = {"id": f"e{i}", "ts": f"2026-11-21T{8 + i * 10 // n:02d}:{(i * 7) % 60:02d}:00Z", "agent": rnd.choice(AGENTS),
                 "event": ev}
            if ev == "tool_call":
                r["tool"] = rnd.choice(["search", "db_query", "send_email"])
                r["ms"] = rnd.randint(20, 3000)
            if ev == "error":
                r["code"] = rnd.choice(["E401", "E500", "E429"])
            if ev == "handoff":
                r["to"] = rnd.choice(AGENTS)
            recs.append(r)
        legend = ("Evenements (dans l'ordre chronologique) : id, ts (horodatage), agent, event (login, tool_call, "
                  "error, handoff, logout) ; selon l'evenement : tool et ms (duree en millisecondes) pour tool_call, "
                  "code pour error, to (agent qui recoit) pour handoff. En CSTL : type=EVENT, subject = agent, "
                  "object = event.")
        a = rnd.choice(AGENTS)
        q = [(f"Combien d'erreurs de code E500 l'agent {a} a-t-il eues ?",
              sum(r["agent"] == a and r.get("code") == "E500" for r in recs)),
             ("Quelle est la duree totale (ms) des tool_call avec l'outil db_query ? (un nombre)",
              sum(r.get("ms", 0) for r in recs if r.get("tool") == "db_query")),
             ("Quels agents ont recu au moins un handoff ? (liste de noms)",
              sorted({r["to"] for r in recs if r.get("to")}))]
        return recs, "EVENT", "agent", "event", legend, q
    # negociation
    items = ["gpu_rack", "license_pack", "support_plan", "storage_tb"]
    recs = []
    for i in range(n):
        frm, to = rnd.sample(AGENTS[:4], 2)
        r = {"id": f"m{i}", "from": frm, "to": to, "item": rnd.choice(items), "price": rnd.randint(50, 500) * 10,
             "qty": rnd.randint(1, 20), "kind": "offer" if i == 0 or rnd.random() < 0.3 else rnd.choice(["counter", "accept", "reject"])}
        if i > 0 and r["kind"] != "offer":
            r["reply_to"] = f"m{rnd.randrange(max(0, i - 5), i)}"
        if rnd.random() < 0.25:
            r["condition"] = rnd.choice(["delivery_30d", "net_60", "volume_discount"])
        recs.append(r)
    legend = ("Negociation (dans l'ordre d'envoi) : id, from, to, item, price (prix unitaire), qty, kind (offer, counter, "
              "accept, reject), reply_to (message auquel il repond, s'il existe), condition (si elle existe). "
              "En CSTL : type=OFFER, subject = from, object = item.")
    it = rnd.choice(items)
    last = [r for r in recs if r["item"] == it and r["kind"] in ("offer", "counter")]
    accepted = {r["reply_to"] for r in recs if r["kind"] == "accept" and r.get("reply_to")}
    q = [(f"Quel est le prix unitaire de la DERNIERE offre ou contre-offre (kind offer ou counter) sur {it} ? "
          "(un nombre, 0 s'il n'y en a pas)", last[-1]["price"] if last else 0),
         ("Quels messages ont recu au moins une reponse de kind accept ? (liste d'id)",
          sorted(accepted, key=lambda x: int(x[1:]))),
         ("Quelle est la valeur totale (price x qty) des messages de kind offer qui ont une condition ? (un nombre)",
          sum(r["price"] * r["qty"] for r in recs if r["kind"] == "offer" and r.get("condition")))]
    return recs, "OFFER", "from", "item", legend, q


def to_relations(recs, rtype, subj, obj):
    out = []
    for r in recs:
        rel = {"type": r.get("_type", rtype), "subject": str(r[subj]), "object": str(r[obj])}
        for k, v in r.items():
            if k in (subj, obj, "_type") or (rtype is None and k == "predicate"):
                continue
            rel[k] = str(v)
        out.append(rel)
    return out


def render(fmt, kind, recs, rtype, subj, obj):
    clean = [{k: v for k, v in r.items() if k != "_type"} for r in recs]
    if fmt == "json":
        return json.dumps({kind: clean}, ensure_ascii=False, separators=(",", ":"))
    if fmt == "toon":
        fields = []
        for r in clean:
            fields += [k for k in r if k not in fields]
        return toon_lite.encode({kind: [{f: r.get(f) for f in fields} for r in clean]})
    rels = to_relations(recs, rtype, subj, obj)
    if fmt == "cstl_tableau":
        return cstl_compact.encode(rels, table=True)
    if fmt == "cstl_symboles":
        return cstl_compact.encode_symbols(rels)
    return rt.render_relations(rels) if fmt == "cstl" else cstl_compact.encode(rels)


# ---------------------------------------------------------------- evaluation

def prompt(fmt, legend, data, qs):
    lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(qs))
    return (f"Voici des donnees echangees entre agents.\n{NOTES[fmt]}\n{legend}\n\nDonnees :\n{data}\n\n"
            f"Questions :\n{lines}\n\nReponds uniquement par "
            '{"r1": <reponse 1>, "r2": <reponse 2>, "r3": <reponse 3>} (un nombre, une chaine ou une liste selon la question).')


def same(answer, truth):
    if isinstance(truth, list):
        return isinstance(answer, list) and sorted(str(x).strip() for x in answer) == sorted(str(x) for x in truth)
    if isinstance(truth, (int, float)):
        try:
            return abs(float(answer) - float(truth)) < 0.006
        except (TypeError, ValueError):
            return False
    return str(answer).strip() == str(truth)


def parse(text):
    start, end = text.find("{"), text.rfind("}")
    try:
        o = json.loads(text[start:end + 1])
        return [o.get(f"r{i}") for i in (1, 2, 3)]
    except (ValueError, AttributeError):
        return [None, None, None]


TOKENS = {}


def count(model, text):
    client = getattr(model, "client", None)
    if client is None:
        return len(text) // 4
    if text not in TOKENS:
        TOKENS[text] = client.messages.count_tokens(model=model.model, messages=[{"role": "user", "content": text}]).input_tokens
    return TOKENS[text]


def run_case(model, kind, n, seed, fmt, run):
    recs, rtype, subj, obj, legend, qs = gen(kind, n, seed)
    data = render(fmt, kind, recs, rtype, subj, obj)
    raw = model.generate(prompt(fmt, legend, data, qs), max_tokens=4000)
    answers = parse(raw)
    ok = [same(a, t) for a, (_, t) in zip(answers, qs)]
    return {"kind": kind, "size": n, "seed": seed, "format": fmt, "run": run, "ok": ok, "all_ok": all(ok),
            "answers": answers, "truth": [t for _, t in qs], "tokens_data": count(model, data), "raw": raw[:1500]}


class StubBrain:
    """Plomberie seule : repond toujours 0."""

    def generate(self, prompt, max_tokens=500):
        return '{"r1": 0, "r2": 0, "r3": 0}'


def summarize(rows, kind=None):
    out = {}
    for f in FORMATS:
        r = [x for x in rows if x["format"] == f and (kind is None or x["kind"] == kind)]
        groups = {}
        for x in r:
            groups.setdefault((x["kind"], x["size"], x["seed"]), []).append(x["all_ok"])
        out[f] = {"questions_ok": sum(sum(x["ok"]) for x in r), "questions": sum(len(x["ok"]) for x in r),
                  "cases_ok": sum(x["all_ok"] for x in r), "cases": len(r),
                  "pass_k": sum(all(v) for v in groups.values()), "datasets": len(groups),
                  "tokens_data": round(sum(x["tokens_data"] for x in r) / max(1, len(r)))}
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
        model = StubBrain()
    else:
        import team_task_benchmark as tb
        model = tb.ClaudeBrain(args.model)
    jobs = [(k, n, s, f, run) for run in range(args.runs) for k in TYPES for n in SIZES for s in SEEDS for f in FORMATS]
    with ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
        rows = list(pool.map(lambda j: run_case(model, *j), jobs))
    label = "stub [plomberie seule, aucune conclusion]" if args.stub else args.model
    print(f"\nModele : {label}, {args.runs} passe(s) x {len(TYPES) * len(SIZES) * len(SEEDS)} jeux de donnees (3 questions chacun)")
    for kind in (None, *TYPES):
        print(f"\n  {'TOUS LES TYPES' if kind is None else kind} :")
        print(f"    {'format':12} | questions justes | cas tout juste | pass^{args.runs} | tokens donnees")
        for f, s in summarize(rows, kind).items():
            print(f"    {f:12} | {s['questions_ok']:>7}/{s['questions']:<8} | {s['cases_ok']:>6}/{s['cases']:<7} | "
                  f"{s['pass_k']:>3}/{s['datasets']:<3} | {s['tokens_data']:>14}")
    dest = REPO / "scripts" / f"data_types_benchmark_{'stub' if args.stub else args.model}.json"
    dest.write_text(json.dumps({"summary": summarize(rows), "by_type": {k: summarize(rows, k) for k in TYPES}, "rows": rows},
                               indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
