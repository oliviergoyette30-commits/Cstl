#!/usr/bin/env python3
"""
CSTL LLM Agent — Ed25519 signatures + Hermes/Anthropic/Gemini providers
Autonomous agent dialogue with peer-mode self (alice ↔ bob ↔ charlie)
"""

import sys
import os
import json
import urllib.request
import socket
import time
import argparse
import hashlib
from pathlib import Path
from typing import Optional, Tuple

# Ed25519 crypto
try:
    from cryptography.hazmat.primitives.asymmetric import ed25519
    from cryptography.hazmat.primitives import serialization
    HAS_CRYPTO = True
except ImportError:
    HAS_CRYPTO = False
    print("⚠️ cryptography not installed. Install: pip3 install cryptography", file=sys.stderr)

# LLM providers
try:
    import anthropic
    HAS_ANTHROPIC = True
except ImportError:
    HAS_ANTHROPIC = False


try:
    import google.generativeai as genai
    HAS_GEMINI = True
except ImportError:
    HAS_GEMINI = False


# ============================================================================
# Ed25519 Signing & Verification
# ============================================================================

def load_or_create_keypair(keyfile: Path) -> Tuple[Optional[bytes], str]:
    """Load or generate Ed25519 keypair"""
    if not HAS_CRYPTO:
        return None, "a" * 64

    keyfile.parent.mkdir(parents=True, exist_ok=True)

    if keyfile.exists():
        priv_bytes = keyfile.read_bytes()
        priv_key = ed25519.Ed25519PrivateKey.from_private_bytes(priv_bytes)
    else:
        priv_key = ed25519.Ed25519PrivateKey.generate()
        priv_bytes = priv_key.private_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PrivateFormat.Raw,
            encryption_algorithm=serialization.NoEncryption()
        )
        keyfile.write_bytes(priv_bytes)

    pub_key = priv_key.public_key()
    pub_bytes = pub_key.public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw
    )
    pub_hex = pub_bytes.hex()

    return priv_bytes, pub_hex


def cstl_signing_bytes(version: str, mode: str, meta: dict, intent: dict, relations: list) -> bytes:
    """
    Generate canonical CSTL signing bytes (Rust/Python byte-for-byte identical).
    Format: VERSION|v\nMODE|m\nMETA|k=v|k=v|...\nINTENT|k=v|k=v|...\nRELATIONS|...
    Matches src/server/audit.rs::signing_bytes() exactly.
    """
    import unicodedata

    # NFC normalization (same as Rust)
    def nfc(s):
        return unicodedata.normalize('NFC', str(s))

    canon = []
    canon.append(f"VERSION|{nfc(version)}")
    canon.append(f"MODE|{nfc(mode)}")

    # META (sorted by key, BTreeMap order)
    meta_filtered = {k: v for k, v in meta.items() if k != "PARENT_HASH"}
    meta_parts = [f"{nfc(k)}={nfc(v)}" for k, v in sorted(meta_filtered.items())]
    canon.append("META" + ("|" + "|".join(meta_parts) if meta_parts else ""))

    # INTENT (sorted, exclude signature/rotation_signature)
    intent_filtered = {k: v for k, v in intent.items()
                      if k not in ("signature", "rotation_signature")}
    intent_parts = [f"{nfc(k)}={nfc(v)}" for k, v in sorted(intent_filtered.items())]
    canon.append("INTENT" + ("|" + "|".join(intent_parts) if intent_parts else ""))

    # RELATIONS (each relation sorted, then relations list sorted)
    rel_strings = []
    for rel in relations:
        rel_filtered = {k: v for k, v in rel.items()
                       if k not in ("signature", "rotation_signature")}
        rel_parts = [f"{nfc(k)}={nfc(v)}" for k, v in sorted(rel_filtered.items())]
        rel_strings.append(",".join(rel_parts))
    rel_strings.sort()

    canon.append("RELATIONS" + ("|" + "|".join(rel_strings) if rel_strings else ""))

    text = "\n".join(canon)
    return text.encode("utf-8")


def sign_intent(priv_bytes: Optional[bytes], pub_hex: str, **kwargs) -> str:
    """Sign CSTL intent with Ed25519"""
    if not HAS_CRYPTO or not priv_bytes:
        return "x" * 128

    canonical = cstl_signing_bytes(
        kwargs.get("version", "v5.0.0"),
        kwargs.get("mode", "A"),
        kwargs.get("meta", {}),
        kwargs.get("intent", {}),
        kwargs.get("relations", [])
    )

    priv_key = ed25519.Ed25519PrivateKey.from_private_bytes(priv_bytes)
    sig_bytes = priv_key.sign(canonical)
    return sig_bytes.hex()


