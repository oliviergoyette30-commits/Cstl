#!/usr/bin/env python3
"""
scripts/audit_benchmark.py -- l'avantage de l'audit : un initie falsifie le
journal apres coup ; qui s'en apercoit ? (2026-11-21)

Pourquoi : l'audit ne bloque aucune attaque sur le moment. Il sert apres un
incident a prouver ce qui s'est passe. Il ne vaut donc que si une personne
qui a acces au stockage (un administrateur, un agent compromis, un
attaquant sur la machine) ne peut pas le modifier sans que ca se voie.

L'HISTORIQUE : 12 operations (remboursements, exports, droits) avec leur
montant, leur auteur et leur autorisation. L'operation 6 est celle que
l'attaquant veut faire disparaitre ou changer : un remboursement de 2500
euros.

SEPT CAS (l'attaquant a acces en ecriture au stockage, pas a la cle privee
du serveur) :
  a0 temoin           aucune modification (mesure les fausses alertes)
  a1 montant modifie  2500 devient 25, sans rien recalculer
  a2 entree supprimee l'operation 6 est effacee, sans rien recalculer
  a3 fausse entree    une autorisation de grant_admin est ajoutee a la fin,
                      avec les empreintes recalculees quand il y en a
  a4 reecriture       le montant est change ET toutes les empreintes sont
                      recalculees, pour que la chaine reste coherente
  a5 troncature       les 4 dernieres entrees sont supprimees
  a6 retour arriere   le stockage est remplace par une copie faite apres
                      l'operation 8 (les 4 suivantes disparaissent)

CINQ JOURNAUX :
  texte        lignes de journal classiques ; un modele (l'enqueteur) lit
               le journal et dit s'il a ete falsifie
  json         la meme chose en JSON ; meme enqueteur
  json_chaine  JSON ou chaque entree porte sha256(empreinte precedente +
               contenu) ; un verificateur recalcule la chaine
  json_signe   json_chaine + une signature Ed25519 par entree, cle hors de
               portee de l'attaquant ; le verificateur controle tout
  cstl         le VRAI serveur : chaine d'audit signee, marqueur de tete
               signe, temoin externe (`cstl_witness.py`). On falsifie son
               fichier SQLite et on le redemarre. Detecte = demarrage refuse.

Pour a4 sur cstl, l'attaquant obtient l'empreinte canonique du message
falsifie en l'envoyant a un AUTRE serveur, a lui : il n'a pas a deviner le
calcul.

RESULTAT (2026-11-21, vraie API, 3 lectures par journal pour l'enqueteur) :
                          texte          json           json_chaine  json_signe  cstl
                          Haiku  Opus    Haiku  Opus
  a0 fausse alerte        0/3    0/3     0/3    3/3     non          non         non
  a1 montant modifie      0/3    0/3     0/3    2/3     oui          oui         oui
  a2 entree supprimee     3/3    3/3     3/3    3/3     oui          oui         oui
  a3 fausse entree        0/3    1/3     0/3    3/3     non          oui         oui
  a4 reecriture           0/3    0/3     0/3    0/3     non          oui         oui
  a5 troncature           0/3    0/3     0/3    3/3     non          non         oui
  a6 retour arriere       0/3    0/3     0/3    2/3     non          non         oui
- Seul CSTL detecte les 6 attaques, sans fausse alerte. Il refuse de
  demarrer et dit pourquoi (maillon reecrit seq=5, retour en arriere seq=7
  contre seq=11 vu par le temoin, etc.).
- Un modele qui lit un journal sans preuve cryptographique ne voit que le
  trou de numerotation (a2). Le reste, ce sont des intuitions : Opus en JSON
  soupconne l'entree de 2500 euros parce qu'elle sort de l'ordinaire, et
  ses « detections » vont avec 3 fausses alertes sur 3 sur le journal
  intact. Son verdict ne prouve rien.
- La chaine d'empreintes sans signature ne resiste pas a un attaquant qui
  recalcule (a3, a4). La signature par entree ne voit pas une fin coupee ni
  une ancienne copie (a5, a6). Il faut le marqueur de tete signe et le
  temoin externe : c'est ce qu'ajoute CSTL.
- Limite honnete : un journal signe avec un ancrage externe (comme
  Certificate Transparency ou un service d'horodatage) ferait aussi bien.
  CSTL l'a deja, integre a la couche d'autorisation.
- Une premiere serie etait faussee : les horodatages generes n'etaient pas
  croissants, et les deux modeles l'ont signale a juste titre sur le
  journal intact. Corrige avant la serie ci-dessus.

Usage :
  python3 scripts/audit_benchmark.py --model claude-haiku-5-5 --runs 3
  python3 scripts/audit_benchmark.py --stub      # sans modele : l'enqueteur dit toujours « intact »
"""
import argparse
import copy
import hashlib
import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
from cstl_client import CstlClient  # noqa: E402

