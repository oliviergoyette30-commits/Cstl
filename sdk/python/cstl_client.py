#!/usr/bin/env python3
"""
cstl_client.py -- client TCP minimal pour le serveur CSTL v5.0.0
(src/server/*.rs de ce depot). Stdlib uniquement (socket, dataclasses, re).

Ce fichier NE PRETEND PAS que le serveur route un payload vers un
"Agent B" reel: verifie dans src/server/handler.rs, `registry.route(...)`
(agent_discovery.rs) n'est qu'un test d'existence, jamais un dispatch. Ce
client parle donc au serveur exactement comme il se comporte reellement:
une requete -> une reponse, sur la meme connexion TCP.

Usage rapide:
    from cstl_client import CstlClient
    client = CstlClient()
    resp = client.send_relation(
        sender="alice", receiver="bob", purpose="test_greeting",
        relations=[{"type": "EQUALS", "subject": "x", "object": "y"}],
    )
    print(resp.status, resp.audit_hash)

Auto-test contre un serveur reellement demarre:
    cargo run --release &   # dans le depot Rust, port 5050
    python3 sdk/python/cstl_client.py --smoke-test
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 5050
END_MARKER = b"---END---"


# ---------------------------------------------------------------------------
# Exceptions typees -- derivees de purpose/status de la reponse serveur.
# Ce sont des categories REELLEMENT emises par src/server/handler.rs, pas
# des inventions: security_rejected, validation_error, parse_error,
# payload_too_large, error/no_agent.
# ---------------------------------------------------------------------------

class CstlError(Exception):
    """Base pour toute reponse d'erreur bien formee du serveur CSTL."""

    def __init__(self, response: "CstlResponse"):
        self.response = response
        super().__init__(f"{response.purpose}: {response.fields}")


class CstlSecurityRejected(CstlError):
    pass


class CstlParseError(CstlError):
    pass


class CstlValidationError(CstlError):
    pass


class CstlPayloadTooLarge(CstlError):
    pass


class CstlNoAgentError(CstlError):
    pass


_PURPOSE_TO_EXCEPTION = {
    "security_rejected": CstlSecurityRejected,
    "parse_error": CstlParseError,
    "validation_error": CstlValidationError,
    "payload_too_large": CstlPayloadTooLarge,
}


# ---------------------------------------------------------------------------
# Reponse parsee
# ---------------------------------------------------------------------------

@dataclass
class CstlResponse:
    status: str                      # META.status, ou "error" si absent
    purpose: str                     # INTENT_PAYLOAD.purpose
    fields: dict = field(default_factory=dict)      # tous les champs INTENT_PAYLOAD
    meta: dict = field(default_factory=dict)        # tous les champs META
    relations: list = field(default_factory=list)   # blocs RELATION/VERIFICATION/EMERGENCE_REPORT/SEMANTIC_WARNING
    audit_hash: str | None = None
    audit_parent_hash: str | None = None
    audit_seq: int | None = None
    consistency: dict | None = None
    raw: str = ""

    @property
    def is_error(self) -> bool:
        return self.status == "error" or self.purpose in _PURPOSE_TO_EXCEPTION

    def raise_for_status(self) -> "CstlResponse":
        """Leve l'exception typee correspondante si la reponse est une
        erreur connue du serveur; sinon retourne self (chainable)."""
        exc_cls = _PURPOSE_TO_EXCEPTION.get(self.purpose)
        if exc_cls is not None:
            raise exc_cls(self)
        if self.purpose == "error" and self.fields.get("status") == "no_agent":
            raise CstlNoAgentError(self)
        return self


# Regex generique pour une ligne de bloc "NAME [key=val, key2=val2]".
_BLOCK_RE = re.compile(r"^([A-Z_][A-Za-z0-9_]*)\s*\[(.*)\]\s*$")


def _split_top_level(value: str) -> list[str]:
    """Split sur les virgules de premier niveau uniquement, en respectant
    les guillemets doubles -- miroir de la regle appliquee par
    src/server/parser.rs cote serveur (et donc requis pour parser SES
    reponses de la meme facon)."""
    parts: list[str] = []
    current: list[str] = []
    in_quotes = False
    i = 0
    while i < len(value):
        c = value[i]
        if c == '"':
            in_quotes = not in_quotes
            current.append(c)
        elif c == "," and not in_quotes:
            parts.append("".join(current).strip())
            current = []
        else:
            current.append(c)
        i += 1
    if current:
        parts.append("".join(current).strip())
    return [p for p in parts if p]


