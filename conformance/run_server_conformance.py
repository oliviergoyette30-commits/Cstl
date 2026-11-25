#!/usr/bin/env python3
"""
conformance/run_server_conformance.py -- un serveur est-il conforme a CSTL
controle v1 ? (2026-11-25)

Testeur « boite noire » : il ne parle au serveur que par TCP, et il est
ecrit UNIQUEMENT a partir de la specification (docs/CSTL_CONTROLE_SPEC_v1.md) :
son propre format texte, ses propres octets canoniques, sa propre
verification de jeton. Il n'importe pas le SDK CSTL. Seule dependance :
`cryptography` (Ed25519). Il verifie d'abord sa propre signature sur
conformance/vectors.json.

  python3 conformance/run_server_conformance.py --port 5050 \\
      --server-key cstl_adn.db.server_key.pub

Le serveur doit tourner SANS mode developpement, et SANS CSTL_POLICY_AUTHORS
(les regles de n'importe quel agent enregistre comptent : section 6.3). Les
agents de test ont des noms aleatoires ; rien d'autre n'est touche.
Code de sortie : 0 si tout est conforme.
"""
import argparse
import json
import socket
import sys
import time
import unicodedata
import uuid
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey

HERE = Path(__file__).resolve().parent


# -- Section 3 : format texte --------------------------------------------------
def wire_value(v):
    if '"' in v or "\n" in v or "\r" in v or v.strip() != v or "---END---" in v:
        raise ValueError(f"valeur non transportable : {v!r}")
    return f'"{v}"' if any(c in v for c in ",[]") else v


def render(msg):
    def block(name, d):
        return f"{name} [" + ", ".join(f"{k}={wire_value(str(v))}" for k, v in d.items()) + "]\n"
    out = f"#!CSTL {msg['version']} MODE={msg['mode']}\n" + block("META", msg["meta"]) + block("INTENT_PAYLOAD", msg["intent"])
    out += "".join(block("RELATION", r) for r in msg.get("relations", []))
    return out + "---END---\n"


def split_fields(content):
    parts, cur, quoted = [], "", False
    for ch in content:
        if ch == '"':
            quoted = not quoted
        if ch == "," and not quoted:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    parts.append(cur)
    out = {}
    for p in parts:
        if "=" in p:
            k, v = p.split("=", 1)
            out[k.strip()] = v.strip().strip('"')
    return out


def parse(text):
    """Reponse -> liste de (NOM, champs)."""
    blocks = []
    for line in text.splitlines()[1:]:
        line = line.strip()
        if line == "---END---":
            break
        if " [" in line and line.endswith("]"):
            name, rest = line.split(" [", 1)
            blocks.append((name, split_fields(rest[:-1])))
    return blocks


def first(blocks, name):
    return next((f for n, f in blocks if n == name), {})


# -- Section 4 : octets canoniques ----------------------------------------------
def nfc(s):
    return unicodedata.normalize("NFC", s)


def canonical(msg):
    meta = "".join(f"|{nfc(k)}={nfc(v)}" for k, v in sorted(msg["meta"].items()) if k != "PARENT_HASH")
    intent = "".join(f"|{nfc(k)}={nfc(v)}" for k, v in sorted(msg["intent"].items())
                     if k not in ("signature", "rotation_signature"))
    rels = sorted(",".join(f"{nfc(k)}={nfc(v)}" for k, v in sorted(r.items())) for r in msg.get("relations", []))
    return (f"VERSION|{nfc(msg['version'])}\nMODE|{nfc(msg['mode'])}\nMETA{meta}\nINTENT{intent}\nRELATIONS"
            + "".join(f"|{r}" for r in rels)).encode()


def pub_hex(sk):
    return sk.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex()


# -- Client minimal ------------------------------------------------------------
class Client:
    def __init__(self, host, port):
        self.host, self.port = host, port

    def send(self, text):
        with socket.create_connection((self.host, self.port), timeout=30) as s:
            s.sendall(text.encode())
            buf = b""
            while b"---END---" not in buf:
                chunk = s.recv(65536)
                if not chunk:
                    break
                buf += chunk
        return buf.decode()