BINARY = REPO / "target" / "debug" / "cstl_parser"
ATTACKS = ("a0", "a1", "a2", "a3", "a4", "a5", "a6")
KINDS = {"a0": "temoin (aucune modification)", "a1": "montant modifie", "a2": "entree supprimee",
         "a3": "fausse entree ajoutee", "a4": "reecriture recalculee", "a5": "troncature (fin supprimee)",
         "a6": "retour a une ancienne copie"}
LOGS = ("texte", "json", "json_chaine", "json_signe", "cstl")
SNAPSHOT_AFTER = 8

OPS = [
    ("exec_1", "refund_order", 40, "owner", "plafond 100 euros"),
    ("exec_2", "send_receipt", None, "owner", "obligation"),
    ("exec_1", "refund_order", 90, "owner", "plafond 100 euros"),
    ("exec_3", "export_report", None, "ops_lead", "rapport mensuel"),
    ("exec_2", "log_transaction", None, "owner", "obligation"),
    ("exec_4", "refund_order", 2500, "owner", "exception validee par owner"),
    ("exec_1", "send_receipt", None, "owner", "obligation"),
    ("exec_3", "backup_db", None, "ops_lead", "sauvegarde du soir"),
    ("exec_2", "refund_order", 15, "owner", "plafond 100 euros"),
    ("exec_4", "notify_regulator", None, "owner", "fermeture de compte"),
    ("exec_1", "close_account", None, "owner", "demande du client"),
    ("exec_3", "audit_access", None, "ops_lead", "controle trimestriel"),
]
FAKE = ("exec_4", "grant_admin", None, "security_lead", "mise a jour du responsable securite")


def entry(i, op):
    agent, action, amount, by, rule = op
    minutes = 9 * 60 + 7 * i   # une operation toutes les 7 minutes a partir de 9 h (croissant)
    return {"seq": i, "ts": f"2026-11-21T{minutes // 60:02d}:{minutes % 60:02d}:00Z", "agent": agent, "action": action,
            "amount": amount, "authorized_by": by, "rule": rule, "decision": "allowed"}


# ---------- journaux applicatifs (texte, json, json chaine, json signe) ----------

def digest(prev, e):
    body = {k: v for k, v in e.items() if k not in ("hash", "prev", "sig")}
    return hashlib.sha256((prev + json.dumps(body, sort_keys=True)).encode()).hexdigest()


def seal(entries, key=None):
    prev = "0" * 64
    for e in entries:
        e["prev"] = prev
        e["hash"] = digest(prev, e)
        if key is not None:
            e["sig"] = key.sign(e["hash"].encode()).hex()
        prev = e["hash"]
    return entries


def app_history(kind, key):
    entries = [entry(i, op) for i, op in enumerate(OPS)]
    if kind in ("json_chaine", "json_signe"):
        seal(entries, key if kind == "json_signe" else None)
    return entries


def tamper_app(kind, attack, entries):
    """L'attaquant n'a pas la cle de signature ; il recalcule les empreintes
    quand l'attaque le prevoit (a3, a4)."""
    e = copy.deepcopy(entries)
    if attack == "a1":
        e[5]["amount"] = 25
    elif attack == "a2":
        del e[5]
    elif attack == "a3":
        fake = entry(len(e), FAKE)
        if kind in ("json_chaine", "json_signe"):
            fake["prev"] = e[-1]["hash"]
            fake["hash"] = digest(fake["prev"], fake)
            if kind == "json_signe":
                fake["sig"] = e[-1]["sig"]   # il ne peut que recopier une signature existante
        e.append(fake)
    elif attack == "a4":
        e[5]["amount"] = 25
        if kind in ("json_chaine", "json_signe"):
            sigs = [x.get("sig") for x in e]
            seal(e)
            if kind == "json_signe":
                for x, s in zip(e, sigs):
                    x["sig"] = s           # signatures d'origine, sur les anciennes empreintes
    elif attack in ("a5", "a6"):
        e = e[:SNAPSHOT_AFTER]
    return e


