#!/usr/bin/env python3
"""
cstl_a2a.py -- CSTL devant un agent A2A (Agent2Agent, JSON-RPC, 2026-11-25).

Lu dans la specification A2A 1.0 (a2aproject/A2A, docs/specification.md) :
JSON-RPC 2.0 sur HTTP, methodes en PascalCase (`SendMessage`,
`SendStreamingMessage` en SSE, `GetTask`, `CancelTask`...), carte d'agent a
`/.well-known/agent-card.json` qui declare ses `securitySchemes`, et
l'autorisation est laissee a chaque serveur (section 7.5). Les noms 0.3
(`message/send`...) restent acceptes : un en-tete `A2A-Version` vide veut
dire 0.3 (section 3.6).

LE PROXY (`cstl-a2a-proxy`) se place devant un agent A2A existant et joue le
role du systeme qui execute :
  - chaque requete JSON-RPC doit porter, dans l'en-tete
    `X-CSTL-Authorization`, un jeton CSTL signe par le serveur CSTL pour
    l'action `a2a.<cible>.<Methode>` (nom 1.0) ;
  - le proxy verifie ce jeton lui-meme (CstlGuard, cle publique du serveur
    obtenue hors bande) : signature, action, validite, usage unique ;
  - seulement alors il transmet, avec `X-CSTL-Agent: <appelant verifie>`,
    pour que l'agent cible sache QUI appelle ;
  - sinon : HTTP 401 (pas de jeton) ou 403 (jeton refuse), erreur JSON-RPC
    -32050 avec la raison ; l'agent cible ne recoit rien.
  - La carte d'agent est republiee par le proxy : adresse du proxy, et un
    `securitySchemes.cstl` (apiKeySecurityScheme, en-tete
    X-CSTL-Authorization) ajoute aux exigences.
Ferme par defaut, et sans confiance dans le reseau : la cle du serveur CSTL
est obligatoire.

LE CLIENT : `call(agent, url, target, method, params)` demande le jeton
(`agent.authorize`, refuse -> ActionDenied, rien n'est envoye), puis appelle
l'agent A2A a travers le proxy. Les regles s'ecrivent comme les autres :
RELATION [type=PERFORM, subject=<appelant>, object=a2a.billing.SendMessage,
modality=MUST_NOT].

Limites v1 : une requete JSON-RPC a la fois (pas de lot) ; le proxy controle
QUELLE methode est appelee sur QUEL agent, pas le contenu du message ; pas de
montant. Test en direct : test_a2a_proxy_live.py.
"""
import argparse
import base64
import json
import os
import re
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

SDK = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK))

HEADER = "X-CSTL-Authorization"
AGENT_HEADER = "X-CSTL-Agent"
CARD_PATH = "/.well-known/agent-card.json"
ERROR_CODE = -32050

# Noms 0.3 -> noms 1.0 (section 5.3 / 9.4 de la specification 1.0).
V03 = {
    "message/send": "SendMessage",
    "message/stream": "SendStreamingMessage",
    "tasks/get": "GetTask",
    "tasks/list": "ListTasks",
    "tasks/cancel": "CancelTask",
    "tasks/resubscribe": "SubscribeToTask",
    "tasks/pushNotificationConfig/set": "CreateTaskPushNotificationConfig",
    "tasks/pushNotificationConfig/get": "GetTaskPushNotificationConfig",
    "tasks/pushNotificationConfig/list": "ListTaskPushNotificationConfigs",
    "tasks/pushNotificationConfig/delete": "DeleteTaskPushNotificationConfig",
    "agent/getAuthenticatedExtendedCard": "GetExtendedAgentCard",
}


def canonical_method(method):
    return V03.get(method, method)


ACTION_CHARS = re.compile(r"[A-Za-z0-9_.-]+")


def action_for(target, method):
    """Action CSTL d'un appel A2A : `a2a.<cible>.<Methode>` (nom 1.0). Les
    noms d'action n'admettent que [A-Za-z0-9_.-] (specification controle v1,
    section 7.1) ; une methode inconnue hors de cet alphabet est refusee."""
    action = f"a2a.{target}.{canonical_method(method)}"
    if not ACTION_CHARS.fullmatch(action) or len(action) > 128:
        raise ValueError(f"methode ou nom d'agent hors de l'alphabet des actions : {action!r}")
    return action


def encode_token(token):
    d = {k: getattr(token, k) for k in ("agent", "action", "request_id", "issued_at", "expires_at",
                                         "audit_hash", "signature", "amount")}
    return base64.urlsafe_b64encode(json.dumps(d, separators=(",", ":")).encode()).decode().rstrip("=")


