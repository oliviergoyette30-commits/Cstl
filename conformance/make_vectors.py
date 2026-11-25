#!/usr/bin/env python3
"""
conformance/make_vectors.py -- produit conformance/vectors.json (2026-11-25).

Vecteurs de la specification CSTL controle v1 (docs/CSTL_CONTROLE_SPEC_v1.md) :
  - signature des messages : dictionnaire -> octets canoniques -> signature
    Ed25519 (deterministe : cle privee fixe, donc signature fixe) ;
  - jetons d'autorisation : champs -> message canonique -> signature du
    serveur, puis verdicts attendus du systeme qui execute.

Les cles privees ici sont PUBLIQUES (vecteurs de test) : ne jamais les
utiliser ailleurs. Verifie par Python (test_conformance.py) et par Rust
(tests/conformance_vectors_test.rs) : les deux doivent trouver les memes
octets et les memes verdicts.

  python3 conformance/make_vectors.py      # reecrit vectors.json
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "sdk" / "python"))
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey  # noqa: E402
from cryptography.hazmat.primitives import serialization  # noqa: E402
import cstl_signing  # noqa: E402
from cstl_guard import AuthorizationToken  # noqa: E402

AGENT_SK = "11" * 32
SERVER_SK = "22" * 32


def pub(sk_hex):
    k = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(sk_hex))
    return k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex()


def sign(sk_hex, data):
    return Ed25519PrivateKey.from_private_bytes(bytes.fromhex(sk_hex)).sign(data).hex()


def signing_case(name, payload, note):
    payload = json.loads(json.dumps(payload))
    payload["meta"]["public_key"] = pub(AGENT_SK)
    canon = cstl_signing.signing_bytes(payload)
    payload["intent"]["signature"] = sign(AGENT_SK, canon)
    return {"name": name, "note": note, "message": payload, "canonical": canon.decode()}


def base(**intent):
    return {"version": "v5.0.0", "mode": "A", "meta": {"encoder": "a1", "produced_by": "a1"},
            "intent": {"purpose": "action_request", "sender": "a1", "receiver": "server", **intent},
            "relations": []}


def main():
    signing = [
        signing_case("action_request", base(action="wipe_db", request_id="r1"),
                     "cas de base : META puis INTENT tries par cle, sans relation"),
        signing_case("relations_triees", {**base(), "intent": {"purpose": "inform", "sender": "a1", "receiver": "server"},
                                          "relations": [{"type": "STATE", "subject": "z", "object": "1"},
                                                        {"type": "PERFORM", "subject": "a1", "object": "wipe_db",
                                                         "modality": "MUST_NOT", "reason": "prod, jamais"}]},
                     "chaque relation : champs tries, joints par des virgules ; puis les relations triees entre elles"),
        signing_case("nfc", base(action="résumer", request_id="r2"),
                     "valeur en forme decomposee (e + accent combinant) : signee en NFC (e accent precompose)"),
        signing_case("exclusions", {**base(action="x", request_id="r3", rotation_signature="ab" * 64),
                                    "meta": {"encoder": "a1", "produced_by": "a1", "PARENT_HASH": "sha256:00"}},
                     "INTENT.signature, INTENT.rotation_signature et META.PARENT_HASH ne sont pas signes"),
    ]
    fields = {"agent": "bot", "action": "refund_order", "request_id": "r42", "issued_at": 1000,
              "expires_at": 1060, "audit_hash": "sha256:" + "ab" * 32, "amount": "80"}
    tok = AuthorizationToken(signature="", **fields)
    msg = tok.message()
    sig = sign(SERVER_SK, msg)
    verdicts = [
        {"case": "valide", "action": "refund_order", "agent": "bot", "amount": "80", "now": 1010, "expect": "ok"},
        {"case": "montant egal ecrit autrement", "action": "refund_order", "agent": "bot", "amount": "80.0", "now": 1010, "expect": "ok"},
        {"case": "autre montant", "action": "refund_order", "agent": "bot", "amount": "8000", "now": 1010, "expect": "wrong_amount"},
        {"case": "sans montant a l'execution", "action": "refund_order", "agent": "bot", "amount": None, "now": 1010, "expect": "wrong_amount"},
        {"case": "autre action", "action": "wipe_db", "agent": "bot", "amount": "80", "now": 1010, "expect": "wrong_action"},
        {"case": "autre agent", "action": "refund_order", "agent": "eve", "amount": "80", "now": 1010, "expect": "wrong_agent"},
        {"case": "expire", "action": "refund_order", "agent": "bot", "amount": "80", "now": 1061, "expect": "expired"},
        {"case": "pas encore valide", "action": "refund_order", "agent": "bot", "amount": "80", "now": 990, "expect": "not_yet_valid"},
        {"case": "champ modifie (montant 8000 dans le jeton)", "token": {"amount": "8000"}, "action": "refund_order",
         "agent": "bot", "amount": "8000", "now": 1010, "expect": "bad_signature"},
    ]
    out = {
        "spec": "CSTL controle v1 (docs/CSTL_CONTROLE_SPEC_v1.md)",
        "warning": "cles privees de test, publiques : ne jamais les utiliser ailleurs",
        "agent_private_key": AGENT_SK, "agent_public_key": pub(AGENT_SK),
        "signing": signing,
        "token": {"server_private_key": SERVER_SK, "server_public_key": pub(SERVER_SK), "fields": fields,
                  "message": msg.decode(), "signature": sig, "skew": 0, "verdicts": verdicts},
    }
    (HERE / "vectors.json").write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
    print(f"{len(signing)} vecteurs de signature, {len(verdicts)} verdicts de jeton -> conformance/vectors.json")


if __name__ == "__main__":
    main()
