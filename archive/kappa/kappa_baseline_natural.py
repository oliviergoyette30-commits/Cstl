#!/usr/bin/env python3
"""Groupe de controle (2026-10-06, demande explicite d'Olivier : "il
faudrait le comparer avec le texte naturel") -- meme corpus de 18 items,
meme modele, meme infra d'appel API que
kappa_generate_v2_deterministic_literals.py, mais SANS passer par CSTL du
tout : juste une paraphrase directe texte->texte par le LLM.

Pourquoi ce groupe de controle est necessaire : jusqu'ici, tout desaccord
de juge etait attribue soit a la structure CSTL, soit a la couche de
reconstruction en prose -- implicitement, on suppose que SANS CSTL, les 3
juges seraient unanimes. Rien ne le prouve. Un LLM qui paraphrase un texte
directement (sans round-trip CSTL) peut lui aussi introduire les memes
glissements de temps verbal, de synonymie, ou d'ambiguite de surface qu'on
a mesures et corriges cote CSTL (medium_005/edge_003 : temps verbal ;
complex_003 : synonymie verbale). Si le kappa/AC1/PABAK du groupe de
controle est du MEME ordre que celui du pipeline CSTL, la conclusion change
du tout au tout : le "bruit" mesure n'est pas un defaut du pipeline CSTL,
c'est le bruit de fond normal de toute paraphrase LLM, CSTL ou pas. Si au
contraire CSTL mesure NETTEMENT moins bien que la paraphrase directe, c'est
une preuve reelle d'une perte de fidelite imputable specifiquement au
round-trip CSTL.

Protocole deliberement identique a celui de
kappa_generate_v2_deterministic_literals.py pour rester comparable :
- Meme corpus (importe directement, pas duplique).
- Meme modele (ANTHROPIC_MODEL, meme defaut).
- Un seul appel LLM par item (paraphrase directe), au lieu de deux
  (encode_to_cstl + reconstruct_from_cstl) -- pas de masquage de
  litteraux ici, cette couche-la n'existe que pour proteger les valeurs
  pendant le transit CSTL ; sans CSTL il n'y a rien a proteger.
- Meme format de judge_prompt (TEXT A / TEXT B, PRESERVED/NOT_PRESERVED),
  a soumettre aux MEMES 3 juges externes (ChatGPT, Gemini, Mistral) pour
  que les deux kappas soient calcules sur le meme protocole de jugement.
- Les metriques (Fleiss, AC1, PABAK) se calculent ensuite avec
  compute_kappa_metrics.py sur les deux jeux de votes, cote a cote.

Usage : python3 kappa_baseline_natural.py
Sorties : kappa_v2_baseline_results.json, judge_prompt_baseline.txt
"""
import json
from pathlib import Path

from kappa_generate_v2_deterministic_literals import CORPUS, call_claude, MODEL

RESULTS_FILE = Path("kappa_v2_baseline_results.json")
JUDGE_PROMPT_FILE = Path("judge_prompt_baseline.txt")

# Memes 18 items que judge_prompt_v2.txt -- complex_002 et edge_001 sont
# dans CORPUS (20 items au total) mais volontairement exclus du lot juge
# depuis le debut de ce diagnostic (voir
# claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md). Les exclure ici aussi
# est necessaire pour que la comparaison CSTL vs paraphrase directe porte
# sur EXACTEMENT le meme ensemble d'items des deux cotes.
EXCLUDED = {"complex_002", "edge_001"}
ITEMS = {k: v for k, v in CORPUS.items() if k not in EXCLUDED}


def natural_paraphrase(original: str) -> str:
    prompt = f"""Paraphrase the following sentence (or short passage) in different words.

Preserve the exact same factual/logical meaning -- do not add any information that is not in the original, and do not drop or soften any detail (including tense, negation, exceptions, quantities, or who is doing what to whom). Change the wording/phrasing, not the content. Output ONLY the paraphrase, nothing else -- no preamble, no explanation.

TEXT:
{original}"""
    return call_claude(prompt, max_tokens=200)


def main():
    results = {}
    if RESULTS_FILE.exists():
        try:
            results = json.loads(RESULTS_FILE.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as exc:
            print(f"[AVERTISSEMENT] impossible de charger {RESULTS_FILE} existant ({exc}) -- redemarre a vide.")
            results = {}

    print(f"Modele: {MODEL}")
    print(f"Corpus: {len(ITEMS)} textes (groupe de controle, PAS de CSTL)\n")

    for i, (text_id, entry) in enumerate(ITEMS.items(), 1):
        original = entry["text"]
        print(f"[{i}/{len(CORPUS)}] {text_id} ({entry['diff']})")
        print("    paraphrase directe (sans CSTL)...")
        paraphrase = natural_paraphrase(original)
        results[text_id] = {
            "diff": entry["diff"],
            "original": original,
            "paraphrase": paraphrase,
        }

    RESULTS_FILE.write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")

    lines = []
    lines.append("You are evaluating whether meaning was preserved through a text transformation.")
    lines.append(f"For each of the {len(results)} items below, you are shown TEXT A (original) and TEXT B (a paraphrase).")
    lines.append("Judge ONLY whether TEXT B preserves the essential factual/logical meaning of TEXT A -- ignore style, wording, or phrasing differences.")
    lines.append("")
    lines.append("Answer with EXACTLY one line per item, in this format, nothing else:")
    lines.append("<item_id>: PRESERVED")
    lines.append("or")
    lines.append("<item_id>: NOT_PRESERVED")
    lines.append("")
    lines.append(f"Output all {len(results)} lines, in the order given, and nothing before or after them.")
    lines.append("")
    lines.append("=" * 60)
    lines.append("")
    for text_id, r in results.items():
        lines.append(f"[{text_id}]")
        lines.append(f"TEXT A: {r['original']}")
        lines.append(f"TEXT B: {r['paraphrase']}")
        lines.append("")

    JUDGE_PROMPT_FILE.write_text("\n".join(lines), encoding="utf-8")

    print(f"\nResultats: {RESULTS_FILE}")
    print(f"Prompt a coller dans les 3 chatbots (meme protocole que judge_prompt_v2.txt): {JUDGE_PROMPT_FILE}")


if __name__ == "__main__":
    main()
