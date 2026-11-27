#!/usr/bin/env python3
"""
cstl_mcp_remote.py -- le serveur MCP de CSTL, DISTANT (2026-11-27) : pour les
connecteurs personnalises de claude.ai (web, mobile), et tout client MCP qui
parle le transport HTTP (Claude Code : `claude mcp add --transport http`).

Memes outils que le serveur local (`cstl_mcp_server.py`, stdio) :
cstl_policy, cstl_authorize, cstl_send_compact, cstl_send_relations,
cstl_compact_guide. Le serveur agit au nom d'UN agent CSTL (CSTL_AGENT) :
toutes les sessions qui s'y connectent sont cet agent.

Transport : MCP « Streamable HTTP » (specification 2025-06-18), un seul
point d'entree, en JSON (pas de flux SSE : aucun outil n'en a besoin).
  POST   recoit un message JSON-RPC ; reponse JSON, ou 202 pour une
         notification ; `initialize` ouvre une session (Mcp-Session-Id)
  GET    405 (pas de flux initie par le serveur)
  DELETE ferme la session

Acces -- un JETON SECRET obligatoire (CSTL_MCP_TOKEN, ou --token) :
  - dans le chemin : https://votre-domaine/mcp/<jeton>   (claude.ai : collez
    cette adresse comme URL du connecteur ; claude.ai ne permet pas d'ajouter
    un en-tete)
  - ou dans l'en-tete Authorization: Bearer <jeton>, sur /mcp
Sans jeton valide : 401, et rien n'est execute. Comparaison en temps
constant. `cstl-mcp-remote --new-token` en tire un au hasard (256 bits).
Une requete navigateur d'une autre origine que claude.ai / claude.com
(en-tete Origin) est refusee (403) : protection contre le rebinding DNS.

HTTPS : claude.ai n'appelle qu'une adresse publique en HTTPS. Soit
--tls-cert / --tls-key ici, soit (recommande) un mandataire devant :
Caddy (`reverse_proxy 127.0.0.1:8765`) ou un tunnel (Cloudflare Tunnel).
Le serveur CSTL lui-meme reste sur 127.0.0.1 : seul ce pont est expose.

  cstl-mcp-remote --new-token                      # affiche un jeton
  CSTL_AGENT=agent_claude CSTL_MCP_TOKEN=... cstl-mcp-remote --listen 127.0.0.1:8765

Limites : pas d'OAuth (le jeton dans l'URL donne l'acces a qui la connait :
traitez-la comme un mot de passe, et changez-la si elle fuit) ; une seule
identite CSTL par serveur. Test en direct : test_mcp_remote_live.py.
"""
import argparse
import hmac
import json
import os
import secrets
import ssl
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cstl_mcp_server as local  # noqa: E402  (memes outils, meme logique)

SUPPORTED_VERSIONS = ("2025-06-18", "2025-03-26")
DEFAULT_ORIGINS = ("https://claude.ai", "https://claude.com")
MAX_BODY = 1 << 20
SESSION_TTL = 24 * 3600
MAX_SESSIONS = 1000


class Remote:
    def __init__(self, token, origins=DEFAULT_ORIGINS):
        if not token or len(token) < 32:
            raise SystemExit("jeton requis (CSTL_MCP_TOKEN ou --token), 32 caracteres au moins : "
                             "cstl-mcp-remote --new-token")
        self.token = token.encode()
        self.origins = set(origins)
        self.bridge = local.Bridge()
        self.lock = threading.Lock()                     # un seul agent CSTL : appels en serie
        self.sessions = {}

    def authorized(self, path, header):
        """Le jeton du chemin (/mcp/<jeton>) ou de l'en-tete Bearer."""
        parts = path.split("?")[0].rstrip("/").split("/")
        given = None
        if len(parts) == 3 and parts[1] == "mcp":
            given = parts[2]
        elif len(parts) == 2 and parts[1] == "mcp" and header and header.startswith("Bearer "):
            given = header[7:].strip()
        return given is not None and hmac.compare_digest(given.encode(), self.token)

    def new_session(self, version):
        now = time.time()
        with self.lock:
            for sid in [s for s, (_, t) in self.sessions.items() if now - t > SESSION_TTL]:
                del self.sessions[sid]
            if len(self.sessions) >= MAX_SESSIONS:
                oldest = min(self.sessions, key=lambda s: self.sessions[s][1])
                del self.sessions[oldest]
            sid = uuid.uuid4().hex
            self.sessions[sid] = (version, now)
            return sid

    def session(self, sid):
        with self.lock:
            s = self.sessions.get(sid)
            if s:
                self.sessions[sid] = (s[0], time.time())
            return s

    def end(self, sid):
        with self.lock:
            return self.sessions.pop(sid, None) is not None

    def answer(self, msg):
        if msg.get("method") == "initialize":
            asked = (msg.get("params") or {}).get("protocolVersion")
            version = asked if asked in SUPPORTED_VERSIONS else SUPPORTED_VERSIONS[0]
            reply = local.handle(self.bridge, msg)
            reply["result"]["protocolVersion"] = version
            reply["result"]["instructions"] = (
                "Serveur de controle CSTL. Avant toute action qui a un effet (rembourser, supprimer, envoyer, "
                "payer...), appelez cstl_authorize et n'agissez que si allowed=true. cstl_policy donne vos "
                "obligations et interdictions.")
            return reply, version
        with self.lock:
            return local.handle(self.bridge, msg), None


