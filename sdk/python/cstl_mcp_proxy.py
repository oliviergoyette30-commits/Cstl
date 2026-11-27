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

  - `--args-schema schema.json` (2026-11-27) : borne aussi les AUTRES
    arguments, avec les types de cstl-gate (int, num, bool, word, text,
    {"enum": [...]}, {"regex": "..."}). Par outil :
      {"write_file": {"path": {"regex": "/srv/out/[A-Za-z0-9_.-]+"},
                      "$required": ["path"], "$only": true},
       "$default": "deny"}
    Un argument decrit doit avoir le bon type ; `$required` les rend
    obligatoires ; `$only: true` refuse tout argument non decrit ;
    `"$default": "deny"` refuse tout outil absent du schema. Un appel non
    conforme est refuse (`argument_refuse`) AVANT la demande au serveur
    CSTL : il n'est jamais execute.

Limites : un appel d'outil attend la reponse du serveur CSTL avant de
passer (les autres messages ne sont pas bloques par le serveur MCP, mais
sont traites dans l'ordre). Sans `--args-schema`, le proxy controle QUEL
outil est appele, avec quel montant, pas les autres arguments. Le schema
ne regarde que les arguments de premier niveau ; un objet imbrique se
borne avec un `regex` sur sa forme texte, ou pas du tout.

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

    def __init__(self, agent_name, host, port, server_key=None, prefix="", amount_arg=None, args_schema=None):
        import cstl_llm_agent as m
        self.m = m
        self.schema = load_args_schema(args_schema)
        self.agent = m.CstlAgent(agent_name, "none", host=host, port=port)
        self.prefix, self.amount_arg = prefix, amount_arg
        self.guard = None
        if server_key:
            from cstl_guard import CstlGuard
            self.guard = CstlGuard(server_key)

    def decide(self, tool, arguments):
        """(autorise, raison, regle). Toute erreur refuse."""
        problem = check_arguments(self.schema, tool, arguments)
        if problem:
            return False, f"argument_refuse:{problem}", None
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


def load_args_schema(schema):
    """None, un dict, ou le chemin d'un fichier JSON. Un schema mal ecrit
    arrete le proxy au demarrage, pas au premier appel."""
    if schema is None:
        return None
    if not isinstance(schema, dict):
        schema = json.loads(Path(schema).read_text())
    from cstl_gate import check_param, BadRequest
    if schema.get("$default", "allow") not in ("allow", "deny"):
        raise SystemExit("args-schema : $default vaut allow ou deny")
    for tool, spec in schema.items():
        if tool.startswith("$"):
            continue
        if not isinstance(spec, dict):
            raise SystemExit(f"args-schema : {tool} doit etre un objet")
        for name, kind in spec.items():
            if name in ("$required",) and not (isinstance(kind, list) and all(isinstance(x, str) for x in kind)):
                raise SystemExit(f"args-schema : {tool}.$required doit etre une liste de noms")
            if name == "$only" and not isinstance(kind, bool):
                raise SystemExit(f"args-schema : {tool}.$only doit etre true ou false")
            if not name.startswith("$"):
                try:                       # type inconnu -> refuse des maintenant
                    if isinstance(kind, dict) and "regex" in kind:
                        __import__("re").compile(kind["regex"])
                    check_param(name, kind, "")
                except BadRequest as e:
                    if "type inconnu" in str(e):
                        raise SystemExit(f"args-schema : {tool}.{e}") from None
                except Exception:          # noqa: BLE001 -- regex invalide, etc.
                    raise SystemExit(f"args-schema : {tool}.{name} mal ecrit") from None
    return schema


def check_arguments(schema, tool, arguments):
    """Texte du probleme, ou None si l'appel est conforme."""
    if schema is None:
        return None
    spec = schema.get(tool)
    if spec is None:
        return "outil_hors_schema" if schema.get("$default") == "deny" else None
    if arguments is None:
        arguments = {}
    if not isinstance(arguments, dict):
        return "arguments_non_objet"
    from cstl_gate import check_param, BadRequest
    missing = [a for a in spec.get("$required", []) if a not in arguments]
    if missing:
        return f"manquant:{','.join(missing)}"
    described = {k for k in spec if not k.startswith("$")}
    if spec.get("$only"):
        extra = sorted(set(arguments) - described)
        if extra:
            return f"en_trop:{','.join(extra)}"
    for name in sorted(described & set(arguments)):
        try:
            check_param(name, spec[name], arguments[name])
        except BadRequest:
            return f"invalide:{name}"
    return None


def denial(mid, tool, reason, rule):
    text = f"Refuse par CSTL : outil {tool!r}, raison {reason}" + (f", regle {rule}" if rule else "")
    return {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": text}], "isError": True}}


def main(argv=None):
    ap = argparse.ArgumentParser(description="CSTL devant un serveur MCP")
    ap.add_argument("--server-key", default=os.environ.get("CSTL_SERVER_PUBLIC_KEY"),
                    help="cle publique du serveur CSTL (hex, ou chemin d'un fichier .pub)")
    ap.add_argument("--prefix", default="", help="prefixe des actions demandees")
    ap.add_argument("--amount-arg", default=None, help="argument d'outil qui porte le montant")
    ap.add_argument("--args-schema", default=None, help="fichier JSON qui borne les arguments de chaque outil")
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
                server_key=key, prefix=args.prefix, amount_arg=args.amount_arg,
                args_schema=args.args_schema)
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
