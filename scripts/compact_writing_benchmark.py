#!/usr/bin/env python3
"""
scripts/compact_writing_benchmark.py -- un modele ECRIT-il le CSTL compact
aussi bien qu'il le lit ? (2026-11-21)

Pourquoi : `format_short_benchmark.py` a montre que le CSTL compact se LIT
aussi bien que le CSTL complet ou le JSON, pour 2,5 fois moins de tokens.
Mais un agent doit aussi ECRIRE ses messages. S'il se trompe plus souvent
en compact, le gain ne vaut rien.

LA TACHE : une politique decrite en francais (10, 30 ou 60 regles, roles,
faits vrais ou faux, conditions de montant et de fait). Le modele l'ecrit
dans un format. Un programme relit sa sortie et la compare, relation par
relation, a la politique d'origine.

TROIS FORMATS :
  cstl     lignes RELATION [type=..., subject=..., ...] (lues par
           `relay_tamper_benchmark.relations_in_text`)
  json     liste JSON de relations (memes champs)
  compact  CSTL compact (`sdk/python/cstl_compact.py`, lu par `decode`)

SCORE : politique exacte (toutes les relations, ni plus ni moins), nombre de
relations manquantes ou en trop, sorties illisibles, tokens de la sortie
(API count_tokens).

RESULTAT (2026-11-21, vraie API, 3 passes x 12 politiques, consignes avec exemple complet) :
             politiques exactes     pass^3          tokens ecrits
             Haiku     Opus         Haiku   Opus
  cstl       29/36     31/36        9/12    10/12   1 884
  json       33/36     30/36        10/12   9/12    2 140
  compact    21/36     36/36        4/12    12/12     781
- Opus ecrit le compact SANS AUCUNE erreur (36/36, meme a 60 regles), mieux
  que le CSTL complet et le JSON, ou il oublie des relations sur les
  grandes politiques.
- Haiku ecrit le compact MOINS bien (21/36) : 12 regles oubliees et 5
  inversions entre if: et unless: (« sauf si » ecrit if:). Il fait mieux en
  JSON (33/36).
- Le compact coute 2,4 fois moins de tokens a ecrire (781 contre 1 884 et
  2 140) : la sortie est la partie la plus chere d'un appel.
- Une premiere serie (`*_avant_exemples.json`) etait faussee par la
  consigne : elle ne montrait la syntaxe complete que pour les regles, et
  le guide compact notait la raison facultative entre crochets. Corrige.

Usage :
  python3 scripts/compact_writing_benchmark.py --model claude-haiku-5-5 --runs 3
  python3 scripts/compact_writing_benchmark.py --stub      # plomberie seule, AUCUNE conclusion
"""
import argparse
import json
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import format_comprehension_benchmark as fc  # noqa: E402
import relay_tamper_benchmark as rt  # noqa: E402
import cstl_compact  # noqa: E402
import toon_lite  # noqa: E402

FORMATS = ("cstl", "json", "compact", "toon", "compact_tableau", "compact_symboles")
SPEC_FIELDS = ("Une regle est PERFORM avec subject (un agent, ou role:<nom>), object (l'action), modality (MUST ou "
               "MUST_NOT) et, s'il y en a, amount_above (la regle ne vaut qu'au-dela de ce montant), if_fact "
               "(seulement si ce fait est vrai), unless_fact (sauf si ce fait est vrai). Un role est une relation "
               "POSSESSES (subject = l'agent, object = role:<nom>). Un fait est une relation STATE (subject = le fait, "
               "object = true ou false). N'ecris que les attributs qui existent.")
# Meme exemple (memes 4 elements) dans chaque format, pour qu'aucun format ne soit desavantage par la consigne.
# 2026-11-21 : la premiere serie ne montrait que la syntaxe des regles en CSTL complet ; Haiku ecrivait alors
# « STATE subject=... object=... » sans RELATION [...] (illisible), et recopiait les crochets « [\"raison\"] » du
# guide compact. Serie refaite avec ces exemples complets.
EXAMPLE = [{"type": "POSSESSES", "subject": "agent_a", "object": "role:finance"},
           {"type": "STATE", "subject": "audit_open", "object": "true"},
           {"type": "PERFORM", "subject": "agent_a", "object": "wipe_db", "modality": "MUST_NOT", "amount_above": "100",
            "unless_fact": "audit_open"},
           {"type": "PERFORM", "subject": "role:finance", "object": "log_transaction", "modality": "MUST"}]
