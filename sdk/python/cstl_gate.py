#!/usr/bin/env python3
"""
cstl_gate.py -- la garde qui EXECUTE : SQL (SQLite, PostgreSQL), shell, HTTP
(2026-11-25).

Pourquoi : un jeton CSTL ne rend une action obligatoire que si le systeme qui
l'execute l'exige. Une garde n'est donc obligatoire que si ELLE SEULE detient
le privilege : le mot de passe de la base, la cle de l'API, le droit de
lancer la commande. `cstl-gate` est ce service : il detient ces acces, et
n'execute que des OPERATIONS NOMMEES decrites dans un fichier de
configuration, jamais du SQL, une commande ou une URL fournis par l'agent.

  cstl-gate --config gate.json --server-key cstl_adn.db.server_key.pub --listen 127.0.0.1:9300

Un agent demande d'abord le jeton (`agent.authorize(<action>, amount=...)`),
puis :

  POST /v1/run   X-CSTL-Authorization: <jeton, base64url JSON>
  {"operation": "refund_order", "params": {"order_id": 42, "amount": 80}}

La garde verifie le jeton elle-meme (CstlGuard : signature sous la cle du
serveur obtenue hors bande, action de l'operation, agent permis, montant =
le parametre declare, validite, usage unique), valide chaque parametre selon
son type, puis execute. Sinon : 401 (pas de jeton), 403 (jeton refuse), 400
(parametre invalide), 404 (operation inconnue) ; rien n'est execute.

Configuration (JSON) :

  {"databases": {
     "billing": {"driver": "sqlite", "path": "billing.db"},
     "pg":      {"driver": "postgres", "dsn": "host=/run/pg dbname=app user=gate",
                 "password_env": "PG_PASSWORD"}},
   "operations": {
     "refund_order": {"type": "sql", "db": "billing",
                      "sql": "UPDATE orders SET refunded = :amount WHERE id = :order_id",
                      "params": {"order_id": "int", "amount": "num"}, "amount_param": "amount"},
     "backup":       {"type": "shell", "argv": ["/usr/bin/pg_dump", "-f", "/backups/{name}.sql", "app"],
                      "params": {"name": "word"}},
     "notify":       {"type": "http", "method": "POST", "url": "https://chat.example/hook",
                      "headers": {"Authorization": "env:CHAT_TOKEN"}, "json": {"text": "{message}"},
                      "params": {"message": "text"}}}}

  - `action` : l'action CSTL de l'operation (defaut : son nom) ;
    `agents` : liste facultative des agents permis.
  - Types de parametres :
    - `int`, `num` et `bool` ;
    - `word` : 1 a 64 caracteres parmi [A-Za-z0-9_.-], sans tiret au debut ;
    - `text` : texte d'une ligne, au plus 1000 caracteres ;
    - `{"enum": [...]}` : une des valeurs listees ;
    - `{"regex": "..."}` : une valeur qui correspond a la regex en entier.
    Un parametre absent, en trop ou mal type est refuse.
  - SQL : marqueurs `:nom`, toujours lies, jamais colles dans le texte.
    SQLite par le module sqlite3 ; PostgreSQL par `psql`, ou `:nom` devient
    `:'nom'` (citation litterale faite par psql), sans pilote Python.
  - Shell : liste d'arguments, executee SANS interpreteur ; `{nom}` est
    remplace dans un argument (une valeur ne peut donc ni ajouter un
    argument, ni enchainer une commande).
  - HTTP : `env:NOM` lit un secret dans l'environnement de la garde, jamais
    dans le fichier ni dans la requete de l'agent.

Limite : la garde ne protege que ce qu'elle seule peut faire. Si l'agent a
aussi le mot de passe de la base ou un shell avec les memes droits, il peut
la contourner : retirer ces acces a l'agent fait partie de l'installation.
Test en direct : test_gate_live.py.
"""
import argparse
import json
import os
import re
import sqlite3
import subprocess
import sys
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

SDK = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK))
from cstl_a2a import HEADER, decode_token  # noqa: E402  (meme encodage de jeton que le proxy A2A)

WORD = re.compile(r"[A-Za-z0-9_.][A-Za-z0-9_.-]{0,63}")
SQL_PARAM = re.compile(r"(?<![:\w]):([A-Za-z_]\w*)")


class BadRequest(Exception):
    pass


