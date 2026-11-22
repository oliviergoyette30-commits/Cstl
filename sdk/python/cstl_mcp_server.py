#!/usr/bin/env python3
"""
cstl_mcp_server.py -- serveur MCP (Model Context Protocol) pour CSTL
(2026-11-22).

Pourquoi : la valeur de CSTL est son serveur (politique signee gardee par le
serveur, decision par la couche 9, audit signe). Ce pont la rend utilisable
par n'importe quel agent qui parle MCP (Claude Desktop, Claude Code, d'autres
clients), sans qu'il apprenne CSTL : il appelle des outils, le SDK s'occupe
des cles, des signatures et du format.

Transport : stdio, un message JSON-RPC 2.0 par ligne (transport stdio de
MCP). Aucune dependance en dehors du SDK CSTL.

OUTILS :
  cstl_policy          politique effective de l'agent (format : phrases,
                       compact ou symboles)
  cstl_authorize       demander AVANT d'agir ; renvoie la decision et, si
                       l'action est permise, le jeton signe par le serveur
  cstl_send_compact    envoyer des relations ecrites en CSTL compact (avec ou
                       sans mot-cle, lignes ou tableaux)
  cstl_send_relations  envoyer des relations en JSON
  cstl_compact_guide   le guide du CSTL compact sans mot-cle

CONFIGURATION (variables d'environnement) :
  CSTL_AGENT   nom de l'agent (obligatoire) ; sa cle Ed25519 est creee au
               premier lancement dans ~/.cstl/agent_<nom>.key
  CSTL_HOST    hote du serveur CSTL (127.0.0.1 par defaut)
  CSTL_PORT    port TCP du serveur CSTL (5050 par defaut)

Exemple de configuration d'un client MCP :
  {"mcpServers": {"cstl": {"command": "python3",
     "args": ["/chemin/vers/Cstl/sdk/python/cstl_mcp_server.py"],
     "env": {"CSTL_AGENT": "agent_support", "CSTL_PORT": "5050"}}}}
"""
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

PROTOCOL_VERSION = "2025-06-18"

TOOLS = [
    {"name": "cstl_policy",
     "description": "Politique effective de cet agent, calculee par le serveur CSTL : obligations, permissions, "
                    "interdictions, conditions (montants, faits, roles), regles ignorees. A lire avant d'agir.",
     "inputSchema": {"type": "object", "properties": {
         "format": {"type": "string", "enum": ["phrases", "compact", "symboles"],
                    "description": "phrases (par defaut), compact, ou symboles (le plus court)"}}}},
    {"name": "cstl_authorize",
     "description": "Demander au serveur CSTL, AVANT d'agir, si une action est permise. Renvoie la decision, la "
                    "raison et, si elle est permise, un jeton signe par le serveur a presenter au systeme vise.",
     "inputSchema": {"type": "object", "required": ["action"], "properties": {
         "action": {"type": "string", "description": "nom de l'action, ex. refund_order"},
         "amount": {"type": "number", "description": "montant de l'action, s'il y en a un"}}}},
    {"name": "cstl_send_compact",
     "description": "Envoyer des relations ecrites en CSTL compact (avec ou sans mot-cle, voir cstl_compact_guide). "
                    "Elles sont relues sans rien deviner, signees et envoyees au serveur, qui les verifie.",
     "inputSchema": {"type": "object", "required": ["text"], "properties": {
         "text": {"type": "string"}, "receiver": {"type": "string", "description": "server par defaut"}}}},
    {"name": "cstl_send_relations",
     "description": "Envoyer des relations CSTL en JSON, ex. [{\"type\": \"PERFORM\", \"subject\": \"agent_a\", "
                    "\"object\": \"refund_order\", \"modality\": \"MUST_NOT\", \"amount_above\": \"100\"}].",
     "inputSchema": {"type": "object", "required": ["relations"], "properties": {
         "relations": {"type": "array", "items": {"type": "object"}}, "receiver": {"type": "string"}}}},
    {"name": "cstl_compact_guide",
     "description": "Guide du CSTL compact sans mot-cle, pour ecrire avec cstl_send_compact.",
     "inputSchema": {"type": "object", "properties": {}}},
]


class Bridge:
    def __init__(self):
        import cstl_llm_agent as m
        name = os.environ.get("CSTL_AGENT")
        if not name:
            raise SystemExit("CSTL_AGENT manquant : nom de l'agent CSTL")
        self.agent = m.CstlAgent(name, "none", host=os.environ.get("CSTL_HOST", "127.0.0.1"),
                                 port=int(os.environ.get("CSTL_PORT", "5050")))

    def ready(self):
        if not self.agent.registered and not self.agent.register():
            raise RuntimeError("enregistrement refuse par le serveur CSTL")

    def call(self, name, args):
        import cstl_compact
        if name == "cstl_compact_guide":
            return cstl_compact.GUIDE_SYMBOLS
        self.ready()
        if name == "cstl_policy":
            return self.agent.policy_text(args.get("format", "phrases"))
        if name == "cstl_authorize":
            allowed, resp = self.agent.request_action(str(args["action"]), args.get("amount"))
            f = resp.fields
            out = {"allowed": allowed, "decision": f.get("decision"), "reason": f.get("reason"), "rule": f.get("rule")}
            if allowed:
                out["token"] = {k: f.get(k) for k in ("request_id", "issued_at", "expires_at", "audit_hash", "amount",
                                                     "authorization") if f.get(k) is not None}
            return json.dumps(out, ensure_ascii=False)
        if name in ("cstl_send_compact", "cstl_send_relations"):
            receiver = args.get("receiver", "server")
            resp = (self.agent.send_compact(receiver, str(args["text"])) if name == "cstl_send_compact"
                    else self.agent.send_relations(receiver, list(args["relations"])))
            return json.dumps({"accepted": not resp.is_error, "purpose": resp.purpose,
                               "detail": resp.fields.get("detail") or resp.fields.get("reason")}, ensure_ascii=False)
        raise KeyError(name)


def handle(bridge, msg):
    """Reponse JSON-RPC a un message, ou None pour une notification."""
    method, mid, params = msg.get("method"), msg.get("id"), msg.get("params") or {}
    if mid is None:
        return None
    if method == "initialize":
        result = {"protocolVersion": params.get("protocolVersion", PROTOCOL_VERSION),
                  "capabilities": {"tools": {}}, "serverInfo": {"name": "cstl", "version": "0.1.0"}}
    elif method == "ping":
        result = {}
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        name = params.get("name")
        if name not in {t["name"] for t in TOOLS}:
            return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": f"outil inconnu : {name}"}}
        try:
            result = {"content": [{"type": "text", "text": bridge.call(name, params.get("arguments") or {})}],
                      "isError": False}
        except Exception as e:  # noqa: BLE001 -- l'erreur est rendue a l'agent, pas avalee
            result = {"content": [{"type": "text", "text": f"{type(e).__name__} : {e}"}], "isError": True}
    else:
        return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": f"methode inconnue : {method}"}}
    return {"jsonrpc": "2.0", "id": mid, "result": result}


def main():
    bridge = Bridge()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            reply = {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "JSON illisible"}}
        else:
            reply = handle(bridge, msg)
        if reply is not None:
            sys.stdout.write(json.dumps(reply, ensure_ascii=False) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    # stdout est reserve au protocole : les messages du SDK (enregistrement...)
    # partent sur stderr.
    import builtins
    _print = builtins.print
    builtins.print = lambda *a, **k: _print(*a, **{**k, "file": sys.stderr})
    main()