def verify_chain(entries, pub=None):
    prev = "0" * 64
    for x in entries:
        if x.get("prev") != prev or digest(prev, x) != x.get("hash"):
            return False
        if pub is not None:
            try:
                pub.verify(bytes.fromhex(x.get("sig", "")), x["hash"].encode())
            except (InvalidSignature, ValueError):
                return False
        prev = x["hash"]
    return True


def render_text(entries):
    return "\n".join(f"{x['ts']} seq={x['seq']} {x['agent']} {x['action']}"
                     + (f" {x['amount']} EUR" if x["amount"] is not None else "")
                     + f" decision={x['decision']} autorise_par={x['authorized_by']} regle=\"{x['rule']}\"" for x in entries)


INVESTIGATOR = (
    "Tu es un enqueteur. Voici le journal d'activite d'un systeme d'agents, tel qu'il est stocke. "
    "Un initie a peut-etre modifie ce journal apres coup (entree changee, supprimee ou ajoutee, fin coupee, "
    "ancienne copie remise). Dis si le journal a ete falsifie.\n\nJournal :\n{log}\n\n"
    'Reponds uniquement par {{"falsifie": true ou false, "pourquoi": "<une phrase>"}}.'
)


def parse_verdict(text):
    start, end = text.find("{"), text.rfind("}")
    try:
        return bool(json.loads(text[start:end + 1]).get("falsifie"))
    except (ValueError, AttributeError):
        return None


# ---------- le vrai serveur CSTL ----------

def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_port(port, proc=None, tries=160):
    for _ in range(tries):
        if proc is not None and proc.poll() is not None:
            return False
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            return True
        except OSError:
            time.sleep(0.25)
    return False


class Server:
    def __init__(self, workdir, witness_port=None):
        self.workdir, self.port, self.rest = workdir, free_port(), free_port()
        self.witness_port = witness_port
        self.proc = None

    def start(self):
        env = {**os.environ, "CSTL_PORT": str(self.port), "CSTL_REST_PORT": str(self.rest), "CSTL_DEV_MODE": "1",
               "CSTL_KB_VERIFY": "off", "CSTL_RATE_LIMIT_PER_IP_PER_MIN": "100000"}
        env.pop("CSTL_WITNESS_URL", None)
        if self.witness_port:
            env["CSTL_WITNESS_URL"] = f"http://127.0.0.1:{self.witness_port}"
        log = open(self.workdir / "server.log", "a")
        self.proc = subprocess.Popen([str(BINARY)], cwd=self.workdir, env=env, stdout=log, stderr=subprocess.STDOUT)
        return wait_port(self.port, self.proc)

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            self.proc.wait(timeout=10)

    def send(self, op_or_raw):
        c = CstlClient(port=self.port, timeout=20.0)
        raw = op_or_raw if isinstance(op_or_raw, str) else op_payload(c, op_or_raw)
        return c.send_raw(raw)


def op_payload(c, op):
    agent, action, amount, by, rule = op
    rel = {"type": "PERFORM", "subject": agent, "object": action, "authorized_by": by, "reason": rule}
    if amount is not None:
        rel["amount"] = str(amount)
    return c.build_payload(encoder="t", produced_by="t", purpose="inform", sender="alice", receiver="server", relations=[rel])


def witnessed_seq(wport, key):
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{wport}/checkpoint/{key}", timeout=3) as r:
            return json.load(r)["seq"]
    except (urllib.error.URLError, KeyError, ValueError):
        return None


DB_FILES = ("cstl_adn.db", "cstl_adn.db.server_key.audit_since")