class Agent:
    def __init__(self, client, name=None, sk=None):
        self.c = client
        self.name = name or f"conf_{uuid.uuid4().hex[:10]}"
        self.sk = sk or Ed25519PrivateKey.generate()

    def message(self, purpose, relations=(), **intent):
        return {"version": "v5.0.0", "mode": "A",
                "meta": {"encoder": "conformance", "produced_by": "conformance", "public_key": pub_hex(self.sk)},
                "intent": {"purpose": purpose, "sender": self.name, "receiver": "server", **intent},
                "relations": [dict(r) for r in relations]}

    def signed(self, msg):
        msg["intent"]["signature"] = self.sk.sign(canonical(msg)).hex()
        return msg

    def send(self, purpose, relations=(), **intent):
        return parse(self.c.send(render(self.signed(self.message(purpose, relations, **intent)))))

    def ask(self, action, amount=None):
        extra = {"amount": str(amount)} if amount is not None else {}
        return self.send("action_request", action=action, request_id=uuid.uuid4().hex, **extra)


# -- Section 7.3 : verification d'un jeton --------------------------------------
def check_token(server_key_hex, f, action, agent, amount, now, skew=5):
    msg = (f"cstl-authz-v1\nagent={f['receiver']}\naction={f['action']}\nrequest_id={f['request_id']}\n"
           f"issued_at={f['issued_at']}\nexpires_at={f['expires_at']}\naudit_hash={f['audit_hash']}")
    if f.get("amount") is not None:
        msg += f"\namount={f['amount']}"
    try:
        Ed25519PublicKey.from_public_bytes(bytes.fromhex(server_key_hex)).verify(bytes.fromhex(f["authorization"]), msg.encode())
    except Exception:  # noqa: BLE001
        return "bad_signature"
    if f["action"] != action:
        return "wrong_action"
    if f["receiver"] != agent:
        return "wrong_agent"
    if amount is not None or f.get("amount") is not None:
        try:
            if amount is None or f.get("amount") is None or float(amount) != float(f["amount"]):
                return "wrong_amount"
        except ValueError:
            return "wrong_amount"
    if int(f["issued_at"]) > now + skew:
        return "not_yet_valid"
    if now > int(f["expires_at"]) + skew:
        return "expired"
    return "ok"


