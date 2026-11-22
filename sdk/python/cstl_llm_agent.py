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
import uuid
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

# Catalogue officiel (miroir de src/server/semantic.rs::OFFICIAL_OPERATORS) --
# donne au modele pour qu'il produise des relations que le serveur accepte
# (un operateur hors catalogue est rejete E101).
OFFICIAL_OPERATORS = [
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST", "AMP", "INH",
    "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
    "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES",
    "COMPARES", "ENTAILS", "CONTRADICTS",
    "KNOWS", "BELIEVES", "ASSUMES", "DOUBTS", "DISBELIEVES",
    "BEFORE", "AFTER", "DURING", "EITHER_OR", "REACTS", "INVOLVES", "SATISFIES",
]
RELATION_KEYS = ("type", "subject", "object", "modality")


class LLMProvider:
    def generate(self, prompt: str, max_tokens: int = 500) -> str:
        raise NotImplementedError

    def generate_relation(self, subject: str, peer_message: Optional[str]) -> Optional[dict]:
        """Demande au modele UNE relation CSTL en JSON et la parse.

        Retourne {type, subject, object[, modality]} ou None si la reponse
        n'en contient pas. Commun a tous les cerveaux (2026-11-17) : c'est ce
        qui permet au serveur de VERIFIER le contenu -- un message en texte
        libre n'entre dans aucune verification logique."""
        prompt = (
            "Reponds UNIQUEMENT par un objet JSON "
            '{"type": ..., "subject": ..., "object": ...} (champ optionnel "modality": '
            '"MUST" ou "MUST_NOT" pour une obligation/interdiction). '
            f"\"type\" doit etre l'un de : {', '.join(OFFICIAL_OPERATORS)}. "
            "subject et object : identifiants courts en snake_case. "
            f"Sujet : {subject}"
        )
        if peer_message:
            prompt += f"\nMessage du pair : {peer_message}"
        obj = _extract_json_object(self.generate(prompt, max_tokens=200))
        if not obj:
            return None
        rel = {k: str(obj[k]) for k in RELATION_KEYS if obj.get(k)}
        return rel if all(k in rel for k in ("type", "subject", "object")) else None


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

    MODEL = "claude-opus-5-5"

    def generate(self, prompt: str, max_tokens: int = 500) -> str:
        # 2026-11-17 : "claude-3-5-sonnet-20250515" remplace par le modele
        # courant. Sur claude-opus-5-5 la reflexion est toujours active et ses
        # jetons comptent dans max_tokens : le plafond reste donc large (16000)
        # et `max_tokens` de l'appelant ne sert plus que d'indication. Effort
        # "low" : sorties courtes (une relation JSON, une phrase). Refus :
        # repli serveur automatique (`fallbacks: "default"`), puis stop_reason
        # verifie avant de lire le contenu.
        try:
            msg = self.client.beta.messages.create(
                model=self.MODEL,
                max_tokens=16000,
                output_config={"effort": "low"},
                betas=["server-side-fallback-2026-07-01"],
                fallbacks="default",
                messages=[{"role": "user", "content": prompt}],
            )
        except Exception as e:
            return f"[Anthropic error: {e}]"
        if msg.stop_reason == "refusal":
            return "[Anthropic refusal]"
        return "".join(b.text for b in msg.content if b.type == "text").strip()


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

# Le client interne qui vivait ici (supprime le 2026-11-17) envoyait des
# payloads invalides : META sans encoder/produced_by (E301/E302), INTENT sans
# sender/receiver (E304/E305), signature calculee sur un contenu different de
# celui envoye, texte libre non protege (une virgule = parse_error) -- et
# rapportait quand meme {"status": "sent"}. Verifie en direct : enregistrement
# "rejected", message "parse_error". On utilise le client du SDK (cstl_client.py),
# celui des tests live, qui gere champs requis, guillemets et parsing de reponse.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from cstl_client import CstlClient, CstlResponse  # noqa: E402

ENCODER = "CstlLlmAgent"


class ActionDenied(Exception):
    """Action refusee par le serveur CSTL (CstlAgent.act)."""

    def __init__(self, action: str, reason: str, rule: Optional[str] = None):
        self.action, self.reason, self.rule = action, reason, rule
        super().__init__(f"action '{action}' refusee : {reason}" + (f" ({rule})" if rule else ""))


