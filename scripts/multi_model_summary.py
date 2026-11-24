#!/usr/bin/env python3
"""
scripts/multi_model_summary.py -- tableau des trois bancs, modele par modele
(2026-11-23).

Lit les resultats deja enregistres (aucun appel de modele) :
  lecture        scripts/nested_comprehension_<modele>.json  (36 questions par format)
  ecriture       scripts/nested_writing_<modele>.json         (12 documents par format)
  vide qui parle scripts/unknown_benchmark.json               (24 questions par format)
Les jeux en erreur d'API sont exclus du denominateur (x/y).

RESULTAT (2026-11-23, 10 modeles, 5 fournisseurs ; Gemma 4 31B : ecriture
lue dans nested_writing_gemma-4-31b-it_passe1/passe3.json) :
  totaux               json            toon            arbre indente
  lecture (360)        315             319             312
  ecriture (strict)    110/118 (93 %)  102/116 (88 %)  107/120 (89 %)  (arbre : avec /N)
  vide qui parle (240) 215             212             214             (json naif : 68)
- Modeles forts (GPT-5 mini, Claude Haiku et Sonnet 5.5, DeepSeek V4.1,
  Qwen 3.8, Gemma 4 26B et 31B) : arbre indente a egalite avec le JSON, a une
  question pres, en lecture comme en ecriture ; avec 35 a 39 % de tokens en
  moins.
- Modeles plus faibles (Gemini 3.5 Flash-Lite, Llama 4 Maverick, Mistral
  Medium 3.5) : erreurs dans tous les formats ; l'arbre indente y est un peu
  derriere le JSON en lecture et en ecriture, devant sur le vide qui parle
  pour Flash-Lite et Llama.
- Le JSON naif (inconnu -> null) echoue sur le vide chez TOUS les modeles.
- Cout OpenRouter : 5,37 $.

Usage : python3 scripts/multi_model_summary.py
        python3 scripts/multi_model_summary.py --export docs/BENCHMARK_CSTL.md
  (--export ecrit le rapport complet a partager, et la meme chose en JSON a
  cote : tableaux, protocole, et comment y ajouter un modele)
"""
import json
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
S = REPO / "scripts"


def score(rows, fmt):
    r = [x for x in rows if x.get("format") == fmt and x.get("ok") is not None]
    return sum(sum(x["ok"]) for x in r), sum(len(x["ok"]) for x in r)


def models():
    found = {p.name[len("nested_comprehension_"):-5] for p in S.glob("nested_comprehension_*.json")}
    found |= {p.name[len("nested_writing_"):-5] for p in S.glob("nested_writing_*.json") if "passe" not in p.name}
    return sorted(found)


def main():
    unknown = json.loads((S / "unknown_benchmark.json").read_text()) if (S / "unknown_benchmark.json").exists() else {}
    fmt_cell = lambda a, b: f"{a}/{b}" if b else "-"  # noqa: E731
    print("LECTURE (questions justes)            json      toon      cstl_indent")
    for m in models():
        p = S / f"nested_comprehension_{m}.json"
        if p.exists():
            rows = json.loads(p.read_text())["rows"]
            print(f"  {m:34} " + "  ".join(f"{fmt_cell(*score(rows, f)):>8}" for f in ("json", "toon", "cstl_indent")))
    print("\nECRITURE (documents identiques)       json      toon      cstl_indent_ligne (strict)")
    for m in models():
        p = S / f"nested_writing_{m}.json"
        if not p.exists():
            continue
        rows = json.loads(p.read_text())["rows"]
        cells = []
        for f in ("json", "toon", "cstl_indent_ligne"):
            r = [x for x in rows if x["format"] == f and x.get("ok") is not None]
            cells.append(fmt_cell(sum(x["exact"] for x in r), len(r)))
        print(f"  {m:34} " + "  ".join(f"{c:>8}" for c in cells))
    print("\nVIDE QUI PARLE (questions justes)    json_naif   json   toon   cstl_indent")
    for key, rows in sorted(unknown.items()):
        if not key.startswith("comprehension_"):
            continue
        m = key[len("comprehension_"):]
        print(f"  {m:34} " + "  ".join(f"{fmt_cell(*score(rows, f)):>7}" for f in ("json_naif", "json", "toon", "cstl_indent")))


# ------------------------------------------------------------------ export

LABELS = {"json": "JSON compact", "toon": "TOON", "cstl_indent": "CSTL arbre indente",
          "cstl_indent_ligne": "CSTL arbre indente + /N", "json_naif": "JSON naif (inconnu -> null)"}


