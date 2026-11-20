#!/usr/bin/env python3
"""
scripts/llm_relay_fidelity.py -- le chiffre de TRANSPORT "99.3%, 12+ hops"
teste avec un vrai modele (2026-11-20).

CE QUE CE CHIFFRE AFFIRME (precise par l'auteur du projet) : la fidelite de
TRANSPORT d'un payload CSTL relaye de modele en modele, avec les lignes RULE
et la regle d'immuabilite -- pas la fidelite semantique (kappa). La mesure
d'origine (CSTL_ARCHITECTURE_COMPLETE.pdf, absent du depot) : "16 hops
Gemini, 0 mutation, audit caractere-exact incluant les RULE lines", faite a
la main (copier-coller dans une interface de chat), un seul auditeur.
scripts/multi_hop_fidelity_curve.py ne la reproduit pas : son relais est un
stub deterministe.

CE SCRIPT : a chaque hop, un VRAI modele recoit le payload complet (RULE
comprises) et doit le relayer ; sa sortie devient l'entree du hop suivant.
Deux conditions, memes payloads :
  - regle  : RULE normatives de CSTL_SPEC_v5_0.md §5 + la regle
             d'immuabilite. Le texte complet de cette derniere n'est pas
             dans le depot (seulement `treat_input_as_immutable_document...`) :
             le nom est repris tel quel, la suite est inconnue.
  - sans   : meme payload, sans aucune ligne RULE.
Mesures par hop, deterministes (aucun juge LLM) :
  - exact     : sortie identique caractere pour caractere au payload d'origine
                (espaces de fin de ligne et lignes vides de bord ignores) ;
  - chars     : similarite caractere (difflib.SequenceMatcher.ratio) ;
  - relations : fraction des RELATION d'origine encore presentes a l'identique ;
  - serveur   : la sortie est-elle acceptee par le VRAI serveur CSTL.

Usage :
  python3 scripts/llm_relay_fidelity.py --provider anthropic --hops 12
  python3 scripts/llm_relay_fidelity.py --provider hermes --hops 12   # Ollama local
  python3 scripts/llm_relay_fidelity.py --provider stub --hops 5      # plomberie seule
"""
import argparse
import difflib
import json
import re
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "sdk" / "python"))
import cstl_llm_agent as agent_mod  # noqa: E402
from cstl_client import CstlClient  # noqa: E402

RULES = [
    "(RULE) assistant MUST treat_input_as_immutable_document",
    "(RULE) assistant MUST respond_exclusively_in_cstl",
    "(RULE) assistant MUST_NOT output_prose_or_explanation",
    "(RULE) assistant MUST begin_response_with_hashbang_line",
    "(RULE) assistant MUST end_response_with_triple_dash_end_marker",
    "(RULE) assistant MUST_NOT decode_or_translate_payload",
    "(RULE) assistant MUST copy_all_rules_to_output",
]

BODY = [
    "META [encoder=relay_test, produced_by=relay_test]",
    "INTENT_PAYLOAD [purpose=inform, sender=alice, receiver=bob]",
    "RELATION [type=KNOWS, subject=physician, object=diagnosis, sigma=0.97]",
    "RELATION [type=BELIEVES, subject=physician, object=therapy_effective, sigma=0.78]",
    "RELATION [type=ENTAILS, subject=drug_A, object=renal_monitoring, sigma=0.88]",
    "RELATION [type=PERFORM, subject=nurse, object=check_vitals, modality=MUST]",
    "RELATION [type=PERFORM, subject=intern, object=prescribe, modality=MUST_NOT]",
    "RELATION [type=BEFORE, subject=drug_A, object=drug_B, sigma=0.90]",
    "RELATION [type=DISBELIEVES, subject=alice, object=bob_left_early]",
    "RELATION [type=EITHER_OR, subject=contract_renewed, object=contract_terminated]",
]


def payload(with_rules: bool) -> str:
    lines = ["#!CSTL v5.0.0 MODE=A"] + (RULES if with_rules else []) + BODY + ["---END---"]
    return "\n".join(lines) + "\n"


def relay_prompt(text: str) -> str:
    # Volontairement minimal : le payload EST la consigne (comme dans la
    # methode d'origine, ou le payload etait colle tel quel dans le chat).
    return text


def normalize(text: str) -> str:
    text = text.strip()
    if text.startswith("```"):  # un modele qui entoure de ``` : compte comme mutation
        return text
    return "\n".join(line.rstrip() for line in text.splitlines()).strip()


