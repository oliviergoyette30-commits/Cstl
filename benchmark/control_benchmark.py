#!/usr/bin/env python3
"""
benchmark/control_benchmark.py -- banc public, deterministe, sans cle d'API :
qui decide juste quand la politique passe par un canal attaque ? (2026-11-25)

Chaque cas (benchmark/cases/*.json, format dans benchmark/README.md) decrit :
  - les messages de politique de l'AUTEUR, dans l'ordre (regles et faits) ;
  - ce que fait l'ATTAQUANT : modifier un message, le rejouer, le supprimer,
    livrer dans le desordre, signer avec sa propre cle en se faisant passer
    pour l'auteur (forge_author), ou publier sous son vrai nom d'agent
    enregistre (insider) ;
  - les demandes d'action de l'agent (pire cas : il tente tout).
La bonne decision est celle de la politique LEGITIME : tous les messages de
l'auteur, dans l'ordre, sans attaque.

Mecanismes compares (meme evaluateur de regles pour les quatre premiers,
verifie egal a la couche 9 du serveur sur la politique legitime) :
  aucun          pas de controle : tout passe
  moteur         moteur de regles sur la politique RECUE (principe d'OPA ou
                 de Cedar), sans verifier son origine
  signe          politique signee par l'auteur (Ed25519, cle epinglee), comme
                 un JWT ; un message invalide -> tout refuser (ferme)
  signe_versionne la meilleure pratique classique : signature, numero de
                 version strictement croissant (rejeu et desordre refuses),
                 et sur message invalide, rechargement de la politique
                 courante chez l'auteur
  cstl           le VRAI serveur CSTL (CSTL_POLICY_AUTHORS = l'auteur) : les
                 messages sont envoyes, signes, au serveur, et les decisions
                 viennent de action_request

Deux modeles de menace, publies tous les deux :
  T1 relais      l'attaquant controle le chemin de DISTRIBUTION de la
                 politique vers les systemes qui decident. Avec CSTL, l'auteur
                 ecrit directement au serveur : l'attaquant peut seulement y
                 injecter ses propres messages (copies modifiees, rejeux,
                 faux auteur, initie).
  T2 auteur      l'attaquant controle aussi la liaison auteur -> serveur
                 CSTL : suppression, desordre et modification atteignent CSTL.

Score par demande : juste / violation (permis a tort) / refus a tort.
Deterministe : relancer donne le meme resultat (pas besoin d'ecart-type).

  cargo build --bin cstl_parser
  python3 benchmark/control_benchmark.py                 # tableau + benchmark/results.json
  python3 benchmark/control_benchmark.py --cases mes_cas/   # vos propres attaques
"""
import argparse
import copy
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.exceptions import InvalidSignature

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(REPO / "conformance"))
import run_server_conformance as wire  # noqa: E402  (client ecrit d'apres la specification seule)

MECHANISMS = ("aucun", "moteur", "signe", "signe_versionne", "cstl")


# -- Politique abstraite et evaluateur -------------------------------------------
def decide(messages, action, amount):
    """Regles cumulees dans l'ordre ; pour une action, la DERNIERE regle decide
    (comme la couche 9 avec CSTL_POLICY_AUTHORS). Prudence : montant ou fait
    inconnu -> une interdiction conditionnelle s'applique. Sans regle : permis."""
    facts, last = {}, None
    for m in messages:
        facts.update(m.get("facts", {}))
        for r in m.get("rules", []):
            if r["action"] == action:
                last = r
    if last is None or last["modality"] in ("MAY", "MUST"):
        return True
    applies = True
    if "amount_above" in last:
        applies = amount is None or float(amount) > float(last["amount_above"])
    if "if_fact" in last:
        applies = applies and facts.get(last["if_fact"], True) is not False
    if "unless_fact" in last:
        applies = applies and facts.get(last["unless_fact"]) is not True
    return not applies


class Signer:
    def __init__(self):
        self.sk = Ed25519PrivateKey.generate()
        self.pk = self.sk.public_key()

    def sign(self, body):
        return self.sk.sign(json.dumps(body, sort_keys=True).encode()).hex()

    def verify(self, body, sig):
        try:
            self.pk.verify(bytes.fromhex(sig), json.dumps(body, sort_keys=True).encode())
            return True
        except (InvalidSignature, ValueError):
            return False


def apply_change(msg, op):
    m = copy.deepcopy(msg)
    if "set_rule" in op:
        r = m["rules"][op["set_rule"]["index"]]
        r.update({k: v for k, v in op["set_rule"].items() if k != "index"})
    if "drop_rule" in op:
        del m["rules"][op["drop_rule"]]
    if "set_fact" in op:
        m.setdefault("facts", {}).update(op["set_fact"])
    return m


