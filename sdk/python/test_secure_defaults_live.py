#!/usr/bin/env python3
"""
test_secure_defaults_live.py -- securite PAR DEFAUT, contre le VRAI binaire
(2026-11-21).

Verifie, sans aucun reglage :
- un message non signe est refuse (avant : accepte sous n'importe quel nom) ;
- apres un redemarrage, un agent garde son identite : son nom ne peut pas
  etre repris avec une autre cle (avant : main.rs remplacait le registre
  recharge depuis la base, et l'usurpation passait -- verifie en direct) ;
- le mode developpement (CSTL_DEV_MODE=1) est refuse hors ecoute locale ;
- `--rotate-server-key` change la cle du serveur sans casser l'historique.
Saute si target/debug/cstl_parser n'est pas compile.
"""
import os
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT, REST = 5969, 8969
BASE_ENV = {k: v for k, v in os.environ.items() if k not in ("CSTL_DEV_MODE", "CSTL_REQUIRE_SIGNATURES", "CSTL_BIND_HOST")}


def start(workdir, extra=None):
    env = {**BASE_ENV, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), **(extra or {})}
    log = open(workdir / "server.log", "a")
    proc = subprocess.Popen([str(BINARY)], cwd=workdir, env=env, stdout=log, stderr=subprocess.STDOUT)
    for _ in range(120):
        if proc.poll() is not None:
            return proc, False
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.3).close()
            return proc, True
        except OSError:
            time.sleep(0.25)
    return proc, False


def stop(proc):
    proc.terminate()
    proc.wait(timeout=10)


@pytest.fixture
def workdir():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    try:
        socket.create_connection(("127.0.0.1", PORT), timeout=0.3).close()
        pytest.fail(f"port {PORT} deja occupe")
    except OSError:
        pass
    return Path(tempfile.mkdtemp(prefix="cstl_secure_"))


def agent(monkeypatch, home, name):
    import cstl_llm_agent as m

    class Stub(m.LLMProvider):
        def generate(self, prompt, max_tokens=500):
            return ""

    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(m.HermesAgentBrain, "from_env", classmethod(lambda cls: Stub()))
    return m.CstlAgent(name, "hermes", port=PORT)


def test_unsigned_message_is_refused_by_default(workdir):
    from cstl_client import CstlClient
    proc, up = start(workdir)
    assert up
    try:
        c = CstlClient(port=PORT)
        r = c.send_raw(c.build_payload(encoder="t", produced_by="t", purpose="inform", sender="alice", receiver="server",
                                       relations=[{"type": "STATE", "subject": "x", "object": "y"}]))
        assert r.purpose == "signature_rejected", r.raw
    finally:
        stop(proc)


def test_identity_survives_restart_and_cannot_be_taken(workdir, monkeypatch, tmp_path):
    name = f"victim_{uuid.uuid4().hex[:8]}"
    proc, up = start(workdir)
    assert up
    try:
        assert agent(monkeypatch, tmp_path / "owner", name).register()
    finally:
        stop(proc)
    proc, up = start(workdir)
    assert up
    try:
        assert not agent(monkeypatch, tmp_path / "thief", name).register(), "usurpation apres redemarrage"
        assert agent(monkeypatch, tmp_path / "owner", name).register(), "le vrai detenteur est toujours reconnu"
    finally:
        stop(proc)


def test_dev_mode_is_refused_outside_localhost(workdir):
    proc, up = start(workdir, {"CSTL_DEV_MODE": "1", "CSTL_BIND_HOST": "0.0.0.0"})
    if up:
        stop(proc)
    assert not up and proc.wait(timeout=10) != 0
    assert "CSTL_DEV_MODE n'est autorise qu'en ecoute locale" in (workdir / "server.log").read_text()


def test_server_key_rotation_keeps_the_history(workdir, monkeypatch, tmp_path):
    name = f"bot_{uuid.uuid4().hex[:8]}"
    proc, up = start(workdir)
    assert up
    try:
        bot = agent(monkeypatch, tmp_path / "bot", name)
        assert bot.register()
        assert not bot.send_relations("server", [{"type": "STATE", "subject": "cstl", "object": "alive"}]).is_error
    finally:
        stop(proc)
    old_key = (workdir / "cstl_adn.db.server_key.pub").read_text().strip()
    rot = subprocess.run([str(BINARY), "--rotate-server-key"], cwd=workdir, env=BASE_ENV, capture_output=True, text=True, timeout=60)
    assert rot.returncode == 0, rot.stderr[-1500:]
    new_key = (workdir / "cstl_adn.db.server_key.pub").read_text().strip()
    assert new_key != old_key and len(new_key) == 64
    # Le serveur redemarre : l'historique signe par l'ancienne cle se verifie.
    proc, up = start(workdir, {"CSTL_SERVER_PUBLIC_KEY": new_key})
    assert up, (workdir / "server.log").read_text()[-2000:]
    try:
        bot = agent(monkeypatch, tmp_path / "bot", name)
        resp = bot.send_relations("server", [{"type": "STATE", "subject": "cstl", "object": "rotated"}])
        assert not resp.is_error, resp.raw
        token = bot.authorize("read_logs")
        from cstl_guard import CstlGuard, Unauthorized
        assert CstlGuard(new_key).run(token, "read_logs", lambda: "ok") == "ok"
        with pytest.raises(Unauthorized, match="bad_signature"):
            CstlGuard(old_key).run(bot.authorize("read_logs"), "read_logs", lambda: "ok")
    finally:
        stop(proc)
    # Ancienne cle epinglee : refus de demarrer.
    proc, up = start(workdir, {"CSTL_SERVER_PUBLIC_KEY": old_key})
    if up:
        stop(proc)
    assert not up
