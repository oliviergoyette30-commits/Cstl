#!/usr/bin/env python3
"""
Vérification structurelle de cstl_llm_agent.py (Feature C)
- Syntaxe Python correcte
- AnthropicAgentBrain.from_env() retourne None sans anthropic/clé
- signing_bytes calcul canonicalization
"""

import sys
import os
from pathlib import Path

# Add SDK to path
SDK_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK_DIR))

def check_syntax():
    """Vérifier que le fichier Python est syntaxiquement correct."""
    import py_compile
    try:
        py_compile.compile(str(SDK_DIR / 'cstl_llm_agent.py'), doraise=True)
        print("✓ Python syntax OK")
        return True
    except py_compile.PyCompileError as e:
        print(f"✗ Syntax error: {e}")
        return False

def check_anthropic_from_env():
    """Vérifier que AnthropicAgentBrain.from_env() retourne None sans anthropic."""
    try:
        from cstl_llm_agent import AnthropicAgentBrain

        # S'assurer qu'ANTHROPIC_API_KEY n'est pas défini
        os.environ.pop("ANTHROPIC_API_KEY", None)

        result = AnthropicAgentBrain.from_env()
        if result is None:
            print("✓ AnthropicAgentBrain.from_env() returns None (anthropic not installed or no key)")
            return True
        else:
            print(f"✗ AnthropicAgentBrain.from_env() should return None, got {type(result)}")
            return False
    except Exception as e:
        print(f"✗ Error testing AnthropicAgentBrain.from_env(): {e}")
        return False

def check_signing_bytes():
    """Vérifier que cstl_signing_bytes existe et produit du canonique."""
    try:
        from cstl_llm_agent import cstl_signing_bytes

        # Test payload
        version = "v5.0.0"
        mode = "A"
        meta = {"encoder": "TestAgent", "public_key": "aa" * 32}
        intent = {"purpose": "test", "sender": "alice"}
        relations = []

        result = cstl_signing_bytes(version, mode, meta, intent, relations)

        # Vérifier que le résultat est bytes
        if isinstance(result, bytes):
            print(f"✓ cstl_signing_bytes returns bytes (length={len(result)})")

            # Vérifier que la canonicalisation exclut signature/rotation_signature
            result_str = result.decode('utf-8')
            if "signature" not in result_str:
                print("✓ Signing bytes excludes 'signature' field")
            else:
                print("✗ Signing bytes should exclude 'signature' field")
                return False

            # Vérifier l'ordre: VERSION, MODE, META (sorted), INTENT (sorted), RELATIONS
            if result_str.startswith("VERSION|v5.0.0"):
                print("✓ Signing bytes starts with VERSION")
            else:
                print("✗ Signing bytes should start with VERSION")
                return False

            return True
        else:
            print(f"✗ cstl_signing_bytes should return bytes, got {type(result)}")
            return False
    except ImportError as e:
        if "cryptography" in str(e):
            print("⚠ cryptography not installed, signing_bytes test skipped")
            return True
        else:
            print(f"✗ Error: {e}")
            return False
    except Exception as e:
        print(f"✗ Error testing cstl_signing_bytes: {e}")
        return False

def check_load_or_create_keypair():
    """Vérifier que load_or_create_keypair existe et gère l'absence de cryptography."""
    try:
        from cstl_llm_agent import load_or_create_keypair

        # Test sans créer de fichier réel (Path.home() / ".cstl_test")
        # La fonction devrait retourner (None, hex_string) si cryptography n'est pas installé

        keyfile = Path.home() / ".cstl_test" / "testkey.key"
        keyfile.parent.mkdir(parents=True, exist_ok=True)

        # Nettoyer si elle existe
        if keyfile.exists():
            keyfile.unlink()

        priv_bytes, pub_hex = load_or_create_keypair(keyfile)

        # Vérifier que pub_hex est une chaîne hex de 64 caractères (32 octets * 2)
        if isinstance(pub_hex, str) and len(pub_hex) == 64 and all(c in '0123456789abcdef' for c in pub_hex):
            print(f"✓ load_or_create_keypair returns valid public_key hex ({len(pub_hex)} chars)")

            # Vérifier que la clé a été créée
            if keyfile.exists():
                print("✓ load_or_create_keypair creates keyfile")
                # Nettoyer
                keyfile.unlink()
                return True
            else:
                print("⚠ load_or_create_keypair did not create keyfile (cryptography not installed)")
                return True
        else:
            print(f"✗ public_key should be 64 hex chars, got: {pub_hex[:20]}...")
            return False
    except Exception as e:
        print(f"✗ Error testing load_or_create_keypair: {e}")
        return False

def main():
    print("=" * 70)
    print("CSTL LLM Agent (Feature C) — Structural Verification")
    print("=" * 70)

    tests = [
        ("Python Syntax", check_syntax),
        ("AnthropicAgentBrain.from_env()", check_anthropic_from_env),
        ("cstl_signing_bytes()", check_signing_bytes),
        ("load_or_create_keypair()", check_load_or_create_keypair),
    ]

    results = []
    for name, test_func in tests:
        print(f"\n{name}:")
        try:
            results.append(test_func())
        except Exception as e:
            print(f"✗ Unexpected error: {e}")
            results.append(False)

    print("\n" + "=" * 70)
    passed = sum(results)
    total = len(results)
    print(f"Results: {passed}/{total} tests passed")

    if passed == total:
        print("✓ All structural verification tests PASSED")
        return 0
    else:
        print(f"✗ {total - passed} test(s) failed")
        return 1


# --- Point d'entree pytest (2026-11-17) ---------------------------------------
# Les check_* retournent True/False (mode script, voir main()). Appelees
# directement par pytest, un `return False` etait compte comme un SUCCES :
# aucune de ces verifications ne pouvait echouer sous pytest. Les wrappers
# ci-dessous transforment le booleen en vraie assertion.

def test_syntax():
    assert check_syntax()

def test_anthropic_from_env():
    assert check_anthropic_from_env()

def test_signing_bytes():
    assert check_signing_bytes()

def test_load_or_create_keypair():
    assert check_load_or_create_keypair()

if __name__ == "__main__":
    sys.exit(main())
