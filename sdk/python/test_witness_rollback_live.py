#!/usr/bin/env python3
"""
test_witness_rollback_live.py -- retour en arriere complet, contre le VRAI
serveur et le VRAI temoin (2026-11-20).

Scenario : le serveur ecrit 1 maillon, on copie la base ET son marqueur
signe (la sauvegarde d'un attaquant), il en ecrit 2 de plus, puis on remet
la copie. Sans temoin, le serveur redemarre sur l'etat ancien sans rien voir
(le trou, fige ici). Avec temoin, il refuse de demarrer. Le temoin lui-meme
refuse aussi qu'on le fasse reculer. Saute si target/debug/cstl_parser n'est
pas compile.
"""
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
PORT, REST, WPORT = 5959, 8989, 8191
FILES = ("cstl_adn.db", "cstl_adn.db.server_key.audit_since")


def wait_port(port, proc=None, tries=120):
    for _ in range(tries):
        if proc is not None and proc.poll() is not None:
            return False
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            return True
        except OSError:
            time.sleep(0.25)
    return False


def start_server(workdir, witness=True):
    # Messages de demonstration non signes (alice) : mode developpement.
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_DEV_MODE": "1"}
    env.pop("CSTL_WITNESS_URL", None)
    if witness:
        env["CSTL_WITNESS_URL"] = f"http://127.0.0.1:{WPORT}"
    log = open(workdir / "server.log", "a")
    proc = subprocess.Popen([str(BINARY)], cwd=workdir, env=env, stdout=log, stderr=subprocess.STDOUT)
    return proc, wait_port(PORT, proc)


def stop(proc):
    proc.terminate()
    proc.wait(timeout=10)


def send(i):
    sys.path.insert(0, str(SDK))
    from cstl_client import CstlClient
    c = CstlClient(port=PORT, timeout=20.0)
    resp = c.send_raw(c.build_payload(encoder="t", produced_by="t", purpose="inform", sender="alice", receiver="server",
                                      relations=[{"type": "STATE", "subject": f"fact_{i}", "object": "ok"}]))
    assert resp.audit_seq is not None, resp.raw


def witnessed_seq(key):
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{WPORT}/checkpoint/{key}", timeout=3) as r:
            return json.load(r)["seq"]
    except urllib.error.HTTPError:
        return None


def run_scenario(workdir, witness):
    proc, up = start_server(workdir, witness)
    assert up, (workdir / "server.log").read_text()[-2000:]
    try:
        send(0)
    finally:
        stop(proc)
    snapshot = workdir / "snapshot"
    snapshot.mkdir()
    for f in FILES:
        shutil.copy2(workdir / f, snapshot / f)
    proc, up = start_server(workdir, witness)
    assert up, (workdir / "server.log").read_text()[-2000:]
    try:
        send(1)
        send(2)
        key = (workdir / "cstl_adn.db.server_key.pub").read_text().strip()
        if witness:
            for _ in range(40):
                if witnessed_seq(key) == 2:
                    break
                time.sleep(0.25)
            assert witnessed_seq(key) == 2
    finally:
        stop(proc)
    for f in FILES:  # l'attaquant remet sa copie
        shutil.copy2(snapshot / f, workdir / f)
    proc, up = start_server(workdir, witness)
    return proc, up, key


@pytest.fixture
def witness(tmp_path):
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    for port in (PORT, WPORT):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.3).close()
            pytest.fail(f"port {port} deja occupe")
        except OSError:
            pass
    proc = subprocess.Popen([sys.executable, str(SDK / "cstl_witness.py"), "--port", str(WPORT),
                             "--store", str(tmp_path / "witness.json"), "--trust-first-key"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    assert wait_port(WPORT, proc)
    yield tmp_path
    stop(proc)


def test_without_witness_a_restored_copy_goes_unnoticed():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    workdir = Path(tempfile.mkdtemp(prefix="cstl_rollback_nowit_"))
    proc, up, _ = run_scenario(workdir, witness=False)
    assert up, "sans temoin, l'etat ancien redemarre sans alerte (limite figee)"
    stop(proc)


def test_with_witness_a_restored_copy_is_refused(witness):
    workdir = Path(tempfile.mkdtemp(prefix="cstl_rollback_wit_"))
    proc, up, key = run_scenario(workdir, witness=True)
    if up:
        stop(proc)
    assert not up and proc.wait(timeout=10) != 0
    log = (workdir / "server.log").read_text()
    assert "retour en arriere" in log and "seq=2" in log, log[-2000:]
    # Le temoin, lui, n'a pas recule.
    assert witnessed_seq(key) == 2


def test_witness_refuses_to_move_backwards_or_fork(witness):
    sys.path.insert(0, str(SDK))
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    from cstl_witness import audit_message
    priv = Ed25519PrivateKey.generate()
    key = priv.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()

    def post(seq, h, sign_with=priv):
        cp = {"server_key": key, "seq": seq, "hash": h, "parent_hash": "root", "sender": "a", "receiver": "b", "purpose": "inform"}
        cp["signature"] = sign_with.sign(audit_message(cp)).hex()
        req = urllib.request.Request(f"http://127.0.0.1:{WPORT}/checkpoint", data=json.dumps(cp).encode(),
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=3) as r:
                return r.status
        except urllib.error.HTTPError as e:
            return e.code

    assert post(5, "h5") == 200          # premiere cle : epinglee (--trust-first-key)
    assert post(3, "h3") == 409          # recul refuse
    assert post(5, "autre") == 409       # fourche refusee
    assert post(6, "h6", Ed25519PrivateKey.generate()) == 403   # mauvaise signature
    assert post(7, "h7") == 200 and witnessed_seq(key) == 7
