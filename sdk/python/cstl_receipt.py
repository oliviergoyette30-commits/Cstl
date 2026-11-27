"""
cstl_receipt.py -- recus signes par le serveur pour une politique publiee
(2026-11-25, specification controle v1, section 6.4).

Un message qui porte `INTENT_PAYLOAD.policy_seq` n'est accepte que s'il est
plus recent que le dernier accepte de cet auteur (`stale_policy` sinon), et
la reponse porte un bloc RECEIPT signe par la cle du serveur :

  RECEIPT [sender=..., policy_seq=..., request_hash=sha256:..., audit_hash=..., signature=...]

Message signe (lignes separees par \\n, sans \\n final) :

  cstl-receipt-v1
  sender=<sender>
  policy_seq=<policy_seq>
  request_hash=sha256:<sha256 des octets canoniques du message envoye>
  audit_hash=<audit_hash>

L'auteur le verifie avec la cle publique du serveur obtenue HORS BANDE : un
faux accuse de reception, ou le recu d'un message modifie en route, ne passe
pas.
"""
import hashlib
import time
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey


def receipt_message(sender, policy_seq, request_hash, audit_hash):
    return (f"cstl-receipt-v1\nsender={sender}\npolicy_seq={policy_seq}\n"
            f"request_hash={request_hash}\naudit_hash={audit_hash}").encode()


def request_hash(canonical_bytes):
    return "sha256:" + hashlib.sha256(canonical_bytes).hexdigest()


def _fields(line):
    content = line[line.index("[") + 1:line.rindex("]")]
    out, cur, quoted = {}, "", False
    for ch in content + ",":
        if ch == '"':
            quoted = not quoted
        if ch == "," and not quoted:
            if "=" in cur:
                k, v = cur.split("=", 1)
                out[k.strip()] = v.strip().strip('"')
            cur = ""
        else:
            cur += ch
    return out


def find_receipt(raw):
    for line in raw.splitlines():
        if line.startswith("RECEIPT ["):
            return _fields(line)
    return None


def verify_receipt(raw, server_public_key_hex, sender, policy_seq, canonical_bytes):
    """Le recu de la reponse `raw` s'il est valide pour CE message, sinon None."""
    r = find_receipt(raw)
    if not r:
        return None
    want = request_hash(canonical_bytes)
    if r.get("sender") != sender or r.get("policy_seq") != str(policy_seq) or r.get("request_hash") != want:
        return None
    try:
        Ed25519PublicKey.from_public_bytes(bytes.fromhex(server_public_key_hex.strip())).verify(
            bytes.fromhex(r.get("signature", "")), receipt_message(sender, policy_seq, want, r.get("audit_hash", "")))
    except (InvalidSignature, ValueError):
        return None
    return r


def next_seq(seq_file):
    """Sequence strictement croissante, meme apres un redemarrage : max(horloge
    en microsecondes, derniere + 1), gardee dans `seq_file`."""
    p = Path(seq_file)
    last = int(p.read_text()) if p.exists() else 0
    seq = max(time.time_ns() // 1000, last + 1)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(str(seq))
    return seq
