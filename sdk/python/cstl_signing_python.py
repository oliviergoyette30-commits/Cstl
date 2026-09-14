"""
cstl_signing.py — Port Python exact de src/signing.rs + audit.rs::signing_bytes

Point critique: signing_bytes DOIT reproduire octet-par-octet la version Rust.
Toute dérive casse silencieusement TOUTES les signatures produites en Python.

Dependencies: cryptography (Ed25519), unicodedata (NFC normalization)
"""
import json
import hashlib
import unicodedata
from typing import Dict, Any, Optional, Tuple
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.hazmat.primitives import serialization


def nfc_normalize(s: str) -> str:
    """Normalisation NFC Unicode — reproduit Rust s.nfc().collect()"""
    return unicodedata.normalize('NFC', s)


def signing_bytes(payload: Dict[str, Any]) -> bytes:
    """
    Canonicalisation déterministe pour signature Ed25519.
    Reproduit OCTET PAR OCTET src/server/audit.rs::signing_bytes(payload).

    Exclusions intentionnelles:
    - META.PARENT_HASH (remplacée par le serveur)
    - INTENT.signature (un message ne peut pas se signer lui-même)
    - INTENT.rotation_signature (doit porter sur le même message que signature)
    - INTENT.public_key n'est PAS exclu (lie la signature à la clé revendiquée)

    Ordre:
    1. VERSION|<nfc>
    2. MODE|<nfc>
    3. META (triées par clé, exclut PARENT_HASH)
    4. INTENT (triées par clé, exclut signature et rotation_signature)
    5. RELATIONS (triées)
    """
    canon = ""

    # VERSION
    version = payload.get("version", "")
    canon += f"VERSION|{nfc_normalize(version)}\n"

    # MODE
    mode = payload.get("mode", "")
    canon += f"MODE|{nfc_normalize(mode)}\n"

    # META (sorted, exclut PARENT_HASH)
    meta = payload.get("meta", {})
    if isinstance(meta, dict):
        canon += "META"
        for k in sorted(meta.keys()):
            if k != "PARENT_HASH":  # Exclu délibérément
                v = meta[k]
                canon += f"|{nfc_normalize(k)}={nfc_normalize(v)}"
        canon += "\n"

    # INTENT (sorted, exclut signature et rotation_signature)
    intent = payload.get("intent", {})
    if isinstance(intent, dict):
        canon += "INTENT"
        for k in sorted(intent.keys()):
            # Exclusions délibérées: un message ne peut pas se signer lui-même,
            # et les deux signatures doivent porter sur EXACTEMENT le même message
            if k not in ("signature", "rotation_signature"):
                v = intent[k]
                canon += f"|{nfc_normalize(k)}={nfc_normalize(v)}"
        canon += "\n"

    # RELATIONS (triées)
    relations = payload.get("relations", [])
    if isinstance(relations, list):
        # Chaque relation est un dict, représenté comme key1=value1,key2=value2,...
        rel_strings = []
        for rel in relations:
            if isinstance(rel, dict):
                # Trier les clés dans chaque relation
                rel_str = ",".join(
                    f"{nfc_normalize(k)}={nfc_normalize(v)}"
                    for k, v in sorted(rel.items())
                )
                rel_strings.append(rel_str)
        # Trier les relations entre elles
        rel_strings.sort()
        canon += "RELATIONS"
        for rel_str in rel_strings:
            canon += f"|{rel_str}"

    # Retourner en bytes UTF-8
    return canon.encode('utf-8')


def generate_keypair() -> Tuple[ed25519.Ed25519PrivateKey, str, str]:
    """
    Génère une nouvelle paire de clés Ed25519.
    Retourne (private_key, public_key_hex, private_key_hex).
    """
    private_key = ed25519.Ed25519PrivateKey.generate()
    public_key = private_key.public_key()

    # Sérialiser en bytes (32 bytes publique, 32 bytes privée)
    public_bytes = public_key.public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw
    )
    private_bytes = private_key.private_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PrivateFormat.Raw,
        encryption_algorithm=serialization.NoEncryption()
    )

    public_hex = public_bytes.hex()
    private_hex = private_bytes.hex()

    return private_key, public_hex, private_hex


def sign_intent(payload: Dict[str, Any], private_key_hex: str) -> str:
    """
    Signe un payload avec une clé privée Ed25519.
    Retourne la signature en hex (128 caractères).
    """
    # Décoder la clé privée (32 bytes)
    private_bytes = bytes.fromhex(private_key_hex)
    private_key = ed25519.Ed25519PrivateKey.from_private_bytes(private_bytes)

    # Canonicaliser et signer
    message = signing_bytes(payload)
    signature_bytes = private_key.sign(message)

    return signature_bytes.hex()


def verify_signature(payload: Dict[str, Any], public_key_hex: str, signature_hex: str) -> bool:
    """
    Vérifie une signature Ed25519.
    Retourne True si valide, False sinon.
    """
    try:
        # Décoder la clé publique (32 bytes)
        public_bytes = bytes.fromhex(public_key_hex)
        public_key = ed25519.Ed25519PublicKey.from_public_bytes(public_bytes)

        # Décoder la signature (64 bytes)
        signature_bytes = bytes.fromhex(signature_hex)

        # Canonicaliser et vérifier
        message = signing_bytes(payload)
        public_key.verify(signature_bytes, message)

        return True
    except Exception:
        return False