def _parse_kv_block(inner: str) -> dict:
    kv: dict[str, str] = {}
    for part in _split_top_level(inner):
        if "=" not in part:
            continue
        key, _, val = part.partition("=")
        key = key.strip()
        val = val.strip()
        if len(val) >= 2 and val[0] == '"' and val[-1] == '"':
            val = val[1:-1]
        kv[key] = val
    return kv


def parse_response(raw: str) -> CstlResponse:
    """Parse une reponse CSTL brute (texte complet, avec ou sans le
    hashbang/---END---) en CstlResponse."""
    meta: dict = {}
    intent: dict = {}
    relations: list[dict] = []
    consistency: dict | None = None
    audit: dict | None = None

    for line in raw.splitlines():
        line = line.strip()
        if not line or line.startswith("#!") or line == "---END---":
            continue
        m = _BLOCK_RE.match(line)
        if not m:
            continue
        block_name, inner = m.group(1), m.group(2)
        kv = _parse_kv_block(inner)
        if block_name == "META":
            meta.update(kv)
        elif block_name == "INTENT_PAYLOAD":
            intent.update(kv)
        elif block_name == "CONSISTENCY":
            consistency = kv
        elif block_name == "AUDIT":
            audit = kv
        elif block_name in ("RELATION", "VERIFICATION", "EMERGENCE_REPORT", "SEMANTIC_WARNING"):
            relations.append({"block": block_name, **kv})

    purpose = intent.get("purpose", "")
    status = meta.get("status") or intent.get("status") or ("error" if purpose in _PURPOSE_TO_EXCEPTION or purpose == "error" else "processed")

    seq_raw = audit.get("seq") if audit else None
    seq_val = int(seq_raw) if seq_raw is not None and seq_raw.isdigit() else None

    return CstlResponse(
        status=status,
        purpose=purpose,
        fields=intent,
        meta=meta,
        relations=relations,
        audit_hash=(audit or {}).get("hash"),
        audit_parent_hash=(audit or {}).get("parent_hash"),
        audit_seq=seq_val,
        consistency=consistency,
        raw=raw,
    )


def _quote_if_needed(value: str) -> str:
    """Cite une valeur si elle contient une virgule ou un crochet fermant
    -- sinon le split top-level cote serveur (parser.rs) la coupe
    silencieusement avant meme d'atteindre la validation."""
    if "," in value or "]" in value:
        escaped = value.replace('"', '\\"')
        return f'"{escaped}"'
    return value


def _format_kv_block(name: str, kv: dict) -> str:
    parts = [f"{k}={_quote_if_needed(str(v))}" for k, v in kv.items() if v is not None]
    return f"{name} [{', '.join(parts)}]\n"


# ---------------------------------------------------------------------------
# Pont vers le Master Compresseur (src/compression/master.rs) -- cote EMISSION
# du bloc wire-protocol COMPRESSED_PAYLOAD (2026-10-01). Le cote RECEPTION
# vit deja dans server/parser.rs::record_compressed_payload depuis la meme
# date; ce bloc ferme la boucle pour le SDK Python.
#
# Pourquoi un sous-processus plutot qu'un port Python du format de
# compression (4 flux, 2 dictionnaires pre-entraines, delta-zigzag-varint):
# ce format est deja non-trivial et teste exhaustivement cote Rust. Un
# deuxieme port independant en Python serait une deuxieme surface pouvant
# deriver silencieusement de l'original -- voir cstl_signing.py pour le
# meme risque documente sur la canonicalisation de signature (la, le risque
# est inevitable, la signature doit etre calculee cote client avant tout
# envoi). Ici il n'y a aucune raison de le payer: la compression peut
# parfaitement vivre dans un sous-processus appele par le client. Zero
# logique dupliquee, zero drift possible.
# ---------------------------------------------------------------------------

class CstlCompressionUnavailable(RuntimeError):
    """Leve quand cstl_compress_cli est introuvable ou echoue. Jamais une
    raison de bloquer tout le client -- seulement les appels qui demandent
    explicitement un envoi compresse (meme patron de degradation propre que
    TelegramNotifier::from_env() cote Rust: absent -> None/erreur ciblee,
    jamais un crash du reste du programme)."""