def verify_signature(payload: dict, public_key_hex: str, signature_hex: str) -> bool:
    """
    Verify CSTL payload signature with Ed25519 (Couche 7 v5.2).

    Reproduces Rust src/signing.rs::verify_raw() exactly:
    - Decodes hex public key (32 bytes) and signature (64 bytes)
    - Canonicalizes payload using signing_bytes()
    - Verifies Ed25519 signature

    Args:
        payload: dict with version, mode, meta, intent, relations
        public_key_hex: hex-encoded 32-byte Ed25519 public key (64 chars)
        signature_hex: hex-encoded 64-byte signature (128 chars)

    Returns:
        True if signature is valid, False otherwise

    Raises:
        ValueError: if key or signature hex is invalid
    """
    if not HAS_CRYPTO:
        raise ImportError("cryptography not installed")

    # Decode public key (32 bytes = 64 hex chars)
    try:
        pub_bytes = bytes.fromhex(public_key_hex)
        if len(pub_bytes) != 32:
            raise ValueError(f"bad_public_key_length: expected 32 bytes, got {len(pub_bytes)}")
    except ValueError as e:
        if "invalid_hex" in str(e).lower():
            raise ValueError("invalid_hex")
        raise ValueError("invalid_hex")

    # Decode signature (64 bytes = 128 hex chars)
    try:
        sig_bytes = bytes.fromhex(signature_hex)
        if len(sig_bytes) != 64:
            raise ValueError(f"bad_signature_length: expected 64 bytes, got {len(sig_bytes)}")
    except ValueError as e:
        if "invalid_hex" in str(e).lower():
            raise ValueError("invalid_hex")
        raise ValueError("invalid_hex")

    # Canonicalize payload exactly like Rust signing_bytes()
    message = cstl_signing_bytes(
        payload.get("version", "v5.0.0"),
        payload.get("mode", "A"),
        payload.get("meta", {}),
        payload.get("intent", {}),
        payload.get("relations", [])
    )

    # Verify signature
    try:
        pub_key = ed25519.Ed25519PublicKey.from_public_bytes(pub_bytes)
        pub_key.verify(sig_bytes, message)
        return True
    except Exception:
        return False


# ============================================================================
# LLM Provider Interface
# ============================================================================

class LLMProvider:
    def generate(self, prompt: str, max_tokens: int = 500) -> str:
        raise NotImplementedError


def _extract_json_object(text: str) -> Optional[dict]:
    """Extrait le premier objet JSON d'un texte de modele (tolere du texte
    autour : premiere '{' jusqu'a la derniere '}'). None si absent/invalide."""
    start, end = text.find("{"), text.rfind("}")
    if start == -1 or end <= start:
        return None
    try:
        obj = json.loads(text[start:end + 1])
    except ValueError:
        return None
    return obj if isinstance(obj, dict) else None


class HermesAgentBrain(LLMProvider):
    """Hermes via l'API REST d'Ollama (POST /api/generate), stdlib uniquement.

    Reecrit le 2026-11-17 : l'ancienne version exigeait le paquet `ollama`,
    figeait localhost:11434 et le modele, et `from_env()` retournait un objet
    meme sans serveur Ollama (creer `ollama.Client` n'ouvre aucune connexion) --
    la "degradation propre" annoncee n'existait pas. Configuration :
    CSTL_OLLAMA_HOST (defaut 127.0.0.1), CSTL_OLLAMA_PORT (11434),
    CSTL_OLLAMA_MODEL (hermes3:8b).
    """

    DEFAULT_MODEL = "hermes3:8b"

    def __init__(self, host: str = "127.0.0.1", port: int = 11434,
                 model: str = DEFAULT_MODEL, timeout: float = 60.0):
        self.host = host
        self.port = int(port)
        self.model = model
        self.timeout = timeout

    @property
    def base_url(self) -> str:
        return f"http://{self.host}:{self.port}"

    @classmethod
    def from_env(cls) -> Optional['HermesAgentBrain']:
        """Retourne une instance seulement si un serveur Ollama REPOND
        (GET /api/tags, timeout court), sinon None."""
        brain = cls(
            host=os.environ.get("CSTL_OLLAMA_HOST", "127.0.0.1"),
            port=int(os.environ.get("CSTL_OLLAMA_PORT", "11434")),
            model=os.environ.get("CSTL_OLLAMA_MODEL", cls.DEFAULT_MODEL),
        )
        try:
            with urllib.request.urlopen(f"{brain.base_url}/api/tags", timeout=1.0):
                pass
        except Exception:
            return None
        return brain

    def _post_generate(self, prompt: str, max_tokens: int) -> str:
        body = json.dumps({
            "model": self.model,
            "prompt": prompt,
            "stream": False,
            "options": {"num_predict": max_tokens},
        }).encode("utf-8")
        req = urllib.request.Request(
            f"{self.base_url}/api/generate", data=body,
            headers={"Content-Type": "application/json"}, method="POST",
        )
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
        return str(payload.get("response", "")).strip()

    def generate(self, prompt: str, max_tokens: int = 500) -> str:
        try:
            return self._post_generate(prompt, max_tokens)
        except Exception as e:
            return f"[Hermes error: {e}]"

    def generate_relation(self, subject: str, peer_message: Optional[str]) -> Optional[dict]:
        """Demande une relation CSTL au format JSON {type, subject, object}.
        Retourne le dict parse, ou None si la reponse n'en contient pas."""
        prompt = (
            "Reponds UNIQUEMENT par un objet JSON {\"type\": ..., \"subject\": ..., "
            f"\"object\": ...}} decrivant une relation sur: {subject}"
        )
        if peer_message:
            prompt += f"\nMessage du pair: {peer_message}"
        try:
            return _extract_json_object(self._post_generate(prompt, 200))
        except Exception:
            return None