def delivered(case, author, mallory):
    """Ce que recoit le systeme qui decide, sur le chemin attaque : liste de
    {body, sig, claims}. body = {version, rules, facts}."""
    legit = [{"version": i + 1, **m} for i, m in enumerate(case["messages"])]
    signed = [{"body": b, "sig": author.sign(b), "claims": "author"} for b in legit]
    out = []
    for op in case["attack"]:
        kind = op["op"]
        if kind in ("deliver", "replay"):
            out.append(signed[op["message"]])
        elif kind == "modify":
            out.append({**signed[op["message"]], "body": apply_change(signed[op["message"]]["body"], op)})
        elif kind in ("forge_author", "insider"):
            body = {"version": len(legit) + 1, "rules": op.get("rules", []), "facts": op.get("facts", {})}
            out.append({"body": body, "sig": mallory.sign(body), "claims": "author" if kind == "forge_author" else "mallory"})
    return legit, out


def classic(mech, legit, items, author):
    """Messages acceptes par un mecanisme classique ; None = tout refuser."""
    if mech == "aucun":
        return "all"
    if mech == "moteur":
        return [it["body"] for it in items]                        # tout ce qui arrive compte
    accepted, top = [], 0
    for it in items:
        ok = it["claims"] == "author" and author.verify(it["body"], it["sig"])
        if mech == "signe":
            if not ok:
                return None                                         # ferme : tout refuser
            accepted.append(it["body"])
        else:                                                       # signe_versionne
            if not ok:
                accepted, top = list(legit), len(legit)             # rechargement chez l'auteur
                continue
            if it["body"]["version"] > top:
                accepted.append(it["body"])
                top = it["body"]["version"]
    return accepted


