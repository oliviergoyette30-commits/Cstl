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
import re
import socket
import sys
from dataclasses import dataclass, field

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
# Client
# ---------------------------------------------------------------------------

class CstlClient:
    def __init__(self, host: str = DEFAULT_HOST, port: int = DEFAULT_PORT,
                 timeout: float = 10.0, keep_alive: bool = False):
        self.host = host
        self.port = port
        self.timeout = timeout
        self.keep_alive = keep_alive
        self._sock: socket.socket | None = None

    def _connect(self) -> socket.socket:
        if self.keep_alive and self._sock is not None:
            return self._sock
        sock = socket.create_connection((self.host, self.port), timeout=self.timeout)
        if self.keep_alive:
            self._sock = sock
        return sock

    def close(self) -> None:
        if self._sock is not None:
            try:
                self._sock.close()
            finally:
                self._sock = None

    def build_payload(self, *, encoder: str, produced_by: str, purpose: str,
                       sender: str, receiver: str,
                       relations: list[dict] | None = None,
                       mode: str = "A", version: str = "v5.0.0",
                       extra_meta: dict | None = None,
                       extra_intent: dict | None = None) -> str:
        meta_kv = {"encoder": encoder, "produced_by": produced_by}
        if extra_meta:
            meta_kv.update(extra_meta)
        intent_kv = {"purpose": purpose, "sender": sender, "receiver": receiver}
        if extra_intent:
            intent_kv.update(extra_intent)

        lines = [f"#!CSTL {version} MODE={mode}\n"]
        lines.append(_format_kv_block("META", meta_kv))
        lines.append(_format_kv_block("INTENT_PAYLOAD", intent_kv))
        for rel in relations or []:
            lines.append(_format_kv_block("RELATION", rel))
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
            return parse_response(raw)
        finally:
            if not self.keep_alive:
                sock.close()

    def send_relation(self, sender: str, receiver: str, purpose: str,
                       relations: list[dict], encoder: str = "Agent",
                       produced_by: str = "Client",
                       extra_intent: dict | None = None) -> CstlResponse:
        payload = self.build_payload(
            encoder=encoder, produced_by=produced_by, purpose=purpose,
            sender=sender, receiver=receiver, relations=relations,
            extra_intent=extra_intent,
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

    print(f"[1/3] Envoi d'un payload valide vers {host}:{port} ...")
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

    print("[2/3] Envoi d'un payload invalide (sender manquant) ...")
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

    print("[3/3] Decision de council par un sender non autorise ...")
    resp3 = client.send_council_decision(
        sender="not_olivier", target_hash=resp.audit_hash, decision="commit",
    )
    print(f"      status={resp3.status!r} purpose={resp3.purpose!r} fields={resp3.fields}")

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
