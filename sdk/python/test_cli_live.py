#!/usr/bin/env python3
"""
test_cli_live.py -- la commande `cstl`, cle en main, de bout en bout contre
le VRAI serveur (2026-11-27).

Un dossier vierge, un HOME vierge : `cstl init`, `cstl serve` (en
arriere-plan), `cstl policy apply`, `cstl agent add`, `cstl check` (permis,
plafond, interdit), `cstl policy show`, `cstl stop`, `cstl revoke`,
`cstl status`. Chaque commande est lancee comme un utilisateur la lancerait,
dans un sous-processus.

Saute si target/debug/cstl_parser n'est pas compile.
"""
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
PORT, REST = 5868, 8868


def free(port):
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", port)) != 0


@pytest.fixture(scope="module")
def project(tmp_path_factory):
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    if not free(PORT):
        pytest.skip(f"port {PORT} occupe")
    work = tmp_path_factory.mktemp("cstl_cli")
    home = work / "home"
    home.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith("CSTL_")}
    env.update({"HOME": str(home), "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST),
                "CSTL_KB_VERIFY": "off", "CSTL_BINARY": str(BINARY)})

    def cstl(*args, ok=True):
        r = subprocess.run([sys.executable, str(SDK / "cstl_cli.py"), *args], cwd=work, env=env,
                           capture_output=True, text=True, timeout=60)
        if ok is True:
            assert r.returncode == 0, (args, r.stdout, r.stderr)
        return r

    init = cstl("init")
    assert (work / "cstl.json").is_file() and (work / "policy.cstl").is_file()
    assert "owner_ops" in init.stdout
    server = subprocess.Popen([sys.executable, str(SDK / "cstl_cli.py"), "serve"], cwd=work, env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        if not free(PORT):
            break
        time.sleep(0.25)
    else:
        server.terminate()
        pytest.fail("cstl serve n'a pas demarre")
    yield work, cstl
    server.terminate()
    server.wait(timeout=10)


def test_turnkey_flow(project):
    work, cstl = project
    assert (work / "cstl-data" / "cstl_adn.db.server_key.pub").is_file(), "donnees du serveur dans ./cstl-data"

    st = cstl("status")
    assert "joignable" in st.stdout and "imposes (CSTL_POLICY_AUTHORS)" in st.stdout, st.stdout

    applied = cstl("policy", "apply", "policy.cstl")
    assert "3 relation(s) publiee(s) par owner_ops, en vigueur" in applied.stdout

    # Sans cle locale, `check` refuse au lieu d'enregistrer un nom par erreur.
    r = cstl("check", "agent_support", "refund_order", ok=None)
    assert r.returncode == 1 and "cstl agent add agent_support" in r.stderr

    assert "agent_support enregistre" in cstl("agent", "add", "agent_support").stdout
    r = cstl("check", "agent_support", "refund_order", "--amount", "80", ok=None)
    assert r.returncode == 0 and "AUTORISE" in r.stdout, r.stdout
    r = cstl("check", "agent_support", "refund_order", "--amount", "500", ok=None)
    assert r.returncode == 2 and "REFUSE" in r.stdout, r.stdout
    r = cstl("check", "agent_support", "wipe_db", ok=None)
    assert r.returncode == 2 and "REFUSE" in r.stdout

    show = cstl("policy", "show", "agent_support", "--format", "compact")
    assert "wipe_db" in show.stdout and "refund_order" in show.stdout, show.stdout

    assert "ACTIF" in cstl("stop", "action:refund_*", "--reason", "test").stdout
    r = cstl("check", "agent_support", "refund_order", "--amount", "80", ok=None)
    assert r.returncode == 2 and "emergency_stop" in r.stdout, r.stdout
    assert "leve" in cstl("stop", "action:refund_*", "--off").stdout
    assert cstl("check", "agent_support", "refund_order", "--amount", "80", ok=None).returncode == 0

    assert "revoquee" in cstl("revoke", "agent_support", "--reason", "test").stdout
    r = cstl("check", "agent_support", "refund_order", "--amount", "80", ok=None)
    assert r.returncode == 1 and "revoquee" in r.stderr, (r.stdout, r.stderr)


def test_a_bad_policy_line_stops_the_command(project):
    work, cstl = project
    (work / "bad.cstl").write_text("agent_support MAYBE refund_order\n")
    r = cstl("policy", "apply", "bad.cstl", ok=None)
    assert r.returncode == 1 and "bad.cstl" in r.stderr


def test_passthrough_commands_are_reachable(project):
    _, cstl = project
    for name in ("approve", "gate", "deliberate"):
        r = cstl(name, "--help", ok=None)
        assert r.returncode == 0 and "usage" in r.stdout.lower(), (name, r.stderr)
