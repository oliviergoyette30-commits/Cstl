#!/usr/bin/env python3
"""
scripts/benchmark_cstl_vs_jsonschema.py -- CSTL vs "JSON bien fait", memes messages.

QUESTION : un developpeur qui veut des messages agent-a-agent fiables peut
utiliser JSON Schema strict + signature Ed25519 + registre de cles. Qu'est-ce
que CSTL attrape EN PLUS, et qu'est-ce qu'il rate ?

METHODE (equitable par construction) :
- Chaque cas existe en deux encodages du MEME contenu : texte CSTL envoye au
  VRAI serveur Rust compile (TCP, aucun mock), et objet JSON valide par une
  alternative forte :
    * JSON Schema strict (champs requis, pas de champ inconnu, operateurs en
      enum = les 40 operateurs officiels CSTL + le predicat KB `born_in`,
      modalites en enum) -- valide avec la bibliotheque `jsonschema` ;
    * signature Ed25519 sur le JSON canonique (cles triees), verifiee contre
      un registre de cles -- exactement ce que ferait JWS + annuaire.
  La base est volontairement STATELESS par message : c'est ce que donnent
  ces standards tels quels. Toute verification qui demande l'historique
  (contradiction entre messages) exigerait du code maison en plus.
- La verite terrain (`should_detect`) est fixee dans CASES AVANT execution.
  Des messages VALIDES mesurent les faux positifs.
- "Detecte" = rejete OU signale (avertissement dans la reponse). Les deux
  sont comptes separement : CSTL rejette certaines erreurs et ne fait que
  signaler les autres (choix de conception : les incoherences de faits sont
  consultatives).

NE MESURE PAS : la qualite d'encodage par un LLM (les payloads sont ecrits a
la main), ni la performance. Mesure uniquement : a contenu egal, qui attrape
quoi.

Usage : cargo build --bin cstl_parser && python3 scripts/benchmark_cstl_vs_jsonschema.py
(lance son propre serveur dans un repertoire temporaire, port 5050).
"""
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "sdk" / "python"))
from cstl_signing import generate_keypair, sign_intent  # noqa: E402

import jsonschema  # noqa: E402
from cryptography.hazmat.primitives.asymmetric import ed25519  # noqa: E402

PORT = 5050
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

# ---------------------------------------------------------------- base JSON
JSON_SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "required": ["sender", "receiver", "purpose", "relations"],
    "properties": {
        "sender": {"type": "string", "minLength": 1},
        "receiver": {"type": "string", "minLength": 1},
        "purpose": {"type": "string", "minLength": 1},
        "public_key": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
        "signature": {"type": "string", "pattern": "^[0-9a-f]{128}$"},
        "defines": {"type": "array", "items": {
            "type": "object", "additionalProperties": False,
            "required": ["name", "entity_type", "id"],
            "properties": {"name": {"type": "string"}, "entity_type": {"type": "string"},
                           "id": {"type": "string"}}}},
        "relations": {"type": "array", "items": {
            "type": "object", "additionalProperties": False,
            "required": ["type", "subject", "object"],
            "properties": {
                "type": {"enum": OFFICIAL_OPERATORS + ["born_in"]},
                "subject": {"type": "string", "minLength": 1},
                "object": {"type": "string", "minLength": 1},
                "modality": {"enum": ["MUST", "MUST_NOT", "REQUIRE", "FORBID", "MAY"]},
            }}},
    },
}


def canonical_json(msg):
    body = {k: v for k, v in msg.items() if k != "signature"}
    return json.dumps(body, sort_keys=True, separators=(",", ":")).encode()


def json_sign(msg, priv_hex):
    sk = ed25519.Ed25519PrivateKey.from_private_bytes(bytes.fromhex(priv_hex))
    return sk.sign(canonical_json(msg)).hex()


def baseline_check(msg, key_registry):
    """JSON Schema strict + Ed25519 + registre. Retourne (rejete, raison)."""
    try:
        jsonschema.validate(msg, JSON_SCHEMA)
    except jsonschema.ValidationError as e:
        return True, f"schema: {e.message[:70]}"
    registered = key_registry.get(msg["sender"])
    if registered:
        if "signature" not in msg:
            return True, "signature manquante"
        if msg.get("public_key") != registered:
            return True, "cle != cle enregistree"
        pk = ed25519.Ed25519PublicKey.from_public_bytes(bytes.fromhex(registered))
        try:
            pk.verify(bytes.fromhex(msg["signature"]), canonical_json(msg))
        except Exception:
            return True, "signature invalide"
    return False, "accepte"