def decode_token(text):
    from cstl_guard import AuthorizationToken
    raw = base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))
    d = json.loads(raw)
    return AuthorizationToken(agent=str(d["agent"]), action=str(d["action"]), request_id=str(d["request_id"]),
                              issued_at=int(d["issued_at"]), expires_at=int(d["expires_at"]),
                              audit_hash=str(d["audit_hash"]), signature=str(d["signature"]),
                              amount=None if d.get("amount") is None else str(d["amount"]))


# -- Client ---------------------------------------------------------------------
def call(agent, url, target, method, params, *, version="1.0", request_id=1, timeout=60):
    """Appelle une methode A2A a travers le proxy, avec un jeton pour CETTE
    methode sur CET agent. Retourne (statut HTTP, en-tetes, corps en octets)."""
    token = agent.authorize(action_for(target, method))        # ActionDenied si refuse
    body = json.dumps({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}).encode()
    headers = {"Content-Type": "application/json", HEADER: encode_token(token)}
    if version:
        headers["A2A-Version"] = version
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, dict(r.headers), r.read()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read()


def call_batch(agent, url, target, calls, *, version="1.0", timeout=60):
    """Lot JSON-RPC (2026-11-25) : `calls` = [(methode, params), ...]. Un jeton
    par appel, dans l'ordre, dans le meme en-tete (separes par des virgules).
    Si UN appel est refuse par le serveur CSTL, ActionDenied est leve et rien
    n'est envoye ; si le proxy refuse UN jeton, il refuse tout le lot."""
    tokens = [agent.authorize(action_for(target, m)) for m, _ in calls]
    body = json.dumps([{"jsonrpc": "2.0", "id": i + 1, "method": m, "params": p}
                       for i, (m, p) in enumerate(calls)]).encode()
    headers = {"Content-Type": "application/json", HEADER: ",".join(encode_token(t) for t in tokens)}
    if version:
        headers["A2A-Version"] = version
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, dict(r.headers), r.read()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read()


MAX_BATCH = 50


# -- Proxy ----------------------------------------------------------------------
def check_calls(guard, target, header_value, calls):
    """Verifie les jetons d'une requete A2A (un appel ou un lot), comme le
    proxy et le middleware embarque (`cstl_connect.A2AGuard`).

    Retourne (statut HTTP, verdict, detail, agent appelant) : 200 si tout
    passe, 401 sans jeton, 403 sinon. Tout ou rien : chaque jeton est verifie
    avant qu'aucun ne soit consomme ; un seul refus refuse le lot entier."""
    if not header_value:
        return 401, "no_token", f"en-tete {HEADER} absent", None
    from cstl_guard import Unauthorized
    try:
        tokens = [decode_token(t.strip()) for t in header_value.split(",")]
    except Exception:  # noqa: BLE001
        return 403, "bad_token", "jeton illisible", None
    if len(tokens) != len(calls):
        return 403, "token_count", f"{len(calls)} appels, {len(tokens)} jetons", None
    if len({t.agent for t in tokens}) != 1:
        return 403, "mixed_agents", "tous les jetons d'un lot doivent etre du meme agent", None
    try:
        actions = [action_for(target, c["method"]) for c in calls]
    except ValueError as e:
        return 403, "bad_method", str(e), None
    try:
        for t, a in zip(tokens, actions):
            guard.check(t, a)                       # tout verifier avant de rien consommer
        for t, a in zip(tokens, actions):
            guard.run(t, a, lambda: None)           # signature, action, temps, usage unique
    except Unauthorized as e:
        return 403, e.reason, "lot refuse en entier", None
    return 200, "ok", "", tokens[0].agent


def deny_body(mid, verdict, detail=""):
    """Corps JSON-RPC d'un refus CSTL (code -32050, raison CSTL_<VERDICT>)."""
    return {"jsonrpc": "2.0", "id": mid, "error": {
        "code": ERROR_CODE, "message": f"Refuse par CSTL : {verdict}",
        "data": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": f"CSTL_{verdict.upper()}",
                  "domain": "cstl", "metadata": {"detail": detail[:200]}}]}}