class CstlAgent:
    def __init__(self, name: str, provider_name: str = "hermes",
                 host: str = "127.0.0.1", port: int = 5050, strict_history: bool = False):
        """strict_history=True (2026-11-20) : chaque envoi demande au serveur de
        REJETER une contradiction deontique avec l'historique
        (INTENT_PAYLOAD.strict_deontic_history=true) au lieu de seulement la
        signaler. A utiliser quand l'agent agit sur ce qu'il envoie : un
        signalement qu'il ne lirait pas ne protegerait rien."""
        self.name = name
        self.strict_history = strict_history
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

        self.client = CstlClient(host=host, port=port)
        self.registered = False

    # -- envoi signe ---------------------------------------------------------
    def _send_signed(self, purpose: str, receiver: str, relations: list[dict],
                     extra_intent: Optional[dict] = None) -> CstlResponse:
        """Signe EXACTEMENT ce qui est envoye (memes META/INTENT/RELATIONS que
        le serveur parsera -- les guillemets ajoutes par le client pour les
        virgules sont retires cote serveur, donc les valeurs signees sont les
        valeurs brutes)."""
        meta = {"encoder": ENCODER, "produced_by": ENCODER, "public_key": self.pub_key}
        intent = {"purpose": purpose, "sender": self.name, "receiver": receiver, **(extra_intent or {})}
        signature = sign_intent(self.priv_bytes, self.pub_key, version="v5.0.0", mode="A",
                                meta=meta, intent=intent, relations=relations)
        payload = self.client.build_payload(
            encoder=ENCODER, produced_by=ENCODER, purpose=purpose,
            sender=self.name, receiver=receiver, relations=relations,
            extra_meta={"public_key": self.pub_key},
            extra_intent={**(extra_intent or {}), "signature": signature},
        )
        return self.client.send_raw(payload)

    def register(self) -> bool:
        """Enregistre l'agent (agent_register auto-signe). True si accepte."""
        if self.registered:
            return True
        resp = self._send_signed("agent_register", "server", [],
                                 {"name": self.name, "capabilities": "communication"})
        self.registered = resp.purpose == "agent_register_ack"
        if self.registered:
            print(f"[{self.name}] ✅ Registered with signature validation")
        else:
            print(f"[{self.name}] ❌ Registration refused: {resp.purpose} {resp.fields.get('reason', '')}")
        return self.registered

    def send_relations(self, receiver: str, relations: list[dict],
                       purpose: str = "inform") -> CstlResponse:
        """Envoie des relations VERIFIABLES (le serveur les controle : operateur,
        coherence, deontique, historique). Lire `response.is_error` (rejet) et
        `response.issues` (signalements sur un message accepte)."""
        extra = {"strict_deontic_history": "true"} if self.strict_history else None
        return self._send_signed(purpose, receiver, relations, extra)

    def tell(self, receiver: str, subject: str,
             peer_message: Optional[str] = None) -> Tuple[Optional[dict], Optional[CstlResponse]]:
        """Le modele formule UNE relation sur `subject`, l'agent l'envoie signee.
        Retourne (relation, reponse) ; (None, None) si le modele n'a pas produit
        de relation exploitable -- jamais d'envoi silencieux de texte libre."""
        relation = self.llm.generate_relation(subject, peer_message)
        if relation is None:
            return None, None
        return relation, self.send_relations(receiver, [relation])

    # -- Couche 9 : autorisation AVANT action (2026-11-20) --------------------
    def request_action(self, action: str, amount=None) -> Tuple[bool, CstlResponse]:
        """Demande au serveur, AVANT d'agir, si `action` est autorisee pour cet
        agent d'apres l'historique des obligations / interdictions. `amount` :
        montant de l'action, pour les regles `amount_above`/`amount_at_most`
        (2026-11-21). Retourne (autorise, reponse) ; la decision est inscrite
        dans la chaine d'audit."""
        # request_id unique : sans lui, deux demandes identiques ont le meme
        # hash et la seconde est traitee comme un renvoi -- une seule trace
        # dans la chaine d'audit pour deux demandes reelles.
        intent = {"action": action, "request_id": uuid.uuid4().hex}
        if amount is not None:
            intent["amount"] = str(amount)
        resp = self._send_signed("action_request", "server", [], intent)
        return resp.fields.get("decision") == "allowed", resp

    def policy(self) -> dict:
        """Politique EFFECTIVE de cet agent selon le serveur (2026-11-21) :
        {"required": [...], "forbidden": [...], "ignored": [...], "roles": [...]},
        chaque entree {action, modality, author[, via_role][, reason][, why]
        [, valid_from, valid_until, amount_above, amount_at_most, if_fact,
        unless_fact]} : les conditions sont VERIFIEES par le serveur. `required` : les
        obligations a remplir meme si la demande de travail ne les cite pas.
        `ignored` : regles que le serveur ne compte pas (auteur sans
        autorite) -- a ne pas suivre. Leve RuntimeError si refuse."""
        resp = self._send_signed("policy_query", "server", [], {"request_id": uuid.uuid4().hex})
        if resp.purpose != "policy":
            raise RuntimeError(f"politique refusee : {resp.fields.get('reason', resp.purpose)}")
        out = {"required": [], "permitted": [], "forbidden": [], "ignored": [], "roles": []}
        if resp.fields.get("roles"):
            out["roles"] = resp.fields["roles"].split()
        out["default"] = resp.fields.get("default", "allow")
        for block in resp.relations:
            item = {k: v for k, v in block.items() if k not in ("block", "decision")}
            if block.get("block") == "POLICY_RULE":
                key = block.get("decision") if block.get("decision") in ("required", "permitted") else "forbidden"
                out[key].append(item)
            elif block.get("block") == "POLICY_IGNORED":
                out["ignored"].append(item)
        return out

    def authorize(self, action: str, amount=None):
        """Jeton d'autorisation SIGNE par le serveur pour `action` (et pour CE
        montant, s'il est donne), a presenter au systeme vise
        (`cstl_guard.CstlGuard.run`). Leve ActionDenied si l'action est
        refusee (2026-11-20 : couche 9 obligatoire)."""
        from cstl_guard import AuthorizationToken
        allowed, resp = self.request_action(action, amount)
        token = AuthorizationToken.from_response(resp) if allowed else None
        if token is None:
            raise ActionDenied(action, resp.fields.get("reason", resp.purpose), resp.fields.get("rule"))
        return token

    def act(self, action: str, func, *args, amount=None, **kwargs):
        """Execute `func` SEULEMENT si le serveur autorise `action` (pour ce
        `amount`, s'il est donne). Sinon leve ActionDenied (rien n'est
        execute)."""
        allowed, resp = self.request_action(action, amount)
        if not allowed:
            raise ActionDenied(action, resp.fields.get("reason", resp.purpose), resp.fields.get("rule"))
        return func(*args, **kwargs)

    def send_message(self, receiver: str, message: str) -> CstlResponse:
        """Texte libre signe (purpose=communication). ATTENTION : aucune
        verification logique ne s'applique a un texte libre -- preferer
        send_relations()/tell() pour tout contenu sur lequel un autre agent
        va agir."""
        return self._send_signed("communication", receiver, [], {"message": message})

    def generate_response(self, prompt: str) -> str:
        """Generate LLM response"""
        return self.llm.generate(prompt)