# ---------------------------------------------------------------- CSTL
def cstl_text(sender, relations, defines=(), public_key=None, signature=None, extra_intent=""):
    meta = "encoder=bench, produced_by=bench"
    if public_key:
        meta += f", public_key={public_key}"
    sig = f", signature={signature}" if signature else ""
    lines = ["#!CSTL v5.0.0 MODE=A", f"META [{meta}]",
             f"INTENT_PAYLOAD [purpose=inform, sender={sender}, receiver=server{extra_intent}{sig}]"]
    for d in defines:
        lines.append(f"DEFINE {d['name']} AS {d['entity_type']} [id={d['id']}]")
    for r in relations:
        lines.append("RELATION [" + ", ".join(f"{k}={v}" for k, v in r.items()) + "]")
    lines.append("---END---")
    return "\n".join(lines) + "\n"


def cstl_sign(sender, relations, public_key, priv_hex, purpose="inform", extra_intent=None):
    intent = {"purpose": purpose, "sender": sender, "receiver": "server", **(extra_intent or {})}
    payload = {"version": "v5.0.0", "mode": "A",
               "meta": {"encoder": "bench", "produced_by": "bench", "public_key": public_key},
               "intent": intent, "relations": relations}
    return sign_intent(payload, priv_hex)


def send(text):
    with socket.create_connection(("127.0.0.1", PORT), timeout=10) as s:
        s.sendall(text.encode())
        out = b""
        while b"---END---" not in out:
            chunk = s.recv(65536)
            if not chunk:
                break
            out += chunk
    return out.decode(errors="replace")


REJECT_PURPOSES = ("validation_error", "signature_rejected", "operator_whitelist_rejected",
                   "rejected", "error")


def canonical_cycles(text):
    """Le serveur affiche un cycle a partir d'un point de depart qui varie
    d'une execution a l'autre (ordre de HashMap) : "A -> B -> C -> A" ou
    "C -> A -> B -> C". On le reecrit a partir de son plus petit element pour
    que les resultats enregistres ne changent pas sans raison."""
    def fix(m):
        nodes = [n.strip() for n in m.group(1).split("->")][:-1]
        i = nodes.index(min(nodes))
        rot = nodes[i:] + nodes[:i]
        return "(" + " -> ".join(rot + [rot[0]]) + ")"
    return re.sub(r"\(([^()]*->[^()]*)\)", fix, text)


def cstl_verdict(resp):
    """Lit la VRAIE reponse : (rejete, signale, detail)."""
    meta_error = bool(re.search(r"^META \[.*status=error", resp, re.M))
    purpose = re.search(r"^INTENT_PAYLOAD \[purpose=([^,\]]+)", resp, re.M)
    purpose = purpose.group(1) if purpose else "?"
    rejected = meta_error or any(p in purpose for p in REJECT_PURPOSES)
    flags = []
    if re.search(r"^CONSISTENCY \[consistent=false", resp, re.M):
        flags.append("CONSISTENCY=false")
    if re.search(r"^DEONTIC_AUDIT \[consistent=false", resp, re.M):
        flags.append("DEONTIC_AUDIT=false")
    for m in re.finditer(r"^SEMANTIC_WARNING \[detail=([^\]]{0,60})", resp, re.M):
        flags.append("WARN:" + m.group(1))
    detail = purpose if rejected else ("; ".join(flags) or "accepte sans signal")
    detail = canonical_cycles(detail)
    if rejected:
        reason = re.search(r"(?:reason|detail)=([^,\]]{0,70})", resp)
        if reason:
            detail += f" ({reason.group(1)})"
    return rejected, bool(flags) and not rejected, detail


