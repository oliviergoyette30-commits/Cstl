#!/usr/bin/env python3
"""
test_oidc_live.py -- enregistrement lie a une identite d'entreprise OIDC,
contre le VRAI serveur (2026-11-25). Le "fournisseur" est simule par des
cles RSA et EC generees ici (JWKS dans un fichier) :
  - un jeton RS256 ou ES256 valide, dont `sub` est le nom de l'agent, permet
    l'enregistrement ;
  - sans jeton, avec un autre nom, expire, pour un autre public, signe par
    une autre cle ou en alg=none : refuse.
Saute si target/debug/cstl_parser n'est pas compile.
"""
import base64
import contextlib
import io
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

import pytest
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec, padding, rsa
from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))
PORT, REST = 5959, 8989
ISS, AUD = "https://idp.acme.example", "cstl-prod"


def b64(b):
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def uint(n, size=None):
    return n.to_bytes(size or (n.bit_length() + 7) // 8, "big")


RSA = rsa.generate_private_key(public_exponent=65537, key_size=2048)
EC = ec.generate_private_key(ec.SECP256R1())
OTHER = rsa.generate_private_key(public_exponent=65537, key_size=2048)


def jwks():
    rn = RSA.public_key().public_numbers()
    en = EC.public_key().public_numbers()
    return {"keys": [{"kty": "RSA", "kid": "rsa1", "use": "sig", "alg": "RS256", "n": b64(uint(rn.n)), "e": b64(uint(rn.e))},
                     {"kty": "EC", "kid": "ec1", "use": "sig", "crv": "P-256", "x": b64(uint(en.x, 32)), "y": b64(uint(en.y, 32))}]}


def jwt(claims, alg="RS256", kid="rsa1", key=None):
    head = b64(json.dumps({"alg": alg, "kid": kid, "typ": "JWT"}).encode())
    body = b64(json.dumps(claims).encode())
    msg = f"{head}.{body}".encode()
    if alg == "RS256":
        sig = (key or RSA).sign(msg, padding.PKCS1v15(), hashes.SHA256())
    elif alg == "ES256":
        r, s = decode_dss_signature(EC.sign(msg, ec.ECDSA(hashes.SHA256())))
        sig = uint(r, 32) + uint(s, 32)
    else:
        sig = b""
    return f"{head}.{body}.{b64(sig)}"


def claims(name, **kw):
    now = int(time.time())
    return {"iss": ISS, "aud": AUD, "sub": name, "iat": now, "exp": now + 600, **kw}


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    work = Path(tempfile.mkdtemp(prefix="cstl_oidc_"))
    (work / "jwks.json").write_text(json.dumps(jwks()))
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": str(REST), "CSTL_KB_VERIFY": "off",
           "CSTL_OIDC_ISSUER": ISS, "CSTL_OIDC_AUDIENCE": AUD, "CSTL_OIDC_JWKS_PATH": str(work / "jwks.json")}
    env.pop("CSTL_DEV_MODE", None)
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    yield work
    proc.terminate()
    proc.wait(timeout=10)


def attempt(server, monkeypatch, token, name=None):
    monkeypatch.setenv("HOME", str(server))
    monkeypatch.delenv("CSTL_OIDC_TOKEN", raising=False)
    import cstl_llm_agent as m
    a = m.CstlAgent(name or f"agent_{uuid.uuid4().hex[:6]}", "none", port=PORT)
    with contextlib.redirect_stdout(io.StringIO()):
        ok = a.register(oidc_token=token(a.name) if callable(token) else token)
    return ok, a.last_register_response.fields.get("reason"), a


def test_valid_id_tokens_register_the_agent(server, monkeypatch):
    ok, why, a = attempt(server, monkeypatch, lambda n: jwt(claims(n)))
    assert ok, why
    assert a.request_action("read_logs")[0]
    ok, why, _ = attempt(server, monkeypatch, lambda n: jwt(claims(n, aud=[AUD, "autre"]), alg="ES256", kid="ec1"))
    assert ok, why


@pytest.mark.parametrize("token,reason", [
    (None, "oidc_token_required"),
    (lambda n: jwt(claims("someone_else")), "oidc_name_mismatch"),
    (lambda n: jwt(claims(n, exp=int(time.time()) - 3600)), "oidc_token_expired"),
    (lambda n: jwt(claims(n, aud="autre")), "oidc_wrong_audience"),
    (lambda n: jwt(claims(n, iss="https://evil.example")), "oidc_wrong_issuer"),
    (lambda n: jwt(claims(n), key=OTHER), "oidc_signature_invalid"),
    (lambda n: jwt(claims(n), alg="none"), "oidc_algorithm_refused"),
    (lambda n: jwt(claims(n), kid="inconnue"), "oidc_key_unknown"),
])
def test_bad_or_missing_tokens_are_refused(server, monkeypatch, token, reason):
    ok, why, a = attempt(server, monkeypatch, token)
    assert not ok and why == reason
    assert not a.request_action("read_logs")[0], "jamais enregistre"