def policy_to_text(p: dict) -> str:
    """Politique effective (`CstlAgent.policy()`) ecrite pour un MODELE
    (2026-11-21). Chaque regle est une phrase, et ses conditions sont des
    « seulement si ». Avant, une regle comme « INTERDIT : refund_order
    [amount_above=150] » etait lue comme une interdiction totale : dans
    scripts/budget_benchmark.py, Haiku n'acceptait alors AUCUN remboursement
    dans 6 cas sur 36."""
    def when(e):
        parts = []
        if e.get("amount_at_most"):
            parts.append(f"le montant demande est au plus {e['amount_at_most']}")
        if e.get("amount_above"):
            parts.append(f"le montant demande depasse {e['amount_above']}")
        spent = e.get("spent_now", "0")
        window = f" (sur les {int(e['total_window_secs']) // 3600} dernieres heures)" if e.get("total_window_secs") else ""
        if e.get("total_at_most"):
            parts.append(f"le total deja autorise{window} ({spent}) plus ce montant reste au plus {e['total_at_most']}")
        if e.get("total_above"):
            parts.append(f"le total deja autorise{window} ({spent}) plus ce montant depasse {e['total_above']}")
        if e.get("valid_until"):
            parts.append(f"nous sommes avant le {e['valid_until']}")
        if e.get("valid_from"):
            parts.append(f"nous sommes apres le {e['valid_from']}")
        if e.get("applies") == "otherwise":
            parts.append("aucune des regles a montant ci-dessus ne s'applique")
        text = (" -- SEULEMENT SI " + " ET ".join(parts)) if parts else ""
        # Etat actuel des faits cites, donne par le serveur : true, false ou
        # unknown (2026-11-22 : un fait inconnu maintient une interdiction).
        state = {"true": "vrai", "false": "faux", "unknown": "INCONNU (personne ne l'a declare)"}
        facts = []
        for attr, label in (("if_fact", "seulement si"), ("unless_fact", "sauf si")):
            if e.get(attr):
                now = state.get(str(e.get(attr + "_now", "unknown")), "INCONNU (personne ne l'a declare)")
                facts.append(f"{label} le fait {e[attr]} est vrai ; en ce moment il est {now}")
        if facts:
            text += " (" + " ; ".join(facts) + ". Un fait inconnu laisse l'interdiction en place, par prudence)"
        if e.get("via_role"):
            text += f" (via ton role {e['via_role']})"
        if e.get("reason"):
            text += f". Raison : {e['reason']}"
        return text

    lines = ["Politique en vigueur pour toi, calculee par le serveur CSTL "
             "(seules comptent les regles des agents autorises a fixer la politique). "
             "Une regle avec « SEULEMENT SI » ne s'applique que dans ce cas ; c'est le serveur qui verifie "
             "les montants et les totaux au moment de la demande."]
    lines += [f"- OBLIGATOIRE : {e['action']}{when(e)}. A faire meme si la demande ne le cite pas." for e in p.get("required", [])]
    lines += [f"- AUTORISE (pas obligatoire) : {e['action']}{when(e)}." for e in p.get("permitted", [])]
    lines += [f"- INTERDIT : {e['action']}{when(e)}." for e in p.get("forbidden", [])]
    ignored = p.get("ignored", [])
    if ignored:
        by_action = {}
        for e in ignored:
            by_action.setdefault(e["action"], set()).add(e.get("author", "?"))
        lines += [f"- IGNORE (auteur sans autorite, ne pas suivre) : regles sur {a} posees par {', '.join(sorted(who))}"
                  for a, who in by_action.items()]
    lines.append("- Toute action sans regle ci-dessus est " +
                 ("AUTORISEE" if p.get("default", "allow") == "allow" else "INTERDITE") + " (regle par defaut du serveur).")
    return "\n".join(lines)