# ---------------------------------------------------------------- cas
def build_cases(agent, pub, priv, attacker_pub, attacker_priv):
    R = lambda t, s, o, **kw: {"type": t, "subject": s, "object": o, **kw}  # noqa: E731
    cases = []

    def add(cid, cat, desc, should, relations, defines=(), setup=None, signed=None,
            extra_intent=None):
        cases.append(dict(id=cid, cat=cat, desc=desc, should_detect=should, relations=relations,
                          defines=list(defines), setup=setup or [], signed=signed,
                          extra_intent=extra_intent or {}))

    # Valides (faux positifs)
    add("V1", "valide", "fait simple", False, [R("STATE", "cstl", "alive")])
    add("V2", "valide", "obligation seule (MUST)", False, [R("PERFORM", "nurse", "check_vitals", modality="MUST")])
    add("V3", "valide", "interdiction seule (MUST_NOT)", False, [R("PERFORM", "intern", "prescribe", modality="MUST_NOT")])
    add("V4", "valide", "ordre temporel A avant B", False, [R("BEFORE", "signing", "delivery")])
    add("V5", "valide", "message signe correctement", False, [R("STATE", "order_42", "shipped")], signed="good")
    # Structure
    add("E1", "structure", "operateur inconnu (TERMINATES_IF)", True, [R("TERMINATES_IF", "contract", "breach")])
    add("E2", "structure", "relation sans objet", True, [{"type": "STATE", "subject": "x"}])
    # Logique dans UN message
    add("L1", "logique", "MUST et MUST_NOT sur la meme action", True,
        [R("PERFORM", "pilot", "takeoff", modality="MUST"), R("PERFORM", "pilot", "takeoff", modality="MUST_NOT")])
    add("L2", "logique", "deux lieux de naissance contradictoires", True,
        [R("born_in", "agent_x", "Paris"), R("born_in", "agent_x", "Berlin")])
    add("L3", "logique", "cycle temporel A<B<C<A", True,
        [R("BEFORE", "EvA", "EvB"), R("BEFORE", "EvB", "EvC"), R("BEFORE", "EvC", "EvA")])
    add("L4", "logique", "entite definie jamais referencee", True,
        [R("STATE", "e1", "active")],
        defines=[{"name": "Party", "entity_type": "Entity", "id": "e1"},
                 {"name": "Agreement", "entity_type": "Contract", "id": "e9"}])
    # Logique ENTRE messages (historique)
    add("H1", "historique", "fait contredisant un message precedent", True,
        [R("born_in", "agent_y", "Berlin")], setup=[[R("born_in", "agent_y", "Paris")]])
    add("H2", "historique", "MUST contredisant un MUST_NOT precedent", True,
        [R("PERFORM", "bot", "delete_logs", modality="MUST")],
        setup=[[R("PERFORM", "bot", "delete_logs", modality="MUST_NOT")]])
    add("H3", "historique", "cycle temporel reparti sur 2 messages", True,
        [R("BEFORE", "StepB", "StepA")], setup=[[R("BEFORE", "StepA", "StepB")]])
    # Identite
    add("S1", "identite", "usurpation (signe avec la cle d'un attaquant)", True,
        [R("STATE", "account", "closed")], signed="attacker")
    add("S2", "identite", "message modifie apres signature", True,
        [R("STATE", "payment", "approved")], signed="tampered")
    add("S3", "identite", "message non signe d'un agent a cle", True,
        [R("STATE", "door", "unlocked")], signed="none")
    return cases


def run_case(case, agent, pub, priv, attacker_pub, attacker_priv, key_registry):
    sender = agent if case["signed"] else "alice"
    # --- setup (historique) : meme sequence pour les deux systemes
    for rels in case["setup"]:
        send(cstl_text(sender, rels))
    # --- CSTL
    rels, defs = case["relations"], case["defines"]
    signed = case["signed"]
    if signed in ("good", "tampered"):
        sig = cstl_sign(sender, rels, pub, priv)
        sent = rels
        if signed == "tampered":
            sent = [dict(r, object="denied") for r in rels]
        text = cstl_text(sender, sent, defs, public_key=pub, signature=sig)
    elif signed == "attacker":
        sig = cstl_sign(sender, rels, attacker_pub, attacker_priv)
        text = cstl_text(sender, rels, defs, public_key=attacker_pub, signature=sig)
    elif signed == "none":
        text = cstl_text(sender, rels, defs)
    else:
        text = cstl_text(sender, rels, defs)
    resp = send(text)
    c_rej, c_flag, c_detail = cstl_verdict(resp)
    # --- base JSON (stateless : le setup ne lui apporte rien, c'est le point)
    msg = {"sender": sender, "receiver": "server", "purpose": "inform", "relations": rels}
    if defs:
        msg["defines"] = defs
    if signed in ("good", "tampered"):
        msg["public_key"] = pub
        msg["signature"] = json_sign(msg, priv)
        if signed == "tampered":
            msg["relations"] = [dict(r, object="denied") for r in rels]
    elif signed == "attacker":
        msg["public_key"] = attacker_pub
        msg["signature"] = json_sign(msg, attacker_priv)
    b_rej, b_detail = baseline_check(msg, key_registry)
    return dict(cstl_rejected=c_rej, cstl_flagged=c_flag, cstl_detail=c_detail,
                base_rejected=b_rej, base_detail=b_detail, raw=resp)


def register(name, pub, priv):
    intent = {"purpose": "agent_register", "sender": name, "receiver": "server",
              "name": name, "capabilities": "communication"}
    payload = {"version": "v5.0.0", "mode": "A",
               "meta": {"encoder": "bench", "produced_by": "bench", "public_key": pub},
               "intent": intent, "relations": []}
    sig = sign_intent(payload, priv)
    text = ("#!CSTL v5.0.0 MODE=A\n"
            f"META [encoder=bench, produced_by=bench, public_key={pub}]\n"
            f"INTENT_PAYLOAD [purpose=agent_register, sender={name}, receiver=server, "
            f"name={name}, capabilities=communication, signature={sig}]\n---END---\n")
    resp = send(text)
    assert "agent_register_ack" in resp, resp
    return resp


