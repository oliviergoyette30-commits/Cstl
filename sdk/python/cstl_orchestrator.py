#!/usr/bin/env python3
"""
cstl_orchestrator.py -- CLI minimal pour faire dialoguer deux parties
(humain ou LLM) via le vrai serveur CSTL, en utilisant cstl_client.py.

IMPORTANT (honnetete architecturale): le serveur CSTL (src/server/handler.rs)
ne route PAS lui-meme un payload d'un "agent A" vers un "agent B" -- verifie
dans le code: `AgentRegistry::route()` (agent_discovery.rs) n'est qu'un
test d'existence sur une capability, jamais un dispatch reel. Chaque appel
TCP est un aller-retour requete/reponse avec LE SERVEUR, pas avec un autre
agent. Ce script simule donc un dialogue A<->B en gerant le routage
COTE CLIENT: il envoie chaque message au serveur pour validation/audit/
verification factuelle, affiche la reponse, puis chaine le tour suivant
via le hash d'audit retourne (PARENT_HASH, meme logique que send2.sh /
send3.sh a la racine du depot). Il n'y a aucune simulation de
"converge/diverge/quarantine": ces concepts n'existent pas dans le code
serveur, seulement dans la documentation conceptuelle (README.md).

Usage:
    # Mode interactif -- boucle stdin, alterne agent_a / agent_b
    python3 sdk/python/cstl_orchestrator.py --relay

    # Mode scripte -- rejoue deux fichiers de messages, un message par ligne
    python3 sdk/python/cstl_orchestrator.py --script agentA.txt agentB.txt
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from cstl_client import CstlClient, CstlResponse


def _describe(resp: CstlResponse) -> str:
    bits = [f"status={resp.status}", f"purpose={resp.purpose}"]
    if resp.audit_hash:
        bits.append(f"audit_hash={resp.audit_hash[:16]}...")
    if resp.consistency:
        bits.append(
            "consistency("
            f"consistent={resp.consistency.get('consistent')}, "
            f"contradictions={resp.consistency.get('contradictions')}, "
            f"cycles={resp.consistency.get('cycles')}, "
            f"sigma={resp.consistency.get('sigma')})"
        )
    warnings = [r for r in resp.relations if r.get("block") == "SEMANTIC_WARNING"]
    if warnings:
        bits.append(f"semantic_warnings={len(warnings)}")
    return " | ".join(bits)


def _send_turn(client: CstlClient, sender: str, receiver: str, message: str,
               parent_hash: str | None) -> CstlResponse:
    extra_intent = {"parent_hash": parent_hash} if parent_hash else None
    resp = client.send_relation(
        sender=sender, receiver=receiver, purpose="dialogue_turn",
        relations=[{"type": "STATES", "subject": sender, "object": message}],
        encoder="Orchestrator", produced_by=sender,
        extra_intent=extra_intent,
    )
    return resp


def run_relay(client: CstlClient) -> None:
    print("Mode relay: tapez un message, il sera envoye comme agent_a puis "
          "agent_b tour a tour. Ctrl-D pour quitter.\n")
    turn_sender, turn_receiver = "agent_a", "agent_b"
    parent_hash: str | None = None
    for line in sys.stdin:
        message = line.rstrip("\n")
        if not message:
            continue
        resp = _send_turn(client, turn_sender, turn_receiver, message, parent_hash)
        print(f"[{turn_sender} -> {turn_receiver}] {_describe(resp)}")
        if resp.audit_hash:
            parent_hash = resp.audit_hash
        turn_sender, turn_receiver = turn_receiver, turn_sender


def run_script(client: CstlClient, path_a: str, path_b: str) -> None:
    lines_a = Path(path_a).read_text(encoding="utf-8").splitlines()
    lines_b = Path(path_b).read_text(encoding="utf-8").splitlines()
    parent_hash: str | None = None
    for i in range(max(len(lines_a), len(lines_b))):
        if i < len(lines_a) and lines_a[i].strip():
            resp = _send_turn(client, "agent_a", "agent_b", lines_a[i], parent_hash)
            print(f"[agent_a -> agent_b] {_describe(resp)}")
            if resp.audit_hash:
                parent_hash = resp.audit_hash
        if i < len(lines_b) and lines_b[i].strip():
            resp = _send_turn(client, "agent_b", "agent_a", lines_b[i], parent_hash)
            print(f"[agent_b -> agent_a] {_describe(resp)}")
            if resp.audit_hash:
                parent_hash = resp.audit_hash


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5050)
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--relay", action="store_true",
                       help="Boucle interactive stdin, alterne agent_a/agent_b.")
    mode.add_argument("--script", nargs=2, metavar=("AGENT_A_FILE", "AGENT_B_FILE"),
                       help="Rejoue une conversation scriptee depuis deux fichiers.")
    args = ap.parse_args()

    client = CstlClient(host=args.host, port=args.port, timeout=10.0)
    try:
        if args.relay:
            run_relay(client)
        else:
            run_script(client, args.script[0], args.script[1])
    finally:
        client.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
