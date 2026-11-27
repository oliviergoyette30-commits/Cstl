#!/usr/bin/env python3
"""
scripts/nested_benchmark.py -- les formats face a des DONNEES IMBRIQUEES
(2026-11-22).

Pourquoi : CSTL est plat (relations type/sujet/objet + attributs). Un arbre
(commande -> lignes -> options) doit etre APLATI : chaque element devient une
relation qui pointe vers son parent. JSON et TOON representent l'imbrication
directement. C'etait le point faible attendu de CSTL, jamais mesure.

TROIS JEUX (2 tailles x 2 graines = 12 jeux de donnees) :
  commandes      commande -> lignes -> options
  organisation   departements -> equipes -> membres
  contrats       contrats -> sections -> clauses (obligations, interdictions)
Des champs facultatifs rendent les donnees irregulieres, comme en vrai.

FORMATS :
  json           JSON compact, imbrique
  toon           TOON imbrique (`toon_lite.encode_nested`, specification v4.4)
  cstl           CSTL complet, arbre aplati
  cstl_compact   CSTL compact en lignes, arbre aplati
  cstl_tableau   CSTL compact en tableaux, arbre aplati
  cstl_symboles  CSTL sans mot-cle, arbre aplati

APLATISSEMENT (sans perte, verifie dans les deux sens) : chaque objet d'une
liste devient une relation type=<nom de la liste>, subject=<son id> (son champ
`id`, sinon <id du parent>.<position>), object=<id du parent> (`-` a la
racine), ses champs simples en attributs. L'ordre est garde.

MESURES : caracteres, tokens (tokenizer Gemini, `--tokens`, gratuit), et
comprehension (`--comprehension MODELE`) : 3 questions par jeu, qui obligent a
suivre l'arbre, verite calculee par programme.

RESULTAT (2026-11-22, 12 jeux, tokens comptes par le tokenizer Gemini) :
                 commandes  organisation  contrats   TOTAL
  json             4 892       2 912       9 773    17 577
  toon             6 145       3 409      11 548    21 102
  cstl            11 837       6 546      24 853    43 236
  cstl_compact     7 769       4 481      16 849    29 099
  cstl_tableau     5 804       3 392      12 513    21 709
  cstl_symboles    5 780       3 368      12 489    21 637
  cstl_arbre       3 989       2 390       8 145    14 524
- Aplatir l'arbre en relations (cstl, compact, tableau, symboles) coute plus
  que le JSON (+23 % au mieux) : chaque element repete son id ET celui de son
  parent. C'etait le point faible attendu, et il est confirme.
- Le mode ARBRE du CSTL compact (`cstl_compact.encode_tree`, une section par
  liste, l'id du parent seulement, la position deduite de l'ordre) bat le
  JSON de 17 % et TOON de 31 %.
- TOON fait 20 % de plus que le JSON sur ces donnees, comme sa propre
  documentation l'annonce pour les donnees imbriquees.
- Allers-retours sans perte pour les 7 formats.

COMPREHENSION (2026-11-22, Gemma 4 31B, 1 passe, 36 questions par format) :
  json 32/33 (1 erreur d'API), toon 36/36, cstl_symboles 34/36, cstl_arbre 32/36
- Les erreurs portent surtout sur une question : « contrats avec une clause
  terminate dont amount_above est inferieur a 1000 ». Le modele compte aussi
  les clauses SANS amount_above (marque « - » en CSTL). Le JSON y fait aussi
  une erreur. Le mode arbre en fait une de plus : un comptage par departement.
- Le mode arbre est donc le plus court (17 % de moins que le JSON) mais un
  peu moins bien suivi par Gemma (89 % contre 97 %). Serie courte, un seul
  modele : a confirmer.

COMPREHENSION, PASSE 2 (2026-11-22, Gemma 4 31B, memes 36 questions, avec
l'arbre indente ; 3 jeux perdus sur des 503 reposes avec --resume) :
  cstl_indent 36/36, json 35/36, toon 35/36, cstl_arbre 31/36
- L'arbre indente est a la fois le plus court (10 792 tokens, 39 % de moins
  que le JSON) et le seul sans erreur : sans identifiant a relier, et sans
  « - » pour un champ absent (un champ facultatif n'est ecrit que s'il existe).
- Un seul modele, une passe : a confirmer sur d'autres modeles.

Usage :
  python3 scripts/nested_benchmark.py
  GEMINI_API_KEY=... python3 scripts/nested_benchmark.py --tokens
  GEMINI_API_KEY=... python3 scripts/nested_benchmark.py --comprehension gemma-4-31b-it
"""
import argparse
import json
import os
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