def check_param(name, kind, value):
    """Valeur validee (en texte pour SQL/shell/HTTP) ou BadRequest."""
    if kind == "int":
        if isinstance(value, bool) or not isinstance(value, int):
            if not (isinstance(value, str) and re.fullmatch(r"-?\d{1,18}", value)):
                raise BadRequest(f"{name} : entier attendu")
        return str(int(value))
    if kind == "num":
        try:
            if isinstance(value, bool):
                raise ValueError
            f = float(value)
            if f != f or f in (float("inf"), float("-inf")):
                raise ValueError
        except (TypeError, ValueError):
            raise BadRequest(f"{name} : nombre attendu") from None
        return repr(f) if isinstance(value, float) else str(value)
    if kind == "bool":
        if not isinstance(value, bool):
            raise BadRequest(f"{name} : booleen attendu")
        return "true" if value else "false"
    if not isinstance(value, str):
        raise BadRequest(f"{name} : texte attendu")
    if kind == "word":
        if not WORD.fullmatch(value):
            raise BadRequest(f"{name} : mot attendu ([A-Za-z0-9_.-], 64 max, sans tiret au debut)")
        return value
    if kind == "text":
        if len(value) > 1000 or any(c in value for c in "\r\n\x00"):
            raise BadRequest(f"{name} : texte d'une ligne, 1000 caracteres max")
        return value
    if isinstance(kind, dict) and "enum" in kind:
        if value not in kind["enum"]:
            raise BadRequest(f"{name} : une valeur parmi {kind['enum']}")
        return value
    if isinstance(kind, dict) and "regex" in kind:
        if not re.fullmatch(kind["regex"], value) or "\x00" in value:
            raise BadRequest(f"{name} : ne correspond pas a {kind['regex']}")
        return value
    raise BadRequest(f"{name} : type inconnu {kind!r}")


def validate(op, params):
    if not isinstance(params, dict):
        raise BadRequest("params : un objet est attendu")
    spec = op.get("params", {})
    extra, missing = set(params) - set(spec), set(spec) - set(params)
    if extra or missing:
        raise BadRequest(f"parametres en trop {sorted(extra)} / manquants {sorted(missing)}")
    return {k: check_param(k, spec[k], params[k]) for k in spec}


def fill(template, values):
    """`{nom}` -> valeur, dans une chaine ou recursivement dans du JSON."""
    if isinstance(template, str):
        return re.sub(r"\{([A-Za-z_]\w*)\}", lambda m: values[m.group(1)], template)
    if isinstance(template, list):
        return [fill(x, values) for x in template]
    if isinstance(template, dict):
        return {k: fill(v, values) for k, v in template.items()}
    return template


def typed(spec, values):
    """Valeurs validees -> types SQL (SQLite lie des valeurs typees)."""
    conv = {"int": int, "num": float, "bool": lambda v: 1 if v == "true" else 0}
    return {k: conv.get(spec[k], str)(v) if isinstance(spec[k], str) else v for k, v in values.items()}


