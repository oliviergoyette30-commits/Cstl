#!/usr/bin/env python3
"""
scripts/llm_agent_eval.py -- un VRAI modele produit-il des relations CSTL
valides et fideles ? (2026-11-19)

Le banc d'essai scripts/benchmark_cstl_vs_jsonschema.py montre que le
serveur attrape les erreurs logiques (12/12) -- sur des messages ecrits A LA
MAIN. Ce script pose la question suivante : quand c'est un LLM qui ecrit les
relations, sont-elles (1) produites, (2) acceptees par le serveur, (3)
fideles aux nuances logiques de la phrase (negation, condition,
interdiction, croyance, exception) ?

Pour chaque phrase : CstlAgent.tell() -> le modele formule UNE relation ->
envoyee signee au VRAI serveur (lance par ce script, port 5050) -> on lit la
reponse. La fidelite est jugee de facon deterministe : l'operateur (ou la
modalite) produit doit appartenir a l'ensemble attendu, fixe AVANT execution
dans CORPUS. Aucun juge LLM.

LIMITES : une seule relation par phrase (une phrase riche en demanderait
plusieurs) ; l'ensemble "attendu" est un critere minimal (bon operateur), pas
une preuve de fidelite complete ; 12 phrases, donc un ordre de grandeur, pas
une statistique.

Usage :
  python3 scripts/llm_agent_eval.py --provider anthropic   # ANTHROPIC_API_KEY requis
  python3 scripts/llm_agent_eval.py --provider hermes      # Ollama local requis
  python3 scripts/llm_agent_eval.py --provider stub        # plomberie seule, sans modele
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "sdk" / "python"))
import cstl_llm_agent as agent_mod  # noqa: E402

# (id, phrase, operateurs acceptes, modalites acceptees ou None si sans objet)
CORPUS = [
    ("fact_1", "The capital of France is Paris.", {"STATE", "POSSESSES", "CO_LOCATES", "EQUALS"}, None),
    ("fact_2", "Einstein wrote the theory of relativity.", {"ARR.PRODUCE", "ARR.CREATE", "PERFORM", "STATE"}, None),
    ("fact_3", "Water boils at 100 degrees Celsius.", {"STATE", "TRANSFORM", "REACTS"}, None),
    ("know_1", "Alice knows Bob.", {"KNOWS"}, None),
    ("neg_1", "Alice does not believe that Bob left early.", {"DISBELIEVES", "DOUBTS"}, None),
    ("belief_1", "The manager assumes the budget will be approved.", {"ASSUMES", "BELIEVES"}, None),
    ("cond_1", "If Alice and Bob leave, the project fails.", {"ENTAILS"}, None),
    ("either_1", "The contract is either renewed or terminated.", {"EITHER_OR"}, None),
    ("forbid_1", "The NDA prohibits disclosure without prior written consent.", None, {"MUST_NOT", "FORBID"}),
    ("oblig_1", "Data controllers must implement security measures.", None, {"MUST", "REQUIRE"}),
    ("time_1", "The contract was signed before the delivery.", {"BEFORE", "AFTER"}, None),
    ("oppose_1", "The employees oppose the layoffs.", {"OPPOSES", "RESIST"}, None),
]

STUB_ANSWERS = {
    "fact_1": '{"type": "STATE", "subject": "france_capital", "object": "paris"}',
    "neg_1": '{"type": "DISBELIEVES", "subject": "alice", "object": "bob_left_early"}',
    "forbid_1": '{"type": "PERFORM", "subject": "party", "object": "disclosure", "modality": "MUST_NOT"}',
    "cond_1": '{"type": "TERMINATES_IF", "subject": "alice_bob_leave", "object": "project_fails"}',
}


def start_server():
    binary = REPO / "target" / "debug" / "cstl_parser"
    if not binary.exists():
        sys.exit("cargo build --bin cstl_parser d'abord")
    try:
        socket.create_connection(("127.0.0.1", 5050), timeout=0.5).close()
        sys.exit("le port 5050 est deja occupe : arreter l'autre serveur CSTL d'abord")
    except OSError:
        pass
    workdir = tempfile.mkdtemp(prefix="cstl_llm_eval_")
    proc = subprocess.Popen([str(binary)], cwd=workdir, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", 5050), timeout=0.5).close()
            return proc
        except OSError:
            time.sleep(0.25)
    proc.terminate()
    sys.exit("serveur injoignable")


def make_agent(provider):
    os.environ.setdefault("HOME", tempfile.mkdtemp())
    if provider == "stub":
        current = {"id": None}

        class Stub(agent_mod.LLMProvider):
            def generate(self, prompt, max_tokens=500):
                return STUB_ANSWERS.get(current["id"], "je ne sais pas")

        agent_mod.HermesAgentBrain.from_env = classmethod(lambda cls: Stub())
        return agent_mod.CstlAgent(f"eval_stub_{os.getpid()}", "hermes"), current
    return agent_mod.CstlAgent(f"eval_{provider}_{os.getpid()}", provider), {"id": None}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--provider", required=True, choices=["anthropic", "hermes", "gemini", "stub"])
    args = ap.parse_args()

    proc = start_server()
    results = []
    try:
        agent, current = make_agent(args.provider)
        if not agent.register():
            sys.exit("enregistrement refuse")
        for cid, text, ops, mods in CORPUS:
            current["id"] = cid
            rel, resp = agent.tell("evaluator", text)
            row = {"id": cid, "text": text, "relation": rel}
            if rel is None:
                row.update(produced=False, accepted=False, faithful=False, server="-")
            else:
                accepted = not (resp.is_error or resp.status == "error")
                in_catalog = rel["type"] in agent_mod.OFFICIAL_OPERATORS
                faithful = (rel["type"] in ops) if ops else (rel.get("modality") in mods)
                row.update(produced=True, accepted=accepted, in_catalog=in_catalog,
                           faithful=faithful, server=agent_mod.describe(resp))
            results.append(row)
    finally:
        proc.terminate()
        proc.wait(timeout=10)

    print(f"\nModele : {args.provider}\n")
    for r in results:
        rel = r["relation"]
        shown = f"{rel['type']}{'/' + rel['modality'] if rel and rel.get('modality') else ''}" if rel else "(rien)"
        if not r["produced"]:
            flags = "AUCUNE RELATION"
        else:
            flags = ("accepte " if r["accepted"] else "REJETE  ") + \
                    ("fidele" if r["faithful"] else "INFIDELE")
        print(f"  {r['id']:9} {flags:24} {shown:22} {r['text']}")
    n = len(results)
    produced = sum(r["produced"] for r in results)
    accepted = sum(r["accepted"] for r in results)
    faithful = sum(r["faithful"] for r in results)
    print(f"\n  Relation produite : {produced}/{n}   acceptee par le serveur : {accepted}/{n}"
          f"   operateur/modalite fidele : {faithful}/{n}")
    out = REPO / "scripts" / f"llm_agent_eval_{args.provider}.json"
    out.write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n")
    print(f"  Resultats bruts : {out.relative_to(REPO)}")


if __name__ == "__main__":
    main()
