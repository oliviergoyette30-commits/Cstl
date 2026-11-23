#!/usr/bin/env python3
"""
scripts/unknown_benchmark.py -- le « vide qui parle » mesure (2026-11-23).

Pourquoi : le corollaire fondateur de CSTL dit que « l'information est et
restera muette face a un vide relie qui parle ». L'arbre indente ecrit
maintenant une valeur INCONNUE nativement (`?`, `?{sigma:0.3,pending:...}`),
distincte de null (connue : pas de valeur) et d'un champ absent (sans objet).
Ce banc mesure si un modele distingue ces quatre etats, et a quel cout en
tokens, selon le format.

Donnees : des dossiers de patients ; deux examens (renal, potassium), chacun
dans un des quatre etats :
  valeur        un nombre
  null          connu, sans valeur (examen impossible)
  inconnu       pas encore mesure, avec son incertitude (sigma) et parfois ce
                qu'on attend (pending)
  absent        sans objet (examen non prescrit)

Formats :
  json_naif     le JSON tel qu'on l'ecrit d'habitude : l'inconnu devient null
                (le vide est muet : l'information est PERDUE, pas mal lue)
  json          JSON avec la convention {"$unknown": {...}}
  toon          TOON (toon_lite) avec la meme convention
  cstl_indent   arbre indente : `?` natif

Questions (memes pour tous) : patients a l'examen inconnu, patients a
l'examen connu sans valeur, patients dont la mesure est attendue au prochain
controle, patients a la valeur connue au-dessus d'un seuil.

RESULTATS (2026-11-23, Gemma 4 31B, 6 jeux, 24 questions par format, tous
complets ; tokens Gemini) :
  format         tokens   justes
  csv             2 293   24/24   (donnees plates : son terrain ideal)
  cstl_indent     2 390   24/24
  cstl_arbre      2 665   24/24
  json_naif       2 972    8/24   (inconnu 1/6, null 0/6, pending 1/6, seuil 6/6)
  json (compact)  3 697   24/24
  yaml            4 802   24/24
  toon            4 928   24/24
  xml             5 809   24/24
  json_indente    7 604   24/24
- Le JSON naif rate les questions sur le vide : l'information n'existe plus
  (tout est null), le modele n'y peut rien. Le corollaire, mesure.
- Des que l'inconnu est ECRIT, Gemma ne se trompe dans aucun format : ce qui
  compte est que le vide parle, pas le format. La difference est le cout :
  de 2 293 a 7 604 tokens pour la meme information.
- CSV gagne de peu parce que ces dossiers sont plats ; il ne sait ni
  imbriquer ni porter des details variables (colonnes _sigma/_pending
  inventees). L'arbre indente est le plus court des formats generaux, et le
  seul complet ET plus court que le JSON naif qui perd l'information.
- Jeux perdus sur des erreurs d'API reposes avec --resume (0 manquant).

Usage :
  GEMINI_API_KEY=... python3 scripts/unknown_benchmark.py --tokens
  GEMINI_API_KEY=... python3 scripts/unknown_benchmark.py --comprehension gemma-4-31b-it [--resume]
  (--resume garde les reponses deja obtenues : ajouter un format ne repose
  que ses questions)
"""
import argparse
import json
import os
import random
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import cstl_compact  # noqa: E402
import nested_benchmark as nb  # noqa: E402
import toon_lite  # noqa: E402

FORMATS = ("json_naif", "json", "toon", "cstl_indent", "json_indente", "yaml", "xml", "csv", "cstl_arbre")
SIZES = (12, 30)
SEEDS = (1, 2, 3)
EXAMS = ("renal", "potassium")

SEMANTICS = ("Chaque patient a deux examens, renal et potassium. Un examen peut etre : une valeur (nombre) ; "
             "null = connu mais sans valeur (examen impossible) ; inconnu = pas encore mesure, avec son "
             "incertitude sigma et parfois ce qu'on attend (pending) ; absent du dossier = sans objet (examen "
             "non prescrit).")
NOTES = {
    "json_naif": "Format : JSON. " + SEMANTICS + " Dans ce JSON, une valeur inconnue est ecrite null.",
    "json": "Format : JSON. " + SEMANTICS + ' Une valeur inconnue s\'ecrit {"$unknown": {...details}}.',
    "toon": ("Format : TOON. Indentation = imbrication ; « cle[N]{champs}: » puis une ligne par element ; « - » = "
             "element de liste. " + SEMANTICS + ' Une valeur inconnue est un objet "$unknown" avec ses details.'),
    "json_indente": "Format : JSON indente. " + SEMANTICS + ' Une valeur inconnue s\'ecrit {"$unknown": {...details}}.',
    "yaml": "Format : YAML. " + SEMANTICS + " Une valeur inconnue est un objet $unknown avec ses details.",
    "xml": ("Format : XML. " + SEMANTICS + " Un examen est un element : sa valeur en texte ; null s'ecrit "
            "null=\"true\" ; une valeur inconnue s'ecrit unknown=\"true\" avec ses details en attributs ; un "
            "examen absent n'a pas d'element."),
    "csv": ("Format : CSV, une ligne par patient. " + SEMANTICS + " Colonne d'examen : un nombre ; « null » = "
            "connu sans valeur ; « ? » = inconnu, avec ses details dans les colonnes <examen>_sigma et "
            "<examen>_pending ; cellule vide = examen absent."),
    "cstl_arbre": (nb.NOTES["cstl_arbre"] + " " + SEMANTICS + ' Une valeur inconnue est une cellule JSON '
                   '"{\"$unknown\": {...}}"~ ; null s\'ecrit null.'),
    "cstl_indent": (nb.NOTES["cstl_indent"] + " " + SEMANTICS + " Une valeur inconnue s'ecrit « ? », ou "
                    "« ?{details} » avec ses details ; null s'ecrit null ; un examen absent n'est pas ecrit."),
}


