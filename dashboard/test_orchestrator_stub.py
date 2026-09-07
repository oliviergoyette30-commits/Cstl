#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
dashboard/test_orchestrator_stub.py -- preuve que la MECANIQUE de
run_orchestrated_relay() (dashboard/server.py) fonctionne reellement :
boucle de N tours, enregistrement Ed25519 reel aupres du serveur, VRAIS
envois TCP au serveur CSTL, capture de trace (payload envoye + reponse
recue par tour) -- en injectant un "brain" factice deterministe a la place
d'un vrai agent LLM (Hermes/Gemini/Anthropic), exactement comme
DeterministicStubBackend est deja utilise ailleurs dans ce depot
(scripts/operator_agreement_study.py) pour isoler un mecanisme de la
disponibilite d'un vrai modele.

CE QUE CE TEST PROUVE : la mecanique (turns reellement executes, agent
reellement enregistre, chaque tour reellement envoye/recu par TCP au
serveur, trace reellement peuplee).

CE QUE CE TEST NE PROUVE PAS : la qualite d'un contenu genere par un vrai
modele -- ca reste a verifier par l'utilisateur, sur sa machine, avec un
vrai Hermes/Gemini/Anthropic disponible (voir sdk/python/cstl_llm_agent.py).

PREREQUIS : un vrai serveur CSTL (cargo run --release) qui tourne sur
CSTL_SERVER_HOST:CSTL_SERVER_PORT (defaut 127.0.0.1:5050). Ce test ne
demarre PAS le serveur lui-meme -- test d'integration, pas un test unitaire
isole.

Usage:
    cargo run --release &
    python3 dashboard/test_orchestrator_stub.py
"""

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import server as dashboard_server  # noqa: E402


class DeterministicStubBrain:
    """Brain factice deterministe -- meme interface (generate_relation,
    .model) que AnthropicAgentBrain/GeminiAgentBrain/HermesAgentBrain
    (sdk/python/cstl_llm_agent.py), duck-typing, aucune generation reelle.
    Retourne une relation fixe et previsible, differente a chaque tour pour
    verifier que la boucle avance reellement (pas juste qu'elle boucle sur
    le meme tour)."""

    model = "deterministic-stub-v1"

    def __init__(self):
        self.calls = []

    def generate_relation(self, topic, peer_message):
        n = len(self.calls) + 1
        self.calls.append((topic, peer_message))
        return {"type": "STATES", "subject": f"stub_turn_{n}", "object": topic}


def main() -> int:
    print(f"[test] Cible: {dashboard_server.CSTL_SERVER_HOST}:{dashboard_server.CSTL_SERVER_PORT}")

    # Cle Ed25519 dediee a ce test, dans un repertoire temporaire -- ne
    # touche jamais ~/.cstl/dashboard_orchestrator_ed25519.key (identite de
    # production du dashboard).
    with tempfile.TemporaryDirectory() as tmp:
        original_keyfile = dashboard_server.ORCHESTRATOR_KEYFILE
        dashboard_server.ORCHESTRATOR_KEYFILE = Path(tmp) / "test_orchestrator.key"
        try:
            stub = DeterministicStubBrain()
            result = dashboard_server.run_orchestrated_relay(turns=3, topic="test_mecanique_relais", brain=stub)
        finally:
            dashboard_server.ORCHESTRATOR_KEYFILE = original_keyfile

    print(json.dumps(result, indent=2, ensure_ascii=False))

    failures = []
    if not result.get("ok"):
        failures.append(f"run_orchestrated_relay a echoue: {result.get('error')}")
    else:
        if result.get("real_brain") is not False:
            failures.append("real_brain devrait etre False quand un brain est injecte")
        if result.get("turns_completed") != 3:
            failures.append(f"turns_completed attendu=3, recu={result.get('turns_completed')}")
        trace = result.get("trace", [])
        if not trace or trace[0].get("kind") != "agent_register":
            failures.append("le premier element de trace devrait etre l'enregistrement de l'agent")
        relay_turns = [t for t in trace if t.get("kind") == "relay_turn"]
        if len(relay_turns) != 3:
            failures.append(f"3 tours de relais attendus dans la trace, trouve {len(relay_turns)}")
        for i, t in enumerate(relay_turns, start=1):
            if not t.get("payload_sent") or "STATES" not in t["payload_sent"]:
                failures.append(f"tour {i}: payload_sent absent ou ne contient pas la relation envoyee")
            if not t.get("raw_response"):
                failures.append(f"tour {i}: raw_response absente -- le serveur n'a pas repondu ou la reponse n'a pas ete capturee")
            if t.get("brain") != "DeterministicStubBrain":
                failures.append(f"tour {i}: brain attendu=DeterministicStubBrain, recu={t.get('brain')}")
        if len(stub.calls) != 3:
            failures.append(f"le stub aurait du etre appele 3 fois, appele {len(stub.calls)} fois")

    print()
    if failures:
        print(f"ECHEC ({len(failures)}):")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("OK -- mecanique du relais (boucle de tours, enregistrement reel, "
          "envois TCP reels, capture de trace) verifiee avec un brain factice. "
          "La qualite d'un contenu genere par un vrai modele reste a verifier "
          "par l'utilisateur, avec un vrai Hermes/Gemini/Anthropic disponible.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
