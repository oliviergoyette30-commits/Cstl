#!/usr/bin/env python3
"""
test_spiffe_live.py -- identite de charge de travail SPIFFE en TLS mutuel,
contre le VRAI serveur (2026-11-25). Le certificat client porte
spiffe://acme.org/sa/<nom> ; avec CSTL_SPIFFE_TRUST_DOMAIN=acme.org, la
connexion ne peut parler qu'au nom de <nom> :
  - le bon agent s'enregistre et agit ;
  - un autre agent, meme avec SA propre cle valide, est refuse sur cette
    connexion (spiffe_identity_mismatch) ;
  - un certificat d'un autre domaine de confiance est refuse ;
  - avec CSTL_SPIFFE_REQUIRED, un certificat sans identifiant SPIFFE aussi.
Saute si cstl_parser ou openssl manque.
"""
import contextlib
import io
import os
import shutil
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
PORT, REST = 5858, 8888


def openssl(d, *a):
    subprocess.run(["openssl", *a], cwd=d, check=True, capture_output=True)


def issue(d, name, cn, ext):
    (d / f"{name}.ext").write_text(ext + "authorityKeyIdentifier=keyid\nsubjectKeyIdentifier=hash\n"
                                   "keyUsage=critical,digitalSignature\n")
    openssl(d, "req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-subj", f"/CN={cn}",
            "-keyout", f"{name}.key", "-out", f"{name}.csr")
    openssl(d, "x509", "-req", "-in", f"{name}.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial",
            "-days", "2", "-extfile", f"{name}.ext", "-out", f"{name}.pem")
    openssl(d, "pkcs8", "-topk8", "-nocrypt", "-in", f"{name}.key", "-out", f"{name}.p8")


@pytest.fixture(scope="module")
def pki():
    if not BINARY.exists() or not shutil.which("openssl"):
        pytest.skip("cstl_parser et openssl requis")
    class Dir(type(Path())):
        bot = ""
    d = Dir(tempfile.mkdtemp(prefix="cstl_spiffe_"))
    openssl(d, "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "2",
            "-subj", "/CN=acme-ca", "-addext", "basicConstraints=critical,CA:TRUE",
            "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-keyout", "ca.key", "-out", "ca.pem")
    issue(d, "server", "localhost", "subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n")
    Dir.bot = f"bot_{uuid.uuid4().hex[:6]}"
    issue(d, "bot", "bot", f"subjectAltName=URI:spiffe://acme.org/ns/prod/sa/{d.bot}\nextendedKeyUsage=clientAuth\n")
    issue(d, "evil", "evil", f"subjectAltName=URI:spiffe://evil.org/sa/{d.bot}\nextendedKeyUsage=clientAuth\n")
    issue(d, "plain", "plain", "extendedKeyUsage=clientAuth\n")
    return d


def start(pki, required):
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_KB_VERIFY": "off",
           "CSTL_TLS_CERT_PATH": str(pki / "server.pem"), "CSTL_TLS_KEY_PATH": str(pki / "server.p8"),
           "CSTL_TLS_REQUIRE_MUTUAL_AUTH": "true", "CSTL_TLS_CLIENT_CA_PATH": str(pki / "ca.pem"),
           "CSTL_SPIFFE_TRUST_DOMAIN": "acme.org", "CSTL_SPIFFE_REQUIRED": "true" if required else "false"}
    env.pop("CSTL_DEV_MODE", None)
    work = tempfile.mkdtemp(prefix="cstl_spiffe_srv_")
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    return proc, work


def agent_with_cert(pki, monkeypatch, name, cert):
    monkeypatch.setenv("HOME", str(pki))
    monkeypatch.setenv("CSTL_TLS_CA_PATH", str(pki / "ca.pem"))
    monkeypatch.setenv("CSTL_TLS_CLIENT_CERT_PATH", str(pki / f"{cert}.pem"))
    monkeypatch.setenv("CSTL_TLS_CLIENT_KEY_PATH", str(pki / f"{cert}.key"))
    import cstl_llm_agent as m
    with contextlib.redirect_stdout(io.StringIO()):
        return m.CstlAgent(name, "none", port=PORT)


def register(a):
    with contextlib.redirect_stdout(io.StringIO()):
        return a.register()


def test_connection_speaks_only_for_its_spiffe_identity(pki, monkeypatch):
    proc, _ = start(pki, required=False)
    try:
        bot = agent_with_cert(pki, monkeypatch, pki.bot, "bot")
        assert register(bot)
        ok, r = bot.request_action("read_logs")
        assert ok, r.fields

        # Un autre agent, avec sa propre cle valide, sur la connexion de bot.
        other = agent_with_cert(pki, monkeypatch, f"owner_{uuid.uuid4().hex[:6]}", "bot")
        resp = other._send_signed("agent_register", "server", [], {"name": other.name, "capabilities": "x"})
        assert resp.fields.get("reason") == "spiffe_identity_mismatch", resp.fields

        # Le meme nom depuis un autre domaine de confiance : refuse.
        evil = agent_with_cert(pki, monkeypatch, pki.bot, "evil")
        ok, r = evil.request_action("read_logs")
        assert not ok and r.fields.get("reason") == "spiffe_foreign_identity", r.fields

        # Sans identifiant SPIFFE (et non requis) : la signature suffit.
        plain = agent_with_cert(pki, monkeypatch, f"plain_{uuid.uuid4().hex[:6]}", "plain")
        assert register(plain)
    finally:
        proc.terminate()
        proc.wait(timeout=10)


def test_required_refuses_certificates_without_spiffe_id(pki, monkeypatch):
    proc, _ = start(pki, required=True)
    try:
        plain = agent_with_cert(pki, monkeypatch, f"plain_{uuid.uuid4().hex[:6]}", "plain")
        resp = plain._send_signed("agent_register", "server", [], {"name": plain.name, "capabilities": "x"})
        assert resp.fields.get("reason") == "spiffe_identity_required", resp.fields
        bot = agent_with_cert(pki, monkeypatch, pki.bot, "bot")
        assert register(bot)
    finally:
        proc.terminate()
        proc.wait(timeout=10)