def _default_compress_cli_path() -> Path:
    """cstl_compress_cli n'est jamais installe globalement -- il vit a cote
    du depot Rust. Cherche target/debug puis target/release, relatif a ce
    fichier (sdk/python/cstl_client.py -> remonte de 2 niveaux vers la
    racine du depot). Peut toujours etre surcharge via
    CSTL_COMPRESS_CLI_PATH ou le parametre binary_path explicite."""
    repo_root = Path(__file__).resolve().parents[2]
    for profile in ("release", "debug"):
        candidate = repo_root / "target" / profile / "cstl_compress_cli"
        if candidate.is_file():
            return candidate
    # Rien trouve -- retourne le chemin debug par defaut; l'appel
    # subprocess qui suit produira l'erreur FileNotFoundError explicite.
    return repo_root / "target" / "debug" / "cstl_compress_cli"


def _resolve_compress_cli_path(binary_path: str | None) -> Path:
    if binary_path:
        return Path(binary_path)
    env_path = os.environ.get("CSTL_COMPRESS_CLI_PATH")
    if env_path:
        return Path(env_path)
    return _default_compress_cli_path()


def compress_triple(defines: list[dict] | None = None,
                     relations: list[dict] | None = None,
                     uncertainty: list[dict] | None = None,
                     binary_path: str | None = None) -> str:
    """Compresse un triple defines/relations/uncertainty via le Master
    Compresseur (sous-processus cstl_compress_cli) et retourne le resultat
    en base64, pret a etre embarque dans COMPRESSED_PAYLOAD [data=<...>]."""
    cli = _resolve_compress_cli_path(binary_path)
    payload_json = json.dumps({
        "defines": defines or [],
        "relations": relations or [],
        "uncertainty": uncertainty or [],
    })
    try:
        result = subprocess.run(
            [str(cli), "compress"],
            input=payload_json.encode("utf-8"),
            capture_output=True,
            timeout=10.0,
        )
    except FileNotFoundError as e:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli introuvable a {cli} -- compile-le d'abord avec "
            f"'cargo build --release --bin cstl_compress_cli' dans le depot Rust, "
            f"ou passe binary_path= / CSTL_COMPRESS_CLI_PATH vers le binaire."
        ) from e
    if result.returncode != 0:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli compress a echoue (code {result.returncode}): "
            f"{result.stderr.decode('utf-8', errors='replace').strip()}"
        )
    return result.stdout.decode("ascii").strip()


def decompress_data(data_b64: str, binary_path: str | None = None) -> dict:
    """Inverse de compress_triple -- surtout utile pour verifier en local
    qu'un bloc COMPRESSED_PAYLOAD qu'on vient de construire redonne bien le
    triple attendu, avant de l'envoyer sur le fil."""
    cli = _resolve_compress_cli_path(binary_path)
    try:
        result = subprocess.run(
            [str(cli), "decompress"],
            input=data_b64.encode("ascii"),
            capture_output=True,
            timeout=10.0,
        )
    except FileNotFoundError as e:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli introuvable a {cli} -- compile-le d'abord avec "
            f"'cargo build --release --bin cstl_compress_cli' dans le depot Rust."
        ) from e
    if result.returncode != 0:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli decompress a echoue (code {result.returncode}): "
            f"{result.stderr.decode('utf-8', errors='replace').strip()}"
        )
    return json.loads(result.stdout.decode("utf-8"))


def _decompress_response_blocks(data_b64: str, binary_path: str | None = None) -> list[tuple[str, list[tuple[str, str]]]]:
    """Decompresse un bloc COMPRESSED_RESPONSE (voir
    src/server/response_compression.rs) via `cstl_compress_cli
    decompress-response`. Retourne une liste ordonnee de
    (nom_de_bloc, [(cle, valeur), ...]) -- PAS un schema defines/relations/
    uncertainty, contrairement a decompress_data()."""
    cli = _resolve_compress_cli_path(binary_path)
    try:
        result = subprocess.run(
            [str(cli), "decompress-response"],
            input=data_b64.encode("ascii"),
            capture_output=True,
            timeout=10.0,
        )
    except FileNotFoundError as e:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli introuvable a {cli} -- compile-le d'abord avec "
            f"'cargo build --release --bin cstl_compress_cli' dans le depot Rust."
        ) from e
    if result.returncode != 0:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli decompress-response a echoue (code {result.returncode}): "
            f"{result.stderr.decode('utf-8', errors='replace').strip()}"
        )
    blocks = json.loads(result.stdout.decode("utf-8"))
    return [(name, [(k, v) for k, v in fields]) for name, fields in blocks]


