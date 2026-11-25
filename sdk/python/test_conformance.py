#!/usr/bin/env python3
"""
test_conformance.py -- CSTL controle v1 (2026-11-25).

1. Le SDK Python donne exactement les vecteurs de conformance/vectors.json
   (octets canoniques, signatures, message et verdicts des jetons).
2. Le testeur boite noire (conformance/run_server_conformance.py), ecrit
   d'apres la specification seule, passe 13/13 contre le vrai serveur.
   Saute si target/debug/cstl_parser n'est pas compile.
"""
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
REPO = SDK.parent.parent
BINARY = REPO / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
import cstl_signing  # noqa: E402
from cstl_guard import AuthorizationToken, CstlGuard, Unauthorized  # noqa: E402

V = json.loads((REPO / "conformance" / "vectors.json").read_text())


def test_sdk_signing_matches_vectors():
    for case in V["signing"]:
        msg = case["message"]
        assert cstl_signing.signing_bytes(msg).decode() == case["canonical"], case["name"]
        assert cstl_signing.sign_intent(msg, V["agent_private_key"]) == msg["intent"]["signature"], case["name"]


def test_sdk_guard_matches_token_verdicts():
    t = V["token"]
    base = AuthorizationToken(signature=t["signature"], **t["fields"])
    assert base.message().decode() == t["message"]
    for c in t["verdicts"]:
        tok = AuthorizationToken(**{**base.__dict__, **c.get("token", {})})
        guard = CstlGuard(t["server_public_key"], skew=t["skew"], clock=lambda c=c: c["now"])
        try:
            guard.check(tok, c["action"], c["agent"], c["amount"])
            got = "ok"
        except Unauthorized as e:
            got = e.reason
        assert got == c["expect"], c["case"]


def test_black_box_runner_passes_against_the_server(tmp_path):
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    env = {**os.environ, "CSTL_PORT": "6060", "CSTL_REST_PORT": "9090", "CSTL_KB_VERIFY": "off"}
    for k in ("CSTL_DEV_MODE", "CSTL_POLICY_AUTHORS"):
        env.pop(k, None)
    proc = subprocess.Popen([str(BINARY)], cwd=tmp_path, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", 6060), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        r = subprocess.run([sys.executable, "-I", str(REPO / "conformance" / "run_server_conformance.py"),
                            "--port", "6060", "--server-key", str(tmp_path / "cstl_adn.db.server_key.pub")],
                           capture_output=True, text=True, timeout=120)
        assert r.returncode == 0, r.stdout + r.stderr
        assert "14/14 conformes" in r.stdout
    finally:
        proc.terminate()
        proc.wait(timeout=10)
