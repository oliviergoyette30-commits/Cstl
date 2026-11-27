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
SEND_TOOLS = {"cstl_send_compact", "cstl_send_relations"}


class RateLimit:
    """Fenetre glissante d'une minute (2026-11-27, revue d'attaque : 400
    requetes passaient en 1,1 s, et 1 100 sessions creees d'affilee
    faisaient expirer la session d'un utilisateur legitime)."""

    def __init__(self, per_min):
        import collections
        self.per_min, self.hits, self.lock = per_min, collections.deque(), threading.Lock()

    def allow(self):
        now = time.time()
        with self.lock:
            while self.hits and now - self.hits[0] > 60:
                self.hits.popleft()
            if len(self.hits) >= self.per_min:
                return False
            self.hits.append(now)
            return True


DEMO_TOOLS = [
    {"name": "demo_list_repos",
     "description": "DEMONSTRATION (faux depots, faux paiements) : etat actuel, ce qui a vraiment ete execute.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "demo_delete_repo",
     "description": "DEMONSTRATION : supprime un depot (faux). Protege par CSTL : l'outil exige lui-meme un jeton "
                    "signe pour l'action delete_repo, meme sans appel prealable a cstl_authorize.",
     "inputSchema": {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}},
    {"name": "demo_send_payment",
     "description": "DEMONSTRATION : envoie un paiement (faux). Protege par CSTL : l'outil exige lui-meme un jeton "
                    "signe pour l'action send_payment et CE montant.",
     "inputSchema": {"type": "object", "required": ["amount", "to"], "properties": {
         "amount": {"type": "number"}, "to": {"type": "string"}}}},
]


class Demo:
    """Outils de demonstration du VRAI controle (CSTL_DEMO_TOOLS=1) : leurs
    actions (sur de faux depots et paiements, en memoire) sont protegees par
    `@guarded` -- elles ne s'executent qu'avec un jeton signe par le serveur
    et verifie ici, que le modele ait demande l'autorisation ou non."""

    tools = None   # renseigne apres la definition (DEMO_TOOLS)

    def __init__(self):
        import cstl_connect
        self.tools = DEMO_TOOLS
        self.repos = ["site-web", "api", "prod-db"]
        self.payments = []
        conn = cstl_connect.Connector()
        self.ActionDenied = cstl_connect.ActionDenied

        @conn.guarded("delete_repo")
        def delete_repo(name):
            if name not in self.repos:
                return f"depot inconnu : {name}"
            self.repos.remove(name)
            return f"depot {name} SUPPRIME (execute apres verification du jeton signe)"

        @conn.guarded("send_payment", amount_arg="amount")
        def send_payment(amount, to):
            self.payments.append({"amount": amount, "to": to})
            return f"paiement de {amount} a {to} ENVOYE (execute apres verification du jeton signe)"

        self.delete_repo, self.send_payment = delete_repo, send_payment

    def call(self, name, args):
        if name == "demo_list_repos":
            return json.dumps({"depots": self.repos, "paiements_executes": self.payments}, ensure_ascii=False)
        try:
            if name == "demo_delete_repo":
                return self.delete_repo(str(args["name"]))
            return self.send_payment(args["amount"], str(args["to"]))
        except self.ActionDenied as e:
            raise RuntimeError(f"REFUSE par CSTL, rien n'a ete execute : {e}") from None


GITHUB_TOOLS = [
    {"name": "github_list_issues",
     "description": "Issues ouvertes du depot GitHub configure (lecture, sans controle).",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "github_create_issue",
     "description": "Cree une issue dans le depot GitHub configure. Protege par CSTL (action github_create_issue) : "
                    "l'outil exige un jeton signe par le serveur CSTL avant d'appeler GitHub.",
     "inputSchema": {"type": "object", "required": ["title"], "properties": {
         "title": {"type": "string"}, "body": {"type": "string"}}}},
    {"name": "github_comment",
     "description": "Commente une issue. Protege par CSTL (action github_comment).",
     "inputSchema": {"type": "object", "required": ["number", "body"], "properties": {
         "number": {"type": "integer"}, "body": {"type": "string"}}}},
    {"name": "github_close_issue",
     "description": "Ferme une issue. Protege par CSTL (action github_close_issue).",
     "inputSchema": {"type": "object", "required": ["number"], "properties": {"number": {"type": "integer"}}}},
]