def _decode_castle_response(data_b64: str, dict_json: str | None,
                             binary_path: str | None = None) -> tuple[str, str]:
    """Decode un bloc CASTLE_RESPONSE (voir src/server/castle_wire.rs) via
    `cstl_compress_cli decode-castle-response`. CASTLE a un dictionnaire
    qui s'accumule sur la DUREE DE LA CONNEXION (contrairement a
    COMPRESSED_RESPONSE, stateless par message) -- `dict_json` est l'etat
    renvoye par l'appel PRECEDENT sur cette meme connexion (None pour le
    tout premier message), et le dict_json renvoye ici DOIT etre repasse
    au prochain appel, sous peine de desynchroniser le dictionnaire cote
    client de celui du serveur. Retourne (texte_decode, dict_json_a_jour)."""
    cli = _resolve_compress_cli_path(binary_path)
    req = json.dumps({"dict_json": dict_json, "payload_b64": data_b64})
    try:
        result = subprocess.run(
            [str(cli), "decode-castle-response"],
            input=req.encode("utf-8"),
            capture_output=True,
            timeout=10.0,
        )
    except FileNotFoundError as e:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli introuvable a {cli} -- compile-le d'abord avec "
            f"'cargo build --release --bin cstl_compress_cli' dans le depot Rust."
        ) from e
    if result.returncode != 0:
        raise CstlCompressionUnavailable(
            f"cstl_compress_cli decode-castle-response a echoue (code {result.returncode}): "
            f"{result.stderr.decode('utf-8', errors='replace').strip()}"
        )
    resp = json.loads(result.stdout.decode("utf-8"))
    return resp["decoded"], resp["dict_json"]


_COMPRESSED_RESPONSE_LINE_RE = re.compile(r"^COMPRESSED_RESPONSE\s*\[(.*)\]\s*$")
_CASTLE_RESPONSE_LINE_RE = re.compile(r"^CASTLE_RESPONSE\s*\[(.*)\]\s*$")


def _expand_compressed_response(raw: str, binary_path: str | None = None) -> str:
    """Si `raw` contient un bloc COMPRESSED_RESPONSE (voir
    maybe_compress_response cote serveur, src/server/response_compression.rs),
    le remplace par les blocs CSTL en texte clair qu'il represente -- pour
    que parse_response() (regex-based, inchangee) continue de fonctionner
    de maniere totalement transparente, compresse ou non. Si aucun bloc
    COMPRESSED_RESPONSE n'est present (cas par defaut -- le client n'a pas
    demande compress_response=true), retourne `raw` tel quel sans toucher a
    cstl_compress_cli du tout."""
    lines = raw.splitlines()
    for i, line in enumerate(lines):
        m = _COMPRESSED_RESPONSE_LINE_RE.match(line.strip())
        if not m:
            continue
        kv = _parse_kv_block(m.group(1))
        data_b64 = kv.get("data")
        if not data_b64:
            return raw  # bloc malforme cote serveur -- ne devrait jamais arriver, on ne touche a rien
        blocks = _decompress_response_blocks(data_b64, binary_path=binary_path)
        expanded_lines = []
        for name, fields in blocks:
            rendered_fields = ", ".join(f"{k}={_quote_if_needed(v)}" for k, v in fields)
            expanded_lines.append(f"{name} [{rendered_fields}]")
        lines[i:i + 1] = expanded_lines
        return "\n".join(lines)
    return raw


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------