class AnthropicAgentBrain(LLMProvider):
    """Claude via Anthropic API"""

    def __init__(self, api_key: str):
        self.client = anthropic.Anthropic(api_key=api_key)

    @classmethod
    def from_env(cls) -> Optional['AnthropicAgentBrain']:
        """
        Return AnthropicAgentBrain instance if both `anthropic` package
        and ANTHROPIC_API_KEY are available, else None.

        Dégradation propre (même pattern que Rust TelegramNotifier::from_env) —
        le serveur continue sans agent Anthropic si la dépendance ou clé manquent.
        """
        if not HAS_ANTHROPIC:
            return None

        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            return None

        try:
            return cls(api_key)
        except Exception:
            return None

    def generate(self, prompt: str, max_tokens: int = 500) -> str:
        try:
            msg = self.client.messages.create(
                model="claude-3-5-sonnet-20250515",
                max_tokens=max_tokens,
                messages=[{"role": "user", "content": prompt}]
            )
            return msg.content[0].text.strip()
        except Exception as e:
            return f"[Anthropic error: {e}]"


class GeminiAgentBrain(LLMProvider):
    """Gemini via Google API"""

    def __init__(self, api_key: str):
        genai.configure(api_key=api_key)
        self.model = genai.GenerativeModel("gemini-1.5-pro")

    @classmethod
    def from_env(cls) -> Optional['GeminiAgentBrain']:
        """Return GeminiAgentBrain if genai package and GOOGLE_API_KEY available, else None."""
        if not HAS_GEMINI:
            return None

        api_key = os.environ.get("GOOGLE_API_KEY")
        if not api_key:
            return None

        try:
            return cls(api_key)
        except Exception:
            return None

    def generate(self, prompt: str, max_tokens: int = 500) -> str:
        try:
            response = self.model.generate_content(prompt)
            return response.text.strip()
        except Exception as e:
            return f"[Gemini error: {e}]"


# ============================================================================
# CSTL Client
# ============================================================================

class CstlClient:
    def __init__(self, host: str = "127.0.0.1", port: int = 5050, timeout: float = 10.0):
        self.host = host
        self.port = port
        self.timeout = timeout

    def send_message(self, sender: str, receiver: str, purpose: str,
                    message: str, pub_key: str, signature: str) -> dict:
        """Send CSTL message with Ed25519 signature"""

        payload = (
            f"#!CSTL v5.0.0 MODE=A\n"
            f"META [sender={sender}, receiver={receiver}, public_key={pub_key}, timestamp={time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}]\n"
            f"INTENT_PAYLOAD [purpose={purpose}, message={message}, signature={signature}]\n"
            f"---END---\n"
        )

        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(self.timeout)
            sock.connect((self.host, self.port))
            sock.sendall(payload.encode("utf-8"))

            response = sock.recv(4096)
            sock.close()

            response_str = response.decode("utf-8", errors="ignore")
            return {"status": "sent", "response": response_str[:200]}
        except Exception as e:
            return {"status": "error", "error": str(e)}

    def register_agent(self, name: str, pub_key: str, signature: str) -> dict:
        """Register agent with signature"""

        payload = (
            f"#!CSTL v5.0.0 MODE=A\n"
            f"META [sender={name}, public_key={pub_key}, timestamp={time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}]\n"
            f"INTENT_PAYLOAD [purpose=agent_register, name={name}, capabilities=auth;verify;sync, signature={signature}]\n"
            f"---END---\n"
        )

        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(self.timeout)
            sock.connect((self.host, self.port))
            sock.sendall(payload.encode("utf-8"))

            response = sock.recv(4096)
            sock.close()

            response_str = response.decode("utf-8", errors="ignore")
            if "agent_register_ack" in response_str:
                return {"status": "registered", "response": response_str[:200]}
            else:
                return {"status": "rejected", "response": response_str[:200]}
        except Exception as e:
            return {"status": "error", "error": str(e)}