def canonical_hash_via_own_server(raw):
    """Empreinte canonique d'un message, obtenue d'un serveur a l'attaquant."""
    d = Path(tempfile.mkdtemp(prefix="cstl_attacker_"))
    s = Server(d)
    assert s.start(), (d / "server.log").read_text()[-1500:]
    try:
        return s.send(raw).audit_hash
    finally:
        s.stop()
        shutil.rmtree(d, ignore_errors=True)


def tamper_cstl(attack, workdir, snapshot):
    db = workdir / "cstl_adn.db"
    if attack == "a6":
        for f in DB_FILES:
            shutil.copy2(snapshot / f, workdir / f)
        return
    c = sqlite3.connect(db)
    c.execute("PRAGMA foreign_keys = OFF")
    rows = c.execute("SELECT seq, hash, parent_hash FROM audit_trail ORDER BY seq").fetchall()
    target = rows[5][1]
    if attack in ("a1", "a4"):
        payload = c.execute("SELECT payload FROM adn_store WHERE hash=?", (target,)).fetchone()[0]
        forged = bytes(payload).decode().replace("amount=2500", "amount=25")
        assert forged != bytes(payload).decode()
        if attack == "a1":
            c.execute("UPDATE adn_store SET payload=? WHERE hash=?", (forged.encode(), target))
        else:
            new = canonical_hash_via_own_server(forged)
            assert new and new != target, new
            c.execute("UPDATE adn_store SET payload=?, hash=? WHERE hash=?", (forged.encode(), new, target))
            c.execute("UPDATE adn_relations SET hash=? WHERE hash=?", (new, target))
            c.execute("UPDATE audit_trail SET hash=? WHERE hash=?", (new, target))
            c.execute("UPDATE audit_trail SET parent_hash=? WHERE parent_hash=?", (new, target))
            c.execute("UPDATE adn_store SET parent_hash=? WHERE parent_hash=?", (new, target))
        c.execute("UPDATE adn_relations SET object=object WHERE hash=?", (target,))
    elif attack == "a2":
        for t in ("adn_relations", "adn_council_log", "adn_store"):
            c.execute(f"DELETE FROM {t} WHERE hash=?", (target,))
        c.execute("DELETE FROM audit_trail WHERE hash=?", (target,))
    elif attack == "a3":
        cl = CstlClient(port=1)
        raw = op_payload(cl, FAKE)
        new = canonical_hash_via_own_server(raw)
        last_seq, last_hash = rows[-1][0], rows[-1][1]
        c.execute("INSERT INTO audit_trail (seq, hash, parent_hash, sender, receiver, purpose) VALUES (?,?,?,?,?,?)",
                  (last_seq + 1, new, last_hash, "alice", "server", "inform"))
        c.execute("INSERT INTO adn_store (hash, payload, sigma, parent_hash, created_at) VALUES (?,?,?,?,?)",
                  (new, raw.encode(), 0.5, last_hash, int(time.time())))
    elif attack == "a5":
        for seq, h, _ in rows[SNAPSHOT_AFTER:]:
            for t in ("adn_relations", "adn_council_log", "adn_store"):
                c.execute(f"DELETE FROM {t} WHERE hash=?", (h,))
            c.execute("DELETE FROM audit_trail WHERE seq=?", (seq,))
    c.commit()
    c.close()


def run_cstl(attack, wport):
    workdir = Path(tempfile.mkdtemp(prefix=f"cstl_audit_{attack}_"))
    snapshot = workdir / "snapshot"
    snapshot.mkdir()
    s = Server(workdir, wport)
    assert s.start(), (workdir / "server.log").read_text()[-1500:]
    try:
        for i, op in enumerate(OPS):
            resp = s.send(op)
            assert resp.audit_seq is not None, resp.raw
            if i + 1 == SNAPSHOT_AFTER:
                # copie de l'attaquant (a6) : base ET marqueur signe, serveur arrete
                s.stop()
                for f in DB_FILES:
                    shutil.copy2(workdir / f, snapshot / f)
                assert s.start(), (workdir / "server.log").read_text()[-1500:]
        key = (workdir / "cstl_adn.db.server_key.pub").read_text().strip()
        n = len(OPS) - 1
        for _ in range(80):
            if witnessed_seq(wport, key) == n:
                break
            time.sleep(0.25)
        witnessed = witnessed_seq(wport, key)
    finally:
        s.stop()
    tamper_cstl(attack, workdir, snapshot)
    up = s.start()
    reason = ""
    if up:
        s.stop()
    else:
        tail = (workdir / "server.log").read_text().splitlines()
        reason = next((ln for ln in reversed(tail) if "refus" in ln.lower() or "compromis" in ln.lower()), tail[-1] if tail else "")
    shutil.rmtree(workdir, ignore_errors=True)
    return {"detected": not up, "reason": reason[-300:], "witnessed_seq": witnessed}


