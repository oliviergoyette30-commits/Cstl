#!/usr/bin/env python3
"""
scripts/make_standalone_try.py -- fabrique `scripts/cstl_essai_tout_en_un.py`,
un SEUL fichier Python, sans dependance (ni le depot, ni pip), pour essayer
l'arbre indente CSTL avec n'importe quel modele par copier-coller (2026-11-22).

Le fichier produit contient : les vrais decodeurs (`sdk/python/cstl_compact.py`
et `scripts/toon_lite.py`, inclus tels quels), les jeux de donnees et les
questions de `nested_benchmark.py`, et les consignes de
`nested_writing_benchmark.py`. Il est regenere a partir du depot : relancer
ce script apres une modification de ces fichiers.

Usage : python3 scripts/make_standalone_try.py
"""
import inspect
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
import nested_benchmark as nb  # noqa: E402
import nested_writing_benchmark as nw  # noqa: E402

OUT = REPO / "scripts" / "cstl_essai_tout_en_un.py"
FORMATS = ("cstl_indent", "cstl_indent_fin", "toon", "json")


def embedded(name, path):
    src = path.read_text()
    assert "'''" not in src and not src.endswith("\\")
    return f"{name} = _module({name!r}, r'''{src}''')\n"


def main():
    notes = {f: nb.NOTES[f] for f in FORMATS}
    parts = [
        '''#!/usr/bin/env python3
"""
cstl_essai_tout_en_un.py -- essayer l'arbre indente CSTL avec N'IMPORTE QUEL
modele (ChatGPT, Claude.ai, Gemini, Mistral, modele local), par copier-coller.
Un seul fichier, Python 3.8+, aucune installation.

FICHIER GENERE par scripts/make_standalone_try.py (depot CSTL) : ne pas
modifier a la main. Les decodeurs inclus sont les vrais (cstl_compact.py,
toon_lite.py) : rien n'est devine.

1. Ecrire toutes les consignes d'un coup dans un dossier :
     python3 cstl_essai_tout_en_un.py consignes essais/
   -> un fichier par test ; coller chacun dans une NOUVELLE conversation.
2. Enregistrer chaque reponse du modele dans le meme dossier, sous le meme
   nom avec .reponse.txt a la place de .txt
   (ex. ecriture_cstl_indent.reponse.txt).
3. Tout verifier d'un coup :
     python3 cstl_essai_tout_en_un.py verifier essais/

Tests :
  ecriture_<format>  le modele convertit un JSON imbrique (24 contrats) dans
                     le format ; verifie : relu ? identique ? (strict, puis
                     tolerant) ; % de valeurs justes
  lecture_<format>   3 questions sur les memes donnees ; bonnes reponses
Formats : cstl_indent (comptes en tete), cstl_indent_fin (comptes a la fin,
@end), toon, json (reference).
Option : --kind commandes|organisation|contrats --size 8|24 --seed 1|2
(les memes pour consignes et verifier).
"""
import argparse
import json
import random
import re
import sys
import types
import zlib
from pathlib import Path


def _module(name, src):
    mod = types.ModuleType(name)
    exec(compile(src, f"<{name}>", "exec"), mod.__dict__)
    return mod


''',
        embedded("cstl_compact", REPO / "sdk" / "python" / "cstl_compact.py"),
        embedded("toon_lite", REPO / "scripts" / "toon_lite.py"),
        f"\nKINDS = {nb.KINDS!r}\nNOTES = {notes!r}\n\n\n",
        inspect.getsource(nb.gen), "\n\n", inspect.getsource(nb.questions), "\n\n", inspect.getsource(nb.same), "\n\n",
        inspect.getsource(nw.leaves), "\n\n", inspect.getsource(nw.strip_fences), "\n\n",
        '''def render(fmt, data):
    if fmt == "json":
        return json.dumps(data, ensure_ascii=False, separators=(",", ":"))
    if fmt == "toon":
        return toon_lite.encode_nested(data)
    return cstl_compact.encode_tree_indented(data, counts="end" if fmt == "cstl_indent_fin" else "head")


def decode(fmt, text, lenient=False):
    if fmt == "json":
        return json.loads(text)
    if fmt == "toon":
        return toon_lite.decode_nested(text)
    return cstl_compact.decode_tree_indented(text, lenient=lenient)


def prompt_ecriture(fmt, data, kind):
    ex = gen(KINDS[(KINDS.index(kind) + 1) % len(KINDS)], 3, 99)
    return (f"Convertis le document JSON ci-dessous dans le format decrit.\\n{NOTES[fmt]}\\n\\n"
            f"Exemple (autres donnees).\\nJSON :\\n{json.dumps(ex, ensure_ascii=False)}\\n"
            f"Meme document dans le format :\\n{render(fmt, ex)}\\n\\n"
            f"Document a convertir :\\n{json.dumps(data, ensure_ascii=False)}\\n\\n"
            "Reponds uniquement par le document converti, sans explication, dans UN SEUL bloc de code (```), "
            "pour garder l'indentation.")


def prompt_lecture(fmt, data, kind, seed):
    lines = "\\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(questions(kind, data, seed)))
    return (f"Voici des donnees echangees entre agents.\\n{NOTES[fmt]}\\n\\nDonnees :\\n{render(fmt, data)}\\n\\n"
            f"Questions :\\n{lines}\\n\\nReponds uniquement par {{\\"r1\\": ..., \\"r2\\": ..., \\"r3\\": ...}}.")


def check_ecriture(fmt, data, raw):
    text = strip_fences(raw)
    out = []
    try:
        back, err = decode(fmt, text), None
    except Exception as e:  # noqa: BLE001 -- toute erreur du decodeur = illisible
        back, err = None, f"{type(e).__name__}: {e}"
    if err and fmt.startswith("cstl_indent"):
        warn = []
        try:
            soft = cstl_compact.decode_tree_indented(text, lenient=True, warnings=warn)
        except Exception:  # noqa: BLE001
            soft = None
        if soft == data:
            out.append(f"  STRICT : refuse ({err[:120]})")
            out.append("  TOLERANT : IDENTIQUE au JSON de depart" + "".join(f"\\n    avertissement : {w}" for w in warn))
            return out, 1.0, "tolerant"
    truth = leaves(data)
    got = leaves(back) if err is None else set()
    score = len(truth & got) / max(len(truth), len(got), 1)
    if err:
        out.append(f"  ILLISIBLE : {err[:200]}")
        verdict = "illisible"
    elif back == data:
        out.append("  IDENTIQUE au JSON de depart")
        verdict = "identique"
    else:
        verdict = "different"
        out.append("  LISIBLE mais DIFFERENT. Premieres differences :")
        out += [f"    manque ou faux : {p} = {v}" for p, v in sorted(truth - got)[:6]]
        out += [f"    en trop        : {p} = {v}" for p, v in sorted(got - truth)[:6]]
    return out, score, verdict


def check_lecture(data, kind, seed, raw):
    a, b = raw.find("{"), raw.rfind("}")
    try:
        ans = json.loads(raw[a:b + 1])
    except ValueError:
        return ["  ILLISIBLE : JSON {\\"r1\\": ..., \\"r2\\": ..., \\"r3\\": ...} attendu"], 0
    out, ok = [], 0
    for i, (q, truth) in enumerate(questions(kind, data, seed)):
        got = ans.get(f"r{i + 1}")
        good = same(got, truth)
        ok += good
        out.append(f"  {'OK  ' if good else 'FAUX'} r{i + 1} : modele {got!r} | attendu {truth!r}")
    return out, ok


TESTS = [f"ecriture_{f}" for f in ("cstl_indent", "cstl_indent_fin", "toon", "json")] + \\
        [f"lecture_{f}" for f in ("cstl_indent", "toon", "json")]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["consignes", "verifier"])
    ap.add_argument("dossier")
    ap.add_argument("--kind", default="contrats", choices=KINDS)
    ap.add_argument("--size", type=int, default=24)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    d = Path(args.dossier)
    data = gen(args.kind, args.size, args.seed)
    if args.action == "consignes":
        d.mkdir(parents=True, exist_ok=True)
        for t in TESTS:
            kind, fmt = t.split("_", 1)
            text = prompt_ecriture(fmt, data, args.kind) if kind == "ecriture" else prompt_lecture(fmt, data, args.kind, args.seed)
            (d / f"{t}.txt").write_text(text + "\\n", encoding="utf-8")
        print(f"{len(TESTS)} consignes ecrites dans {d}/ :")
        for t in TESTS:
            print(f"  {t}.txt  ->  reponse attendue dans {t}.reponse.txt")
        print("Une NOUVELLE conversation par consigne. Puis : python3 cstl_essai_tout_en_un.py verifier", d)
        return
    rows = []
    for t in TESTS:
        f = d / f"{t}.reponse.txt"
        if not f.exists():
            continue
        raw = f.read_text(encoding="utf-8")
        kind, fmt = t.split("_", 1)
        print(f"\\n{t}")
        if kind == "ecriture":
            lines, score, verdict = check_ecriture(fmt, data, raw)
            print("\\n".join(lines))
            print(f"  valeurs justes : {100 * score:.1f} %")
            rows.append((t, f"{verdict}, {100 * score:.1f} % des valeurs"))
        else:
            lines, ok = check_lecture(data, args.kind, args.seed, raw)
            print("\\n".join(lines))
            rows.append((t, f"{ok}/3"))
    if not rows:
        sys.exit(f"Aucune reponse trouvee dans {d}/ (fichiers <test>.reponse.txt).")
    print("\\nRESUME")
    for t, r in rows:
        print(f"  {t:26} {r}")


if __name__ == "__main__":
    main()
''',
    ]
    OUT.write_text("".join(parts))
    print(f"{OUT.relative_to(REPO)} : {len(OUT.read_text().splitlines())} lignes")


if __name__ == "__main__":
    main()