FORMATS = ("json", "toon", "cstl", "cstl_compact", "cstl_tableau", "cstl_symboles", "cstl_arbre", "cstl_indent", "cstl_indent_fin", "cstl_indent_ligne", "cstl_indent_ligne2")
KINDS = ("commandes", "organisation", "contrats")
SIZES = (8, 24)
SEEDS = (1, 2)


# ------------------------------------------------------------------ donnees

def gen(kind, n, seed):
    rnd = random.Random(zlib.crc32(f"{kind}/{n}/{seed}".encode()))
    if kind == "commandes":
        items = ["gpu_rack", "license_pack", "support_plan", "storage_tb", "router"]
        orders = []
        for i in range(n):
            o = {"id": f"o{i}", "client": f"client_{rnd.randint(1, 6)}", "date": f"2026-11-{rnd.randint(1, 28):02d}"}
            if rnd.random() < 0.3:
                o["note"] = rnd.choice(["urgent", "fragile", "vip"])
            o["lines"] = []
            for _ in range(rnd.randint(1, 4)):
                line = {"item": rnd.choice(items), "qty": rnd.randint(1, 5), "price": rnd.randint(10, 900)}
                if rnd.random() < 0.4:
                    line["options"] = [{"name": nm, "price": rnd.randint(5, 80)}
                                       for nm in rnd.sample(["warranty", "express", "install"], rnd.randint(1, 2))]
                o["lines"].append(line)
            orders.append(o)
        return {"orders": orders}
    if kind == "organisation":
        depts = []
        for i in range(max(2, n // 4)):
            d = {"id": f"dept_{i}", "name": rnd.choice(["finance", "support", "ops", "legal", "rnd"]) + f"_{i}",
                 "budget": rnd.randint(10, 500) * 1000, "teams": []}
            for j in range(rnd.randint(1, 3)):
                t = {"id": f"team_{i}_{j}", "lead": f"agent_{rnd.randint(1, 40)}", "members": []}
                for _ in range(rnd.randint(1, 5)):
                    m = {"agent": f"agent_{rnd.randint(1, 40)}", "role": rnd.choice(["analyst", "operator", "reviewer"]),
                         "clearance": rnd.choice(["low", "medium", "high"])}
                    if rnd.random() < 0.3:
                        m["since"] = f"202{rnd.randint(3, 6)}-0{rnd.randint(1, 9)}-01"
                    t["members"].append(m)
                d["teams"].append(t)
            depts.append(d)
        return {"departments": depts}
    contracts = []
    actions = ["terminate", "renew", "audit", "disclose", "pay", "notify"]
    for i in range(n):
        c = {"id": f"ctr_{i}", "party": f"party_{rnd.randint(1, 9)}", "sections": []}
        for j in range(rnd.randint(1, 3)):
            s = {"title": rnd.choice(["payment", "termination", "privacy", "audit", "liability"]) + f"_{j}", "clauses": []}
            for k in range(rnd.randint(1, 4)):
                cl = {"ref": f"{i}.{j}.{k}", "modality": rnd.choice(["MUST", "MUST_NOT", "MAY"]), "action": rnd.choice(actions)}
                if rnd.random() < 0.4:
                    cl["amount_above"] = rnd.choice([100, 500, 1000, 5000])
                s["clauses"].append(cl)
            c["sections"].append(s)
        contracts.append(c)
    return {"contracts": contracts}


def questions(kind, data, seed):
    rnd = random.Random(seed * 97 + len(json.dumps(data)))
    if kind == "commandes":
        o = rnd.choice(data["orders"])
        total = sum(ln["qty"] * (ln["price"] + sum(op["price"] for op in ln.get("options", []))) for ln in o["lines"])
        return [(f"Valeur totale de la commande {o['id']} : somme, sur ses lignes, de qty x (price + prix de toutes les "
                 "options de la ligne). (un nombre)", total),
                ("Combien de lignes, toutes commandes confondues, ont une option nommee warranty ?",
                 sum(any(op["name"] == "warranty" for op in ln.get("options", [])) for o2 in data["orders"] for ln in o2["lines"])),
                ("Quelles commandes contiennent une ligne gpu_rack avec qty d'au moins 3 ? (liste d'id)",
                 sorted({o2["id"] for o2 in data["orders"] for ln in o2["lines"] if ln["item"] == "gpu_rack" and ln["qty"] >= 3},
                        key=lambda x: int(x[1:])))]
    if kind == "organisation":
        d = rnd.choice(data["departments"])
        members = [(t, m) for d2 in data["departments"] for t in d2["teams"] for m in t["members"]]
        a = rnd.choice(members)[1]["agent"]
        return [(f"Combien de membres ont la clearance high dans le departement {d['id']} ?",
                 sum(m["clearance"] == "high" for t in d["teams"] for m in t["members"])),
                (f"Quels sont les lead des equipes dont {a} est membre ? (liste, sans doublon)",
                 sorted({t["lead"] for t, m in members if m["agent"] == a})),
                ("Quels departements ont un budget superieur a 200000 ET au moins une equipe de plus de 3 membres ? (liste d'id)",
                 sorted(d2["id"] for d2 in data["departments"] if d2["budget"] > 200000 and any(len(t["members"]) > 3 for t in d2["teams"])))]
    c = rnd.choice(data["contracts"])
    cl = rnd.choice([x for c2 in data["contracts"] for s in c2["sections"] for x in s["clauses"]])
    return [(f"Combien de clauses MUST_NOT le contrat {c['id']} contient-il, toutes sections confondues ?",
             sum(x["modality"] == "MUST_NOT" for s in c["sections"] for x in s["clauses"])),
            ("Quels contrats ont une clause d'action terminate avec un amount_above inferieur a 1000 ? (liste d'id)",
             sorted({c2["id"] for c2 in data["contracts"] for s in c2["sections"] for x in s["clauses"]
                     if x["action"] == "terminate" and x.get("amount_above") is not None and x["amount_above"] < 1000},
                    key=lambda x: int(x.split("_")[1]))),
            (f"Quel est le title de la section qui contient la clause de ref {cl['ref']} ? (une chaine)",
             next(s["title"] for c2 in data["contracts"] for s in c2["sections"] if cl in s["clauses"]))]


# ------------------------------------------------------------ aplatissement

def flatten(data):
    """Arbre -> relations (sans perte). Un seul tableau a la racine."""
    (root_key, items), = data.items()
    rels = []

    def walk(listkey, objs, parent):
        for i, o in enumerate(objs):
            oid = str(o["id"]) if "id" in o else f"{parent}.{i}"
            rel = {"type": listkey, "subject": oid, "object": parent}
            children = []
            for k, v in o.items():
                if k == "id":
                    continue
                if isinstance(v, list):
                    children.append((k, v))
                else:
                    rel[k] = json.dumps(v) if not isinstance(v, str) else v
            # un champ id explicite est garde : sans lui, l'id est la position
            rel["has_id"] = "1" if "id" in o else "0"
            rels.append(rel)
            for k, v in children:
                walk(k, v, oid)

    walk(root_key, items, "-")
    return rels


def unflatten(rels):
    nodes, roots, order = {}, [], []
    for r in rels:
        o = {}
        if r.get("has_id") == "1":
            o["id"] = r["subject"]
        for k, v in r.items():
            if k in ("type", "subject", "object", "has_id"):
                continue
            try:
                o[k] = json.loads(v)
                if isinstance(o[k], str):
                    o[k] = v
            except ValueError:
                o[k] = v
        nodes[r["subject"]] = o
        if r["object"] == "-":
            roots.append(o)
            order.append(r["type"])
        else:
            nodes[r["object"]].setdefault(r["type"], []).append(o)
    return {order[0]: roots}


def render(fmt, data):
    if fmt == "json":
        return json.dumps(data, ensure_ascii=False, separators=(",", ":"))
    if fmt == "toon":
        return toon_lite.encode_nested(data)
    if fmt == "cstl_arbre":
        return cstl_compact.encode_tree(data)
    if fmt == "cstl_indent":
        return cstl_compact.encode_tree_indented(data)
    if fmt == "cstl_indent_fin":
        return cstl_compact.encode_tree_indented(data, counts="end")
    if fmt in ("cstl_indent_ligne", "cstl_indent_ligne2"):
        return cstl_compact.encode_tree_indented(data, counts="line")
    rels = flatten(data)
    if fmt == "cstl":
        return rt.render_relations(rels)
    if fmt == "cstl_compact":
        return cstl_compact.encode(rels)
    if fmt == "cstl_tableau":
        return cstl_compact.encode(rels, table=True)
    return cstl_compact.encode_symbols(rels)


def decode(fmt, text):
    if fmt == "json":
        return json.loads(text)
    if fmt == "toon":
        return toon_lite.decode_nested(text)
    if fmt == "cstl_arbre":
        return cstl_compact.decode_tree(text)
    if fmt in ("cstl_indent", "cstl_indent_fin", "cstl_indent_ligne", "cstl_indent_ligne2"):
        return cstl_compact.decode_tree_indented(text)
    rels = rt.relations_in_text(text) if fmt == "cstl" else cstl_compact.decode(text)
    by_subject = {r["subject"]: r for r in rels}
    ordered = [by_subject[r["subject"]] for r in flatten_order(rels)]
    return unflatten(ordered)


def flatten_order(rels):
    """Le mode tableau regroupe par type : on remet chaque relation apres son
    parent, dans l'ordre d'origine des positions."""
    children = {}
    for r in rels:
        children.setdefault(r["object"], []).append(r)
    out = []

    def walk(parent):
        for r in children.get(parent, []):
            out.append(r)
            walk(r["subject"])
    walk("-")
    return out


NOTES = {
    "json": "Format : JSON imbrique.",
    "toon": "Format : TOON. Indentation = imbrication ; « cle[N]{champs}: » puis une ligne par element ; « - » = element de liste.",
    "cstl": ("Format : CSTL aplati. Une RELATION par element : type = nom de la liste qui le contient, subject = son id, "
             "object = id de son parent (« - » a la racine), autres champs en attributs ; has_id=1 si l'id est un vrai "
             "champ id, sinon l'id est <parent>.<position>."),
    "cstl_compact": ("Format : CSTL compact aplati. « REL <type> <subject> <object> cle=valeur... » : type = nom de la "
                     "liste, subject = id de l'element, object = id du parent (« - » a la racine)."),
    "cstl_tableau": ("Format : CSTL compact aplati en tableaux. « <type>[N] subject object col... » puis une ligne par "
                     "element : subject = id de l'element, object = id du parent (« - » a la racine) ; « - » = absent."),
    "cstl_arbre": ("Format : CSTL compact en arbre. Une section par liste : « nom[N]flags: colonnes » puis une ligne "
                   "par element. # = la 1re valeur est l'id propre de l'element ; ^ = la valeur suivante est l'id de son "
                   "parent. Un element sans id propre a pour id <id du parent>.<position parmi ses freres, a partir de "
                   "0> ; « - » = champ absent."),
    "cstl_indent": ("Format : CSTL compact en arbre indente. En tete, une ligne par liste : « @nom[N]^parent: "
                    "colonnes » (N elements en tout ; ^parent = liste dont ce sont les enfants). Puis l'arbre : un "
                    "element par ligne, ses enfants en dessous, indentes de 2 espaces. Les valeurs suivent les colonnes ; "
                    "un champ facultatif s'ecrit nom=valeur, et seulement s'il existe."),
    "cstl_indent_fin": ("Format : CSTL compact en arbre indente. En tete, une ligne par liste : « @nom^parent: "
                        "colonnes » (^parent = liste dont ce sont les enfants). Puis l'arbre : un element par ligne, "
                        "ses enfants en dessous, indentes de 2 espaces. Les colonnes sont les champs presents dans "
                        "TOUS les elements de la liste ; un autre champ s'ecrit nom=valeur, et seulement s'il existe. "
                        "Derniere ligne : « @end nom=N ... », le nombre d'elements ecrits dans chaque liste."),
    "cstl_indent_ligne": ("Format : CSTL compact en arbre indente. En tete, une ligne par liste : « @nom^parent: "
                          "colonnes » (^parent = liste dont ce sont les enfants). Puis l'arbre : un element par "
                          "ligne, ses enfants en dessous, indentes de 2 espaces. Les colonnes sont les champs "
                          "presents dans TOUS les elements de la liste ; un autre champ s'ecrit nom=valeur, et "
                          "seulement s'il existe. Un element qui a des enfants finit sa ligne par « /N », le nombre "
                          "de ses enfants directs. Derniere ligne : « @end »."),
    # 2026-11-24 : meme format, consigne de la regle des colonnes renforcee. Les
    # echecs stricts d'ecriture des modeles faibles venaient d'un champ
    # FACULTATIF mis en colonne (members attend 4 valeurs : « since »).
    "cstl_indent_ligne2": ("Format : CSTL compact en arbre indente. En tete, une ligne par liste : « @nom^parent: "
                           "colonnes » (^parent = liste dont ce sont les enfants). Puis l'arbre : un element par "
                           "ligne, ses enfants en dessous, indentes de 2 espaces. Un element qui a des enfants finit "
                           "sa ligne par « /N », le nombre de ses enfants directs. Derniere ligne : « @end ». "
                           "REGLE DES COLONNES : avant d'ecrire l'en-tete d'une liste, regarde TOUS ses elements. "
                           "Un champ ne devient une colonne que s'il existe dans CHACUN d'eux. Un champ qui manque "
                           "dans au moins un element n'est JAMAIS une colonne : ecris-le « nom=valeur » en fin de "
                           "ligne, seulement sur les elements qui l'ont."),
    "cstl_symboles": ("Format : CSTL compact aplati sans mot-cle. « <type>[N]: col... » puis une ligne par element : "
                      "id de l'element, id du parent (« - » a la racine), puis les colonnes ; « - » = absent."),
}


def extract_answer(raw):
    """La DERNIERE reponse {"r1": ...} du texte (2026-11-23). Certains modeles
    (Llama 4) recopient d'abord les donnees en JSON puis raisonnent avant de
    repondre : prendre du premier « { » au dernier « } » melangeait donnees et
    reponse. Renvoie (reponse, trouvee)."""
    dec, best, i = json.JSONDecoder(), None, raw.find("{")
    while i != -1:
        try:
            obj, _ = dec.raw_decode(raw, i)
            if isinstance(obj, dict) and "r1" in obj:
                best = obj
        except ValueError:
            pass
        i = raw.find("{", i + 1)
    return (best, True) if best is not None else ({}, False)


def same(answer, truth):
    if isinstance(truth, list):
        return isinstance(answer, list) and sorted(str(x).strip() for x in answer) == sorted(str(x) for x in truth)
    if isinstance(truth, (int, float)) and not isinstance(truth, bool):
        try:
            return abs(float(answer) - float(truth)) < 0.006
        except (TypeError, ValueError):
            return False
    return str(answer).strip() == str(truth)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tokens", action="store_true", help="tokens comptes par le tokenizer Gemini (GEMINI_API_KEY)")
    ap.add_argument("--comprehension", metavar="MODELE")
    ap.add_argument("--runs", type=int, default=1)
    ap.add_argument("--formats", default=",".join(FORMATS))
    ap.add_argument("--resume", action="store_true",
                    help="relit le fichier de resultats et ne repose que les jeux en erreur (API surchargee)")
    args = ap.parse_args()
    formats = [f for f in args.formats.split(",") if f]
    datasets = [(k, n, s, gen(k, n, s)) for k in KINDS for n in SIZES for s in SEEDS]

    broken = [f"{k}/{n}/{s}/{f}" for k, n, s, d in datasets for f in FORMATS if decode(f, render(f, d)) != d]
    print(f"{len(datasets)} jeux imbriques ; allers-retours en echec : {broken or 'aucun'}")

    count = None
    if args.tokens:
        import gemini_tokens
        count = lambda text: gemini_tokens.count("gemini-3.5-flash", os.environ["GEMINI_API_KEY"], text)  # noqa: E731
    for unit, measure in (("caracteres", len), ("tokens Gemini", count)):
        if measure is None:
            continue
        print(f"\n  {unit} :")
        print(f"  {'jeu':14} | " + " | ".join(f"{f:>13}" for f in FORMATS))
        totals = {f: 0 for f in FORMATS}
        for kind in KINDS:
            row = {f: sum(measure(render(f, d)) for k, _, _, d in datasets if k == kind) for f in FORMATS}
            for f in FORMATS:
                totals[f] += row[f]
            print(f"  {kind:14} | " + " | ".join(f"{row[f]:>13}" for f in FORMATS))
        print(f"  {'TOTAL':14} | " + " | ".join(f"{totals[f]:>13}" for f in FORMATS))
        if unit != "caracteres":
            Path(REPO / "scripts" / "nested_benchmark_tokens.json").write_text(json.dumps(totals, indent=2) + "\n")

    if args.comprehension:
        import team_task_benchmark as tb
        model = tb.make_brain(args.comprehension)

        def one(job):
            k, n, s, d, f, run = job
            qs = questions(k, d, s)
            lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(qs))
            prompt = (f"Voici des donnees echangees entre agents.\n{NOTES[f]}\n\nDonnees :\n{render(f, d)}\n\nQuestions :\n"
                      f"{lines}\n\nReponds uniquement par {{\"r1\": ..., \"r2\": ..., \"r3\": ...}}.")
            try:
                raw = model.generate(prompt, max_tokens=4000)
            except RuntimeError as e:
                return {"kind": k, "size": n, "seed": s, "format": f, "run": run, "ok": None, "error": str(e)[:200]}
            ans, found = extract_answer(raw)
            ok = [same(ans.get(f"r{i + 1}"), t) for i, (_, t) in enumerate(qs)]
            print(f"    {k:13} n={n:<3} s{s} {f:13} : {sum(ok)}/3", flush=True)
            return {"kind": k, "size": n, "seed": s, "format": f, "run": run, "ok": ok, "parsed": found, "raw": raw[-3000:]}

        jobs = [(k, n, s, d, f, r) for r in range(args.runs) for k, n, s, d in datasets for f in formats]
        dest = REPO / "scripts" / f"nested_comprehension_{args.comprehension.replace('/', '__')}.json"
        rows = [None] * len(jobs)
        old = json.loads(dest.read_text())["rows"] if dest.exists() else []
        if args.resume:
            # Par (type, taille, graine, format, passe) : ajouter un format ne
            # repose que ses questions, et les autres formats sont gardes.
            done = {(r["kind"], r.get("size"), r.get("seed"), r["format"], r.get("run", 0)): r for r in old
                    if r.get("ok") is not None and r.get("parsed", extract_answer(r.get("raw", ""))[1])}
            rows = [done.get((k, n, s, f, run)) for k, n, s, d, f, run in jobs]
        todo = [i for i, r in enumerate(rows) if r is None]
        with ThreadPoolExecutor(max_workers=int(os.environ.get("BENCH_WORKERS", "4"))) as pool:
            for i, r in zip(todo, pool.map(one, [jobs[i] for i in todo])):
                rows[i] = r
        # Les resultats d'autres formats deja dans le fichier sont conserves.
        mine = {(r["kind"], r.get("size"), r.get("seed"), r["format"], r.get("run", 0)) for r in rows}
        rows = rows + [r for r in old if (r["kind"], r.get("size"), r.get("seed"), r["format"], r.get("run", 0))
                       not in mine]
        print(f"\n  Comprehension ({args.comprehension}) :")
        for f in formats:
            r = [x for x in rows if x["format"] == f and x["ok"] is not None]
            print(f"    {f:13} | {sum(sum(x['ok']) for x in r):>3}/{3 * len(r)} questions justes")
        dest.write_text(
            json.dumps({"rows": rows}, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
