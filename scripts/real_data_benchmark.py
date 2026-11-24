#!/usr/bin/env python3
"""
scripts/real_data_benchmark.py -- les bancs d'essai sur des donnees REELLES
(2026-11-24), et la taille mesuree par les tokenizers de plusieurs fournisseurs.

Pourquoi : critique de Gemini (2026-11-23) -- donnees synthetiques, un seul
tokenizer, une seule passe. Ce banc repond aux trois :
  - jeux : 12 reponses d'API publiques reelles (scripts/real_data/, voir
    real_data_build.py) : PyPI, npm, crates.io, Wikidata ;
  - questions GENERIQUES, tirees du document par un programme (aucune ecrite
    a la main pour un format) : recherche d'une valeur, longueur d'une liste,
    nombre d'elements qui ont un champ facultatif ;
  - --runs N : plusieurs passes (ecart-type) ;
  - --tokens-providers : tokens d'entree comptes par CHAQUE fournisseur
    (usage.prompt_tokens d'OpenRouter, moins le cout fixe d'une requete vide).

Formats : json, toon, cstl_indent, cstl_indent_ligne. TOON est exclu d'un
document quand toon_lite ne le relit pas a l'identique (2 documents).

Usage :
  python3 scripts/real_data_benchmark.py --comprehension openai/gpt-5-mini --runs 3
  python3 scripts/real_data_benchmark.py --writing openai/gpt-5-mini
  python3 scripts/real_data_benchmark.py --tokens-providers openai/gpt-5-mini,anthropic/claude-haiku-5.5
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
import nested_writing_benchmark as nw  # noqa: E402
import toon_lite  # noqa: E402

DATA = REPO / "scripts" / "real_data"
OUT = REPO / "scripts" / "real_data_results.json"
FORMATS = ("json", "toon", "cstl_indent", "cstl_indent_ligne")
NOTES = {
    "json": "Format : JSON.",
    "toon": nb.NOTES["toon"],
    "cstl_indent": nb.NOTES["cstl_indent"] + (" Un objet ou une liste dans une valeur s'ecrit en ligne : "
                                              "{cle:valeur,...} ou [a,b]. « @@ » porte les champs simples de la "
                                              "racine. Si plusieurs listes sont au meme niveau, la ligne commence "
                                              "par « nom> »."),
    "cstl_indent_ligne": nb.NOTES["cstl_indent_ligne"] + (" Un objet ou une liste dans une valeur s'ecrit en ligne "
                                                          ": {cle:valeur,...} ou [a,b]. « @@ » porte les champs "
                                                          "simples de la racine. Si plusieurs listes sont au meme "
                                                          "niveau, la ligne commence par « nom> »."),
}


def documents():
    return {p.stem: json.loads(p.read_text()) for p in sorted(DATA.glob("*.json"))}


def render(fmt, d):
    if fmt == "json":
        return json.dumps(d, ensure_ascii=False, separators=(",", ":"))
    if fmt == "toon":
        return toon_lite.encode_nested(d)
    return cstl_compact.encode_tree_indented(d, counts="line" if fmt == "cstl_indent_ligne" else "head")


def decode(fmt, text, lenient=False):
    if fmt == "json":
        return json.loads(text)
    if fmt == "toon":
        return toon_lite.decode_nested(text)
    return cstl_compact.decode_tree_indented(text, lenient=lenient)


def toon_ok(d):
    try:
        return toon_lite.decode_nested(toon_lite.encode_nested(d)) == d
    except Exception:  # noqa: BLE001
        return False


def formats_for(d):
    return [f for f in FORMATS if f != "toon" or toon_ok(d)]


# ------------------------------------------------------------- questions

def lists_of_dicts(v, path=()):
    """(chemin, liste) de toutes les listes d'objets du document."""
    if isinstance(v, dict):
        for k, x in v.items():
            if isinstance(x, list) and len(x) >= 2 and all(isinstance(i, dict) for i in x):
                yield path + (k,), x
            yield from lists_of_dicts(x, path + (k,))
    elif isinstance(v, list):
        for x in v:
            yield from lists_of_dicts(x, path)


def all_lists(v, path=()):
    if isinstance(v, dict):
        for k, x in v.items():
            if isinstance(x, list) and len(x) >= 2:
                yield path + (k,), x
            yield from all_lists(x, path + (k,))


def scalar(x):
    return x is None or isinstance(x, (str, int, float, bool)) and len(str(x)) <= 60


def where(path):
    return " > ".join(f"« {p} »" for p in path)