def collect():
    """Tous les scores, modele par modele (lecture, ecriture, vide qui parle)."""
    unknown = json.loads((S / "unknown_benchmark.json").read_text()) if (S / "unknown_benchmark.json").exists() else {}
    data = {}
    for m in models():
        name = m.replace("__", "/")
        entry = data.setdefault(name, {})
        p = S / f"nested_comprehension_{m}.json"
        if p.exists():
            rows = json.loads(p.read_text())["rows"]
            entry["lecture"] = {f: dict(zip(("justes", "total"), score(rows, f)))
                                for f in ("json", "toon", "cstl_indent", "cstl_indent_ligne") if score(rows, f)[1]}
        p = S / f"nested_writing_{m}.json"
        if m == "gemma-4-31b-it":     # passes 1 et 3, enregistrees a part
            rows = (json.loads((S / "nested_writing_gemma-4-31b-it_passe1.json").read_text())["rows"]
                    + json.loads((S / "nested_writing_gemma-4-31b-it_passe3.json").read_text())["rows"])
        else:
            rows = json.loads(p.read_text())["rows"] if p.exists() else []
        ecr = {}
        for f in ("json", "toon", "cstl_indent_ligne"):
            r = [x for x in rows if x["format"] == f and x.get("ok") is not None]
            if r:
                ecr[f] = {"identiques_strict": sum(x["exact"] for x in r),
                          "identiques_tolerant": sum(x.get("exact_lenient", x["exact"]) for x in r), "total": len(r)}
        if ecr:
            entry["ecriture"] = ecr
        u = unknown.get(f"comprehension_{name}")
        if u:
            entry["vide_qui_parle"] = {f: dict(zip(("justes", "total"), score(u, f)))
                                       for f in ("json_naif", "json", "toon", "cstl_indent") if score(u, f)[1]}
    return data


