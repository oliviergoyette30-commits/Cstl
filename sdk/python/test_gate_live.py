#!/usr/bin/env python3
"""
test_gate_live.py -- cstl_gate.py (la garde qui execute) contre le VRAI
serveur CSTL, avec une vraie base SQLite, un vrai PostgreSQL (s'il est
installe), une commande et une API HTTP de test (2026-11-25).

Ce qui est prouve : une operation n'est executee qu'avec un jeton valide
pour SON action, SON montant et un agent permis ; un parametre mal type
(tentative d'injection SQL ou shell) est refuse avant toute execution ; un
secret HTTP vient de l'environnement de la garde, jamais de l'agent.
Saute si target/debug/cstl_parser n'est pas compile ; la partie PostgreSQL
saute si initdb n'est pas disponible.
"""
import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT, REST = 6363, 9393
PG_BIN = max(Path("/usr/lib/postgresql").glob("*/bin"), default=Path("/nonexistent"),
             key=lambda p: int(p.parent.name) if p.parent.name.isdigit() else 0)
HOOK = []


class Hook(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        HOOK.append((self.headers.get("Authorization"), json.loads(self.rfile.read(int(self.headers["Content-Length"])))))
        self.send_response(204)
        self.end_headers()


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def start_postgres(work):
    """PostgreSQL jetable (utilisateur postgres, socket dans work). None si impossible."""
    if not (PG_BIN / "initdb").exists() or shutil.which("runuser") is None:
        return None
    # Dossier propre sous /tmp : l'utilisateur postgres doit pouvoir traverser ses parents.
    work = Path(tempfile.mkdtemp(prefix="cstl_pg_", dir="/tmp"))
    os.chmod(work, 0o755)
    data, sock = work / "pgdata", work / "pgsock"
    sock.mkdir()
    shutil.chown(sock, "postgres")
    as_pg = lambda *cmd: subprocess.run(["runuser", "-u", "postgres", "--", *cmd], capture_output=True, text=True)  # noqa: E731
    data.mkdir()
    shutil.chown(data, "postgres")
    if as_pg(str(PG_BIN / "initdb"), "-D", str(data), "-A", "trust", "-U", "postgres").returncode != 0:
        return None
    port = free_port()
    r = as_pg(str(PG_BIN / "pg_ctl"), "-D", str(data), "-w", "-l", str(sock / "pg.log"),
              "-o", f"-k {sock} -p {port} -c listen_addresses=''", "start")
    if r.returncode != 0:
        return None
    dsn = f"host={sock} port={port} dbname=postgres user=postgres"
    subprocess.run(["psql", "-X", "-q", "-d", dsn, "-c", "CREATE TABLE notes (author text, body text)"], check=True)
    def stop():
        as_pg(str(PG_BIN / "pg_ctl"), "-D", str(data), "-m", "fast", "stop")
        shutil.rmtree(work, ignore_errors=True)
    return {"dsn": dsn, "stop": stop}


@pytest.fixture(scope="module")
def gate():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_gate_"))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_KB_VERIFY": "off"}
    env.pop("CSTL_DEV_MODE", None)
    env.pop("CSTL_POLICY_AUTHORS", None)
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    db = work / "billing.db"
    con = sqlite3.connect(db)
    con.execute("CREATE TABLE orders (id INTEGER PRIMARY KEY, refunded REAL DEFAULT 0)")
    con.executemany("INSERT INTO orders (id) VALUES (?)", [(1,), (2,), (42,)])
    con.commit()
    con.close()
    hook_port = free_port()
    hook = ThreadingHTTPServer(("127.0.0.1", hook_port), Hook)
    threading.Thread(target=hook.serve_forever, daemon=True).start()
    pg = start_postgres(work)
    config = {
        "databases": {"billing": {"driver": "sqlite", "path": str(db)}},
        "operations": {
            "refund_order": {"type": "sql", "db": "billing", "amount_param": "amount",
                             "sql": "UPDATE orders SET refunded = :amount WHERE id = :order_id",
                             "params": {"order_id": "int", "amount": "num"}},
            "list_refunds": {"type": "sql", "db": "billing", "sql": "SELECT id, refunded FROM orders ORDER BY id", "params": {}},
            "backup": {"type": "shell", "argv": [sys.executable, "-c", "import sys; print('sauvegarde', sys.argv[1])", "{name}"],
                       "params": {"name": "word"}},
            "notify": {"type": "http", "url": f"http://127.0.0.1:{hook_port}/hook",
                       "headers": {"Authorization": "env:GATE_TEST_TOKEN"}, "json": {"text": "{message}"},
                       "params": {"message": "text"}},
            "wipe": {"type": "shell", "argv": [sys.executable, "-c", "print('efface')"], "agents": ["ops_only"]},
        },
    }
    if pg:
        config["databases"]["pg"] = {"driver": "postgres", "dsn": pg["dsn"]}
        config["operations"]["add_note"] = {"type": "sql", "db": "pg", "sql": "INSERT INTO notes VALUES (:author, :body)",
                                            "params": {"author": "word", "body": "text"}}
        config["operations"]["list_notes"] = {"type": "sql", "db": "pg", "sql": "SELECT author, body FROM notes", "params": {}}
    (work / "gate.json").write_text(json.dumps(config))
    os.environ["GATE_TEST_TOKEN"] = "Bearer secret-de-la-garde"
    import cstl_gate
    gport = free_port()
    httpd = cstl_gate.serve(f"127.0.0.1:{gport}", cstl_gate.load_config(work / "gate.json"),
                            (work / "cstl_adn.db.server_key.pub").read_text().strip())
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    yield {"url": f"http://127.0.0.1:{gport}", "work": work, "db": db, "pg": pg}
    httpd.shutdown()
    hook.shutdown()
    if pg:
        pg["stop"]()
    proc.terminate()
    proc.wait(timeout=10)


