#!/usr/bin/env python3
"""
benchmark/opa_runner.py -- le VRAI OPA (Open Policy Agent) dans le banc
public (2026-11-27). Utilise par control_benchmark.py avec --opa <binaire>.

OPA ne decide pas d'une politique envoyee message par message : l'auteur
publie des PAQUETS (bundles), chacun contenant toute sa politique a ce
moment-la, et OPA les telecharge depuis un serveur de paquets. C'est la
pratique recommandee par le projet OPA pour une politique distribuee :
paquets SIGNES par l'auteur (`opa build --signing-key`), verifies par OPA
avec la cle publique de l'auteur (`keys` + `bundles.<nom>.signing`). Un
paquet dont la verification echoue n'est pas active : OPA garde le dernier
paquet valide.

Ce que fait ce module, pour chaque cas d'attaque :
  - l'auteur construit un paquet par message (instantane cumule 1..i) avec
    la vraie commande `opa build`, signe RS256 par SA cle ;
  - le relais attaquant sert a OPA ce qu'il veut, dans l'ordre du cas :
      deliver i / replay i : le paquet i de l'auteur (tel quel) ;
      modify i             : le paquet i, data.json modifie, signature
                             d'origine conservee ;
      forge_author/insider : un paquet signe par la cle de l'attaquant ;
  - un vrai serveur OPA (`opa run --server`) telecharge chaque paquet servi
    (sondage chaque seconde), le verifie et l'active ou le refuse ;
  - les decisions viennent de l'API de donnees d'OPA
    (POST /v1/data/cstl/allow).
La regle (policy.rego) est la meme que l'evaluateur du banc (`decide`) :
la derniere regle d'une action decide, montant ou fait inconnu ->
l'interdiction s'applique. Elle est verifiee egale sur la politique
legitime avant toute mesure.

Deux mecanismes :
  opa          paquets NON signes (OPA tel qu'on le deploie souvent)
  opa_signe    paquets signes et verifies (la pratique recommandee)
Sans paquet actif, OPA ne repond rien : compte comme REFUS (deploiement
ferme par defaut, `default allow := false` cote application).

Limite : un seul canal (auteur -> serveur de paquets -> OPA) ; les modeles
de menace T1 et T2 du banc donnent donc le meme resultat pour OPA.
"""
import gzip
import io
import json
import os
import shutil
import socket
import subprocess
import tarfile
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REGO = """package cstl

import rego.v1

# Meme regle que decide() dans control_benchmark.py : la DERNIERE regle de
# l'action (messages dans l'ordre, regles dans l'ordre) decide ; sans regle :
# permis ; montant ou fait inconnu -> l'interdiction s'applique (prudence).
default allow := true

matching := [r |
	some m in data.policy.messages
	some r in object.get(m, "rules", [])
	r.action == input.action
]

last := matching[count(matching) - 1] if count(matching) > 0

facts := object.union_n([object.get(m, "facts", {}) | some m in data.policy.messages])

allow := false if {
	last.modality == "MUST_NOT"
	amount_applies
	if_applies
	unless_applies
}

amount_applies if not last.amount_above

amount_applies if {
	last.amount_above
	not input.amount
}

amount_applies if to_number(input.amount) > to_number(last.amount_above)

if_applies if not last.if_fact

if_applies if {
	last.if_fact
	not facts[last.if_fact] == false
}

unless_applies if not last.unless_fact

unless_applies if {
	last.unless_fact
	not facts[last.unless_fact] == true
}
"""


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Opa:
    def __init__(self, binary, work=None):
        self.bin = str(binary)
        self.work = Path(work or tempfile.mkdtemp(prefix="cstl_opa_"))
        self.version = subprocess.run([self.bin, "version"], capture_output=True, text=True).stdout.splitlines()[0]
        self.keys = {}
        for who in ("author", "mallory"):
            priv = self.work / f"{who}.pem"
            pub = self.work / f"{who}.pub.pem"
            subprocess.run(["openssl", "genrsa", "-out", str(priv), "2048"], check=True, capture_output=True)
            subprocess.run(["openssl", "rsa", "-in", str(priv), "-pubout", "-out", str(pub)], check=True, capture_output=True)
            self.keys[who] = (priv, pub)

    # -- paquets --------------------------------------------------------------
    def build(self, messages, revision, signer=None):
        d = Path(tempfile.mkdtemp(dir=self.work))
        (d / "policy.rego").write_text(REGO)
        (d / "data.json").write_text(json.dumps({"policy": {"messages": messages}}))
        (d / ".manifest").write_text(json.dumps({"revision": revision}))
        out = d.with_suffix(".tar.gz")
        cmd = [self.bin, "build", "-b", str(d), "-o", str(out)]
        if signer:
            cmd += ["--signing-key", str(self.keys[signer][0]), "--signing-alg", "RS256"]
        subprocess.run(cmd, check=True, capture_output=True, text=True)
        shutil.rmtree(d)
        return out.read_bytes()

    @staticmethod
    def tamper(bundle, messages):
        """Le meme paquet (signature d'origine comprise), data.json remplace."""
        src = tarfile.open(fileobj=io.BytesIO(bundle), mode="r:gz")
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as dst:
            for member in src.getmembers():
                data = src.extractfile(member).read() if member.isfile() else None
                if member.name.lstrip("/") == "data.json":
                    data = json.dumps({"policy": {"messages": messages}}).encode()
                    member.size = len(data)
                dst.addfile(member, io.BytesIO(data) if data is not None else None)
        return buf.getvalue()

    # -- execution d'un cas ---------------------------------------------------
    def run(self, case, signed, apply_change):
        """Decisions d'OPA pour les demandes du cas, apres les livraisons du
        relais. `apply_change` : la fonction du banc qui applique `modify`."""
        msgs = case["messages"]
        signer = "author" if signed else None
        snapshots = [self.build(msgs[:i + 1], f"v{i + 1}", signer) for i in range(len(msgs))]
        served = []
        for op in case["attack"]:
            kind = op["op"]
            if kind in ("deliver", "replay"):
                served.append(snapshots[op["message"]])
            elif kind == "modify":
                i = op["message"]
                altered = msgs[:i] + [apply_change(msgs[i], op)]
                served.append(self.tamper(snapshots[i], altered))
            elif kind in ("forge_author", "insider"):
                body = {"rules": op.get("rules", []), "facts": op.get("facts", {})}
                served.append(self.build(msgs + [body], f"v{len(msgs) + 1}", "mallory" if signed else None))
        return self._serve_and_ask(served, case["requests"], signed)

    def legit(self, case, signed=True):
        msgs = case["messages"]
        bundle = self.build(msgs, f"v{len(msgs)}", "author" if signed else None)
        return self._serve_and_ask([bundle], case["requests"], signed)

    def _serve_and_ask(self, served, requests, signed):
        state = {"bundle": None, "gets": 0}
        lock = threading.Lock()

        class H(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_GET(self):
                with lock:
                    data = state["bundle"]
                    state["gets"] += 1
                if data is None:
                    self.send_response(404)
                    self.end_headers()
                    return
                self.send_response(200)
                self.send_header("Content-Type", "application/gzip")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        hport, oport = free_port(), free_port()
        httpd = ThreadingHTTPServer(("127.0.0.1", hport), H)
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        cfg = {"services": {"relay": {"url": f"http://127.0.0.1:{hport}"}},
               "bundles": {"cstl": {"service": "relay", "resource": "/bundle.tar.gz",
                                    "polling": {"min_delay_seconds": 1, "max_delay_seconds": 1}}}}
        if signed:
            cfg["keys"] = {"author": {"algorithm": "RS256", "key": self.keys["author"][1].read_text()}}
            cfg["bundles"]["cstl"]["signing"] = {"keyid": "author"}
        conf = Path(tempfile.mkdtemp(dir=self.work)) / "config.json"
        conf.write_text(json.dumps(cfg))
        proc = subprocess.Popen([self.bin, "run", "--server", "--addr", f"127.0.0.1:{oport}", "-c", str(conf),
                                 "--log-level", "error"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for _ in range(100):
                try:
                    socket.create_connection(("127.0.0.1", oport), timeout=0.3).close()
                    break
                except OSError:
                    time.sleep(0.1)
            for bundle in served:
                with lock:
                    state["bundle"], state["gets"] = bundle, 0
                # Deux telechargements apres le changement : le premier prend le
                # nouveau paquet, le second n'arrive qu'une fois celui-ci traite
                # (active ou refuse) -- les telechargements sont sequentiels.
                deadline = time.time() + 30
                while time.time() < deadline:
                    with lock:
                        if state["gets"] >= 2:
                            break
                    time.sleep(0.05)
                else:
                    raise RuntimeError("OPA n'a pas telecharge le paquet")
            out = []
            for rq in requests:
                inp = {"action": rq["action"]}
                if rq.get("amount") is not None:
                    inp["amount"] = rq["amount"]
                req = urllib.request.Request(f"http://127.0.0.1:{oport}/v1/data/cstl/allow",
                                             data=json.dumps({"input": inp}).encode(),
                                             headers={"Content-Type": "application/json"}, method="POST")
                with urllib.request.urlopen(req, timeout=10) as r:
                    res = json.loads(r.read())
                out.append(res.get("result") is True)          # aucun paquet actif -> refus
            return out
        finally:
            proc.terminate()
            proc.wait(timeout=10)
            httpd.shutdown()

    def close(self):
        shutil.rmtree(self.work, ignore_errors=True)