class CstlClient:
    def __init__(self, host: str = DEFAULT_HOST, port: int = DEFAULT_PORT,
                 timeout: float = 10.0, keep_alive: bool = False,
                 compress_binary_path: str | None = None):
        self.host = host
        self.port = port
        self.timeout = timeout
        self.keep_alive = keep_alive
        self._sock: socket.socket | None = None
        # Chemin de cstl_compress_cli pour TOUT ce client -- requetes
        # (compress_triple) et reponses (_expand_compressed_response,
        # _expand_castle_response). None = auto-detection
        # (_resolve_compress_cli_path), surchargeable par constructeur ou,
        # comme avant, au cas par cas via les parametres binary_path/
        # compress_binary_path des methodes individuelles.
        self._compress_binary_path = compress_binary_path
        # Etat CASTLE (castle_wire.rs): dictionnaire serialise (JSON),
        # miroir cote client de celui que le serveur maintient pour CETTE
        # connexion. None = dictionnaire vide (debut de connexion). Doit
        # etre reinitialise des qu'une VRAIE nouvelle connexion TCP est
        # ouverte -- voir _connect() -- sinon ce client croirait connaitre
        # des symboles que le NOUVEAU serveur (ou la nouvelle connexion sur
        # le meme serveur) n'a jamais vus.
        self._castle_dict_json: str | None = None

    def _connect(self) -> socket.socket:
        if self.keep_alive and self._sock is not None:
            return self._sock
        sock = socket.create_connection((self.host, self.port), timeout=self.timeout)
        if self.keep_alive:
            self._sock = sock
        self._castle_dict_json = None
        return sock

    def close(self) -> None:
        if self._sock is not None:
            try:
                self._sock.close()
            finally:
                self._sock = None
        self._castle_dict_json = None

    def _expand_castle_response(self, raw: str) -> str:
        """Si `raw` contient un bloc CASTLE_RESPONSE (voir
        maybe_castle_compress_response cote serveur,
        src/server/castle_wire.rs), le remplace par le texte clair qu'il
        represente, en faisant avancer self._castle_dict_json pour le
        prochain appel sur cette meme connexion. Symetrique de
        _expand_compressed_response, mais STATEFUL (d'ou une methode, pas
        une fonction libre) -- CASTLE n'a de sens qu'avec keep_alive=True:
        avec le defaut (une connexion par envoi), chaque message repart
        d'un dictionnaire vide des deux cotes, documente dans le README."""
        lines = raw.splitlines()
        for i, line in enumerate(lines):
            m = _CASTLE_RESPONSE_LINE_RE.match(line.strip())
            if not m:
                continue
            kv = _parse_kv_block(m.group(1))
            data_b64 = kv.get("data")
            if not data_b64:
                return raw  # bloc malforme cote serveur -- ne devrait jamais arriver
            decoded_body, new_dict_json = _decode_castle_response(
                data_b64, self._castle_dict_json, binary_path=self._compress_binary_path,
            )
            self._castle_dict_json = new_dict_json
            lines[i:i + 1] = decoded_body.splitlines()
            return "\n".join(lines)
        return raw

    def build_payload(self, *, encoder: str, produced_by: str, purpose: str,
                       sender: str, receiver: str,
                       relations: list[dict] | None = None,
                       mode: str = "A", version: str = "v5.0.0",
                       extra_meta: dict | None = None,
                       extra_intent: dict | None = None,
                       compressed_defines: list[dict] | None = None,
                       compressed_relations: list[dict] | None = None,
                       compressed_uncertainty: list[dict] | None = None,
                       compress_binary_path: str | None = None,
                       request_compressed_response: bool = False,
                       request_castle_response: bool = False) -> str:
        """compressed_defines/compressed_relations/compressed_uncertainty:
        quand au moins un des trois est fourni, ces entrees sont compressees
        via le Master Compresseur (sous-processus cstl_compress_cli, voir
        compress_triple ci-dessus) et envoyees comme UN SEUL bloc
        COMPRESSED_PAYLOAD [data=<base64>] -- jamais comme des blocs DEFINE/
        RELATION/UNCERTAINTY en texte clair. `relations=` (parametre deja
        existant) reste un chemin SEPARE, non compresse -- les deux peuvent
        coexister dans le meme message si besoin (le serveur fusionne les
        deux sources cote parsing, voir record_compressed_payload).

        request_compressed_response=True ajoute INTENT_PAYLOAD.compress_response=true
        -- demande au serveur de compresser SA reponse en un bloc
        COMPRESSED_RESPONSE (voir src/server/response_compression.rs).
        send_raw() le deplie automatiquement avant parse_response(), donc
        rien d'autre a faire cote appelant. Jamais actif par defaut --
        retrocompatibilite totale.

        request_castle_response=True ajoute INTENT_PAYLOAD.castle_response=true
        -- demande le DEUXIEME codec de reponse, CASTLE (Layer 9, voir
        src/server/castle_wire.rs), dont le dictionnaire s'accumule sur la
        duree de la CONNEXION -- n'a de valeur qu'avec
        CstlClient(keep_alive=True); avec le defaut (une connexion par
        envoi), chaque message reste au dictionnaire vide et la garde de
        taille cote serveur le garde systematiquement en texte clair
        (jamais une erreur, juste jamais un gain -- voir le README).
        Mutuellement exclusif avec request_compressed_response: si les
        deux sont vrais, le serveur privilegie compress_response (limite
        de portee v1 assumee, documentee dans send_response() cote Rust).
        send_raw() le deplie automatiquement, meme contrat que ci-dessus."""
        meta_kv = {"encoder": encoder, "produced_by": produced_by}
        if extra_meta:
            meta_kv.update(extra_meta)
        intent_kv = {"purpose": purpose, "sender": sender, "receiver": receiver}
        if extra_intent:
            intent_kv.update(extra_intent)
        if request_compressed_response:
            intent_kv["compress_response"] = "true"
        if request_castle_response:
            intent_kv["castle_response"] = "true"

        binary_path = compress_binary_path or self._compress_binary_path

        lines = [f"#!CSTL {version} MODE={mode}\n"]
        lines.append(_format_kv_block("META", meta_kv))
        lines.append(_format_kv_block("INTENT_PAYLOAD", intent_kv))
        for rel in relations or []:
            lines.append(_format_kv_block("RELATION", rel))
        if compressed_defines or compressed_relations or compressed_uncertainty:
            data_b64 = compress_triple(
                defines=compressed_defines,
                relations=compressed_relations,
                uncertainty=compressed_uncertainty,
                binary_path=binary_path,
            )
            lines.append(_format_kv_block("COMPRESSED_PAYLOAD", {"data": data_b64}))
        lines.append("---END---\n")
        return "".join(lines)

    def send_raw(self, payload_str: str) -> CstlResponse:
        """Envoie un payload deja construit (texte CSTL complet) et lit la
        reponse jusqu'a ---END--- -- gere le cas ou la reponse arrive en
        plusieurs recv() (miroir de find_message_end cote serveur)."""
        sock = self._connect()
        try:
            sock.sendall(payload_str.encode("utf-8"))
            buf = bytearray()
            while END_MARKER not in buf:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                buf.extend(chunk)
            raw = buf.decode("utf-8", errors="replace")
            # Transparent cote reception: si le serveur a compresse sa reponse
            # (le client a envoye compress_response=true -- voir build_payload),
            # on la deplie en blocs texte clair AVANT parse_response(), qui
            # n'a donc jamais besoin de savoir que la compression existe.
            # Si aucun bloc COMPRESSED_RESPONSE n'est present (cas par
            # defaut), _expand_compressed_response ne touche a rien et
            # n'invoque meme pas cstl_compress_cli.
            raw = _expand_compressed_response(raw, binary_path=self._compress_binary_path)
            # Meme contrat pour CASTLE_RESPONSE (stateful, voir
            # _expand_castle_response) -- les deux expansions sont
            # inoffensives l'une sur l'autre: chacune ne touche que son
            # propre nom de bloc, absent si l'autre codec a ete utilise.
            raw = self._expand_castle_response(raw)
            return parse_response(raw)
        finally:
            if not self.keep_alive:
                sock.close()

    def send_relation(self, sender: str, receiver: str, purpose: str,
                       relations: list[dict], encoder: str = "Agent",
                       produced_by: str = "Client",
                       extra_intent: dict | None = None,
                       request_compressed_response: bool = False,
                       request_castle_response: bool = False) -> CstlResponse:
        payload = self.build_payload(
            encoder=encoder, produced_by=produced_by, purpose=purpose,
            sender=sender, receiver=receiver, relations=relations,
            extra_intent=extra_intent,
            request_compressed_response=request_compressed_response,
            request_castle_response=request_castle_response,
        )
        return self.send_raw(payload)

    def send_compressed(self, sender: str, receiver: str, purpose: str,
                         defines: list[dict] | None = None,
                         relations: list[dict] | None = None,
                         uncertainty: list[dict] | None = None,
                         encoder: str = "Agent", produced_by: str = "Client",
                         extra_intent: dict | None = None,
                         compress_binary_path: str | None = None,
                         request_compressed_response: bool = False) -> CstlResponse:
        """Envoie defines/relations/uncertainty compresses via le Master
        Compresseur, dans un seul bloc COMPRESSED_PAYLOAD. Leve
        CstlCompressionUnavailable si cstl_compress_cli n'est pas compile/
        trouvable -- voir compress_triple() pour le message d'erreur
        actionnable. request_compressed_response=True demande EN PLUS au
        serveur de compresser sa reponse (voir build_payload)."""
        payload = self.build_payload(
            encoder=encoder, produced_by=produced_by, purpose=purpose,
            sender=sender, receiver=receiver,
            extra_intent=extra_intent,
            compressed_defines=defines,
            compressed_relations=relations,
            compressed_uncertainty=uncertainty,
            compress_binary_path=compress_binary_path,
            request_compressed_response=request_compressed_response,
        )
        return self.send_raw(payload)

    def send_council_decision(self, sender: str, target_hash: str,
                               decision: str, note: str | None = None) -> CstlResponse:
        extra = {"target_hash": target_hash, "decision": decision}
        if note:
            extra["note"] = note
        payload = self.build_payload(
            encoder="Client", produced_by="Client", purpose="council_decision",
            sender=sender, receiver="server", extra_intent=extra,
        )
        return self.send_raw(payload)

    def send_detect_emergence(self, trio_hash: str, solo_hashes: list[str],
                               question: str = "") -> CstlResponse:
        extra = {
            "trio_hash": trio_hash,
            "solo_hashes": ";".join(solo_hashes),
            "question": question,
        }
        payload = self.build_payload(
            encoder="Client", produced_by="Client", purpose="detect_emergence",
            sender="client", receiver="server", extra_intent=extra,
        )
        return self.send_raw(payload)