def make_handler(upstream, target, guard, public_url):
    from urllib.parse import urlsplit
    u = urlsplit(upstream)
    card_url = f"{u.scheme}://{u.netloc}{CARD_PATH}"     # la carte est a la racine du domaine (section 8.2)

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def _send(self, status, obj, extra=None):
            data = json.dumps(obj).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            for k, v in (extra or {}).items():
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(data)

        def _deny(self, status, mid, verdict, detail=""):
            self._send(status, deny_body(mid, verdict, detail),
                {"WWW-Authenticate": f'ApiKey header="{HEADER}"'} if status == 401 else None)

        def do_GET(self):
            if self.path.split("?")[0] != CARD_PATH:
                return self._send(404, {"error": "not_found"})
            try:
                with urllib.request.urlopen(card_url, timeout=30) as r:
                    card = json.loads(r.read())
            except Exception as e:  # noqa: BLE001
                return self._send(502, {"error": "carte_injoignable", "detail": str(e)[:200]})
            for itf in card.get("supportedInterfaces", []) or []:
                itf["url"] = public_url
            if "url" in card:                                  # carte 0.3
                card["url"] = public_url
            card.setdefault("securitySchemes", {})["cstl"] = {"apiKeySecurityScheme": {
                "location": "header", "name": HEADER,
                "description": "Jeton CSTL signe (base64url JSON) pour l'action a2a.%s.<Methode>" % target}}
            card["securityRequirements"] = [{"schemes": {"cstl": {"list": []}}}]
            self._send(200, card)

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length)
            try:
                msg = json.loads(body)
            except ValueError:
                return self._send(400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "Invalid JSON payload"}})
            # Lot JSON-RPC (2026-11-25) : un jeton par appel, dans l'ordre ; tout
            # ou rien -- un seul jeton refuse et l'agent ne recoit aucun appel.
            calls = msg if isinstance(msg, list) else [msg]
            if (not calls or len(calls) > MAX_BATCH
                    or not all(isinstance(c, dict) and isinstance(c.get("method"), str) for c in calls)):
                return self._send(400, {"jsonrpc": "2.0", "id": None, "error": {
                    "code": -32600, "message": f"Request payload validation error (1 a {MAX_BATCH} appels JSON-RPC)"}})
            mid = calls[0].get("id") if len(calls) == 1 else None
            status, verdict, detail, caller = check_calls(guard, target, self.headers.get(HEADER), calls)
            if status != 200:
                return self._deny(status, mid, verdict, detail)
            headers = {k: v for k, v in self.headers.items()
                       if k.lower() not in ("host", "content-length", HEADER.lower(), AGENT_HEADER.lower(), "connection")}
            headers[AGENT_HEADER] = caller
            req = urllib.request.Request(upstream, data=body, headers=headers, method="POST")
            try:
                resp = urllib.request.urlopen(req, timeout=300)
            except urllib.error.HTTPError as e:
                resp = e
            except Exception as e:  # noqa: BLE001
                return self._send(502, {"jsonrpc": "2.0", "id": mid, "error": {"code": -32603, "message": f"agent injoignable : {e}"}})
            self.send_response(resp.status if hasattr(resp, "status") else resp.code)
            for k, v in resp.headers.items():
                if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                    self.send_header(k, v)
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            while True:                                         # SSE compris : recopie au fil de l'eau
                chunk = resp.read1(65536) if hasattr(resp, "read1") else resp.read(65536)
                if not chunk:
                    break
                self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            resp.close()

    return Handler


def serve(listen, upstream, target, server_key, public_url=None, replay_db=None):
    from cstl_guard import CstlGuard
    action_for(target, "SendMessage")          # nom d'agent valide dans une action
    host, port = listen.rsplit(":", 1)
    guard = CstlGuard(server_key, replay_db=replay_db)
    return ThreadingHTTPServer((host, int(port)), make_handler(
        upstream, target, guard, public_url or f"http://{host}:{port}/"))


def main(argv=None):
    ap = argparse.ArgumentParser(description="CSTL devant un agent A2A")
    ap.add_argument("--listen", default="127.0.0.1:9100")
    ap.add_argument("--upstream", required=True, help="adresse JSON-RPC de l'agent A2A protege")
    ap.add_argument("--name", required=True, help="nom de l'agent protege dans les actions (a2a.<nom>.<Methode>), [A-Za-z0-9_-]")
    ap.add_argument("--server-key", default=os.environ.get("CSTL_SERVER_PUBLIC_KEY"),
                    help="cle publique du serveur CSTL (hex ou fichier .pub) -- obligatoire")
    ap.add_argument("--public-url", help="adresse du proxy publiee dans la carte d'agent")
    ap.add_argument("--replay-db", help="fichier SQLite des jetons deja utilises (survit a un redemarrage)")
    args = ap.parse_args(argv)
    if not args.server_key:
        raise SystemExit("--server-key obligatoire : sans elle, rien ne prouve qu'un jeton vient du serveur CSTL")
    key = Path(args.server_key).read_text().strip() if Path(args.server_key).is_file() else args.server_key.strip()
    httpd = serve(args.listen, args.upstream, args.name, key, args.public_url, args.replay_db)
    print(f"CSTL devant l'agent A2A {args.name!r} : {args.listen} -> {args.upstream}", file=sys.stderr)
    httpd.serve_forever()


if __name__ == "__main__":
    main()
