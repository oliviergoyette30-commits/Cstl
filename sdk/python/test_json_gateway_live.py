#!/usr/bin/env python3
"""
test_json_gateway_live.py -- entree JSON du serveur (POST /v1/message),
contre le VRAI serveur (2026-11-25).

Le client de ce test ne connait PAS le format texte CSTL : il envoie du JSON
par HTTP (bibliotheque standard) et signe le dictionnaire JSON avec
`cstl_signing.signing_bytes`. Il recoit exactement la protection d'un client
CSTL : enregistrement signe, interdiction appliquee, signature falsifiee
refusee, message non signe refuse, trace d'audit.

Saute si target/debug/cstl_parser n'est pas compile.
"""
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
import cstl_signing  # noqa: E402

PORT, REST = 5656, 8686


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST)}
    env.pop("CSTL_DEV_MODE", None)
    proc = subprocess.Popen([str(BINARY)], cwd=tempfile.mkdtemp(prefix="cstl_json_"), env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", REST), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    else:
        proc.terminate()
        pytest.fail("API REST injoignable")
    yield
    proc.terminate()
    proc.wait(timeout=10)


def post(msg):
    req = urllib.request.Request(f"http://127.0.0.1:{REST}/v1/message", data=json.dumps(msg).encode(),
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


class JsonAgent:
    """Un agent qui ne parle que JSON."""

    def __init__(self, name):
        _, self.pub, self.priv = cstl_signing.generate_keypair()
        self.name = name

    def message(self, purpose, relations=(), sign=True, **intent):
        msg = {"version": "v5.0.0", "mode": "A",
               "meta": {"encoder": "json_client", "produced_by": "json_client", "public_key": self.pub},
               "intent": {"purpose": purpose, "sender": self.name, "receiver": "server", **intent},
               "relations": list(relations)}
        if sign:
            msg["intent"]["signature"] = cstl_signing.sign_intent(msg, self.priv)
        return msg

    def send(self, purpose, relations=(), **intent):
        return post(self.message(purpose, relations, **intent))


def test_json_agent_gets_the_same_protection(server):
    a = JsonAgent(f"json_{uuid.uuid4().hex[:8]}")
    status, r = a.send("agent_register", name=a.name, capabilities="communication")
    assert status == 200 and r["intent"]["purpose"] == "agent_register_ack", r["raw"]

    # Une regle d'interdiction, envoyee en JSON.
    status, r = a.send("inform", [{"type": "PERFORM", "subject": a.name, "object": "wipe_db",
                                   "modality": "MUST_NOT", "reason": "production, jamais"}])
    assert status == 200, r

    status, r = a.send("action_request", action="wipe_db", request_id=uuid.uuid4().hex)
    assert r["intent"]["decision"] == "denied" and r["intent"]["reason"] == "forbidden", r["raw"]
    assert any(b["block"] == "AUDIT" for b in r["blocks"]), r["raw"]

    status, r = a.send("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    assert r["intent"]["decision"] == "allowed", r["raw"]
    assert r["intent"].get("authorization"), "jeton signe par le serveur attendu"


def test_tampered_or_unsigned_json_is_refused(server):
    a = JsonAgent(f"json_{uuid.uuid4().hex[:8]}")
    assert a.send("agent_register", name=a.name, capabilities="communication")[1]["intent"]["purpose"] == "agent_register_ack"

    # Signature calculee, puis action changee apres coup : refuse.
    msg = a.message("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    msg["intent"]["action"] = "wipe_db"
    _, r = post(msg)
    assert r["intent"].get("decision") != "allowed", r["raw"]
    assert r["intent"]["purpose"] == "signature_rejected", r["raw"]

    # Non signe : refuse (signatures obligatoires par defaut).
    _, r = post(a.message("action_request", sign=False, action="read_logs", request_id=uuid.uuid4().hex))
    assert r["intent"].get("decision") != "allowed", r["raw"]

    # Un autre agent ne peut pas parler au nom du premier.
    b = JsonAgent(a.name)
    _, r = b.send("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    assert r["intent"].get("decision") != "allowed", r["raw"]


def test_values_the_wire_cannot_carry_are_refused_before_sending(server):
    a = JsonAgent("json_x")
    for bad in ['avec "guillemets"', "deux\nlignes", " espace", 3]:
        status, r = post(a.message("inform", sign=False, note=bad))
        assert status == 400 and r["error"] == "message_refuse", r
    status, r = post({**a.message("inform"), "extra": "x"})
    assert status == 400 and "champ inconnu" in r["detail"]


def _pki(d: Path):
    """CA jetable, certificat serveur (localhost) et certificat client, par openssl."""
    def run(*a):
        subprocess.run(["openssl", *a], cwd=d, check=True, capture_output=True)
    run("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "2",
        "-subj", "/CN=cstl-test-ca", "-keyout", "ca.key", "-out", "ca.pem")
    (d / "server.ext").write_text("subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n")
    (d / "client.ext").write_text("extendedKeyUsage=clientAuth\n")
    for name, cn in (("server", "localhost"), ("client", "json-gateway")):
        run("req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-subj", f"/CN={cn}",
            "-keyout", f"{name}.key", "-out", f"{name}.csr")
        run("x509", "-req", "-in", f"{name}.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-days", "2",
            "-extfile", f"{name}.ext", "-out", f"{name}.pem")
        # rustls lit PKCS#8 : convertir la cle.
        run("pkcs8", "-topk8", "-nocrypt", "-in", f"{name}.key", "-out", f"{name}.p8")


def test_json_gateway_relays_to_a_mutual_tls_listener():
    """Listener TCP en TLS avec authentification mutuelle (2026-11-25) : l'entree
    JSON s'y connecte avec la CA et son propre certificat client. Sans
    certificat client, le serveur refuse la poignee de main."""
    import shutil
    if not BINARY.exists() or not shutil.which("openssl"):
        pytest.skip("cstl_parser et openssl requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_json_tls_"))
    _pki(work)
    port, rest = 5757, 8787
    env = {**os.environ, "CSTL_PORT": str(port), "CSTL_REST_PORT": str(rest), "CSTL_KB_VERIFY": "off",
           "CSTL_TLS_CERT_PATH": str(work / "server.pem"), "CSTL_TLS_KEY_PATH": str(work / "server.p8"),
           "CSTL_TLS_REQUIRE_MUTUAL_AUTH": "true", "CSTL_TLS_CLIENT_CA_PATH": str(work / "ca.pem"),
           "CSTL_GATEWAY_TLS_CA_PATH": str(work / "ca.pem"),
           "CSTL_GATEWAY_CLIENT_CERT_PATH": str(work / "client.pem"),
           "CSTL_GATEWAY_CLIENT_KEY_PATH": str(work / "client.p8")}
    env.pop("CSTL_DEV_MODE", None)
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", rest), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        a = JsonAgent(f"tls_{uuid.uuid4().hex[:8]}")

        def send(msg):
            req = urllib.request.Request(f"http://127.0.0.1:{rest}/v1/message", data=json.dumps(msg).encode(),
                                         headers={"Content-Type": "application/json"})
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    return r.status, json.loads(r.read())
            except urllib.error.HTTPError as e:
                return e.code, json.loads(e.read())

        status, r = send(a.message("agent_register", name=a.name, capabilities="communication"))
        assert status == 200 and r["intent"]["purpose"] == "agent_register_ack", r
        status, r = send(a.message("action_request", action="read_logs", request_id=uuid.uuid4().hex))
        assert status == 200 and r["intent"]["decision"] == "allowed", r

        # Un client TCP sans certificat : le serveur TLS refuse la connexion.
        import ssl
        ctx = ssl.create_default_context(cafile=str(work / "ca.pem"))
        with pytest.raises((ssl.SSLError, ConnectionResetError, OSError)):
            with socket.create_connection(("127.0.0.1", port), timeout=5) as raw:
                with ctx.wrap_socket(raw, server_hostname="localhost") as s:
                    s.sendall(b"#!CSTL v5.0.0 MODE=A\n---END---\n")
                    if not s.recv(10):
                        raise ConnectionResetError("ferme")
    finally:
        proc.terminate()
        proc.wait(timeout=10)
