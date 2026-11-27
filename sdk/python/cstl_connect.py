#!/usr/bin/env python3
"""
cstl_connect.py -- le connecteur CSTL a embarquer dans VOTRE serveur MCP ou
VOTRE agent A2A (2026-11-27). Aucune dependance hors du SDK.

Les proxies (`cstl mcp-proxy`, `cstl a2a-proxy`) se placent DEVANT un serveur
qu'on ne modifie pas. Ce module sert quand on ecrit le serveur soi-meme : la
verification se fait dans le meme processus, la ou l'action s'execute.

1. Un outil MCP (ou n'importe quelle fonction) : `@guarded`

    from cstl_connect import guarded

    @mcp.tool()                                   # SDK MCP officiel (FastMCP) ou autre
    @guarded("refund_order", amount_arg="amount")
    def refund_order(order_id: int, amount: float) -> str:
        ...

   Avant chaque appel, l'outil demande l'autorisation au serveur CSTL, au nom
   de l'agent CSTL_AGENT ; le jeton signe est verifie (cle publique du
   serveur, action, agent, montant, fenetre de temps, usage unique) ; la
   fonction ne s'execute qu'ensuite. Sinon `ActionDenied` : le SDK MCP en fait
   une erreur d'outil (`isError`) que le modele lit, avec la raison et la
   regle. `on_deny="message"` renvoie plutot ce texte comme resultat.
   - fonctions synchrones et `async def` ;
   - la signature est conservee (`functools.wraps`) : le SDK MCP en tire le
     schema des arguments comme avant ;
   - `action` par defaut : le nom de la fonction (avec `prefix=`) ;
   - FERME par defaut : serveur CSTL injoignable, reponse illisible, jeton
     invalide -> la fonction n'est pas appelee.

2. Un agent A2A (Starlette, FastAPI, SDK a2a-python...) : `A2AGuard`

    from cstl_connect import A2AGuard
    app = A2AGuard(app, name="billing")           # middleware ASGI

   Chaque requete JSON-RPC (POST) doit porter un jeton CSTL par appel dans
   `X-CSTL-Authorization`, pour l'action `a2a.billing.<Methode>` -- exactement
   comme devant `cstl a2a-proxy`, avec la meme verification
   (`cstl_a2a.check_calls`, lots en tout-ou-rien). Sinon 401/403, erreur
   JSON-RPC -32050, et l'application ne recoit RIEN. Si elle passe,
   l'application recoit `X-CSTL-Agent: <appelant verifie>`, et le jeton est
   retire. Les GET (carte d'agent) passent tels quels.
   Le client reste `cstl_a2a.call(agent, url, "billing", "SendMessage", ...)`.

   Hors ASGI : `check_a2a(headers, body, name)` renvoie (statut, corps
   d'erreur ou None, appelant) pour n'importe quel serveur HTTP.

Configuration : options, ou variables d'environnement
  CSTL_AGENT              nom de l'agent CSTL du serveur MCP (pour @guarded)
  CSTL_SERVER_PUBLIC_KEY  cle publique du serveur CSTL (hex, ou chemin du .pub) -- OBLIGATOIRE
  CSTL_HOST, CSTL_PORT    adresse du serveur CSTL (127.0.0.1:5050)
  CSTL_GUARD_ONLINE       host:port : verifier aussi, en direct, que le jeton
                          n'est ni revoque ni arrete (CstlGuard online)

Sans cle publique, rien ne s'execute : croire le serveur sur parole
permettrait a un faux serveur d'autoriser n'importe quoi.

Limite : le connecteur controle QUELLE action est faite, par QUI, avec quel
montant. Les autres arguments se bornent dans la fonction elle-meme (ou avec
`--args-schema` devant un serveur MCP qu'on ne modifie pas).

Tests en direct : test_connect_live.py.
"""
import asyncio
import contextlib
import functools
import inspect
import io
import json
import os
import sys
import threading
from pathlib import Path

SDK = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK))

from cstl_llm_agent import ActionDenied, ApprovalRequired  # noqa: E402,F401  (re-exportes)


