#!/usr/bin/env python3
"""
cstl_approve.py (`cstl-approve`) -- l'approbateur HUMAIN (2026-11-25).

Une action listee dans CSTL_APPROVAL_ACTIONS n'obtient son jeton qu'avec
l'accord signe d'un approbateur (CSTL_APPROVERS). La personne lance :

    cstl-approve --agent human_alice list
    cstl-approve --agent human_alice approve <approval_id> --reason "verifie"
    cstl-approve --agent human_alice reject  <approval_id>

Sa cle est dans ~/.cstl/agent_<nom>.key ; elle s'enregistre au premier appel.
L'accord est signe, audite, et vaut pour UNE demande : meme agent, meme
action, meme montant.
"""
import argparse
import contextlib
import io
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="Accords humains pour les actions CSTL a risque.")
    ap.add_argument("--agent", default=os.environ.get("CSTL_AGENT"), help="nom de l'approbateur (CSTL_AGENT)")
    ap.add_argument("--host", default=os.environ.get("CSTL_HOST", "127.0.0.1"))
    ap.add_argument("--port", type=int, default=int(os.environ.get("CSTL_PORT", "5050")))
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list", help="demandes en attente")
    for name in ("approve", "reject"):
        c = sub.add_parser(name)
        c.add_argument("approval_id")
        c.add_argument("--reason")
    args = ap.parse_args(argv)
    if not args.agent:
        ap.error("--agent (ou CSTL_AGENT) requis")
    import cstl_llm_agent as m
    with contextlib.redirect_stdout(io.StringIO()):
        a = m.CstlAgent(args.agent, "none", host=args.host, port=args.port)
        registered = a.register()
    if not registered:
        print(f"enregistrement de {args.agent} refuse", file=sys.stderr)
        return 2
    try:
        if args.cmd == "list":
            rows = a.pending_approvals()
            if not rows:
                print("Aucune demande en attente.")
            for r in rows:
                when = datetime.fromtimestamp(int(r["requested_at"]), timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
                amount = f", montant {r['amount']}" if r.get("amount") else ""
                print(f"{r['approval_id']}  {r['agent']} veut {r['action']}{amount}  ({when})")
            return 0
        f = (a.approve if args.cmd == "approve" else a.reject)(args.approval_id, args.reason)
        print(f"{'accord donne' if args.cmd == 'approve' else 'refus enregistre'} : {f.get('agent')} / {f.get('action')}")
        return 0
    except m.ControlDenied as e:
        print(f"refuse : {e.reason}", file=sys.stderr)
        return 1


def cli():
    sys.exit(main())


if __name__ == "__main__":
    cli()