def make_handler(remote):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"
        server_version = "cstl-mcp-remote"

        def log_message(self, fmt, *args):       # jamais le chemin : il contient le jeton
            sys.stderr.write(f"[cstl-mcp-remote] {self.command} {args[1] if len(args) > 1 else ''}\n")

        def _send(self, status, obj=None, headers=None):
            data = b"" if obj is None else json.dumps(obj, ensure_ascii=False).encode()
            self.send_response(status)
            if obj is not None:
                self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            for k, v in (headers or {}).items():
                self.send_header(k, v)
            self.end_headers()
            if data:
                self.wfile.write(data)

        def _err(self, status, code, message, mid=None):
            self._send(status, {"jsonrpc": "2.0", "id": mid, "error": {"code": code, "message": message}})

        def _guard(self):
            """Origine, puis jeton. True si la requete peut continuer."""
            origin = self.headers.get("Origin")
            if origin and origin not in remote.origins:
                self._err(403, -32000, "origine refusee")
                return False
            if not remote.authorized(self.path, self.headers.get("Authorization")):
                self._send(401, {"error": "jeton absent ou invalide"}, {"WWW-Authenticate": "Bearer"})
                return False
            return True

        def do_GET(self):
            if self._guard():
                self._send(405, None, {"Allow": "POST, DELETE"})

        def do_DELETE(self):
            if self._guard():
                sid = self.headers.get("Mcp-Session-Id")
                self._send(200 if sid and remote.end(sid) else 404)

        def do_POST(self):
            if not self._guard():
                return
            length = int(self.headers.get("Content-Length") or 0)
            if length <= 0 or length > MAX_BODY:
                return self._err(413 if length > MAX_BODY else 400, -32600, "corps absent ou trop gros")
            try:
                msg = json.loads(self.rfile.read(length))
            except ValueError:
                return self._err(400, -32700, "JSON illisible")
            if not isinstance(msg, dict) or msg.get("jsonrpc") != "2.0":
                return self._err(400, -32600, "un seul message JSON-RPC 2.0 est attendu")
            if msg.get("method") == "initialize":
                reply, version = remote.answer(msg)
                sid = remote.new_session(version)
                return self._send(200, reply, {"Mcp-Session-Id": sid})
            sid = self.headers.get("Mcp-Session-Id")
            if not sid:
                return self._err(400, -32600, "en-tete Mcp-Session-Id manquant (appelez initialize)", msg.get("id"))
            if remote.session(sid) is None:
                return self._err(404, -32001, "session inconnue ou expiree", msg.get("id"))
            pv = self.headers.get("MCP-Protocol-Version")
            if pv and pv not in SUPPORTED_VERSIONS:
                return self._err(400, -32600, f"version du protocole non prise en charge : {pv}", msg.get("id"))
            if "method" not in msg or "id" not in msg:   # notification ou reponse du client
                return self._send(202)
            reply, _ = remote.answer(msg)
            self._send(200, reply)

    return Handler


def main(argv=None):
    ap = argparse.ArgumentParser(description="Serveur MCP distant de CSTL (connecteurs claude.ai).")
    ap.add_argument("--listen", default=os.environ.get("CSTL_MCP_LISTEN", "127.0.0.1:8765"))
    ap.add_argument("--token", default=os.environ.get("CSTL_MCP_TOKEN"), help="jeton secret (CSTL_MCP_TOKEN)")
    ap.add_argument("--new-token", action="store_true", help="affiche un nouveau jeton et quitte")
    ap.add_argument("--tls-cert")
    ap.add_argument("--tls-key")
    ap.add_argument("--allow-origin", action="append", default=[],
                    help="origine navigateur acceptee en plus de claude.ai et claude.com")
    args = ap.parse_args(argv)
    if args.new_token:
        sys.stdout.write(secrets.token_urlsafe(32) + "\n")
        return 0
    remote = Remote(args.token, DEFAULT_ORIGINS + tuple(args.allow_origin))
    host, _, port = args.listen.rpartition(":")
    httpd = ThreadingHTTPServer((host or "127.0.0.1", int(port)), make_handler(remote))
    scheme = "http"
    if args.tls_cert:
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.minimum_version = ssl.TLSVersion.TLSv1_2
        ctx.load_cert_chain(args.tls_cert, args.tls_key)
        httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
        scheme = "https"
    sys.stderr.write(f"[cstl-mcp-remote] agent {remote.bridge.agent.name}, ecoute {scheme}://{host}:{port}/mcp/<jeton>\n")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


def cli():
    """stdout ne sert qu'au jeton de --new-token : les messages du SDK
    (enregistrement...) partent sur stderr."""
    import builtins
    _print = builtins.print
    builtins.print = lambda *a, **k: _print(*a, **{**k, "file": sys.stderr})
    sys.exit(main())


if __name__ == "__main__":
    cli()
