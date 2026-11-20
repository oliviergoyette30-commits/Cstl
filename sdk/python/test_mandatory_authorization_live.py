#!/usr/bin/env python3
"""
test_mandatory_authorization_live.py -- couche 9 OBLIGATOIRE, contre le VRAI
serveur (2026-11-20).

Le systeme vise (ici une fausse base de donnees) n'execute une action que sur
presentation d'un jeton signe par la cle du serveur CSTL. Prouve : un agent
qui ne demande pas, qui falsifie, qui reutilise ou qui detourne un jeton est
bloque par le systeme vise lui-meme. Cle serveur lue dans le fichier
`.pub` (epinglage), jamais dans la reponse. Saute si target/debug/cstl_parser
n'est pas compile.
"""
import dataclasses
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
PORT, REST = 5757, 8787


def start(workdir):
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST)}
    proc = subprocess.Popen([str(BINARY)], cwd=workdir, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            return proc
        except OSError:
            time.sleep(0.25)
    proc.terminate()
    pytest.fail("serveur injoignable")


def stop(proc):
    proc.terminate()
    proc.wait(timeout=10)


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    workdir = Path(tempfile.mkdtemp(prefix="cstl_mandatory_"))
    proc = start(workdir)
    yield workdir
    stop(proc)


def pinned_key(workdir):
    return (workdir / "cstl_adn.db.server_key.pub").read_text().strip()


def make_agent(monkeypatch, tmp_path):
    import cstl_llm_agent as m

    class Stub(m.LLMProvider):
        def generate(self, prompt, max_tokens=500):
            return ""

    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(m.HermesAgentBrain, "from_env", classmethod(lambda cls: Stub()))
    a = m.CstlAgent(f"bot_{uuid.uuid4().hex[:10]}", "hermes", port=PORT)
    assert a.register()
    a.send_relations("server", [
        {"type": "PERFORM", "subject": a.name, "object": "wipe_db", "modality": "MUST_NOT"},
        {"type": "PERFORM", "subject": a.name, "object": "backup_db", "modality": "MUST"},
    ])
    return m, a


class FakeDatabase:
    """Le systeme vise : il n'agit QUE sur jeton valide."""

    def __init__(self, guard):
        self.guard, self.log = guard, []

    def backup(self, token):
        return self.guard.run(token, "backup_db", lambda: self.log.append("backup") or "ok")

    def wipe(self, token):
        return self.guard.run(token, "wipe_db", lambda: self.log.append("WIPE"))


def test_allowed_action_runs_once_and_only_once(server, monkeypatch, tmp_path):
    from cstl_guard import CstlGuard, Unauthorized
    _, bot = make_agent(monkeypatch, tmp_path)
    db = FakeDatabase(CstlGuard(pinned_key(server)))
    token = bot.authorize("backup_db")
    assert db.backup(token) == "ok" and db.log == ["backup"]
    with pytest.raises(Unauthorized, match="already_used"):
        db.backup(token)
    assert db.log == ["backup"]


def test_forbidden_or_unrequested_action_never_runs(server, monkeypatch, tmp_path):
    from cstl_guard import CstlGuard, Unauthorized
    m, bot = make_agent(monkeypatch, tmp_path)
    db = FakeDatabase(CstlGuard(pinned_key(server)))
    with pytest.raises(m.ActionDenied):
        bot.authorize("wipe_db")
    # L'agent passe outre et attaque la base directement : sans jeton, rien.
    with pytest.raises(Unauthorized, match="no_token"):
        db.wipe(None)
    # Il detourne un jeton valide obtenu pour une AUTRE action.
    with pytest.raises(Unauthorized, match="wrong_action"):
        db.wipe(bot.authorize("backup_db"))
    # Il falsifie le jeton (action reecrite) : la signature ne tient plus.
    forged = dataclasses.replace(bot.authorize("backup_db"), action="wipe_db")
    with pytest.raises(Unauthorized, match="bad_signature"):
        db.wipe(forged)
    assert db.log == []


def test_token_from_another_key_or_expired_is_refused(server, monkeypatch, tmp_path):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    from cstl_guard import CstlGuard, Unauthorized
    _, bot = make_agent(monkeypatch, tmp_path)
    token = bot.authorize("backup_db")
    # Faux serveur : un systeme qui epinglerait une autre cle refuse le jeton.
    other = Ed25519PrivateKey.generate().public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()
    with pytest.raises(Unauthorized, match="bad_signature"):
        CstlGuard(other).run(token, "backup_db", lambda: None)
    # Jeton presente apres expiration.
    late = CstlGuard(pinned_key(server), clock=lambda: token.expires_at + 3600)
    with pytest.raises(Unauthorized, match="expired"):
        late.run(token, "backup_db", lambda: None)
    # Et un agent ne peut pas utiliser le jeton d'un autre.
    with pytest.raises(Unauthorized, match="wrong_agent"):
        CstlGuard(pinned_key(server)).run(token, "backup_db", lambda: None, agent="someone_else")


def test_replay_registry_survives_a_restart_of_the_target(server, monkeypatch, tmp_path):
    from cstl_guard import CstlGuard, Unauthorized
    _, bot = make_agent(monkeypatch, tmp_path)
    token = bot.authorize("backup_db")
    replay = str(tmp_path / "used.db")
    assert CstlGuard(pinned_key(server), replay_db=replay).run(token, "backup_db", lambda: "ok") == "ok"
    with pytest.raises(Unauthorized, match="already_used"):
        CstlGuard(pinned_key(server), replay_db=replay).run(token, "backup_db", lambda: "ok")


def test_server_key_is_stable_across_server_restarts(server):
    # Les systemes vises epinglent la cle : un redemarrage ne doit pas la changer.
    workdir = Path(tempfile.mkdtemp(prefix="cstl_mandatory_restart_"))
    sock_free = True
    try:
        socket.create_connection(("127.0.0.1", PORT + 1), timeout=0.3).close()
        sock_free = False
    except OSError:
        pass
    if not sock_free:
        pytest.skip("port occupe")
    env = {**os.environ, "CSTL_PORT": str(PORT + 1), "CSTL_REST_PORT": str(REST + 1)}
    keys = []
    for _ in range(2):
        proc = subprocess.Popen([str(BINARY)], cwd=workdir, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", PORT + 1), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        keys.append(pinned_key(workdir))
        stop(proc)
    assert keys[0] == keys[1] and len(keys[0]) == 64
    assert oct((workdir / "cstl_adn.db.server_key").stat().st_mode & 0o777) == "0o600"