def agents(g, monkeypatch):
    monkeypatch.setenv("HOME", str(g["work"]))
    import cstl_llm_agent as m
    owner = m.CstlAgent(f"owner_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    bot = m.CstlAgent(f"bot_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    assert owner.register() and bot.register()
    owner.send_relations("server", [{"type": "PERFORM", "subject": bot.name, "object": "refund_order",
                                     "modality": "MUST_NOT", "amount_above": "100"}])
    return m, bot


def refunded(g):
    con = sqlite3.connect(g["db"])
    try:
        return dict(con.execute("SELECT id, refunded FROM orders").fetchall())
    finally:
        con.close()


def raw_post(url, body, token=None):
    import urllib.request
    import urllib.error
    from cstl_a2a import HEADER, encode_token
    headers = {"Content-Type": "application/json", **({HEADER: encode_token(token)} if token else {})}
    req = urllib.request.Request(url + "/v1/run", data=json.dumps(body).encode(), headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def test_sql_only_with_the_right_token_and_amount(gate, monkeypatch):
    import cstl_gate
    m, bot = agents(gate, monkeypatch)
    status, r = cstl_gate.call(bot, gate["url"], "refund_order", {"order_id": 42, "amount": 50}, amount=50)
    assert status == 200 and r["result"]["rowcount"] == 1 and r["agent"] == bot.name
    assert refunded(gate)[42] == 50

    with pytest.raises(m.ActionDenied):                     # au-dela du plafond : pas de jeton
        cstl_gate.call(bot, gate["url"], "refund_order", {"order_id": 1, "amount": 500}, amount=500)

    tok = bot.authorize("refund_order", amount=50)          # jeton de 50, execution demandee a 5000
    assert raw_post(gate["url"], {"operation": "refund_order", "params": {"order_id": 2, "amount": 5000}}, tok) == \
        (403, {"error": "wrong_amount", "action": "refund_order"})
    assert raw_post(gate["url"], {"operation": "refund_order", "params": {"order_id": 2, "amount": 50}}, tok)[0] == 200
    assert raw_post(gate["url"], {"operation": "refund_order", "params": {"order_id": 2, "amount": 50}}, tok)[1]["error"] == "already_used"

    tok = bot.authorize("list_refunds")                     # jeton d'une autre operation
    assert raw_post(gate["url"], {"operation": "refund_order", "params": {"order_id": 1, "amount": 1}}, tok)[1]["error"] == "wrong_action"
    assert raw_post(gate["url"], {"operation": "refund_order", "params": {"order_id": 1, "amount": 1}})[0] == 401
    assert refunded(gate) == {1: 0, 2: 50, 42: 50}          # rien d'autre n'a ete execute


def test_injections_and_bad_params_are_refused_before_execution(gate, monkeypatch):
    import cstl_gate
    _, bot = agents(gate, monkeypatch)
    for op, params in [("refund_order", {"order_id": "1; DROP TABLE orders", "amount": 1}),
                       ("refund_order", {"order_id": 1}),
                       ("refund_order", {"order_id": 1, "amount": 1, "extra": "x"}),
                       ("backup", {"name": "x; rm -rf /"}),
                       ("backup", {"name": "-rf"}),
                       ("notify", {"message": "ligne1\nligne2"})]:
        # La demande de jeton porte un montant valide (1) : c'est la GARDE qui doit refuser les parametres.
        status, r = cstl_gate.call(bot, gate["url"], op, params, amount=1 if op == "refund_order" else None)
        assert status == 400 and r["error"] == "parametre_invalide", (op, params, r)
    assert raw_post(gate["url"], {"operation": "DROP TABLE orders", "params": {}})[0] == 404
    assert sqlite3.connect(gate["db"]).execute("SELECT count(*) FROM orders").fetchone()[0] == 3


def test_shell_http_and_agent_restriction(gate, monkeypatch):
    import cstl_gate
    _, bot = agents(gate, monkeypatch)
    status, r = cstl_gate.call(bot, gate["url"], "backup", {"name": "nuit_2026-11-25"})
    assert status == 200 and r["result"]["stdout"].strip() == "sauvegarde nuit_2026-11-25"

    HOOK.clear()
    status, r = cstl_gate.call(bot, gate["url"], "notify", {"message": 'remboursement "42" fait'})
    assert status == 200 and r["result"]["status"] == 204
    assert HOOK == [("Bearer secret-de-la-garde", {"text": 'remboursement "42" fait'})]   # secret de la garde

    status, r = cstl_gate.call(bot, gate["url"], "wipe", {})
    assert status == 403 and r["error"] == "agent_non_permis"


def test_postgres_values_are_quoted_by_psql(gate, monkeypatch):
    if not gate["pg"]:
        pytest.skip("PostgreSQL non disponible")
    import cstl_gate
    _, bot = agents(gate, monkeypatch)
    evil = "O'Brien'); DROP TABLE notes; --"
    status, r = cstl_gate.call(bot, gate["url"], "add_note", {"author": "bot", "body": evil})
    assert status == 200, r
    status, r = cstl_gate.call(bot, gate["url"], "list_notes", {})
    assert status == 200 and r["result"]["rows"] == [["bot", evil]]      # stocke tel quel, table intacte