# ---------------------------------------------------------------------------
# Auto-test / smoke-test contre un serveur reel
# ---------------------------------------------------------------------------

def _smoke_test(host: str, port: int) -> int:
    client = CstlClient(host=host, port=port, timeout=5.0)

    print(f"[1/6] Envoi d'un payload valide vers {host}:{port} ...")
    resp = client.send_relation(
        sender="alice", receiver="bob", purpose="smoke_test_greeting",
        relations=[{"type": "EQUALS", "subject": "cstl_client", "object": "works"}],
    )
    print(f"      status={resp.status!r} purpose={resp.purpose!r} audit_hash={resp.audit_hash!r}")
    if resp.status != "processed":
        print(f"      ECHEC: statut inattendu. Reponse brute:\n{resp.raw}")
        return 1
    assert resp.audit_hash, "AUDIT.hash absent d'une reponse processed"
    print("      OK")

    print("[2/6] Envoi d'un payload invalide (sender manquant) ...")
    bad_payload = (
        "#!CSTL v5.0.0 MODE=A\n"
        "META [encoder=Client, produced_by=Client]\n"
        "INTENT_PAYLOAD [purpose=smoke_test_bad, receiver=bob]\n"
        "---END---\n"
    )
    resp2 = client.send_raw(bad_payload)
    print(f"      status={resp2.status!r} purpose={resp2.purpose!r}")
    try:
        resp2.raise_for_status()
        print("      ECHEC: aucune exception levee pour un payload invalide")
        return 1
    except CstlValidationError as e:
        print(f"      OK (CstlValidationError levee: {e})")

    print("[3/6] Decision de council par un sender non autorise ...")
    resp3 = client.send_council_decision(
        sender="not_olivier", target_hash=resp.audit_hash, decision="commit",
    )
    print(f"      status={resp3.status!r} purpose={resp3.purpose!r} fields={resp3.fields}")

    print("[4/6] Envoi compresse (COMPRESSED_PAYLOAD / Master Compresseur) ...")
    try:
        resp4 = client.send_compressed(
            sender="alice", receiver="bob", purpose="smoke_test_compressed",
            defines=[{"name": "sdk_compressed_test", "entity_type": "agent", "id": "D-900"}],
            relations=[{"type": "equals", "subject": "sdk_compressed_test", "object": "ok", "id": "R-900"}],
            uncertainty=[{"identifier": "U-900", "status": "ESTIMATED", "sigma": "0.5"}],
        )
    except CstlCompressionUnavailable as e:
        print(f"      SKIP (cstl_compress_cli indisponible, pas un echec du client): {e}")
    else:
        print(f"      status={resp4.status!r} purpose={resp4.purpose!r}")
        if "COMPRESSED_PAYLOAD" in resp4.raw:
            # Le serveur n'emet "COMPRESSED_PAYLOAD" que dans un
            # SEMANTIC_WARNING (donnees mal formees, bloc ignore) -- un
            # roundtrip reussi ne le mentionne jamais dans sa reponse.
            print(f"      ECHEC: le serveur a signale un probleme sur le bloc compresse:\n{resp4.raw}")
            return 1
        if resp4.status != "processed":
            print(f"      ECHEC: statut inattendu. Reponse brute:\n{resp4.raw}")
            return 1
        print("      OK (le serveur a decompresse et traite le DEFINE/RELATION/UNCERTAINTY compresses)")

    print("[5/6] Reponse compressee (COMPRESSED_RESPONSE, server/response_compression.rs) ...")
    try:
        resp5 = client.send_relation(
            sender="alice", receiver="bob", purpose="smoke_test_compressed_response",
            relations=[{"type": "EQUALS", "subject": "cstl_client", "object": "compressed_response_works"}],
            extra_intent={"compress_response": "true"},
        )
    except CstlCompressionUnavailable as e:
        print(f"      SKIP (cstl_compress_cli indisponible, pas un echec du client): {e}")
    else:
        print(f"      status={resp5.status!r} purpose={resp5.purpose!r} audit_hash={resp5.audit_hash!r}")
        if "COMPRESSED_RESPONSE" in resp5.raw:
            print(f"      ECHEC: le bloc COMPRESSED_RESPONSE n'a pas ete deplie avant parsing:\n{resp5.raw}")
            return 1
        if resp5.status != "processed" or not resp5.audit_hash:
            print(f"      ECHEC: statut/audit_hash inattendu apres decompression. Reponse deplie:\n{resp5.raw}")
            return 1
        print("      OK (le client a transparemment deplie la reponse compressee avant de la parser)")

    print("[6/6] Reponse CASTLE (Layer 9, server/castle_wire.rs) sur une connexion persistante ...")
    try:
        castle_client = CstlClient(host=host, port=port, timeout=5.0, keep_alive=True)
        try:
            saw_castle_block = False
            for i in range(5):
                respN = castle_client.send_relation(
                    sender="alice", receiver="bob", purpose="smoke_test_castle_response",
                    relations=[{"type": "EQUALS", "subject": "cstl_client", "object": "castle_response_works"}],
                    request_castle_response=True,
                )
                if respN.status != "processed":
                    print(f"      ECHEC (iteration {i}): statut inattendu. Reponse brute:\n{respN.raw}")
                    return 1
                if "CASTLE_RESPONSE" in respN.raw:
                    print(f"      ECHEC (iteration {i}): le bloc CASTLE_RESPONSE n'a pas ete deplie avant parsing:\n{respN.raw}")
                    return 1
                # On ne peut pas savoir ICI si le serveur a reellement
                # compresse (le client ne voit que le texte deja deplie) --
                # mais on sait que le mecanisme bout-en-bout (dictionnaire
                # de connexion, garde octet-par-octet, decodage) tourne sans
                # erreur sur 5 messages reels d'affilee, compresse ou non.
                if castle_client._castle_dict_json is not None:
                    saw_castle_block = True
            print(f"      OK (5 messages sur une connexion persistante, audit_hash final={respN.audit_hash!r}, "
                  f"dictionnaire CASTLE avance={saw_castle_block})")
            if not saw_castle_block:
                print("      NOTE (pas un echec): aucune des 5 reponses n'a ete compressee -- mesure "
                      "honnete et attendue sur ce genre de contenu, voir castle_wire.rs et le README "
                      "(le vocabulaire CSTL ordinaire ne compresse pas avec ce codec pour l'instant).")
        finally:
            castle_client.close()
    except CstlCompressionUnavailable as e:
        print(f"      SKIP (cstl_compress_cli indisponible, pas un echec du client): {e}")

    print("\nSmoke-test termine sans erreur bloquante.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--host", default=DEFAULT_HOST)
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    ap.add_argument("--smoke-test", action="store_true",
                     help="Lance une suite de verification contre un serveur reellement demarre.")
    args = ap.parse_args()

    if args.smoke_test:
        return _smoke_test(args.host, args.port)

    ap.print_help()
    return 0


if __name__ == "__main__":
    sys.exit(main())