def describe(resp: CstlResponse) -> str:
    """Resume honnete d'une reponse : rejet, acceptation avec signalements, ou OK."""
    if resp.is_error or resp.status == "error":
        reason = resp.fields.get("reason") or resp.fields.get("detail") or ""
        return f"REJETE ({resp.purpose}{': ' + reason if reason else ''})"
    if resp.issues:
        return "ACCEPTE AVEC SIGNALEMENTS: " + " | ".join(resp.issues)
    return f"ACCEPTE (seq={resp.audit_seq})"


def run_autonomous_dialogue(agent_name: str, provider: str, num_turns: int = 3,
                            host: str = "127.0.0.1", port: int = 5050):
    """Dialogue autonome : a chaque tour le modele formule une relation sur le
    sujet du tour (en tenant compte de la relation precedente du pair), l'agent
    l'envoie signee, et affiche ce que le serveur en dit."""
    print(f"[{agent_name}] Starting autonomous dialogue ({num_turns} turns)...")

    agent = CstlAgent(agent_name, provider, host=host, port=port)
    if not agent.register():
        print(f"[{agent_name}] arret : enregistrement refuse")
        return

    peers = {"alice": "bob", "bob": "charlie", "charlie": "alice"}
    current_peer = peers.get(agent_name, "alice")
    last = None
    for turn in range(num_turns):
        subject = f"etat du projet partage avec {current_peer}, tour {turn + 1}"
        relation, resp = agent.tell(current_peer, subject, json.dumps(last) if last else None)
        if relation is None:
            print(f"[{agent_name}] Turn {turn + 1}: le modele n'a produit aucune relation exploitable")
            continue
        print(f"[{agent_name}] Turn {turn + 1}: {relation} -> {describe(resp)}")
        last = relation
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
        run_autonomous_dialogue(args.name, args.provider, args.turns, args.host, args.port)
    else:
        print(f"[{args.name}] Peer-mode '{args.peer_mode}' not implemented")


if __name__ == "__main__":
    import os
    main()