# ============================================================================
# Agent Orchestration
# ============================================================================

class CstlAgent:
    def __init__(self, name: str, provider_name: str = "hermes"):
        self.name = name
        self.keyfile = Path.home() / ".cstl" / f"agent_{name}.key"

        # Load/create keys
        if HAS_CRYPTO:
            self.priv_bytes, self.pub_key = load_or_create_keypair(self.keyfile)
        else:
            self.priv_bytes, self.pub_key = None, "a" * 64

        # Initialize LLM provider with from_env() pattern
        provider_name = provider_name.lower()
        self.llm = None

        if provider_name == "hermes":
            self.llm = HermesAgentBrain.from_env()
            if not self.llm:
                raise ImportError("Hermes unavailable: no Ollama server answering (set CSTL_OLLAMA_HOST/CSTL_OLLAMA_PORT)")
        elif provider_name == "anthropic":
            self.llm = AnthropicAgentBrain.from_env()
            if not self.llm:
                raise ImportError("Anthropic unavailable: pip3 install anthropic && export ANTHROPIC_API_KEY")
        elif provider_name == "gemini":
            self.llm = GeminiAgentBrain.from_env()
            if not self.llm:
                raise ImportError("Gemini unavailable: pip3 install google-generativeai && export GOOGLE_API_KEY")
        else:
            raise ValueError(f"Unknown provider: {provider_name}")

        self.client = CstlClient()
        self.registered = False

    def register(self):
        """Register with server using Ed25519 signature"""
        if self.registered:
            return

        signature = sign_intent(
            self.priv_bytes,
            self.pub_key,
            version="v5.0.0",
            mode="A",
            meta={"sender": self.name, "public_key": self.pub_key},
            intent={"purpose": "agent_register", "name": self.name, "capabilities": "auth;verify;sync"},
            relations=[]
        )

        result = self.client.register_agent(self.name, self.pub_key, signature)
        if result["status"] == "registered":
            print(f"[{self.name}] ✅ Registered with signature validation")
            self.registered = True
        else:
            print(f"[{self.name}] ⚠️  Registration: {result.get('status')}")

    def send_message(self, receiver: str, message: str):
        """Send signed message to peer"""
        signature = sign_intent(
            self.priv_bytes,
            self.pub_key,
            version="v5.0.0",
            mode="A",
            meta={"sender": self.name, "receiver": receiver, "public_key": self.pub_key},
            intent={"purpose": "communication", "message": message},
            relations=[]
        )

        result = self.client.send_message(
            self.name, receiver, "communication", message, self.pub_key, signature
        )
        return result

    def generate_response(self, prompt: str) -> str:
        """Generate LLM response"""
        return self.llm.generate(prompt)


def run_autonomous_dialogue(agent_name: str, provider: str, num_turns: int = 3):
    """Autonomous agent dialogue"""
    print(f"[{agent_name}] Starting autonomous dialogue ({num_turns} turns)...")

    agent = CstlAgent(agent_name, provider)
    agent.register()

    # Peer assignment for dialogue
    peers = {"alice": "bob", "bob": "charlie", "charlie": "alice"}
    current_peer = peers.get(agent_name, "alice")

    for turn in range(num_turns):
        # Generate message
        prompt = f"You are agent {agent_name}. Respond briefly to: Turn {turn + 1}. Stay in character."
        message = agent.generate_response(prompt)

        print(f"[{agent_name}] Turn {turn + 1}: {message[:50]}...")

        # Send to peer
        result = agent.send_message(current_peer, message)
        if result["status"] == "error":
            print(f"[{agent_name}] ⚠️  Send error: {result.get('error')}")

        time.sleep(0.5)

    print(f"[{agent_name}] ✅ Dialogue complete")


# ============================================================================
# Main
# ============================================================================

def main():
    parser = argparse.ArgumentParser(description="CSTL LLM Agent")
    parser.add_argument("--name", default="alice", help="Agent name")
    parser.add_argument("--provider", default="hermes", help="LLM provider (hermes/anthropic/gemini)")
    parser.add_argument("--peer-mode", default="self", help="Mode: self (autonomous) or stdin")
    parser.add_argument("--turns", type=int, default=3, help="Number of dialogue turns")
    parser.add_argument("--host", default="127.0.0.1", help="Server host")
    parser.add_argument("--port", type=int, default=5050, help="Server port")

    args = parser.parse_args()

    if args.peer_mode == "self":
        run_autonomous_dialogue(args.name, args.provider, args.turns)
    else:
        print(f"[{args.name}] Peer-mode '{args.peer_mode}' not implemented")


if __name__ == "__main__":
    import os
    main()