SPEC = {
    "cstl": ("Ecris la politique en CSTL : une relation par ligne, RELATION [cle=valeur, ...]. " + SPEC_FIELDS +
             "\nExemple :\n" + rt.render_relations(EXAMPLE)),
    "json": ("Ecris la politique en JSON : une liste de relations, chacune un objet {cle: valeur}. " + SPEC_FIELDS +
             "\nExemple :\n" + json.dumps(EXAMPLE, ensure_ascii=False)),
    "compact": "Ecris la politique en CSTL compact.\n" + cstl_compact.GUIDE,
    "compact_symboles": ("Ecris la politique en CSTL compact sans mot-cle.\n" + cstl_compact.GUIDE_SYMBOLS),
    "compact_tableau": "Ecris la politique en CSTL compact en tableaux.\n" + cstl_compact.GUIDE_TABLE,
    "toon": ("Ecris la politique en TOON (Token-Oriented Object Notation) : un tableau roles[N]{agent,role}, un objet "
             "facts (fait: true|false) et un tableau rules[N]{subject,modality,action,amount_above,if_fact,unless_fact}, "
             "une ligne par element, null quand une condition n'existe pas, N = nombre exact de lignes. Une chaine qui "
             "contient « : » (comme role:<nom>) s'ecrit entre guillemets. " +
             SPEC_FIELDS.replace(" N'ecris que les attributs qui existent.", "") +
             "\nExemple :\n" + toon_lite.encode(toon_lite.policy_object(EXAMPLE))),
}


def describe(rels):
    """La politique en francais, une phrase par element (meme ordre)."""
    out = []
    for r in rels:
        if r["type"] == "POSSESSES":
            out.append(f"{r['subject']} a le role {r['object'][5:]}.")
        elif r["type"] == "STATE":
            out.append(f"Le fait {r['subject']} est {'vrai' if r['object'] == 'true' else 'faux'}.")
        else:
            who = f"Les agents qui ont le role {r['subject'][5:]}" if r["subject"].startswith("role:") else r["subject"]
            plural = r["subject"].startswith("role:")
            verb = ("doivent" if plural else "doit") if r["modality"] == "MUST" else ("ne doivent pas" if plural else "ne doit pas")
            cond = []
            if "amount_above" in r:
                cond.append(f"quand le montant depasse {r['amount_above']} euros")
            if "if_fact" in r:
                cond.append(f"seulement si le fait {r['if_fact']} est vrai")
            if "unless_fact" in r:
                cond.append(f"sauf si le fait {r['unless_fact']} est vrai")
            out.append(f"{who} {verb} lancer {r['object']}" + (" " + ", ".join(cond) if cond else "") + ".")
    return "\n".join(out)


def norm(r):
    keep = ("type", "subject", "object", "modality", "amount_above", "if_fact", "unless_fact")
    return tuple((k, str(r[k]).strip()) for k in keep if r.get(k) not in (None, ""))


def parse(fmt, text):
    if fmt == "compact_symboles":
        fmt = "compact_tableau"   # meme decodeur, qui lit aussi la forme sans mot-cle
    if fmt == "compact_tableau":
        body = "\n".join(ln for ln in text.strip("\n").splitlines() if not ln.startswith("```"))
        try:
            rels, problems = cstl_compact.decode_report(body)
        except cstl_compact.CompactError:
            return None
        PARSE_NOTES[text] = problems   # nombre [N] faux : signale (oubli repere), pas fatal pour le score
        return rels
    if fmt == "toon":
        body = "\n".join(ln for ln in text.strip().splitlines() if not ln.startswith("```"))
        try:
            obj, errors = toon_lite.decode(body)
            PARSE_NOTES[text] = errors
            return toon_lite.relations_of(obj)
        except (toon_lite.ToonError, KeyError, TypeError, AttributeError):
            return None
    if fmt == "cstl":
        return rt.relations_in_text(text)
    if fmt == "json":
        start, end = text.find("["), text.rfind("]")
        try:
            items = json.loads(text[start:end + 1])
        except ValueError:
            return None
        return [x for x in items if isinstance(x, dict)]
    body = text.strip()
    if body.startswith("```"):
        body = "\n".join(ln for ln in body.splitlines() if not ln.startswith("```"))
    try:
        return cstl_compact.decode(body)
    except cstl_compact.CompactError:
        return None