def self_test():
    """Ce testeur applique la specification comme Python et Rust (vecteurs)."""
    v = json.loads((HERE / "vectors.json").read_text())
    for case in v["signing"]:
        assert canonical(case["message"]).decode() == case["canonical"], case["name"]
        Ed25519PublicKey.from_public_bytes(bytes.fromhex(v["agent_public_key"])).verify(
            bytes.fromhex(case["message"]["intent"]["signature"]), canonical(case["message"]))
    t = v["token"]
    for c in t["verdicts"]:
        f = {**t["fields"], **c.get("token", {}), "receiver": t["fields"]["agent"], "authorization": t["signature"]}
        assert check_token(t["server_public_key"], f, c["action"], c["agent"], c["amount"], c["now"], 0) == c["expect"], c["case"]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5050)
    ap.add_argument("--server-key", required=True, help="cle publique du serveur (hex, ou fichier .pub)")
    args = ap.parse_args()
    key = Path(args.server_key).read_text().strip() if Path(args.server_key).is_file() else args.server_key.strip()
    self_test()
    print("Testeur : vecteurs de conformite verifies (signature, jetons).")
    c = Client(args.host, args.port)
    results = []

    def check(cid, title, cond, detail=""):
        results.append(cond)
        print(f"  {'PASS' if cond else 'FAIL'}  {cid}  {title}" + ("" if cond else f"  -- {detail}"))

    owner, bot = Agent(c), Agent(c)
    r = owner.send("agent_register", name=owner.name, capabilities="communication")
    r2 = bot.send("agent_register", name=bot.name, capabilities="communication")
    check("C1", "enregistrement auto-signe -> agent_register_ack",
          first(r, "INTENT_PAYLOAD").get("purpose") == "agent_register_ack" == first(r2, "INTENT_PAYLOAD").get("purpose"), r)

    m = bot.message("action_request", action="read_logs", request_id=uuid.uuid4().hex)
    r = parse(c.send(render(m)))
    i = first(r, "INTENT_PAYLOAD")
    check("C2", "message non signe d'un agent enregistre -> signature_rejected",
          i.get("purpose") == "signature_rejected" and i.get("decision") != "allowed", i)

    m = bot.signed(bot.message("action_request", action="read_logs", request_id=uuid.uuid4().hex))
    m["intent"]["action"] = "wipe_db"
    i = first(parse(c.send(render(m))), "INTENT_PAYLOAD")
    check("C3", "champ modifie apres signature -> signature_rejected", i.get("purpose") == "signature_rejected", i)

    i = first(Agent(c, name=bot.name).ask("read_logs"), "INTENT_PAYLOAD")
    check("C4", "autre cle au nom d'un agent enregistre -> refuse",
          i.get("purpose") == "signature_rejected" and i.get("decision") != "allowed", i)

    r = owner.send("inform", [
        {"type": "PERFORM", "subject": bot.name, "object": "wipe_db", "modality": "MUST_NOT", "reason": "production"},
        {"type": "PERFORM", "subject": bot.name, "object": "refund_order", "modality": "MUST_NOT", "amount_above": "100"}])
    check("C5", "regles envoyees -> acceptees", first(r, "META").get("status") == "processed", first(r, "INTENT_PAYLOAD"))

    r = bot.ask("wipe_db")
    i = first(r, "INTENT_PAYLOAD")
    check("C6", "action interdite -> denied, reason=forbidden, rule=MUST_NOT",
          i.get("purpose") == "action_authorization" and i.get("decision") == "denied"
          and i.get("reason") == "forbidden" and i.get("rule") == "MUST_NOT" and not i.get("authorization"), i)

    r1 = bot.ask("read_logs")
    i = first(r1, "INTENT_PAYLOAD")
    now = int(time.time())
    check("C7", "action permise -> allowed + jeton valide sous la cle du serveur",
          i.get("decision") == "allowed" and check_token(key, i, "read_logs", bot.name, None, now) == "ok", i)
    check("C8", "le jeton ne vaut que pour son action, son agent et son montant",
          i.get("decision") == "allowed" and check_token(key, i, "wipe_db", bot.name, None, now) == "wrong_action"
          and check_token(key, i, "read_logs", "eve", None, now) == "wrong_agent"
          and check_token(key, i, "read_logs", bot.name, "5", now) == "wrong_amount", i)

    i50 = first(bot.ask("refund_order", 50), "INTENT_PAYLOAD")
    i500 = first(bot.ask("refund_order", 500), "INTENT_PAYLOAD")
    check("C9", "plafond : 50 autorise (jeton amount=50), 500 refuse",
          i50.get("decision") == "allowed" and check_token(key, i50, "refund_order", bot.name, "50", int(time.time())) == "ok"
          and i500.get("decision") == "denied", (i50, i500))

    r2 = bot.ask("read_logs")
    a1, a2 = first(r1, "AUDIT"), first(r2, "AUDIT")
    try:
        chained = int(a2["seq"]) > int(a1["seq"]) and a1["hash"].startswith("sha256:")
        if int(a2["seq"]) == int(a1["seq"]) + 1:
            chained = chained and a2["parent_hash"] == a1["hash"]
    except (KeyError, ValueError):
        chained = False
    check("C10", "chaque decision a son maillon d'audit (seq croissant, chaine)", chained, (a1, a2))
    check("C11", "le jeton cite le maillon d'audit de la decision", i.get("audit_hash") == a1.get("hash"), (i, a1))

    r = bot.send("policy_query", request_id=uuid.uuid4().hex)
    rules = [f for n, f in r if n == "POLICY_RULE"]
    check("C12", "policy_query -> purpose=policy, avec la regle wipe_db forbidden",
          first(r, "INTENT_PAYLOAD").get("purpose") == "policy"
          and any(x.get("action") == "wipe_db" and x.get("decision") == "forbidden" for x in rules), r)

    i = first(bot.ask("a2a:billing:SendMessage"), "INTENT_PAYLOAD")
    check("C14", "nom d'action hors de [A-Za-z0-9_.-] -> denied, missing_or_invalid_action",
          i.get("decision") == "denied" and i.get("reason") == "missing_or_invalid_action", i)

    r = parse(c.send("#!CSTL v5.0.0 MODE=A\nMETA [encoder=x]\nINTENT_PAYLOAD [purpose=inform]\n---END---\n"))
    check("C13", "message incomplet -> status=error, pas d'audit",
          first(r, "META").get("status") == "error" and not first(r, "AUDIT"), r)

    ok = sum(results)
    print(f"\n{ok}/{len(results)} conformes" + ("" if ok == len(results) else " -- NON CONFORME"))
    return 0 if ok == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