# -- Le vrai serveur CSTL ------------------------------------------------------
class Cstl:
    def __init__(self, binary):
        self.work = Path(tempfile.mkdtemp(prefix="cstl_bench_"))
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            self.port = s.getsockname()[1]
        env = {**os.environ, "CSTL_PORT": str(self.port), "CSTL_REST_PORT": "0", "CSTL_KB_VERIFY": "off",
               "CSTL_POLICY_AUTHORS": "author_*",
               # Le banc ouvre beaucoup de connexions depuis 127.0.0.1 : la limite
               # de debit par IP (protection reelle du serveur) est relevee ici.
               "CSTL_RATE_LIMIT_PER_IP_PER_MIN": "100000"}
        env.pop("CSTL_DEV_MODE", None)
        self.proc = subprocess.Popen([str(binary)], cwd=self.work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(240):
            try:
                socket.create_connection(("127.0.0.1", self.port), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        self.client = wire.Client("127.0.0.1", self.port)

    def stop(self):
        self.proc.terminate()
        self.proc.wait(timeout=15)

    def agents(self):
        tag = uuid.uuid4().hex[:8]
        a = {k: wire.Agent(self.client, name=f"{k}_{tag}") for k in ("author", "bot", "mallory")}
        for x in a.values():
            assert wire.first(x.send("agent_register", name=x.name, capabilities="communication"),
                              "INTENT_PAYLOAD").get("purpose") == "agent_register_ack"
        return a

    @staticmethod
    def relations(bot, body):
        rels = [{"type": "PERFORM", "subject": bot, "object": r["action"],
                 **{k: str(v) for k, v in r.items() if k != "action"}} for r in body.get("rules", [])]
        rels += [{"type": "STATE", "subject": f, "object": "true" if v else "false"} for f, v in body.get("facts", {}).items()]
        return rels

    def run(self, case, threat):
        a = self.agents()
        author, bot, mallory = a["author"], a["bot"], a["mallory"]
        texts = []
        for m in case["messages"]:
            msg = author.signed(author.message("inform", self.relations(bot.name, m), policy_version=str(len(texts) + 1)))
            texts.append((msg, wire.render(msg)))
        send = lambda text: self.client.send(text)  # noqa: E731
        if threat == "T1":
            for _, t in texts:                                       # l'auteur ecrit directement au serveur
                send(t)
        for op in case["attack"]:
            kind = op["op"]
            if kind == "deliver":
                if threat == "T2":
                    send(texts[op["message"]][1])
            elif kind == "replay":
                send(texts[op["message"]][1])                        # copie observee, renvoyee telle quelle
            elif kind == "modify":
                msg = copy.deepcopy(texts[op["message"]][0])
                body = apply_change(case["messages"][op["message"]], op)
                rels = self.relations(bot.name, body)
                msg["relations"] = rels                              # signature d'origine, contenu change
                send(wire.render(msg))
            elif kind == "forge_author":
                fake = wire.Agent(self.client, name=author.name, sk=mallory.sk)
                send(wire.render(fake.signed(fake.message("inform", self.relations(bot.name, op)))))
            elif kind == "insider":
                send(wire.render(mallory.signed(mallory.message("inform", self.relations(bot.name, op)))))
        out = []
        for rq in case["requests"]:
            i = wire.first(bot.ask(rq["action"], rq.get("amount")), "INTENT_PAYLOAD")
            out.append(i.get("decision") == "allowed")
        return out

    def legit(self, case):
        """Decisions du serveur sur la politique legitime (verifie l'evaluateur)."""
        return self.run({**case, "attack": []}, "T1")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--cases", default=str(HERE / "cases"))
    ap.add_argument("--binary", default=str(REPO / "target" / "debug" / "cstl_parser"))
    ap.add_argument("--out", default=str(HERE / "results.json"))
    args = ap.parse_args()
    cases = [json.loads(p.read_text()) for p in sorted(Path(args.cases).glob("*.json"))]
    if not Path(args.binary).exists():
        raise SystemExit("cargo build --bin cstl_parser d'abord")
    srv = Cstl(args.binary)
    author, mallory = Signer(), Signer()
    rows, mismatch = [], []
    try:
        for case in cases:
            legit_msgs = [{"version": i + 1, **m} for i, m in enumerate(case["messages"])]
            expect = [decide(legit_msgs, rq["action"], rq.get("amount")) for rq in case["requests"]]
            server_legit = srv.legit(case)
            if server_legit != expect:
                mismatch.append((case["id"], expect, server_legit))
            _, items = delivered(case, author, mallory)
            got = {}
            for mech in MECHANISMS[:-1]:
                acc = classic(mech, legit_msgs, items, author)
                got[mech] = [True] * len(expect) if acc == "all" else [False] * len(expect) if acc is None else \
                    [decide(acc, rq["action"], rq.get("amount")) for rq in case["requests"]]
            for threat in ("T1", "T2"):
                res = dict(got)
                res["cstl"] = srv.run(case, threat)
                for mech, dec in res.items():
                    rows.append({"case": case["id"], "kind": case["kind"], "threat": threat, "mechanism": mech,
                                 "requests": case["requests"], "expected": expect, "decisions": dec,
                                 "violations": sum(d and not e for d, e in zip(dec, expect)),
                                 "false_refusals": sum(e and not d for d, e in zip(dec, expect))})
    finally:
        srv.stop()

    if mismatch:
        print("ATTENTION : l'evaluateur differe de la couche 9 sur la politique legitime :")
        for m in mismatch:
            print("  ", m)
    print(f"\n{len(cases)} cas, {sum(len(c['requests']) for c in cases)} demandes. "
          "Cellule : violations / refus a tort (0 / 0 = juste partout).\n")
    for threat in ("T1", "T2"):
        print(f"Modele de menace {threat} ({'relais' if threat == 'T1' else 'canal de l auteur'}) :")
        print("  " + f"{'cas':28}" + "".join(f"{m:>17}" for m in MECHANISMS))
        for case in cases:
            cells = []
            for m in MECHANISMS:
                r = next(x for x in rows if x["case"] == case["id"] and x["threat"] == threat and x["mechanism"] == m)
                cells.append(f"{r['violations']} / {r['false_refusals']}")
            print("  " + f"{case['id']:28}" + "".join(f"{c:>17}" for c in cells))
        tot = {m: (sum(x["violations"] for x in rows if x["threat"] == threat and x["mechanism"] == m),
                   sum(x["false_refusals"] for x in rows if x["threat"] == threat and x["mechanism"] == m),
                   sum(1 for x in rows if x["threat"] == threat and x["mechanism"] == m and x["violations"])) for m in MECHANISMS}
        print("  " + f"{'TOTAL violations / refus':28}" + "".join(f"{f'{v} / {f}':>17}" for v, f, _ in tot.values()))
        print("  " + f"{'cas compromis':28}" + "".join(f"{c:>17}" for _, _, c in tot.values()) + "\n")
    Path(args.out).write_text(json.dumps({"cases": len(cases), "evaluator_mismatches": mismatch, "rows": rows},
                                         indent=1, ensure_ascii=False) + "\n")
    print(f"Resultats bruts : {Path(args.out).relative_to(REPO) if Path(args.out).is_relative_to(REPO) else args.out}")
    return 1 if mismatch else 0


if __name__ == "__main__":
    sys.exit(main())