class GitHub:
    """De VRAIES actions sur GitHub (CSTL_GITHUB_TOKEN + CSTL_GITHUB_REPO).
    Le jeton GitHub reste dans ce serveur : le modele ne le voit jamais, et
    ne peut ecrire sur GitHub QUE par ces outils, chacun protege par
    `@guarded` (jeton signe du serveur CSTL exige). C'est le principe de la
    garde : elle seule detient le privilege. Utilisez un jeton a grain fin,
    limite a UN depot et aux issues."""

    def __init__(self, token, repo, api=None):
        import re
        import cstl_connect
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
            raise SystemExit(f"CSTL_GITHUB_REPO invalide : {repo!r} (attendu proprietaire/depot)")
        self.tools = GITHUB_TOOLS
        self.token, self.repo = token, repo
        self.api = (api or os.environ.get("CSTL_GITHUB_API") or "https://api.github.com").rstrip("/")
        conn = cstl_connect.Connector()
        self.ActionDenied = cstl_connect.ActionDenied

        @conn.guarded("github_create_issue")
        def create(title, body=""):
            r = self._req("POST", f"/repos/{self.repo}/issues", {"title": title, "body": body})
            return f"issue #{r['number']} creee : {r.get('html_url', '')}"

        @conn.guarded("github_comment")
        def comment(number, body):
            self._req("POST", f"/repos/{self.repo}/issues/{int(number)}/comments", {"body": body})
            return f"commentaire ajoute a l'issue #{int(number)}"

        @conn.guarded("github_close_issue")
        def close(number):
            self._req("PATCH", f"/repos/{self.repo}/issues/{int(number)}", {"state": "closed"})
            return f"issue #{int(number)} fermee"

        self.create, self.comment, self.close = create, comment, close

    def _req(self, method, path, body=None):
        import urllib.error
        import urllib.request
        req = urllib.request.Request(self.api + path, method=method,
                                     data=None if body is None else json.dumps(body).encode(),
                                     headers={"Authorization": f"Bearer {self.token}",
                                              "Accept": "application/vnd.github+json",
                                              "X-GitHub-Api-Version": "2022-11-28",
                                              "User-Agent": "cstl-mcp-remote",
                                              "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                raw = r.read()
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"GitHub a refuse ({e.code}) : {e.read()[:200].decode(errors='replace')}") from None
        return json.loads(raw) if raw else {}

    def call(self, name, args):
        if name == "github_list_issues":
            issues = self._req("GET", f"/repos/{self.repo}/issues?state=open&per_page=30")
            return json.dumps([{"number": i["number"], "title": i["title"], "user": i["user"]["login"]}
                               for i in issues if "pull_request" not in i], ensure_ascii=False)
        try:
            if name == "github_create_issue":
                return self.create(str(args["title"]), str(args.get("body", "")))
            if name == "github_comment":
                return self.comment(int(args["number"]), str(args["body"]))
            return self.close(int(args["number"]))
        except self.ActionDenied as e:
            raise RuntimeError(f"REFUSE par CSTL, GitHub n'a pas ete appele : {e}") from None


class Remote:
    def __init__(self, token, origins=DEFAULT_ORIGINS):
        if not token or len(token) < 32:
            raise SystemExit("jeton requis (CSTL_MCP_TOKEN ou --token), 32 caracteres au moins : "
                             "cstl-mcp-remote --new-token")
        self.token = token.encode()
        self.origins = set(origins)
        self.bridge = local.Bridge()
        self.extras = []                                 # groupes d'outils proteges par @guarded
        if os.environ.get("CSTL_DEMO_TOOLS") == "1":
            self.extras.append(Demo())
        if os.environ.get("CSTL_GITHUB_TOKEN") and os.environ.get("CSTL_GITHUB_REPO"):
            self.extras.append(GitHub(os.environ["CSTL_GITHUB_TOKEN"], os.environ["CSTL_GITHUB_REPO"]))
        self.lock = threading.Lock()                     # un seul agent CSTL : appels en serie
        self.rate = RateLimit(int(os.environ.get("CSTL_MCP_RATE_PER_MIN", "600")))
        self.init_rate = RateLimit(int(os.environ.get("CSTL_MCP_INIT_PER_MIN", "60")))
        # Outils qui ECRIVENT des relations au nom de l'agent : coupes par defaut
        # (revue d'attaque : une IA compromise s'en servait pour remplir l'audit
        # de regles sans autorite, ignorees mais stockees). CSTL_MCP_SEND_TOOLS=1.
        self.send_tools = os.environ.get("CSTL_MCP_SEND_TOOLS") == "1"
        self.sessions = {}

    def authorized(self, path, header):
        """Le jeton du chemin (/mcp/<jeton>, tout ce qui suit /mcp/, decode :
        un jeton en base64 peut contenir « / ») ou de l'en-tete Bearer."""
        from urllib.parse import unquote
        p = path.split("?")[0]
        given = None
        if p.startswith("/mcp/") and len(p) > 5:
            given = unquote(p[5:])
        elif p.rstrip("/") == "/mcp" and header and header.startswith("Bearer "):
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
        if not self.send_tools and msg.get("method") in ("tools/list", "tools/call"):
            name = (msg.get("params") or {}).get("name")
            if msg["method"] == "tools/call" and name in SEND_TOOLS:
                return {"jsonrpc": "2.0", "id": msg.get("id"), "result": {"content": [{"type": "text", "text":
                        f"outil {name} desactive sur ce serveur (CSTL_MCP_SEND_TOOLS=1 pour l'activer)"}],
                        "isError": True}}, None
            if msg["method"] == "tools/list":
                reply, v = self._answer(msg)
                reply["result"]["tools"] = [t for t in reply["result"]["tools"] if t["name"] not in SEND_TOOLS]
                return reply, v
        return self._answer(msg)

    def _answer(self, msg):
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
        if self.extras and msg.get("method") in ("tools/list", "tools/call"):
            mid, params = msg.get("id"), msg.get("params") or {}
            if msg["method"] == "tools/list":
                with self.lock:
                    reply = local.handle(self.bridge, msg)
                reply["result"]["tools"] = reply["result"]["tools"] + [t for x in self.extras for t in x.tools]
                return reply, None
            owner = next((x for x in self.extras if params.get("name") in {t["name"] for t in x.tools}), None)
            if owner is not None:
                with self.lock:
                    try:
                        text, err = owner.call(params["name"], params.get("arguments") or {}), False
                    except Exception as e:  # noqa: BLE001 -- rendu au modele
                        text, err = str(e), True
                return {"jsonrpc": "2.0", "id": mid,
                        "result": {"content": [{"type": "text", "text": text}], "isError": err}}, None
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
            """Chemin, origine, puis jeton. True si la requete peut continuer.
            Hors de /mcp : 404 (et non 401), sinon un client croit a de
            l'OAuth (claude.ai sondait /.well-known/oauth-*)."""
            p = self.path.split("?")[0]
            if p != "/mcp" and not p.startswith("/mcp/"):
                self._send(404, {"error": "introuvable"})
                return False
            origin = self.headers.get("Origin")
            if origin and origin not in remote.origins:
                self._err(403, -32000, "origine refusee")
                return False
            if not remote.authorized(self.path, self.headers.get("Authorization")):
                self._send(401, {"error": "jeton absent ou invalide"}, {"WWW-Authenticate": "Bearer"})
                return False
            return True

        def do_GET(self):
            if self.path == "/healthz":                   # pour l'hebergeur ; ne dit rien d'autre
                return self._send(200, {"ok": True})
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
            limiter = remote.init_rate if msg.get("method") == "initialize" else remote.rate
            if not limiter.allow():
                return self._send(429, {"jsonrpc": "2.0", "id": msg.get("id"), "error": {
                    "code": -32000, "message": "trop de requetes, reessayez dans une minute"}}, {"Retry-After": "60"})
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