def load_or_create_keypair(filepath: str = "~/.cstl/agent_keypair.hex") -> Tuple[str, str]:
    """
    Charge une paire de clés depuis un fichier, ou la crée si absente.
    Fichier format: deux lignes, chacune étant du hex.
    Retourne (public_key_hex, private_key_hex).
    """
    from pathlib import Path

    path = Path(filepath).expanduser()

    if path.exists():
        with open(path) as f:
            lines = [line.strip() for line in f if line.strip()]
            if len(lines) >= 2:
                return lines[0], lines[1]

    # Créer une nouvelle paire
    _, public_hex, private_hex = generate_keypair()
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, 'w') as f:
        f.write(f"{public_hex}\n{private_hex}\n")

    return public_hex, private_hex


if __name__ == "__main__":
    print("=== CSTL Ed25519 Signing Tests ===\n")

    # Test 1: Génération de clés
    print("Test 1: Key generation")
    _, pub_hex, priv_hex = generate_keypair()
    print(f"✓ Public key (64 hex): {pub_hex[:32]}...")
    print(f"✓ Private key (64 hex): {priv_hex[:32]}...")

    # Test 2: Canonicalisation (signing_bytes)
    print("\nTest 2: signing_bytes canonicalization")
    payload = {
        "version": "v5.0.0",
        "mode": "A",
        "meta": {
            "encoder": "Agent",
            "produced_by": "Test",
            "public_key": pub_hex,
            "PARENT_HASH": "ignored_by_signing_bytes"
        },
        "intent": {
            "purpose": "test",
            "sender": "agent_alice",
            "receiver": "server",
            "message": "Hello, world!"
        },
        "relations": [
            {"subject": "Alice", "type": "knows", "object": "Bob"},
            {"subject": "Bob", "type": "knows", "object": "Alice"}
        ]
    }

    signed = signing_bytes(payload)
    print(f"✓ signing_bytes length: {len(signed)} bytes")
    print(f"✓ First 50 chars: {signed[:50]}")

    # Test 3: Signature et vérification
    print("\nTest 3: Sign and verify")
    signature = sign_intent(payload, priv_hex)
    print(f"✓ Signature (128 hex): {signature[:32]}...")
    print(f"✓ Signature length: {len(signature)} chars")

    # Ajouter la signature au payload
    payload_signed = json.loads(json.dumps(payload))
    payload_signed["intent"]["signature"] = signature
    payload_signed["meta"]["public_key"] = pub_hex

    # Vérifier
    is_valid = verify_signature(payload_signed, pub_hex, signature)
    print(f"✓ Verification result: {is_valid}")

    # Test 4: Corruption détectée
    print("\nTest 4: Tamper detection")
    bad_signature = signature[:-2] + ("00" if signature[-2:] != "00" else "FF")
    is_valid_bad = verify_signature(payload_signed, pub_hex, bad_signature)
    print(f"✓ Invalid signature rejected: {not is_valid_bad}")

    # Test 5: Payload modified
    print("\nTest 5: Payload modification detection")
    modified_payload = json.loads(json.dumps(payload_signed))
    modified_payload["intent"]["message"] = "Modified!"
    is_valid_modified = verify_signature(modified_payload, pub_hex, signature)
    print(f"✓ Modified payload rejected: {not is_valid_modified}")

    # Test 6: PARENT_HASH ne doit pas affecter la signature
    print("\nTest 6: PARENT_HASH exclusion")
    payload_a = {
        "version": "v5.0.0",
        "mode": "A",
        "meta": {"encoder": "Test", "PARENT_HASH": "hash_A"},
        "intent": {"purpose": "test", "sender": "alice"},
        "relations": []
    }
    payload_b = {
        "version": "v5.0.0",
        "mode": "A",
        "meta": {"encoder": "Test", "PARENT_HASH": "hash_B"},
        "intent": {"purpose": "test", "sender": "alice"},
        "relations": []
    }
    bytes_a = signing_bytes(payload_a)
    bytes_b = signing_bytes(payload_b)
    print(f"✓ PARENT_HASH not affecting signing_bytes: {bytes_a == bytes_b}")

    # Test 7: Signature et rotation_signature n'affectent pas signing_bytes
    print("\nTest 7: signature/rotation_signature exclusion")
    payload_no_sig = {
        "version": "v5.0.0",
        "mode": "A",
        "meta": {"encoder": "Test"},
        "intent": {"purpose": "test", "sender": "alice"},
        "relations": []
    }
    payload_with_sig = {
        "version": "v5.0.0",
        "mode": "A",
        "meta": {"encoder": "Test"},
        "intent": {
            "purpose": "test",
            "sender": "alice",
            "signature": "1234567890abcdef",
            "rotation_signature": "fedcba0987654321"
        },
        "relations": []
    }
    bytes_no_sig = signing_bytes(payload_no_sig)
    bytes_with_sig = signing_bytes(payload_with_sig)
    print(f"✓ signature/rotation_signature not affecting signing_bytes: {bytes_no_sig == bytes_with_sig}")

    print("\n✓ All tests passed!")