def export(md_path):
    data = collect()
    tokens = json.loads((S / "nested_benchmark_tokens.json").read_text())
    utok = json.loads((S / "unknown_benchmark.json").read_text()).get("tokens", {})
    out = {"date": "2026-11-23", "tokens_gemini": {"imbrique_12_jeux": tokens, "vide_qui_parle_6_jeux": utok},
           "modeles": data}
    Path(md_path).with_suffix(".json").write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n")

    def cell(d, f, a="justes", b="total"):
        return f"{d[f][a]}/{d[f][b]}" if f in d else "-"

    L = []
    w = L.append
    w("# Banc d'essai CSTL : resultats complets et comment participer\n")
    w("Genere par `scripts/multi_model_summary.py --export` a partir des resultats bruts du depot "
      "(`scripts/nested_*.json`, `scripts/unknown_benchmark.json`). Aucun chiffre n'est ecrit a la main.\n")
    w("## Ce qui est mesure\n")
    w("Trois tests, memes donnees et memes consignes pour chaque format et chaque modele :\n")
    w("1. **Lecture** : 12 jeux de donnees imbriquees (commandes, organisation, contrats ; 8 et 24 elements ; "
      "2 graines), 3 questions par jeu, soit 36 questions par format. La reponse est le dernier objet "
      "`{\"r1\": ..., \"r2\": ..., \"r3\": ...}` du texte.")
    w("2. **Ecriture** : le modele convertit les 12 documents JSON dans le format, a partir d'une description et "
      "d'un exemple d'une autre famille de donnees. Sa sortie est relue par le vrai decodeur ; on compte les "
      "documents identiques au JSON de depart (strict, puis tolerant pour l'arbre indente).")
    w("3. **Vide qui parle** : 6 jeux de dossiers patients ou chaque examen est une valeur, null (connu, sans "
      "valeur), inconnu (avec son incertitude) ou absent ; 4 questions par jeu, soit 24 par format. Le « JSON "
      "naif » ecrit l'inconnu null, comme on le fait d'habitude.\n")
    w("Les jeux en erreur d'API sont exclus du total (x/y), jamais comptes faux.\n")
    w("## Taille (tokens du tokenizer Gemini)\n")
    w("| Format | Donnees imbriquees (12 jeux) | Vide qui parle (6 jeux) |\n|---|---|---|")
    for f, lab in (("json", "JSON compact"), ("toon", "TOON"), ("cstl_indent", "CSTL arbre indente"),
                   ("cstl_indent_ligne", "CSTL arbre indente + /N")):
        t = tokens.get(f)
        w(f"| {lab} | {t} ({100 * (t - tokens['json']) / tokens['json']:+.0f} %) | {utok.get(f, '-')} |")
    w("")
    w("## 1. Lecture (questions justes)\n")
    w("| Modele | JSON | TOON | Arbre indente | Arbre indente + /N |\n|---|---|---|---|---|")
    for m, d in data.items():
        if "lecture" in d:
            l = d["lecture"]
            w(f"| {m} | {cell(l, 'json')} | {cell(l, 'toon')} | {cell(l, 'cstl_indent')} | {cell(l, 'cstl_indent_ligne')} |")
    w("\n## 2. Ecriture (documents identiques au JSON de depart)\n")
    w("| Modele | JSON | TOON | Arbre indente + /N (strict) | Arbre indente + /N (tolerant) |\n|---|---|---|---|---|")
    for m, d in data.items():
        if "ecriture" in d:
            e = d["ecriture"]
            w(f"| {m} | {cell(e, 'json', 'identiques_strict')} | {cell(e, 'toon', 'identiques_strict')} | "
              f"{cell(e, 'cstl_indent_ligne', 'identiques_strict')} | {cell(e, 'cstl_indent_ligne', 'identiques_tolerant')} |")
    w("\nLe mode tolerant (`decode_tree_indented(texte, lenient=True)`) est celui que la documentation recommande "
      "pour un texte ecrit par un modele : un compte faux devient un avertissement, des colonnes finales peuvent "
      "manquer, un champ facultatif peut etre ecrit `nom=valeur` meme s'il est en colonne. Rien n'est devine : "
      "le document relu doit etre identique. JSON et TOON n'ont pas de mode tolerant.\n")
    w("## 3. Vide qui parle (questions justes)\n")
    w("| Modele | JSON naif | JSON | TOON | Arbre indente |\n|---|---|---|---|---|")
    for m, d in data.items():
        if "vide_qui_parle" in d:
            v = d["vide_qui_parle"]
            w(f"| {m} | {cell(v, 'json_naif')} | {cell(v, 'json')} | {cell(v, 'toon')} | {cell(v, 'cstl_indent')} |")
    tot = {}
    for d in data.values():
        for test, key in (("lecture", "justes"), ("vide_qui_parle", "justes")):
            for f, v in d.get(test, {}).items():
                a, b = tot.get((test, f), (0, 0))
                tot[(test, f)] = (a + v[key], b + v["total"])
        for f, v in d.get("ecriture", {}).items():
            for k in ("identiques_strict", "identiques_tolerant"):
                a, b = tot.get((k, f), (0, 0))
                tot[(k, f)] = (a + v[k], b + v["total"])
    w("\n## Totaux\n")
    w("| Test | JSON | TOON | Arbre indente | Arbre indente + /N |\n|---|---|---|---|---|")
    pct = lambda t: f"{t[0]}/{t[1]} ({100 * t[0] / t[1]:.1f} %)" if t and t[1] else "-"  # noqa: E731
    w("| Lecture | " + " | ".join(pct(tot.get(("lecture", f))) for f in ("json", "toon", "cstl_indent", "cstl_indent_ligne")) + " |")
    w("| Ecriture stricte | " + " | ".join(pct(tot.get(("identiques_strict", f))) for f in ("json", "toon")) + " | - | "
      + pct(tot.get(("identiques_strict", "cstl_indent_ligne"))) + " |")
    w("| Ecriture tolerante | - | - | - | " + pct(tot.get(("identiques_tolerant", "cstl_indent_ligne"))) + " |")
    w("| Vide qui parle | " + " | ".join(pct(tot.get(("vide_qui_parle", f))) for f in ("json", "toon", "cstl_indent")) + " | - |")
    w(f"\nJSON naif sur le vide qui parle : {pct(tot.get(('vide_qui_parle', 'json_naif')))}.\n")
    w("## Participer : ajouter un modele\n")
    w("**Avec une cle d'API** (Gemini : `gemini-*`, `gemma-*` ; OpenRouter : `fournisseur/modele`, par ex. "
      "`openai/gpt-5-mini`) :\n")
    w("```bash\nM=openai/gpt-5-mini        # le modele a tester\n"
      "python3 scripts/nested_benchmark.py --comprehension $M --formats json,toon,cstl_indent,cstl_indent_ligne --resume\n"
      "python3 scripts/nested_writing_benchmark.py $M --formats json,toon,cstl_indent_ligne --resume\n"
      "python3 scripts/unknown_benchmark.py --comprehension $M --resume\n"
      "python3 scripts/multi_model_summary.py --export docs/BENCHMARK_CSTL.md\n```\n")
    w("Cles : `GEMINI_API_KEY`, ou `OPENROUTER_API_KEY`. `--resume` ne repose que ce qui manque ou a echoue "
      "sur une erreur d'API. Les resultats bruts (toutes les reponses) vont dans `scripts/` ; proposer une PR "
      "avec ces fichiers.\n")
    w("**Sans code** (fenetre de discussion, n'importe quel modele) : `scripts/cstl_essai_tout_en_un.py` est un "
      "seul fichier Python sans dependance.\n")
    w("```bash\npython3 cstl_essai_tout_en_un.py consignes essais/   # 7 consignes a coller, une conversation chacune\n"
      "# enregistrer chaque premiere reponse dans essais/<test>.reponse.txt\n"
      "python3 cstl_essai_tout_en_un.py verifier essais/\n```\n")
    w("Regles pour un resultat valable : une NOUVELLE conversation par consigne, la PREMIERE reponse telle quelle "
      "(sans demander de correction), et le modele ne doit pas voir le script ni le verificateur.\n")
    w("## Limites\n")
    w("- Donnees generees (commandes, organisation, contrats, dossiers patients), pas encore de donnees reelles.")
    w("- Une passe par question ; des ecarts de 1 a 3 points sont dans le bruit.")
    w("- Les modeles ont vu enormement de JSON a l'entrainement et aucun CSTL : l'arbre indente part avec ce handicap.")
    Path(md_path).write_text("\n".join(L) + "\n")
    print(f"Ecrit : {md_path} et {Path(md_path).with_suffix('.json')}")


if __name__ == "__main__":
    import sys
    if len(sys.argv) > 2 and sys.argv[1] == "--export":
        export(sys.argv[2])
    else:
        main()
