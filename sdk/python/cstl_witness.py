#!/usr/bin/env python3
"""
cstl_witness.py -- TEMOIN externe de la chaine d'audit d'un serveur CSTL
(2026-11-20). A faire tourner sur une AUTRE machine que le serveur.

Pourquoi : sur la machine du serveur, une ancienne copie complete de la base
et de son marqueur signe forme un etat coherent et signe. Rien sur place ne
dit qu'il manque la suite. Le temoin garde, ailleurs, le dernier maillon
signe qu'il a vu. Au demarrage, le serveur lui demande ce maillon et refuse
de demarrer sur un historique plus ancien (voir src/server/witness.rs).

    python3 cstl_witness.py --port 8090 --store witness.json \\
        --server-key "$(cat cstl_adn.db.server_key.pub)"
    # cote serveur :
    CSTL_WITNESS_URL=http://machine-du-temoin:8090 cargo run --bin cstl_parser

Regles :
- seules les cles epinglees (`--server-key`, repetable) sont acceptees ;
  `--trust-first-key` accepte la premiere cle qui se presente, puis plus
  aucune autre ;
- chaque maillon recu doit porter une signature valide de cette cle (message
  canonique identique a `server_key::audit_message`) ;
- le temoin n'accepte que d'AVANCER : un maillon de rang inferieur au dernier
  vu est refuse (409 rollback) ; un maillon different au meme rang aussi
  (409 fork). Un serveur restaure depuis une vieille copie ne peut donc pas
  faire reculer le temoin.

API :
  POST /checkpoint             corps JSON (server_key, seq, hash, parent_hash,
                               sender, receiver, purpose, signature)
  GET  /checkpoint/<cle_hex>   dernier maillon vu, ou 404
"""
import argparse
import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

FIELDS = ("server_key", "seq", "hash", "parent_hash", "sender", "receiver", "purpose", "signature")
MAX_BODY = 16 * 1024


def audit_message(cp: dict) -> bytes:
    return (f"cstl-audit-v1\nseq={cp['seq']}\nhash={cp['hash']}\nparent_hash={cp['parent_hash']}\n"
            f"sender={cp['sender']}\nreceiver={cp['receiver']}\npurpose={cp['purpose']}").encode()


class WitnessStore:
    def __init__(self, path: Path, pinned: set[str], trust_first: bool):
        self.path, self.pinned, self.trust_first = path, {k.lower() for k in pinned}, trust_first
        self.lock = threading.Lock()
        self.latest: dict[str, dict] = json.loads(path.read_text()) if path.exists() else {}
        if trust_first and self.latest:
            self.pinned |= set(self.latest)

    def _save(self):
        tmp = self.path.with_suffix(self.path.suffix + ".tmp")
        tmp.write_text(json.dumps(self.latest, indent=1))
        os.replace(tmp, self.path)

    def get(self, key: str):
        with self.lock:
            return self.latest.get(key.lower())

    def submit(self, cp: dict) -> tuple[int, str]:
        if not all(f in cp for f in FIELDS) or not isinstance(cp["seq"], int) or cp["seq"] < 0:
            return 400, "champs manquants ou invalides"
        key = str(cp["server_key"]).lower()
        with self.lock:
            if key not in self.pinned:
                if not (self.trust_first and not self.pinned):
                    return 403, "cle du serveur non epinglee"
                self.pinned.add(key)
            try:
                Ed25519PublicKey.from_public_bytes(bytes.fromhex(key)).verify(bytes.fromhex(cp["signature"]), audit_message(cp))
            except (InvalidSignature, ValueError):
                return 403, "signature invalide"
            prev = self.latest.get(key)
            if prev is not None:
                if cp["seq"] < prev["seq"]:
                    return 409, f"rollback: seq={cp['seq']} < dernier vu seq={prev['seq']}"
                if cp["seq"] == prev["seq"] and cp["hash"] != prev["hash"]:
                    return 409, f"fork: autre maillon au rang seq={cp['seq']}"
            self.latest[key] = {f: cp[f] for f in FIELDS}
            self._save()
            return 200, "ok"


def make_handler(store: WitnessStore):
    class Handler(BaseHTTPRequestHandler):
        def _reply(self, code: int, body):
            data = (json.dumps(body) if not isinstance(body, str) else json.dumps({"detail": body})).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            parts = self.path.strip("/").split("/")
            if len(parts) == 2 and parts[0] == "checkpoint":
                cp = store.get(parts[1])
                return self._reply(200, cp) if cp else self._reply(404, "aucun maillon connu")
            self._reply(404, "inconnu")

        def do_POST(self):
            if self.path.rstrip("/") != "/checkpoint":
                return self._reply(404, "inconnu")
            length = int(self.headers.get("Content-Length") or 0)
            if length <= 0 or length > MAX_BODY:
                return self._reply(400, "corps absent ou trop grand")
            try:
                cp = json.loads(self.rfile.read(length))
            except ValueError:
                return self._reply(400, "JSON invalide")
            code, detail = store.submit(cp if isinstance(cp, dict) else {})
            self._reply(code, detail)

        def log_message(self, fmt, *args):  # journal sur stderr, sans corps
            super().log_message(fmt, *args)

    return Handler


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8090)
    ap.add_argument("--store", default="cstl_witness.json")
    ap.add_argument("--server-key", action="append", default=[], help="cle publique (hex) epinglee ; repetable")
    ap.add_argument("--trust-first-key", action="store_true", help="accepter la premiere cle qui se presente, puis elle seule")
    args = ap.parse_args()
    if not args.server_key and not args.trust_first_key:
        ap.error("--server-key ou --trust-first-key requis")
    store = WitnessStore(Path(args.store), {k.strip() for k in args.server_key}, args.trust_first_key)
    server = ThreadingHTTPServer((args.host, args.port), make_handler(store))
    print(f"[Temoin] ecoute sur {args.host}:{args.port}, stockage {args.store}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