def main():
    binary = REPO / "target" / "debug" / "cstl_parser"
    if not binary.exists():
        sys.exit("cargo build --bin cstl_parser d'abord")
    workdir = tempfile.mkdtemp(prefix="cstl_bench_")
    log = open(os.path.join(workdir, "server.log"), "w")
    # Les cas de contenu (structure, logique, historique) sont envoyes au nom
    # des agents de demonstration non signes : mode developpement, puisque
    # les signatures sont obligatoires par defaut depuis le 2026-11-21. Les
    # cas d'identite (S1-S3) utilisent des agents a cle et restent stricts.
    env = {**os.environ, "CSTL_DEV_MODE": "1"}
    srv = subprocess.Popen([str(binary)], cwd=workdir, env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        else:
            sys.exit("serveur injoignable")

        _, pub, priv = generate_keypair()
        _, att_pub, att_priv = generate_keypair()
        agent = "bench_agent"
        register(agent, pub, priv)
        key_registry = {agent: pub}

        cases = build_cases(agent, pub, priv, att_pub, att_priv)
        results = []
        for c in cases:
            r = run_case(c, agent, pub, priv, att_pub, att_priv, key_registry)
            results.append({**{k: c[k] for k in ("id", "cat", "desc", "should_detect")}, **r})
        if "--raw" in sys.argv:
            for r in results:
                print(f"===== {r['id']} {r['desc']}\n{r['raw']}")
        report(results)
        # Garde-fou de regression (CI) : CSTL ne doit rater aucune erreur
        # attendue ni rejeter/signaler un message valide. La base JSON n'est
        # pas un critere d'echec -- elle sert de point de comparaison.
        regressions = [r["id"] for r in results
                       if (r["cstl_rejected"] or r["cstl_flagged"]) != r["should_detect"]]
        out = REPO / "scripts" / "benchmark_cstl_vs_jsonschema_results.json"
        out.write_text(json.dumps([{k: v for k, v in r.items() if k != "raw"} for r in results],
                                  indent=2, ensure_ascii=False) + "\n")
        print(f"\nResultats bruts : {out.relative_to(REPO)}")
        if regressions:
            print(f"ECHEC : verdict CSTL different de l'attendu pour {regressions}")
            sys.exitcode = 1
    finally:
        srv.terminate()
        srv.wait(timeout=10)


def report(results):
    def mark(det, should):
        return ("OK " if det == should else "XX ")

    print(f"{'id':4} {'categorie':11} {'attendu':8} | {'CSTL':42} | {'JSON Schema + Ed25519':32} | cas")
    print("-" * 140)
    for r in results:
        c_det = r["cstl_rejected"] or r["cstl_flagged"]
        c_kind = "REJET" if r["cstl_rejected"] else ("SIGNAL" if r["cstl_flagged"] else "accepte")
        b_kind = "REJET" if r["base_rejected"] else "accepte"
        print(f"{r['id']:4} {r['cat']:11} {'detecter' if r['should_detect'] else 'accepter':8} | "
              f"{mark(c_det, r['should_detect'])}{c_kind:7} {r['cstl_detail'][:31]:31} | "
              f"{mark(r['base_rejected'], r['should_detect'])}{b_kind:7} {r['base_detail'][:21]:21} | {r['desc']}")

    errs = [r for r in results if r["should_detect"]]
    valid = [r for r in results if not r["should_detect"]]
    c_rej = sum(r["cstl_rejected"] for r in errs)
    c_flag = sum(r["cstl_flagged"] for r in errs)
    b_rej = sum(r["base_rejected"] for r in errs)
    c_fp = sum(r["cstl_rejected"] or r["cstl_flagged"] for r in valid)
    b_fp = sum(r["base_rejected"] for r in valid)
    print("\nBILAN")
    print(f"  Erreurs detectees  CSTL : {c_rej + c_flag}/{len(errs)}  ({c_rej} rejetees, {c_flag} signalees seulement)")
    print(f"  Erreurs detectees  JSON : {b_rej}/{len(errs)}  (toutes par rejet)")
    print(f"  Faux positifs      CSTL : {c_fp}/{len(valid)}   JSON : {b_fp}/{len(valid)}")
    cats = sorted({r["cat"] for r in errs})
    for cat in cats:
        rs = [r for r in errs if r["cat"] == cat]
        print(f"    {cat:11} CSTL {sum(r['cstl_rejected'] or r['cstl_flagged'] for r in rs)}/{len(rs)}"
              f"   JSON {sum(r['base_rejected'] for r in rs)}/{len(rs)}")


if __name__ == "__main__":
    main()
    sys.exit(getattr(sys, "exitcode", 0))