def gen(n, seed):
    rnd = random.Random(seed * 1009 + n)
    patients = []
    for i in range(n):
        p = {"id": f"p{i}", "age": rnd.randint(20, 90), "ward": rnd.choice(["A", "B", "C"])}
        for exam in EXAMS:
            state = rnd.choices(["value", "null", "unknown", "absent"], [40, 20, 25, 15])[0]
            if state == "value":
                p[exam] = rnd.randint(30, 120) if exam == "renal" else round(rnd.uniform(3.0, 6.0), 1)
            elif state == "null":
                p[exam] = None
            elif state == "unknown":
                details = {"sigma": rnd.choice([0.1, 0.3, 0.5])}
                if rnd.random() < 0.5:
                    details["pending"] = rnd.choice(["next_checkup", "lab_result"])
                p[exam] = cstl_compact.unknown(**details)
        patients.append(p)
    return {"patients": patients}


def naive(v):
    """Ce qu'un producteur JSON ecrit d'habitude : l'inconnu devient null."""
    if cstl_compact.is_unknown(v):
        return None
    if isinstance(v, dict):
        return {k: naive(x) for k, x in v.items()}
    if isinstance(v, list):
        return [naive(x) for x in v]
    return v


def to_xml(data):
    import xml.etree.ElementTree as ET
    root = ET.Element("patients")
    for p in data["patients"]:
        e = ET.SubElement(root, "patient", {k: str(p[k]) for k in ("id", "age", "ward")})
        for exam in EXAMS:
            if exam not in p:
                continue
            v = p[exam]
            if v is None:
                ET.SubElement(e, exam, {"null": "true"})
            elif cstl_compact.is_unknown(v):
                ET.SubElement(e, exam, {"unknown": "true", **{k: str(x) for k, x in v["$unknown"].items()}})
            else:
                ET.SubElement(e, exam).text = str(v)
    ET.indent(root)
    return ET.tostring(root, encoding="unicode")


def to_csv(data):
    import csv
    import io
    cols = ["id", "age", "ward"] + [c for e in EXAMS for c in (e, f"{e}_sigma", f"{e}_pending")]
    buf = io.StringIO()
    w = csv.writer(buf, lineterminator="\n")
    w.writerow(cols)
    for p in data["patients"]:
        row = {"id": p["id"], "age": p["age"], "ward": p["ward"]}
        for exam in EXAMS:
            v = p.get(exam, "")
            if v is None:
                row[exam] = "null"
            elif cstl_compact.is_unknown(v):
                row[exam] = "?"
                row[f"{exam}_sigma"] = v["$unknown"].get("sigma", "")
                row[f"{exam}_pending"] = v["$unknown"].get("pending", "")
            else:
                row[exam] = v
        w.writerow([row.get(c, "") for c in cols])
    return buf.getvalue()


def render(fmt, data):
    if fmt == "json_indente":
        return json.dumps(data, ensure_ascii=False, indent=2)
    if fmt == "yaml":
        import yaml
        return yaml.safe_dump(data, sort_keys=False, allow_unicode=True)
    if fmt == "xml":
        return to_xml(data)
    if fmt == "csv":
        return to_csv(data)
    if fmt == "cstl_arbre":
        return cstl_compact.encode_tree(data)
    if fmt == "json_naif":
        return json.dumps(naive(data), ensure_ascii=False, separators=(",", ":"))
    if fmt == "json":
        return json.dumps(data, ensure_ascii=False, separators=(",", ":"))
    if fmt == "toon":
        return toon_lite.encode_nested(data)
    return cstl_compact.encode_tree_indented(data)


def ids(data, pred):
    return sorted((p["id"] for p in data["patients"] if pred(p)), key=lambda x: int(x[1:]))