def questions(d, seed):
    """3 questions tirees du document, verite calculee par programme."""
    rnd = random.Random(seed)
    found = list(lists_of_dicts(d))
    paths = [p for p, _ in found]
    # Seulement les listes dont le chemin est UNIQUE dans le document : sinon
    # la question serait ambigue (ex. une liste « qualifiers » sous chaque
    # declaration Wikidata).
    lod = [(p, L) for p, L in found if paths.count(p) == 1]
    qs = []
    # 1. recherche : l'element dont <cle> = v, valeur de <champ>
    cands = []
    for p, L in lod:
        for key in L[0]:
            vals = [o.get(key) for o in L]
            if all(scalar(v) and v is not None for v in vals) and len(set(map(str, vals))) == len(vals):
                cands.append((p, L, key))
    if cands:
        p, L, key = rnd.choice(cands)
        o = rnd.choice(L)
        fields = [k for k, v in o.items() if k != key and scalar(v)]
        if fields:
            f = rnd.choice(fields)
            truth = o[f]
            qs.append((f"Dans la liste {where(p)}, pour l'element dont {key} = {o[key]}, quelle est la valeur de "
                       f"{f} ? (la valeur seule ; null si elle est nulle)", "null" if truth is None else truth))
    # 2. longueur d'une liste
    al_all = list(all_lists(d))
    al = [(p, L) for p, L in al_all if [q for q, _ in al_all].count(p) == 1]
    if al:
        p, L = rnd.choice(al)
        qs.append((f"Combien d'elements contient la liste {where(p)} ? (un nombre)", len(L)))
    # 3. champ facultatif present dans certains elements seulement
    cands = []
    for p, L in lod:
        keys = {k for o in L for k in o}
        for k in keys:
            n = sum(k in o for o in L)
            if 0 < n < len(L):
                cands.append((p, k, n))
        for k in L[0]:
            n = sum(k in o and o[k] is None for o in L)
            if 0 < n < len(L):
                cands.append((p, k, n, "null"))
    if cands:
        c = rnd.choice(cands)
        if len(c) == 3:
            qs.append((f"Dans la liste {where(c[0])}, combien d'elements ont un champ « {c[1]} » ? (un nombre)", c[2]))
        else:
            qs.append((f"Dans la liste {where(c[0])}, combien d'elements ont « {c[1]} » egal a null ? (un nombre)",
                       c[2]))
    return qs[:3]


# ------------------------------------------------------------- bancs

def comprehension(model, runs, workers):
    import team_task_benchmark as tb
    brain = tb.make_brain(model)
    docs = documents()

    def one(job):
        name, fmt, run = job
        d = docs[name]
        qs = questions(d, sum(map(ord, name)))
        lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(qs))
        keys = ", ".join(f"\"r{i + 1}\": ..." for i in range(len(qs)))
        prompt = (f"Voici des donnees echangees entre agents.\n{NOTES[fmt]}\n\nDonnees :\n{render(fmt, d)}\n\n"
                  f"Questions :\n{lines}\n\nReponds uniquement par {{{keys}}}.")
        base = {"doc": name, "format": fmt, "run": run}
        try:
            raw = brain.generate(prompt, max_tokens=4000)
        except RuntimeError as e:
            return {**base, "ok": None, "error": str(e)[:200]}
        ans, found = nb.extract_answer(raw)
        ok = [nb.same(ans.get(f"r{i + 1}"), t) for i, (_, t) in enumerate(qs)]
        print(f"    {name:14} {fmt:18} passe {run} : {sum(ok)}/{len(ok)}", flush=True)
        return {**base, "ok": ok, "parsed": found, "raw": raw[-2000:]}

    jobs = [(n, f, r) for r in range(runs) for n, d in docs.items() for f in formats_for(d)]
    return run_jobs(f"comprehension_{model}", jobs, one, workers,
                    key=lambda r: (r["doc"], r["format"], r["run"]), jkey=lambda j: j)


def writing(model, workers):
    import team_task_benchmark as tb
    brain = tb.make_brain(model)
    docs = documents()
    names = list(docs)

    def one(job):
        name, fmt, run = job
        d = docs[name]
        ex_name = names[(names.index(name) + 1) % len(names)]
        ex = docs[ex_name]
        prompt = (f"Convertis le document JSON ci-dessous dans le format decrit.\n{NOTES[fmt]}\n\n"
                  f"Exemple (autres donnees).\nJSON :\n{json.dumps(ex, ensure_ascii=False)}\n"
                  f"Meme document dans le format :\n{render(fmt, ex)}\n\n"
                  f"Document a convertir :\n{json.dumps(d, ensure_ascii=False)}\n\n"
                  "Reponds uniquement par le document converti, sans explication, sans bloc de code.")
        base = {"doc": name, "format": fmt, "run": run}
        try:
            raw = brain.generate(prompt, max_tokens=8000)
        except RuntimeError as e:
            return {**base, "ok": None, "error": str(e)[:200]}
        text = nw.strip_fences(raw)
        try:
            exact = decode(fmt, text) == d
        except Exception:  # noqa: BLE001
            exact = False
        try:
            soft = decode(fmt, text, lenient=True) == d if fmt.startswith("cstl") else exact
        except Exception:  # noqa: BLE001
            soft = False
        print(f"    {name:14} {fmt:18} : {'identique' if exact else 'tolerant: identique' if soft else 'ECHEC'}",
              flush=True)
        return {**base, "ok": True, "exact": exact, "exact_lenient": soft, "raw": text[-3000:]}

    jobs = [(n, f, 0) for n, d in docs.items() for f in formats_for(d) if f != "cstl_indent"]
    return run_jobs(f"writing_{model}", jobs, one, workers,
                    key=lambda r: (r["doc"], r["format"], r["run"]), jkey=lambda j: j)


