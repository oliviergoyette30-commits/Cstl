#!/usr/bin/env python3
"""
cstl_llm_agent.py -- agent LLM reel branche sur le serveur CSTL v5.0.0 par
TCP, via cstl_client.py (feature C, 2026-09-04). Stdlib + `cryptography`
(Ed25519) + `anthropic` et/ou `google-genai` (optionnels, degradation
propre si absents -- voir GeminiAgentBrain, ajoute le 2026-09-05).

Ce que ce fichier ferme concretement: avant lui, CSTL n'avait jamais eu
qu'un seul "agent" reel -- l'utilisateur humain, tapant des payloads a la
main ou via des smoke-tests scriptes. Ici, un LLM genere lui-meme le
contenu semantique d'un message CSTL (purpose, relations), le fait signer
en Ed25519 (src/signing.rs cote serveur), s'enregistre dynamiquement
(purpose=agent_register), puis dialogue avec le serveur exactement comme
n'importe quel autre client cstl_client.py.

Point le plus a risque de tout ce module, isole et VERIFIE en direct dans
cette session (voir examples/print_signing_bytes_fixture.rs, meme fixture
hash-comparee octet par octet cote Rust et cote Python): cstl_signing_bytes
ci-dessous doit reproduire EXACTEMENT la canonicalisation de
src/server/audit.rs::signing_bytes -- toute derive casserait
silencieusement toute signature produite par cet agent.

Limite honnete assumee (documentee dans le plan avant l'implementation,
PAS une decouverte apres coup): le serveur est strictement requete/reponse
par connexion TCP -- deux processus d'agents independants ne peuvent PAS
recevoir de messages non sollicites l'un de l'autre via le serveur.
run_llm_relay() reprend donc la forme de cstl_orchestrator.py::run_relay:
un seul processus Python, tours alternes, un cote genere par le LLM,
l'autre par stdin (ou un second AnthropicAgentBrain si --peer-mode llm).

Ce qui N'A PAS ete verifie en direct dans ce sandbox (ni paquet
`anthropic`/`google-genai`, ni ANTHROPIC_API_KEY/GEMINI_API_KEY disponibles
ici -- confirme par `pip show anthropic`/`echo $ANTHROPIC_API_KEY` et
`echo $GEMINI_API_KEY`, tous vides, bien que `google-genai` ait ete
installe et importe avec succes pour ecrire GeminiAgentBrain): une vraie
reponse generee par un modele. `AnthropicAgentBrain.from_env()` et
`GeminiAgentBrain.from_env()` sont concus pour degrader proprement
(retournent None) dans exactement cet etat -- verifie ci-dessous, dans ce
sandbox, que c'est bien ce qui se passe. La verification AVEC un vrai
modele reste a faire par l'utilisateur sur sa propre machine (instructions
en bas de ce fichier).

Usage (une fois un vrai modele disponible, sur la machine de l'utilisateur):
    # Option Gemini -- palier gratuit reel, cle sur aistudio.google.com/apikey,
    # aucune carte de credit requise (contrairement a l'API Anthropic):
    pip install google-genai
    export GEMINI_API_KEY=...
    # Option Anthropic (API payante, distincte d'un abonnement Claude Max
    # qui ne donne acces qu'au chat, pas a l'API):
    pip install anthropic
    export ANTHROPIC_API_KEY=sk-...

    cargo run --release &                          # serveur CSTL, port 5050
    python3 sdk/python/cstl_llm_agent.py --peer-mode stdin --turns 4 --provider auto

Auto-test structurel (fonctionne sans `anthropic`/`google-genai` ni cle --
c'est ce qui EST verifiable ici):
    python3 sdk/python/cstl_llm_agent.py --structural-selftest
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import unicodedata
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cstl_client import CstlClient, CstlResponse  # noqa: E402

try:
    from cryptography.hazmat.primitives.asymmetric import ed25519
    from cryptography.hazmat.primitives import serialization
    _HAS_CRYPTOGRAPHY = True
except ImportError:
    _HAS_CRYPTOGRAPHY = False


DEFAULT_KEYFILE = Path.home() / ".cstl" / "llm_agent_ed25519.key"


# ---------------------------------------------------------------------------
# Cles Ed25519 -- persistees en clair sur disque (portee v1, meme niveau de
# confiance que le reste du projet: mono-operateur, pas de coffre-fort de
# cles). Un fichier different par agent (parametre --keyfile) permet de
# faire tourner plusieurs identites LLM en parallele.
# ---------------------------------------------------------------------------

def load_or_create_keypair(keyfile: Path) -> tuple["ed25519.Ed25519PrivateKey", str]:
    """Retourne (cle_privee, cle_publique_hex). Cree et persiste une
    nouvelle paire si `keyfile` n'existe pas encore -- sinon la recharge
    telle quelle (meme identite d'un lancement a l'autre)."""
    if not _HAS_CRYPTOGRAPHY:
        raise RuntimeError(
            "Le paquet 'cryptography' est requis pour signer les messages "
            "(pip install cryptography). Sans lui, cet agent ne peut pas "
            "s'enregistrer aupres du serveur CSTL (agent_register exige "
            "une auto-signature Ed25519)."
        )
    keyfile.parent.mkdir(parents=True, exist_ok=True)
    if keyfile.exists():
        raw = keyfile.read_bytes()
        sk = ed25519.Ed25519PrivateKey.from_private_bytes(raw)
    else:
        sk = ed25519.Ed25519PrivateKey.generate()
        raw = sk.private_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PrivateFormat.Raw,
            encryption_algorithm=serialization.NoEncryption(),
        )
        keyfile.write_bytes(raw)
        keyfile.chmod(0o600)
    pk = sk.public_key()
    pk_bytes = pk.public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    return sk, pk_bytes.hex()


# ---------------------------------------------------------------------------
# Port Python de src/server/audit.rs::signing_bytes -- DOIT rester en sync
# octet-par-octet avec l'implementation Rust. Voir
# examples/print_signing_bytes_fixture.rs pour la fixture de reference
# utilisee pour verifier ca dans cette session (hash identique confirme le
# 2026-09-04).
# ---------------------------------------------------------------------------

def _nfc(s: str) -> str:
    return unicodedata.normalize("NFC", s)


def cstl_signing_bytes(version: str, mode: str, meta: dict, intent: dict,
                        relations: list[dict]) -> bytes:
    canon = "VERSION|" + _nfc(version) + "\nMODE|" + _nfc(mode)

    canon += "\nMETA"
    for k in sorted(meta.keys()):
        if k == "PARENT_HASH":
            continue
        canon += "|" + _nfc(k) + "=" + _nfc(str(meta[k]))

    canon += "\nINTENT"
    for k in sorted(intent.keys()):
        if k == "signature":
            continue
        canon += "|" + _nfc(k) + "=" + _nfc(str(intent[k]))

    rel_strings = []
    for r in relations:
        kv = sorted(r.items())
        rel_strings.append(",".join(f"{_nfc(k)}={_nfc(str(v))}" for k, v in kv))
    rel_strings.sort()

    canon += "\nRELATIONS"
    for r in rel_strings:
        canon += "|" + r

    return canon.encode("utf-8")


def sign_intent(sk: "ed25519.Ed25519PrivateKey", pubkey_hex: str, *,
                 version: str, mode: str, meta: dict, intent: dict,
                 relations: list[dict]) -> str:
    """Signe (meta+public_key, intent SANS signature, relations) et
    retourne la signature en hex (128 caracteres). meta doit deja
    contenir public_key -- signing_bytes l'inclut deliberement (voir
    commentaire de src/server/audit.rs::signing_bytes: lie la signature a
    la cle revendiquee)."""
    assert meta.get("public_key") == pubkey_hex, "meta['public_key'] doit etre fixe AVANT de signer"
    msg = cstl_signing_bytes(version, mode, meta, intent, relations)
    sig = sk.sign(msg)
    return sig.hex()


# ---------------------------------------------------------------------------
# Enregistrement dynamique (purpose=agent_register, feature B-2 cote Rust)
# ---------------------------------------------------------------------------

def register_agent(client: CstlClient, sk: "ed25519.Ed25519PrivateKey",
                    pubkey_hex: str, name: str,
                    capabilities: list[str] | None = None,
                    trust_score: float | None = None) -> CstlResponse:
    capabilities = capabilities or ["communication"]
    meta = {"encoder": "LlmAgent", "produced_by": name, "public_key": pubkey_hex}
    intent = {
        "purpose": "agent_register",
        "sender": name,
        "receiver": "server",
        "name": name,
        # ';' et non ',' -- le SDK auto-quote une valeur contenant une
        # virgule (cstl_client.py::_quote_if_needed), ce qui casserait le
        # split cote handler.rs (capabilities.split(';')). Voir le plan
        # (section B-2) pour cette decision.
        "capabilities": ";".join(capabilities),
    }
    if trust_score is not None:
        intent["trust_score"] = str(trust_score)

    sig_hex = sign_intent(sk, pubkey_hex, version="v5.0.0", mode="A",
                           meta=meta, intent=intent, relations=[])
    intent["signature"] = sig_hex

    payload = client.build_payload(
        encoder=meta["encoder"], produced_by=meta["produced_by"],
        purpose="agent_register", sender=name, receiver="server",
        extra_meta={"public_key": pubkey_hex},
        extra_intent={k: v for k, v in intent.items()
                      if k not in ("purpose", "sender", "receiver")},
    )
    return client.send_raw(payload)


def send_signed_relation(client: CstlClient, sk: "ed25519.Ed25519PrivateKey",
                          pubkey_hex: str, *, name: str, receiver: str,
                          purpose: str, relations: list[dict],
                          extra_intent: dict | None = None) -> CstlResponse:
    """Equivalent signe de CstlClient.send_relation -- une fois l'agent
    enregistre avec une public_key, le serveur EXIGE une signature valide
    sur chaque message de cet expediteur (src/signing.rs, STEP 2a de
    handler.rs)."""
    meta = {"encoder": "LlmAgent", "produced_by": name, "public_key": pubkey_hex}
    intent = {"purpose": purpose, "sender": name, "receiver": receiver}
    if extra_intent:
        intent.update(extra_intent)

    sig_hex = sign_intent(sk, pubkey_hex, version="v5.0.0", mode="A",
                           meta=meta, intent=intent, relations=relations)
    intent["signature"] = sig_hex

    payload = client.build_payload(
        encoder=meta["encoder"], produced_by=meta["produced_by"], purpose=purpose,
        sender=name, receiver=receiver, relations=relations,
        extra_meta={"public_key": pubkey_hex},
        extra_intent={k: v for k, v in intent.items()
                      if k not in ("purpose", "sender", "receiver")},
    )
    return client.send_raw(payload)


# ---------------------------------------------------------------------------
# Cerveau LLM -- degradation propre si `anthropic` ou la cle sont absents,
# meme patron que TelegramNotifier::from_env() (src/telegram_council.rs).
# ---------------------------------------------------------------------------

@dataclass
class AnthropicAgentBrain:
    client: object
    model: str

    @classmethod
    def from_env(cls, model: str = "claude-sonnet-4-5") -> Optional["AnthropicAgentBrain"]:
        try:
            import anthropic  # type: ignore
        except ImportError:
            print("[AnthropicAgentBrain] paquet 'anthropic' absent -- pip install anthropic. Degradation: None.", file=sys.stderr)
            return None
        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            print("[AnthropicAgentBrain] ANTHROPIC_API_KEY absent de l'environnement. Degradation: None.", file=sys.stderr)
            return None
        client = anthropic.Anthropic(api_key=api_key)
        return cls(client=client, model=model)

    def generate_relation(self, topic: str, peer_message: str | None) -> dict:
        """Demande au modele UNE relation CSTL (type/subject/object) sur
        `topic`, en tenant compte du dernier message du pair le cas
        echeant. Le modele repond en JSON strict pour rester
        deterministe a parser -- pas de negociation de format en texte
        libre."""
        prompt = (
            "Tu es un agent CSTL. Reponds UNIQUEMENT avec un objet JSON "
            '{"type": "...", "subject": "...", "object": "..."} '
            "representant une relation factuelle ou une prise de position "
            f"sur le sujet suivant: {topic!r}."
        )
        if peer_message:
            prompt += f" Le pair vient de dire: {peer_message!r}."
        response = self.client.messages.create(
            model=self.model,
            max_tokens=256,
            messages=[{"role": "user", "content": prompt}],
        )
        text = "".join(block.text for block in response.content if hasattr(block, "text"))
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            start, end = text.find("{"), text.rfind("}")
            if start != -1 and end != -1:
                return json.loads(text[start:end + 1])
            raise ValueError(f"reponse du modele non-JSON: {text!r}")


# ---------------------------------------------------------------------------
# Cerveau Gemini -- meme interface (generate_relation) et meme patron
# from_env() qu'AnthropicAgentBrain ci-dessus, ajoute le 2026-09-05.
#
# Motivation: Gemini a un palier gratuit reel (cle sur
# aistudio.google.com/apikey, aucune carte de credit requise), contrairement
# a l'API Anthropic -- ce qui rend un VRAI tour de dialogue LLM->serveur
# CSTL accessible a l'utilisateur sans frais, l'API Claude payante restant
# hors de portee ici (l'abonnement Max de l'utilisateur ne donne pas acces
# a l'API, seulement au chat).
#
# `run_llm_relay` traite `AnthropicAgentBrain` et `GeminiAgentBrain` de
# facon polymorphe par duck-typing (les deux exposent `generate_relation`)
# -- pas d'ABC commune ajoutee ici pour rester coherent avec le fait
# qu'AnthropicAgentBrain elle-meme n'en avait pas.
#
# Verifie dans ce sandbox (2026-09-05): le paquet `google-genai` (SDK
# unifie actuel, PAS l'ancien `google-generativeai` deprecie) s'installe et
# s'importe avec succes. AUCUNE cle (`GEMINI_API_KEY`) n'est presente ici
# -- seule la degradation propre de from_env() (retourne None) a pu etre
# verifiee en direct, jamais un vrai tour de dialogue contre l'API Gemini.
# ---------------------------------------------------------------------------

@dataclass
class GeminiAgentBrain:
    client: object
    model: str

    # Defaut corrige le 2026-09-05 (gemini-2.0-flash -> gemini-3.6-flash),
    # sur un vrai appel de l'utilisateur contre l'API Gemini reelle: erreur
    # 404 explicite de Google ("This model models/gemini-2.0-flash is no
    # longer available... use models/gemini-3.6-flash"). Pas une supposition
    # -- le nom du modele courant vient directement de la reponse d'erreur
    # de l'API, la seule verification live possible sans cle dans ce
    # sandbox. Un modele nomme dans le code a une duree de vie courte cote
    # fournisseur -- si ce defaut echoue de nouveau avec un 404 similaire,
    # passer --model/model= explicitement plutot que de patcher ce fichier.
    @classmethod
    def from_env(cls, model: str = "gemini-3.6-flash") -> Optional["GeminiAgentBrain"]:
        try:
            from google import genai  # type: ignore
        except ImportError:
            print("[GeminiAgentBrain] paquet 'google-genai' absent -- pip install google-genai. Degradation: None.", file=sys.stderr)
            return None
        api_key = os.environ.get("GEMINI_API_KEY")
        if not api_key:
            print("[GeminiAgentBrain] GEMINI_API_KEY absent de l'environnement. Degradation: None.", file=sys.stderr)
            return None
        client = genai.Client(api_key=api_key)
        return cls(client=client, model=model)

    def generate_relation(self, topic: str, peer_message: str | None) -> dict:
        """Meme contrat que AnthropicAgentBrain.generate_relation: un objet
        JSON {"type", "subject", "object"}, meme prompt (a la formulation
        pres, inevitable entre deux API differentes), meme tolerance de
        parsing (texte autour du JSON)."""
        prompt = (
            "Tu es un agent CSTL. Reponds UNIQUEMENT avec un objet JSON "
            '{"type": "...", "subject": "...", "object": "..."} '
            "representant une relation factuelle ou une prise de position "
            f"sur le sujet suivant: {topic!r}."
        )
        if peer_message:
            prompt += f" Le pair vient de dire: {peer_message!r}."
        response = self.client.models.generate_content(model=self.model, contents=prompt)
        text = response.text or ""
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            start, end = text.find("{"), text.rfind("}")
            if start != -1 and end != -1:
                return json.loads(text[start:end + 1])
            raise ValueError(f"reponse du modele non-JSON: {text!r}")


def resolve_brain(provider: str, model: str | None = None) -> Optional[object]:
    """`provider` in {"anthropic", "gemini", "auto"}. "auto" essaie Gemini
    en premier (palier gratuit reel, accessible sans frais a l'utilisateur)
    puis retombe sur Anthropic -- ordre delibere, pas arbitraire.

    `model` surcharge le defaut de chaque backend quand fourni -- utile si
    le fournisseur retire encore un nom de modele (voir le commentaire
    au-dessus de GeminiAgentBrain.from_env(): deja arrive une fois avec
    gemini-2.0-flash -> gemini-3.6-flash, decouvert via l'erreur 404 d'un
    vrai appel utilisateur) sans avoir a patcher ce fichier."""
    kwargs = {"model": model} if model else {}
    if provider == "anthropic":
        return AnthropicAgentBrain.from_env(**kwargs)
    if provider == "gemini":
        return GeminiAgentBrain.from_env(**kwargs)
    if provider == "auto":
        return GeminiAgentBrain.from_env(**kwargs) or AnthropicAgentBrain.from_env(**kwargs)
    raise ValueError(f"provider inconnu: {provider!r}")


# ---------------------------------------------------------------------------
# Relais tours-alternes (mirroir de cstl_orchestrator.py::run_relay)
# ---------------------------------------------------------------------------

def run_llm_relay(host: str, port: int, keyfile: Path, agent_name: str,
                   topic: str, turns: int, peer_mode: str, provider: str = "auto",
                   model: str | None = None) -> int:
    if model and provider == "auto":
        print(
            "[LlmAgent] ECHEC: --model exige --provider anthropic|gemini explicite "
            "(en mode auto, un seul nom de modele ne peut pas s'appliquer aux deux "
            "fournisseurs sans ambiguite).",
            file=sys.stderr,
        )
        return 3

    sk, pubkey_hex = load_or_create_keypair(keyfile)
    client = CstlClient(host=host, port=port, timeout=15.0)

    print(f"[LlmAgent] identite: {agent_name} (public_key={pubkey_hex[:16]}...)")
    reg = register_agent(client, sk, pubkey_hex, agent_name)
    print(f"[LlmAgent] agent_register -> purpose={reg.purpose!r} fields={reg.fields}")
    if reg.purpose != "agent_register_ack":
        print("[LlmAgent] ECHEC: enregistrement refuse, arret.", file=sys.stderr)
        return 1

    brain = resolve_brain(provider, model)
    if brain is None:
        print(
            "[LlmAgent] Aucun modele disponible pour provider="
            f"{provider!r} (paquet 'anthropic'/'google-genai' ou "
            "ANTHROPIC_API_KEY/GEMINI_API_KEY manquant) -- l'enregistrement "
            "et la signature sont verifies, mais aucun tour de dialogue "
            "reel ne peut avoir lieu ici. Gemini a un palier gratuit "
            "(cle sur aistudio.google.com/apikey) si l'API Anthropic n'est "
            "pas disponible. Voir le README du SDK pour verifier cette "
            "partie sur une machine avec un vrai modele.",
            file=sys.stderr,
        )
        return 2
    print(f"[LlmAgent] modele: {type(brain).__name__} ({brain.model})")

    peer_message: str | None = None
    for turn in range(1, turns + 1):
        print(f"\n[LlmAgent] --- tour {turn}/{turns} ---")
        relation = brain.generate_relation(topic, peer_message)
        resp = send_signed_relation(
            client, sk, pubkey_hex, name=agent_name, receiver="server",
            purpose="llm_relay_turn", relations=[relation],
        )
        print(f"[LlmAgent] envoye: {relation} -> status={resp.status!r} audit_hash={resp.audit_hash!r}")

        if peer_mode == "stdin":
            peer_message = input("[Pair humain] Reponse (vide pour arreter): ").strip() or None
            if peer_message is None:
                break
        else:
            peer_message = json.dumps(relation, ensure_ascii=False)

    client.close()
    return 0


# ---------------------------------------------------------------------------
# Auto-test structurel -- la seule verification possible dans un sandbox
# sans 'anthropic' ni cle API. Ne se connecte a AUCUN serveur reseau.
# ---------------------------------------------------------------------------

def _structural_selftest() -> int:
    failures = []

    print("[1/4] cryptography disponible et generation de cle...")
    if not _HAS_CRYPTOGRAPHY:
        failures.append("le paquet 'cryptography' est absent -- ne peut pas etre teste plus loin")
    else:
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            keyfile = Path(tmp) / "test.key"
            sk1, pk1 = load_or_create_keypair(keyfile)
            sk2, pk2 = load_or_create_keypair(keyfile)  # doit recharger la MEME cle
            if pk1 != pk2:
                failures.append("load_or_create_keypair ne recharge pas la meme identite au 2e appel")
            else:
                print(f"      OK (public_key stable: {pk1[:16]}...)")

    print("[2/4] cstl_signing_bytes -- fixture croisee Rust/Python...")
    # Fixture IDENTIQUE a examples/print_signing_bytes_fixture.rs, verifiee
    # manuellement octet-par-octet dans cette session (2026-09-04).
    expected_hex = (
        "56455253494f4e7c76352e302e300a4d4f44457c410a4d4554417c656e636f"
        "6465723d4c6c6d4167656e747c70726f64756365645f62793d636166c3a95f"
        "6167656e747c7075626c69635f6b65793d61616262636364640a494e54454e"
        "547c6e6f74653d612c20627c707572706f73653d63726f73735f6c616e675f"
        "666978747572657c72656365697665723d7365727665727c73656e6465723d"
        "6c6c6d5f6167656e740a52454c4154494f4e537c6f626a6563743d4d6f6e74"
        "72c3a9616c2c7375626a6563743d636166c3a95f746573742c747970653d62"
        "6f726e5f696e"
    ).replace("\n", "")
    meta = {
        "encoder": "LlmAgent", "produced_by": "café_agent",
        "public_key": "aabbccdd", "PARENT_HASH": "sha256:doitetreexclu",
    }
    intent = {
        "purpose": "cross_lang_fixture", "sender": "llm_agent", "receiver": "server",
        "note": "a, b", "signature": "doitetreexclu",
    }
    relations = [{"type": "born_in", "subject": "café_test", "object": "Montréal"}]
    got = cstl_signing_bytes("v5.0.0", "A", meta, intent, relations).hex()
    if got != expected_hex:
        failures.append(f"cstl_signing_bytes DIVERGE du fixture Rust de reference:\n  got={got}\n  exp={expected_hex}")
    else:
        print("      OK (identique octet-par-octet a signing_bytes() cote Rust)")

    print("[3/4] AnthropicAgentBrain.from_env() degrade proprement sans paquet/cle...")
    brain = AnthropicAgentBrain.from_env()
    if brain is not None:
        print("      (un vrai modele EST disponible ici -- pas l'etat attendu de ce sandbox, mais pas un echec)")
    else:
        print("      OK (None, comme attendu dans un environnement sans 'anthropic'/ANTHROPIC_API_KEY)")

    print("[4/4] GeminiAgentBrain.from_env() degrade proprement sans paquet/cle...")
    gemini_brain = GeminiAgentBrain.from_env()
    if gemini_brain is not None:
        print("      (un vrai modele EST disponible ici -- pas l'etat attendu de ce sandbox, mais pas un echec)")
    else:
        print("      OK (None, comme attendu dans un environnement sans 'google-genai'/GEMINI_API_KEY)")

    print()
    if failures:
        print(f"❌ {len(failures)} echec(s):")
        for f in failures:
            print(f"   - {f}")
        return 1
    print("✅ Auto-test structurel reussi (aucune verification reseau/LLM reelle -- voir le README).")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5050)
    ap.add_argument("--keyfile", type=Path, default=DEFAULT_KEYFILE)
    ap.add_argument("--name", default="llm_agent")
    ap.add_argument("--topic", default="Est-ce que Montreal est au Canada?")
    ap.add_argument("--turns", type=int, default=3)
    ap.add_argument("--peer-mode", choices=["stdin", "self"], default="self",
                     help="'stdin': un humain repond a chaque tour. 'self': le meme LLM alimente les deux cotes.")
    ap.add_argument("--provider", choices=["anthropic", "gemini", "auto"], default="auto",
                     help="Backend LLM. 'auto' (defaut) essaie Gemini en premier (palier gratuit reel) "
                          "puis retombe sur Anthropic si Gemini est indisponible.")
    ap.add_argument("--model", default=None,
                     help="Surcharge le nom de modele par defaut du backend choisi -- exige "
                          "--provider anthropic|gemini explicite (pas 'auto'). Utile si le "
                          "fournisseur retire le modele par defaut code ici (deja arrive une "
                          "fois avec gemini-2.0-flash, voir GeminiAgentBrain.from_env()).")
    ap.add_argument("--structural-selftest", action="store_true",
                     help="Verifie signing_bytes/keypair/degradation sans toucher au reseau.")
    args = ap.parse_args()

    if args.structural_selftest:
        return _structural_selftest()

    return run_llm_relay(args.host, args.port, args.keyfile, args.name,
                          args.topic, args.turns, args.peer_mode, args.provider, args.model)


if __name__ == "__main__":
    sys.exit(main())