def questions(data, seed):
    rnd = random.Random(seed)
    exam = rnd.choice(EXAMS)
    other = EXAMS[1 - EXAMS.index(exam)]
    unknown_ = lambda p, e: cstl_compact.is_unknown(p.get(e))  # noqa: E731
    threshold = 60 if exam == "renal" else 4.5
    return [
        (f"Quels patients ont l'examen {exam} INCONNU (pas encore mesure) ? (liste d'id)",
         ids(data, lambda p: unknown_(p, exam))),
        (f"Quels patients ont l'examen {other} connu SANS valeur (null) ? (liste d'id)",
         ids(data, lambda p: other in p and p[other] is None)),
        ("Quels patients attendent une mesure au prochain controle (pending=next_checkup), pour l'un ou l'autre "
         "examen ? (liste d'id)",
         ids(data, lambda p: any(unknown_(p, e) and p[e]["$unknown"].get("pending") == "next_checkup"
                                 for e in EXAMS))),
        (f"Quels patients ont une valeur {exam} connue strictement superieure a {threshold} ? (liste d'id)",
         ids(data, lambda p: isinstance(p.get(exam), (int, float)) and not isinstance(p.get(exam), bool)
             and p[exam] > threshold)),
    ]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tokens", action="store_true")
    ap.add_argument("--comprehension", metavar="MODELE")
    ap.add_argument("--resume", action="store_true", help="ne repose que les jeux en erreur d'API")
    args = ap.parse_args()
    datasets = [(n, s, gen(n, s)) for n in SIZES for s in SEEDS]
    assert all(cstl_compact.decode_tree_indented(render("cstl_indent", d)) == d for _, _, d in datasets)
    out_path = REPO / "scripts" / "unknown_benchmark.json"
    out = json.loads(out_path.read_text()) if out_path.exists() else {}

    if args.tokens:
        import gemini_tokens
        key = os.environ["GEMINI_API_KEY"]
        tot = {f: sum(gemini_tokens.count("gemini-3.5-flash", key, render(f, d)) for _, _, d in datasets)
               for f in FORMATS}
        print("Tokens Gemini (6 jeux) :")
        for f in sorted(FORMATS, key=tot.get):
            print(f"  {f:12} {tot[f]:>6}  ({100 * (tot[f] - tot['json']) / tot['json']:+.0f} % par rapport au json)")
        out["tokens"] = tot

    if args.comprehension:
        import team_task_benchmark as tb
        model = tb.make_brain(args.comprehension)

        def one(job):
            n, s, d, f = job
            qs = questions(d, s)
            lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(qs))
            prompt = (f"Voici des donnees echangees entre agents.\n{NOTES[f]}\n\nDonnees :\n{render(f, d)}\n\n"
                      f"Questions :\n{lines}\n\nReponds uniquement par {{\"r1\": ..., \"r2\": ..., \"r3\": ..., "
                      "\"r4\": ...}}.")
            try:
                raw = model.generate(prompt, max_tokens=4000)
            except RuntimeError as e:
                return {"size": n, "seed": s, "format": f, "ok": None, "error": str(e)[:200]}
            a, b = raw.find("{"), raw.rfind("}")
            try:
                ans = json.loads(raw[a:b + 1])
            except ValueError:
                ans = {}
            ok = [nb.same(ans.get(f"r{i + 1}"), t) for i, (_, t) in enumerate(qs)]
            print(f"    n={n:<3} s{s} {f:12} : {sum(ok)}/4  {['ok' if x else 'FAUX' for x in ok]}", flush=True)
            return {"size": n, "seed": s, "format": f, "ok": ok, "raw": raw[:600]}

        jobs = [(n, s, d, f) for n, s, d in datasets for f in FORMATS]
        old = out.get(f"comprehension_{args.comprehension}") if args.resume else None
        done = {(r["size"], r["seed"], r["format"]): r for r in old or [] if r.get("ok") is not None}
        rows = [done.get((n, s, f)) for n, s, _, f in jobs]
        todo = [i for i, r in enumerate(rows) if r is None]
        with ThreadPoolExecutor(max_workers=int(os.environ.get("BENCH_WORKERS", "3"))) as pool:
            for i, r in zip(todo, pool.map(one, [jobs[i] for i in todo])):
                rows[i] = r
        print(f"\n  Comprehension ({args.comprehension}) :")
        for f in FORMATS:
            r = [x for x in rows if x["format"] == f and x["ok"] is not None]
            per_q = [sum(x["ok"][i] for x in r) for i in range(4)]
            print(f"    {f:12} | {sum(per_q):>2}/{4 * len(r)} | par question : inconnu {per_q[0]}/{len(r)}, "
                  f"null {per_q[1]}/{len(r)}, pending {per_q[2]}/{len(r)}, seuil {per_q[3]}/{len(r)}")
        out[f"comprehension_{args.comprehension}"] = rows
    # Plusieurs modeles peuvent tourner en parallele : relire le fichier sous
    # verrou et n'y remplacer que les cles de cette execution.
    import fcntl
    with open(out_path.with_suffix(".lock"), "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        current = json.loads(out_path.read_text()) if out_path.exists() else {}
        mine = {k: v for k, v in out.items() if k == "tokens" and args.tokens
                or k == f"comprehension_{args.comprehension}"}
        current.update(mine)
        out_path.write_text(json.dumps(current, indent=2, ensure_ascii=False) + "\n")
    print(f"\nResultats bruts : {out_path.relative_to(REPO)}")


if __name__ == "__main__":
    main()