def _server_key(explicit):
    key = explicit or os.environ.get("CSTL_SERVER_PUBLIC_KEY")
    if not key:
        raise ValueError("cle publique du serveur CSTL requise (server_key= ou CSTL_SERVER_PUBLIC_KEY) : "
                         "sans elle, un faux serveur pourrait tout autoriser")
    if Path(key).is_file():
        key = Path(key).read_text().strip()
    return key.strip()


def _guard(server_key, online=None, replay_db=None):
    from cstl_guard import CstlGuard
    return CstlGuard(_server_key(server_key), online=online, replay_db=replay_db)


class Connector:
    """Identite CSTL d'un serveur MCP (ou de tout code qui execute des actions)."""

    def __init__(self, agent=None, server_key=None, host=None, port=None, prefix="", online=None,
                 replay_db=None):
        self.name = agent or os.environ.get("CSTL_AGENT")
        if not self.name:
            raise ValueError("nom de l'agent CSTL requis (agent= ou CSTL_AGENT)")
        self.host = host or os.environ.get("CSTL_HOST", "127.0.0.1")
        self.port = int(port or os.environ.get("CSTL_PORT", "5050"))
        self.prefix = prefix
        self.guard = _guard(server_key, online, replay_db)
        self._agent = None
        self._lock = threading.Lock()

    def _client(self):
        if self._agent is None:
            import cstl_llm_agent as m
            with contextlib.redirect_stdout(io.StringIO()):
                a = m.CstlAgent(self.name, "none", host=self.host, port=self.port)
                if not a.register():
                    raise ActionDenied("register", "agent_non_enregistre")
            self._agent = a
        return self._agent

    def authorize(self, action, amount=None):
        """Jeton signe pour `action`, ou ActionDenied (ferme sur toute erreur)."""
        with self._lock:
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    return self._client().authorize(action, amount)
            except ActionDenied:
                raise
            except Exception as e:  # noqa: BLE001 -- ferme par defaut
                self._agent = None
                raise ActionDenied(action, f"cstl_indisponible:{type(e).__name__}") from None

    def _execute(self, token, action, amount):
        """Verifie et consomme le jeton (signature, action, agent, montant,
        temps, usage unique) ; ActionDenied sinon."""
        from cstl_guard import Unauthorized
        try:
            self.guard.run(token, action, lambda: None, agent=self.name, amount=amount)
        except Unauthorized as e:
            raise ActionDenied(action, f"jeton_invalide:{e.reason}") from None

    def guarded(self, action=None, *, amount_arg=None, on_deny="raise"):
        """Decorateur : la fonction ne s'execute qu'avec une autorisation CSTL
        verifiee. `on_deny="message"` renvoie le texte du refus au lieu de
        lever ActionDenied."""
        if on_deny not in ("raise", "message"):
            raise ValueError("on_deny vaut 'raise' ou 'message'")

        def wrap(func):
            act = f"{self.prefix}{action or func.__name__}"
            sig = inspect.signature(func)
            if amount_arg is not None and amount_arg not in sig.parameters:
                raise ValueError(f"{func.__name__} n'a pas d'argument {amount_arg!r}")

            def amount_of(args, kwargs):
                if amount_arg is None:
                    return None
                bound = sig.bind_partial(*args, **kwargs)
                bound.apply_defaults()
                v = bound.arguments.get(amount_arg)
                return None if v is None else str(v)

            def refusal(e):
                if on_deny == "raise":
                    raise e
                return f"Refuse par CSTL : action {e.action!r}, raison {e.reason}" + (f", regle {e.rule}" if e.rule else "")

            if inspect.iscoroutinefunction(func):
                @functools.wraps(func)
                async def async_wrapper(*args, **kwargs):
                    amount = amount_of(args, kwargs)
                    try:
                        token = await asyncio.to_thread(self.authorize, act, amount)
                        await asyncio.to_thread(self._execute, token, act, amount)
                    except ActionDenied as e:
                        return refusal(e)
                    return await func(*args, **kwargs)
                async_wrapper.cstl_action = act
                return async_wrapper

            @functools.wraps(func)
            def wrapper(*args, **kwargs):
                amount = amount_of(args, kwargs)
                try:
                    token = self.authorize(act, amount)
                    self._execute(token, act, amount)
                except ActionDenied as e:
                    return refusal(e)
                return func(*args, **kwargs)
            wrapper.cstl_action = act
            return wrapper
        return wrap