TOKENS = {}
PARSE_NOTES = {}   # longueurs [N] fausses en TOON (signalees, pas fatales)


def count(model, text):
    client = getattr(model, "client", None)
    if client is None or not text:
        return len(text) // 4
    if text not in TOKENS:
        TOKENS[text] = client.messages.count_tokens(model=model.model, messages=[{"role": "user", "content": text}]).input_tokens
    return TOKENS[text]


def run_case(model, size, seed, fmt, run):
    rels, _ = fc.generate(size, seed)
    prompt = (f"Tu es le responsable de la politique d'un systeme d'agents. Voici la politique :\n\n{describe(rels)}\n\n"
              f"{SPEC[fmt]}\n\nReponds uniquement par la politique, sans commentaire.")
    raw = model.generate(prompt, max_tokens=8000)
    got = parse(fmt, raw)
    want = Counter(norm(r) for r in rels)
    have = Counter(norm(r) for r in got) if got is not None else Counter()
    missing, extra = sum((want - have).values()), sum((have - want).values())
    return {"size": size, "seed": seed, "format": fmt, "run": run, "unreadable": got is None,
            "missing": missing, "extra": extra, "exact": got is not None and missing == 0 and extra == 0,
            "tokens_out": count(model, raw), "raw": raw[:30000], "count_errors": PARSE_NOTES.get(raw, [])}


class StubBrain:
    """Plomberie seule : recopie la verite (format compact) ou rien."""

    def generate(self, prompt, max_tokens=500):
        return ""


def summarize(rows, size=None):
    out = {}
    for f in FORMATS:
        r = [x for x in rows if x["format"] == f and (size is None or x["size"] == size)]
        groups = {}
        for x in r:
            groups.setdefault((x["size"], x["seed"]), []).append(x["exact"])
        out[f] = {"cases": len(r), "exact": sum(x["exact"] for x in r), "unreadable": sum(x["unreadable"] for x in r),
                  "missing": sum(x["missing"] for x in r), "extra": sum(x["extra"] for x in r),
                  "pass_k": sum(all(v) for v in groups.values()), "policies": len(groups),
                  "tokens_out": round(sum(x["tokens_out"] for x in r) / max(1, len(r)))}
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
    jobs = [(size, seed, f, run) for run in range(args.runs) for size in fc.SIZES for seed in fc.SEEDS for f in FORMATS]
    with ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
        rows = list(pool.map(lambda j: run_case(model, *j), jobs))
    label = "stub [plomberie seule, aucune conclusion]" if args.stub else args.model
    print(f"\nModele : {label}, {args.runs} passe(s) x {len(fc.SIZES) * len(fc.SEEDS)} politiques a ecrire")
    for size in (None, *fc.SIZES):
        print(f"\n  {'TOUTES' if size is None else f'{size} regles'} :")
        print(f"    {'format':8} | exactes | pass^{args.runs} | illisibles | manquantes | en trop | tokens ecrits")
        for f, s in summarize(rows, size).items():
            print(f"    {f:8} | {s['exact']:>3}/{s['cases']:<3} | {s['pass_k']:>3}/{s['policies']:<3} | {s['unreadable']:>10} | "
                  f"{s['missing']:>10} | {s['extra']:>7} | {s['tokens_out']:>13}")
    dest = REPO / "scripts" / f"compact_writing_benchmark_{'stub' if args.stub else args.model}.json"
    dest.write_text(json.dumps({"summary": summarize(rows), "rows": rows}, indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