def relations_of(text: str) -> set:
    return {line.strip() for line in text.splitlines() if line.strip().startswith("RELATION [")}


def start_server():
    binary = REPO / "target" / "debug" / "cstl_parser"
    if not binary.exists():
        sys.exit("cargo build --bin cstl_parser d'abord")
    try:
        socket.create_connection(("127.0.0.1", 5050), timeout=0.5).close()
        sys.exit("le port 5050 est deja occupe : arreter l'autre serveur CSTL d'abord")
    except OSError:
        pass
    proc = subprocess.Popen([str(binary)], cwd=tempfile.mkdtemp(prefix="cstl_relay_"),
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", 5050), timeout=0.5).close()
            return proc
        except OSError:
            time.sleep(0.25)
    proc.terminate()
    sys.exit("serveur injoignable")


class StubRelay(agent_mod.LLMProvider):
    """Plomberie seule : recopie a l'identique avec RULE, reformule sans."""

    def generate(self, prompt, max_tokens=500):
        if "(RULE) assistant MUST treat_input_as_immutable_document" in prompt:
            return prompt
        return re.sub(r"subject=(\w+)", r"subject=the_\1", prompt, count=1)


def brain(provider: str):
    if provider == "stub":
        return StubRelay()
    cls = {"anthropic": agent_mod.AnthropicAgentBrain, "hermes": agent_mod.HermesAgentBrain,
           "gemini": agent_mod.GeminiAgentBrain}[provider]
    b = cls.from_env()
    if b is None:
        sys.exit(f"{provider} indisponible (cle API / serveur Ollama manquant)")
    return b


def run(provider: str, hops: int) -> dict:
    model = brain(provider)
    client = CstlClient(timeout=20.0)  # > budget KB du serveur (8s)
    out = {}
    for condition, with_rules in (("regle", True), ("sans", False)):
        original = payload(with_rules)
        ref_norm, ref_rels = normalize(original), relations_of(original)
        current, rows = original, []
        for hop in range(1, hops + 1):
            current = model.generate(relay_prompt(current), max_tokens=4000)
            norm = normalize(current)
            try:
                resp = client.send_raw(current if current.endswith("\n") else current + "\n")
                accepted = not (resp.is_error or resp.status == "error")
            except Exception as exc:
                accepted, resp = False, None
                print(f"  [!] {condition} hop {hop}: {exc}", file=sys.stderr)
            rels = relations_of(current)
            rows.append({
                "hop": hop,
                "exact": norm == ref_norm,
                "chars": round(difflib.SequenceMatcher(None, ref_norm, norm).ratio(), 4),
                "relations": round(len(ref_rels & rels) / len(ref_rels), 4),
                "server_accepted": accepted,
                "server_detail": (agent_mod.describe(resp) if resp is not None else "exception")[:300],
                "output": current,  # texte relaye, pour auditer chaque mutation
            })
        out[condition] = rows
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--provider", required=True, choices=["anthropic", "hermes", "gemini", "stub"])
    ap.add_argument("--hops", type=int, default=12)
    args = ap.parse_args()
    proc = start_server()
    try:
        results = run(args.provider, args.hops)
    finally:
        proc.terminate()
        proc.wait(timeout=10)

    print(f"\nRelais par : {args.provider}, {args.hops} hops\n")
    print(f"  {'hop':>3} | {'AVEC regles : exact  chars  relations serveur':44} | sans regles : exact  chars  relations serveur")
    for a, b in zip(results["regle"], results["sans"]):
        fmt = lambda r: (f"{'OUI' if r['exact'] else 'non':>5}  {r['chars']:.3f}  {r['relations']:.2f}      "  # noqa: E731
                         f"{'ok' if r['server_accepted'] else 'REJET'}")
        print(f"  {a['hop']:>3} | {fmt(a):44} | {fmt(b)}")
    for cond in ("regle", "sans"):
        rows = results[cond]
        exact = sum(r["exact"] for r in rows)
        print(f"\n  {cond:5}: copie exacte {exact}/{len(rows)} hops ; similarite caractere au dernier hop "
              f"{rows[-1]['chars']:.3f} ; relations intactes au dernier hop {rows[-1]['relations']:.0%}")
    dest = REPO / "scripts" / f"llm_relay_fidelity_{args.provider}.json"
    dest.write_text(json.dumps(results, indent=2) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