_default = None
_default_lock = threading.Lock()


def _default_connector():
    global _default
    with _default_lock:
        if _default is None:
            _default = Connector()
        return _default


def guarded(action=None, *, amount_arg=None, on_deny="raise", connector=None):
    """`@guarded("action", amount_arg="amount")` avec le connecteur par defaut
    (variables d'environnement), cree au PREMIER appel de l'outil : importer
    le module ne demande rien au serveur CSTL."""
    def wrap(func):
        state = {}

        def inner():
            if "w" not in state:
                c = connector or _default_connector()
                state["w"] = c.guarded(action, amount_arg=amount_arg, on_deny=on_deny)(func)
            return state["w"]

        if inspect.iscoroutinefunction(func):
            @functools.wraps(func)
            async def async_lazy(*args, **kwargs):
                return await inner()(*args, **kwargs)
            return async_lazy

        @functools.wraps(func)
        def lazy(*args, **kwargs):
            return inner()(*args, **kwargs)
        return lazy
    return wrap


# ------------------------------------------------------------------ A2A
def check_a2a(headers, body, name, guard):
    """Pour n'importe quel serveur HTTP : (statut, corps JSON d'erreur ou None,
    appelant verifie). `headers` : dict (cles quelconques en casse)."""
    import cstl_a2a
    h = {str(k).lower(): v for k, v in headers.items()}
    try:
        msg = json.loads(body)
    except ValueError:
        return 400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "Invalid JSON payload"}}, None
    calls = msg if isinstance(msg, list) else [msg]
    if (not calls or len(calls) > cstl_a2a.MAX_BATCH
            or not all(isinstance(c, dict) and isinstance(c.get("method"), str) for c in calls)):
        return 400, {"jsonrpc": "2.0", "id": None, "error": {
            "code": -32600, "message": f"Request payload validation error (1 a {cstl_a2a.MAX_BATCH} appels JSON-RPC)"}}, None
    mid = calls[0].get("id") if len(calls) == 1 else None
    status, verdict, detail, caller = cstl_a2a.check_calls(guard, name, h.get(cstl_a2a.HEADER.lower()), calls)
    if status != 200:
        return status, cstl_a2a.deny_body(mid, verdict, detail), None
    return 200, None, caller


class A2AGuard:
    """Middleware ASGI : CSTL devant VOTRE agent A2A, dans le meme processus."""

    def __init__(self, app, name, server_key=None, online=None, replay_db=None):
        import cstl_a2a
        cstl_a2a.action_for(name, "SendMessage")          # nom hors alphabet -> erreur des maintenant
        self.app, self.name = app, name
        self.guard = _guard(server_key, online, replay_db)
        self.header = cstl_a2a.HEADER.lower().encode()
        self.agent_header = cstl_a2a.AGENT_HEADER.lower().encode()

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http" or scope.get("method") != "POST":
            return await self.app(scope, receive, send)
        body, more = b"", True
        while more:
            m = await receive()
            if m["type"] == "http.disconnect":
                return
            body += m.get("body", b"")
            more = m.get("more_body", False)
        headers = {k.decode("latin-1"): v.decode("latin-1") for k, v in scope.get("headers", [])}
        status, err, caller = await asyncio.to_thread(check_a2a, headers, body, self.name, self.guard)
        if status != 200:
            data = json.dumps(err).encode()
            extra = [(b"www-authenticate", b'ApiKey header="X-CSTL-Authorization"')] if status == 401 else []
            await send({"type": "http.response.start", "status": status,
                        "headers": [(b"content-type", b"application/json"),
                                    (b"content-length", str(len(data)).encode()), *extra]})
            await send({"type": "http.response.body", "body": data})
            return
        inner = dict(scope)
        inner["headers"] = [(k, v) for k, v in scope.get("headers", [])
                            if k.lower() not in (self.header, self.agent_header)]
        inner["headers"].append((self.agent_header, caller.encode()))
        sent = False

        async def replay():
            nonlocal sent
            if not sent:
                sent = True
                return {"type": "http.request", "body": body, "more_body": False}
            return await receive()

        await self.app(inner, replay, send)