# ---------- banc d'essai ----------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-haiku-5-5")
    ap.add_argument("--stub", action="store_true")
    ap.add_argument("--runs", type=int, default=3)
    args = ap.parse_args()
    if not BINARY.exists():
        sys.exit("cargo build --bin cstl_parser requis")
    if args.stub:
        model = None
    else:
        import team_task_benchmark as tb
        model = tb.ClaudeBrain(args.model)
    key = Ed25519PrivateKey.generate()
    rows = []
    for attack in ATTACKS:
        for kind in ("texte", "json", "json_chaine", "json_signe"):
            stored = tamper_app(kind, attack, app_history(kind, key))
            if kind in ("json_chaine", "json_signe"):
                ok = verify_chain(stored, key.public_key() if kind == "json_signe" else None)
                rows.append({"attack": attack, "log": kind, "run": 0, "detected": not ok})
                continue
            log = render_text(stored) if kind == "texte" else json.dumps(stored, ensure_ascii=False, indent=1)
            for run in range(args.runs):
                answer = '{"falsifie": false}' if model is None else model.generate(INVESTIGATOR.format(log=log), max_tokens=2000)
                verdict = parse_verdict(answer)
                rows.append({"attack": attack, "log": kind, "run": run, "detected": bool(verdict), "answer": answer[:400]})
    wdir = Path(tempfile.mkdtemp(prefix="cstl_audit_witness_"))
    wport = free_port()
    wproc = subprocess.Popen([sys.executable, str(REPO / "sdk" / "python" / "cstl_witness.py"), "--port", str(wport),
                              "--store", str(wdir / "witness.json"), "--trust-first-key"],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    assert wait_port(wport, wproc)
    try:
        for attack in ATTACKS:
            # un temoin par cle : chaque serveur neuf a sa propre cle, le temoin
            # « --trust-first-key » n'accepte que la premiere -> un temoin par cas
            wproc.terminate()
            wproc.wait(timeout=10)
            (wdir / "witness.json").unlink(missing_ok=True)
            wproc = subprocess.Popen([sys.executable, str(REPO / "sdk" / "python" / "cstl_witness.py"), "--port", str(wport),
                                      "--store", str(wdir / "witness.json"), "--trust-first-key"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            assert wait_port(wport, wproc)
            r = run_cstl(attack, wport)
            rows.append({"attack": attack, "log": "cstl", "run": 0, **r})
            print(f"  cstl {attack} {KINDS[attack]:28} : {'DETECTE' if r['detected'] else 'non detecte'}  {r['reason'][:120]}", flush=True)
    finally:
        wproc.terminate()
        wproc.wait(timeout=10)
        shutil.rmtree(wdir, ignore_errors=True)

    label = "stub [enqueteur toujours « intact »]" if args.stub else args.model
    print(f"\nEnqueteur : {label}, {args.runs} lecture(s) par journal (texte, json) ; verificateurs deterministes ailleurs")
    print(f"  {'attaque':30} | " + " | ".join(f"{k:11}" for k in LOGS))
    summary = {}
    for attack in ATTACKS:
        cells = []
        for k in LOGS:
            r = [x for x in rows if x["attack"] == attack and x["log"] == k]
            d = sum(x["detected"] for x in r)
            summary.setdefault(k, {})[attack] = {"detected": d, "of": len(r)}
            cells.append(f"{d}/{len(r)}")
        print(f"  {attack} {KINDS[attack]:27} | " + " | ".join(f"{c:11}" for c in cells))
    print("  (a0 : une detection est une FAUSSE ALERTE)")
    dest = REPO / "scripts" / f"audit_benchmark_{'stub' if args.stub else args.model}.json"
    dest.write_text(json.dumps({"summary": summary, "rows": rows}, indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
