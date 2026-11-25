#!/usr/bin/env python3
"""
cstl_mcp_proxy.py -- CSTL devant n'importe quel serveur MCP (2026-11-25).

Le proxy se place entre un client MCP (Claude Desktop, Claude Code, ...) et
un serveur MCP existant, qu'il lance lui-meme. Tout passe tel quel, sauf les
appels d'outils (`tools/call`) : avant chacun, le proxy demande au serveur
CSTL une autorisation signee (`action_request`, action = nom de l'outil). Un
refus ne parvient jamais au serveur MCP : le client recoit une reponse
d'erreur d'outil (`isError: true`) avec la raison et la regle.

  - Ferme par defaut : serveur CSTL injoignable, reponse illisible, jeton
    invalide -> l'outil n'est pas appele.
  - Avec la cle publique du serveur (`--server-key` ou
    CSTL_SERVER_PUBLIC_KEY), chaque autorisation doit porter un jeton signe
    par cette cle, pour CET outil, CET agent et CE montant, jamais deja
    utilise (`CstlGuard`). Un faux serveur ne peut donc pas autoriser un
    outil. Sans cette cle, le proxy croit la reponse du serveur sur parole :
    a reserver a un serveur local de confiance.
  - Chaque demande, acceptee ou refusee, laisse sa trace dans l'audit signe.
  - `--amount-arg amount` : la valeur de cet argument de l'outil est envoyee
    comme montant (regles `amount_above` / `amount_at_most`).
  - `--prefix mcp_` : l'action demandee est `mcp_<outil>`.

Usage (configuration d'un client MCP) :

  {"mcpServers": {"fichiers": {"command": "python3",
     "args": ["/chemin/Cstl/sdk/python/cstl_mcp_proxy.py",
              "--server-key", "/chemin/cstl_adn.db.server_key.pub", "--",
              "npx", "-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
     "env": {"CSTL_AGENT": "agent_fichiers", "CSTL_PORT": "5050"}}}}

Limites : un appel d'outil attend la reponse du serveur CSTL avant de
passer (les autres messages ne sont pas bloques par le serveur MCP, mais
sont traites dans l'ordre). Le proxy controle QUEL outil est appele, avec
quel montant ; il ne comprend pas les autres arguments. Pour borner un
argument, ecrire une regle par outil distinct, ou un outil par usage.

Test en direct : test_mcp_proxy_live.py.
"""
import argparse
import json
import os
import subprocess
import sys
import threading
from pathlib import Path

SDK = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK))


class Gate:
    """Decide, pour chaque appel d'outil, s'il peut passer."""

    def __init__(self, agent_name, host, port, server_key=None, prefix="", amount_arg=None):
        import cstl_llm_agent as m
        self.m = m
        self.agent = m.CstlAgent(agent_name, "none", host=host, port=port)
        self.prefix, self.amount_arg = prefix, amount_arg
        self.guard = None
        if server_key:
            from cstl_guard import CstlGuard
            self.guard = CstlGuard(server_key)

    def decide(self, tool, arguments):
        """(autorise, raison, regle). Toute erreur refuse."""
        action = f"{self.prefix}{tool}"
        amount = None
        if self.amount_arg and isinstance(arguments, dict) and self.amount_arg in arguments:
            amount = arguments[self.amount_arg]
        try:
            if not self.agent.registered and not self.agent.register():
                return False, "agent_non_enregistre", None
            allowed, resp = self.agent.request_action(action, amount)
            f = resp.fields
            if not allowed:
                return False, f.get("reason") or resp.purpose or "refuse", f.get("rule")
            if self.guard is not None:
                from cstl_guard import AuthorizationToken, Unauthorized
                try:
                    self.guard.run(AuthorizationToken.from_response(resp), action, lambda: None,
                                   agent=self.agent.name, amount=amount)
                except Unauthorized as e:
                    return False, f"jeton_invalide:{e.reason}", None
            return True, f.get("reason"), f.get("rule")
        except Exception as e:  # noqa: BLE001 -- ferme par defaut
            return False, f"cstl_indisponible:{type(e).__name__}", None


def denial(mid, tool, reason, rule):
    text = f"Refuse par CSTL : outil {tool!r}, raison {reason}" + (f", regle {rule}" if rule else "")
    return {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": text}], "isError": True}}


def main(argv=None):
    ap = argparse.ArgumentParser(description="CSTL devant un serveur MCP")
    ap.add_argument("--server-key", default=os.environ.get("CSTL_SERVER_PUBLIC_KEY"),
                    help="cle publique du serveur CSTL (hex, ou chemin d'un fichier .pub)")
    ap.add_argument("--prefix", default="", help="prefixe des actions demandees")
    ap.add_argument("--amount-arg", default=None, help="argument d'outil qui porte le montant")
    ap.add_argument("command", nargs=argparse.REMAINDER, help="-- commande du serveur MCP")
    args = ap.parse_args(argv)
    cmd = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not cmd:
        raise SystemExit("commande du serveur MCP manquante (apres --)")
    name = os.environ.get("CSTL_AGENT")
    if not name:
        raise SystemExit("CSTL_AGENT manquant : nom de l'agent CSTL du proxy")
    key = args.server_key
    if key and Path(key).is_file():
        key = Path(key).read_text().strip()

    gate = Gate(name, os.environ.get("CSTL_HOST", "127.0.0.1"), int(os.environ.get("CSTL_PORT", "5050")),
                server_key=key, prefix=args.prefix, amount_arg=args.amount_arg)
    child = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    out_lock = threading.Lock()

    def emit(obj_or_line):
        line = obj_or_line if isinstance(obj_or_line, str) else json.dumps(obj_or_line, ensure_ascii=False)
        with out_lock:
            sys.stdout.write(line.rstrip("\n") + "\n")
            sys.stdout.flush()

    def pump():
        for line in child.stdout:
            if line.strip():
                emit(line)

    threading.Thread(target=pump, daemon=True).start()

    def forward(line):
        child.stdin.write(line.rstrip("\n") + "\n")
        child.stdin.flush()

    def screen(msg):
        """Le message s'il peut passer, sinon None (la reponse de refus est
        emise ici)."""
        if not isinstance(msg, dict) or msg.get("method") != "tools/call":
            return msg
        params = msg.get("params") or {}
        tool = params.get("name")
        if not isinstance(tool, str):
            if "id" in msg:
                emit({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32602, "message": "nom d'outil manquant"}})
            return None
        ok, reason, rule = gate.decide(tool, params.get("arguments"))
        if ok:
            return msg
        if "id" in msg:
            emit(denial(msg["id"], tool, reason, rule))
        return None

    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            forward(line)          # le serveur MCP repondra lui-meme par une erreur
            continue
        if isinstance(msg, list):  # lot JSON-RPC : seuls les elements autorises passent
            kept = [x for x in (screen(m) for m in msg) if x is not None]
            if kept:
                forward(json.dumps(kept, ensure_ascii=False))
            continue
        if screen(msg) is not None:
            forward(line)
    child.stdin.close()
    child.wait()


def cli():
    """Point d'entree (commande installee et script) : stdout est reserve au
    protocole, les messages du SDK (enregistrement...) partent sur stderr."""
    import builtins
    _print = builtins.print
    builtins.print = lambda *a, **k: _print(*a, **{**k, "file": sys.stderr})
    main()


if __name__ == "__main__":
    cli()