def run_jobs(section, jobs, one, workers, key, jkey):
    import fcntl
    store = json.loads(OUT.read_text()) if OUT.exists() else {}
    done = {key(r): r for r in store.get(section, []) if r.get("ok") is not None}
    rows = [done.get(jkey(j)) for j in jobs]
    todo = [i for i, r in enumerate(rows) if r is None]
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for i, r in zip(todo, pool.map(one, [jobs[i] for i in todo])):
            rows[i] = r
    with open(OUT.with_suffix(".lock"), "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        store = json.loads(OUT.read_text()) if OUT.exists() else {}
        store[section] = rows
        OUT.write_text(json.dumps(store, indent=1, ensure_ascii=False) + "\n")
    return rows


def provider_tokens(models):
    """Tokens d'entree de chaque format, comptes par le tokenizer de chaque
    fournisseur (usage.prompt_tokens d'OpenRouter, max_tokens=1), moins le cout
    d'une requete au contenu vide. Jeux reels ET jeux imbriques synthetiques."""
    import urllib.request
    docs = documents()
    synth = {f"synth_{k}_{n}_{s}": nb.gen(k, n, s) for k in nb.KINDS for n in nb.SIZES for s in nb.SEEDS}

    def count(model, text):
        body = json.dumps({"model": model, "max_tokens": 1, "messages": [{"role": "user", "content": text}]}).encode()
        for attempt in range(6):
            req = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions", data=body, headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {os.environ.get('OPENROUTER_API_KEY', 'proxy-injected')}"})
            try:
                with urllib.request.urlopen(req, timeout=120) as r:
                    return json.load(r)["usage"]["prompt_tokens"]
            except Exception:  # noqa: BLE001
                import time
                time.sleep(5 * (attempt + 1))
        return None

    result = {}
    for model in models:
        base = count(model, ".")
        totals = {}
        for group, ds in (("reel", docs), ("synthetique", synth)):
            for f in FORMATS:
                usable = [d for d in ds.values() if f != "toon" or toon_ok(d)]
                with ThreadPoolExecutor(max_workers=6) as pool:
                    counts = list(pool.map(lambda d: count(model, render(f, d)), usable))
                totals[f"{group}_{f}"] = sum(c - base + 1 for c in counts if c is not None)
                totals[f"{group}_{f}_docs"] = len(usable)
            # comparaison juste : seulement les documents que TOON relit (meme base pour tous)
            common = [d for d in ds.values() if toon_ok(d)]
            with ThreadPoolExecutor(max_workers=6) as pool:
                for f in ("json", "cstl_indent"):
                    counts = list(pool.map(lambda d: count(model, render(f, d)), common))
                    totals[f"{group}_{f}_commun"] = sum(c - base + 1 for c in counts if c is not None)
        result[model] = totals
        print(model, json.dumps(totals), flush=True)
    store = json.loads(OUT.read_text()) if OUT.exists() else {}
    store.setdefault("tokens_par_fournisseur", {}).update(result)
    OUT.write_text(json.dumps(store, indent=1, ensure_ascii=False) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--comprehension", metavar="MODELE")
    ap.add_argument("--writing", metavar="MODELE")
    ap.add_argument("--tokens-providers", metavar="M1,M2")
    ap.add_argument("--runs", type=int, default=1)
    ap.add_argument("--show-questions", action="store_true")
    args = ap.parse_args()
    workers = int(os.environ.get("BENCH_WORKERS", "3"))
    docs = documents()
    for n, d in docs.items():
        for f in formats_for(d):
            assert decode(f, render(f, d)) == d, (n, f)
    if args.show_questions:
        for n, d in docs.items():
            for q, t in questions(d, sum(map(ord, n))):
                print(f"{n:14} {q}  -> {t!r}")
    if args.tokens_providers:
        provider_tokens([m for m in args.tokens_providers.split(",") if m])
    if args.comprehension:
        rows = comprehension(args.comprehension, args.runs, workers)
        for f in FORMATS:
            r = [x for x in rows if x and x["format"] == f and x.get("ok") is not None]
            if r:
                print(f"  {f:18} {sum(sum(x['ok']) for x in r)}/{sum(len(x['ok']) for x in r)}")
    if args.writing:
        rows = writing(args.writing, workers)
        for f in FORMATS:
            r = [x for x in rows if x and x["format"] == f and x.get("ok") is not None]
            if r:
                print(f"  {f:18} strict {sum(x['exact'] for x in r)}/{len(r)}  tolerant "
                      f"{sum(x['exact_lenient'] for x in r)}/{len(r)}")


if __name__ == "__main__":
    main()
