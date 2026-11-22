#!/usr/bin/env python3
"""
scripts/try_other_model.py -- essayer l'arbre indente avec N'IMPORTE QUEL
modele, meme sans API : ChatGPT, Claude.ai, Gemini, Mistral, un modele local...
(2026-11-22)

Deux etapes, par copier-coller :
  1. le script ecrit une consigne (prompt) dans un fichier ; on la colle dans
     la fenetre de discussion du modele ;
  2. on colle la reponse du modele dans un fichier, et le script la verifie
     avec le VRAI decodeur (rien n'est devine).

Deux tests :
  ecriture   le modele convertit un document JSON imbrique dans le format ;
             verifie : relu sans erreur ? identique au JSON ? % de valeurs justes
  lecture    le modele repond a 3 questions sur des donnees dans le format ;
             verifie : nombre de bonnes reponses

Formats : cstl_indent (arbre indente), cstl_indent_fin (comptes a la fin, @end),
toon, json (reference).

Exemples :
  python3 scripts/try_other_model.py prompt ecriture cstl_indent > prompt.txt
  (coller prompt.txt dans le modele, enregistrer sa reponse dans reponse.txt)
  python3 scripts/try_other_model.py check ecriture cstl_indent reponse.txt

  python3 scripts/try_other_model.py prompt lecture cstl_indent --kind contrats --size 24
  python3 scripts/try_other_model.py check lecture cstl_indent reponse.txt --kind contrats --size 24

Options : --kind commandes|organisation|contrats  --size 8|24  --seed 1|2
(les memes jeux que nested_benchmark.py ; prompt et check doivent recevoir
les memes options).
"""
import argparse
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
import nested_benchmark as nb  # noqa: E402
import nested_writing_benchmark as nw  # noqa: E402


def lecture_prompt(fmt, data, kind, seed):
    qs = nb.questions(kind, data, seed)
    lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(qs))
    return (f"Voici des donnees echangees entre agents.\n{nb.NOTES[fmt]}\n\nDonnees :\n{nb.render(fmt, data)}\n\n"
            f"Questions :\n{lines}\n\nReponds uniquement par {{\"r1\": ..., \"r2\": ..., \"r3\": ...}}.")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["prompt", "check"])
    ap.add_argument("test", choices=["ecriture", "lecture"])
    ap.add_argument("format", choices=["cstl_indent", "cstl_indent_fin", "toon", "json"])
    ap.add_argument("answer_file", nargs="?", help="reponse du modele (check) ; - = entree standard")
    ap.add_argument("--kind", default="commandes", choices=nb.KINDS)
    ap.add_argument("--size", type=int, default=8)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    data = nb.gen(args.kind, args.size, args.seed)

    if args.action == "prompt":
        # Dans une fenetre de discussion, un bloc de code garde les retours a
        # la ligne et l'indentation au copier-coller ; le check enleve le bloc.
        print(nw.prompt(args.format, data, args.kind).replace(
                  "sans bloc de code.", "dans UN SEUL bloc de code (```), pour garder l'indentation.")
              if args.test == "ecriture" else lecture_prompt(args.format, data, args.kind, args.seed))
        return

    if not args.answer_file:
        ap.error("check : donner le fichier de la reponse du modele (ou - pour l'entree standard)")
    raw = sys.stdin.read() if args.answer_file == "-" else Path(args.answer_file).read_text(encoding="utf-8")

    if args.test == "lecture":
        qs = nb.questions(args.kind, data, args.seed)
        a, b = raw.find("{"), raw.rfind("}")
        try:
            ans = json.loads(raw[a:b + 1])
        except ValueError:
            sys.exit("Reponse illisible : JSON {\"r1\": ..., \"r2\": ..., \"r3\": ...} attendu.")
        ok = 0
        for i, (q, truth) in enumerate(qs):
            got = ans.get(f"r{i + 1}")
            good = nb.same(got, truth)
            ok += good
            print(f"  {'OK  ' if good else 'FAUX'} r{i + 1} : modele {got!r} | attendu {truth!r}\n       {q}")
        print(f"\nLecture ({args.format}) : {ok}/3 bonnes reponses")
        return

    text = nw.strip_fences(raw)
    try:
        back, err = nw.DECODE[args.format](text), None
    except Exception as e:  # noqa: BLE001
        back, err = None, f"{type(e).__name__}: {e}"
    truth = nw.leaves(data)
    got = nw.leaves(back) if err is None else set()
    score = len(truth & got) / max(len(truth), len(got), 1)
    if err and args.format.startswith("cstl_indent"):
        warn = []
        try:
            soft = nb.cstl_compact.decode_tree_indented(text, lenient=True, warnings=warn)
        except Exception as e:  # noqa: BLE001
            soft, warn = None, [f"{type(e).__name__}: {e}"]
        if soft == data:
            print(f"STRICT : refuse ({err}).\nTOLERANT : IDENTIQUE au JSON de depart."
                  + "".join(f"\n  avertissement : {w}" for w in warn))
            print(f"\nEcriture ({args.format}) : valeurs justes 100.0 % en mode tolerant")
            return
    if err:
        print(f"ILLISIBLE : le decodeur refuse la reponse.\n  {err}")
    elif back == data:
        print("IDENTIQUE : la reponse redonne exactement le JSON de depart.")
    else:
        print("LISIBLE mais DIFFERENTE du JSON de depart. Premieres differences :")
        for path, v in sorted(truth - got)[:8]:
            print(f"  manque ou faux : {path} = {v}")
        for path, v in sorted(got - truth)[:8]:
            print(f"  en trop        : {path} = {v}")
    print(f"\nEcriture ({args.format}) : valeurs justes {100 * score:.1f} % ({len(truth & got)}/{len(truth)})")


if __name__ == "__main__":
    main()