def run_sql(db, sql, values, spec):
    if db["driver"] == "sqlite":
        con = sqlite3.connect(db["path"])
        try:
            cur = con.execute(sql, typed(spec, values))
            rows = [list(r) for r in cur.fetchall()] if cur.description else []
            con.commit()
            return {"rows": rows, "rowcount": cur.rowcount}
        finally:
            con.close()
    if db["driver"] == "postgres":
        script = SQL_PARAM.sub(lambda m: f":'{m.group(1)}'", sql)
        args = ["psql", "--no-psqlrc", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1", "-d", db["dsn"]]
        for k, v in values.items():
            args += ["-v", f"{k}={v}"]
        env = dict(os.environ)
        if db.get("password_env"):
            env["PGPASSWORD"] = os.environ.get(db["password_env"], "")
        p = subprocess.run(args, input=script + ";\n", capture_output=True, text=True, timeout=120, env=env)
        if p.returncode != 0:
            raise RuntimeError(p.stderr.strip()[:500])
        return {"rows": [line.split("|") for line in p.stdout.splitlines() if line]}
    raise RuntimeError(f"pilote inconnu {db['driver']!r}")


def execute(config, op, values):
    kind = op["type"]
    if kind == "sql":
        return run_sql(config["databases"][op["db"]], op["sql"], values, op.get("params", {}))
    if kind == "shell":
        argv = fill(op["argv"], values)
        p = subprocess.run(argv, capture_output=True, text=True, timeout=op.get("timeout", 300), shell=False)
        return {"exit_code": p.returncode, "stdout": p.stdout[-20000:], "stderr": p.stderr[-5000:]}
    if kind == "http":
        headers = {k: (os.environ.get(v[4:], "") if v.startswith("env:") else v) for k, v in op.get("headers", {}).items()}
        body = None
        if "json" in op:
            body = json.dumps(fill(op["json"], values)).encode()
            headers.setdefault("Content-Type", "application/json")
        req = urllib.request.Request(fill(op["url"], {k: urllib.request.quote(v, safe="") for k, v in values.items()}),
                                     data=body, headers=headers, method=op.get("method", "POST"))
        try:
            with urllib.request.urlopen(req, timeout=op.get("timeout", 60)) as r:
                return {"status": r.status, "body": r.read(20000).decode(errors="replace")}
        except urllib.error.HTTPError as e:
            return {"status": e.code, "body": e.read(20000).decode(errors="replace")}
    raise RuntimeError(f"type d'operation inconnu {kind!r}")


def load_config(path):
    config = json.loads(Path(path).read_text())
    for name, op in config.get("operations", {}).items():
        if op.get("type") not in ("sql", "shell", "http"):
            raise SystemExit(f"operation {name} : type sql, shell ou http attendu")
        op.setdefault("action", name)
        if not re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", op["action"]):
            raise SystemExit(f"operation {name} : action hors de [A-Za-z0-9_.-]")
        if op["type"] == "sql" and op.get("db") not in config.get("databases", {}):
            raise SystemExit(f"operation {name} : base {op.get('db')!r} inconnue")
        if op["type"] == "sql":
            used = set(SQL_PARAM.findall(op["sql"]))
            if used != set(op.get("params", {})):
                raise SystemExit(f"operation {name} : marqueurs SQL {sorted(used)} != parametres {sorted(op.get('params', {}))}")
        if op["type"] == "shell" and not (isinstance(op.get("argv"), list) and op["argv"]):
            raise SystemExit(f"operation {name} : argv (liste) attendu")
        if op.get("amount_param") and op["amount_param"] not in op.get("params", {}):
            raise SystemExit(f"operation {name} : amount_param absent des parametres")
    return config


def make_handler(config, guard):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def _send(self, status, obj):
            data = json.dumps(obj, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path != "/v1/operations":
                return self._send(404, {"error": "not_found"})
            self._send(200, {"operations": {n: {"action": o["action"], "type": o["type"], "params": o.get("params", {}),
                                                "amount_param": o.get("amount_param")}
                                            for n, o in config["operations"].items()}})

        def do_POST(self):
            if self.path != "/v1/run":
                return self._send(404, {"error": "not_found"})
            try:
                req = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)))
                name, params = req["operation"], req.get("params", {})
            except (ValueError, KeyError, TypeError):
                return self._send(400, {"error": "requete_invalide", "detail": 'attendu {"operation": ..., "params": {...}}'})
            op = config["operations"].get(name) if isinstance(name, str) else None
            if op is None:
                return self._send(404, {"error": "operation_inconnue", "detail": str(name)[:100]})
            raw = self.headers.get(HEADER)
            if not raw:
                return self._send(401, {"error": "no_token", "detail": f"en-tete {HEADER} absent"})
            try:
                values = validate(op, params)
            except BadRequest as e:
                return self._send(400, {"error": "parametre_invalide", "detail": str(e)})
            from cstl_guard import Unauthorized
            try:
                token = decode_token(raw)
            except Exception:  # noqa: BLE001
                return self._send(403, {"error": "bad_token"})
            if op.get("agents") and token.agent not in op["agents"]:
                return self._send(403, {"error": "agent_non_permis", "detail": token.agent})
            amount = params.get(op["amount_param"]) if op.get("amount_param") else None
            try:
                guard.check(token, op["action"], None, amount)
                guard._consume(token)                       # usage unique, AVANT l'execution
            except Unauthorized as e:
                return self._send(403, {"error": e.reason, "action": op["action"]})
            try:
                result = execute(config, op, values)
            except Exception as e:  # noqa: BLE001
                return self._send(500, {"error": "execution_echouee", "detail": str(e)[:500]})
            self._send(200, {"ok": True, "operation": name, "agent": token.agent, "audit_hash": token.audit_hash,
                             "result": result})

    return Handler


def serve(listen, config, server_key, replay_db=None):
    from cstl_guard import CstlGuard
    host, port = listen.rsplit(":", 1)
    return ThreadingHTTPServer((host, int(port)), make_handler(config, CstlGuard(server_key, replay_db=replay_db)))


def call(agent, url, operation, params, *, action=None, amount=None, timeout=300):
    """Cote agent : jeton pour l'action de l'operation, puis execution par la
    garde. Retourne (statut HTTP, reponse JSON). ActionDenied si refuse."""
    from cstl_a2a import encode_token
    token = agent.authorize(action or operation, amount=amount)
    req = urllib.request.Request(url.rstrip("/") + "/v1/run", method="POST",
                                 data=json.dumps({"operation": operation, "params": params}).encode(),
                                 headers={"Content-Type": "application/json", HEADER: encode_token(token)})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def main(argv=None):
    ap = argparse.ArgumentParser(description="CSTL : la garde qui execute (SQL, shell, HTTP)")
    ap.add_argument("--config", required=True)
    ap.add_argument("--listen", default="127.0.0.1:9300")
    ap.add_argument("--server-key", default=os.environ.get("CSTL_SERVER_PUBLIC_KEY"),
                    help="cle publique du serveur CSTL (hex ou fichier .pub) -- obligatoire")
    ap.add_argument("--replay-db", help="fichier SQLite des jetons deja utilises (survit a un redemarrage)")
    args = ap.parse_args(argv)
    if not args.server_key:
        raise SystemExit("--server-key obligatoire")
    key = Path(args.server_key).read_text().strip() if Path(args.server_key).is_file() else args.server_key.strip()
    config = load_config(args.config)
    httpd = serve(args.listen, config, key, args.replay_db)
    print(f"cstl-gate : {args.listen}, {len(config['operations'])} operation(s)", file=sys.stderr)
    httpd.serve_forever()


if __name__ == "__main__":
    main()
