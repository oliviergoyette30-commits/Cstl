#!/usr/bin/env python3
"""
cstl_demo.py -- CSTL en une minute, contre le VRAI serveur (2026-11-25).

  cstl-demo                      (paquet installe)
  python3 sdk/python/cstl_demo.py

Ce que la demo montre, sans rien simuler :
  1. un agent s'enregistre avec sa cle ; un responsable ecrit la politique ;
  2. une action permise recoit un jeton signe par le serveur, que le systeme
     qui execute verifie lui-meme (CstlGuard) ;
  3. une action interdite est refusee, avec la regle ;
  4. un plafond de montant s'applique ;
  5. un jeton falsifie ou reutilise est refuse par le systeme qui execute ;
  6. une autre cle ne peut pas parler au nom de l'agent ;
  7. quelqu'un modifie le journal d'audit dans la base : le serveur refuse
     de redemarrer et dit pourquoi.

Le serveur est le binaire Rust `cstl_parser` : --binary, CSTL_BINARY,
target/release ou target/debug du depot, ou le PATH. Tout se passe dans un
dossier temporaire, efface a la fin ; rien d'autre n'est touche.
"""
import argparse
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

SDK = Path(__file__).resolve().parent
sys.path.insert(0, str(SDK))


def find_binary(explicit=None):
    candidates = [explicit, os.environ.get("CSTL_BINARY")]
    for repo in (SDK.parent.parent, Path.cwd()):
        candidates += [repo / "target" / "release" / "cstl_parser", repo / "target" / "debug" / "cstl_parser"]
    candidates.append(shutil.which("cstl_parser"))
    for c in candidates:
        if c and Path(c).is_file() and os.access(c, os.X_OK):
            return Path(c)
    raise SystemExit("Serveur introuvable. Compilez-le : cargo build --release --bin cstl_parser "
                     "(dans le depot CSTL), ou donnez --binary / CSTL_BINARY.")


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Server:
    def __init__(self, binary, workdir):
        self.binary, self.workdir = binary, workdir
        self.port, self.rest = free_port(), free_port()
        self.proc = None

    def start(self):
        # Serveur de demonstration isole : aucune variable CSTL_* de
        # l'environnement (auteurs de politique, TLS, OIDC... d'un vrai serveur,
        # par exemple dans le conteneur Docker) ne change son comportement.
        env = {k: v for k, v in os.environ.items() if not k.startswith("CSTL_")}
        env.update({"CSTL_PORT": str(self.port), "CSTL_REST_PORT": str(self.rest), "CSTL_KB_VERIFY": "off"})
        self.log = open(self.workdir / "server.log", "a")
        self.proc = subprocess.Popen([str(self.binary)], cwd=self.workdir, env=env,
                                     stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(240):
            if self.proc.poll() is not None:
                return False
            try:
                socket.create_connection(("127.0.0.1", self.port), timeout=0.5).close()
                return True
            except OSError:
                time.sleep(0.25)
        return False

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            self.proc.wait(timeout=15)
        self.log.close()


def say(n, title):
    print(f"\n{n}. {title}")


def ok(text):
    print(f"   ✔ {text}")


def bad(text):
    print(f"   ✘ {text}")


def run(binary):
    work = Path(tempfile.mkdtemp(prefix="cstl_demo_"))
    os.environ["HOME"] = str(work)           # cles des agents dans ce dossier seulement
    import builtins
    quiet_print = builtins.print
    srv = Server(binary, work)
    failures = 0

    def check(cond, good, wrong):
        nonlocal failures
        if cond:
            ok(good)
        else:
            failures += 1
            bad(wrong)

    try:
        print(f"Serveur CSTL : {binary}")
        if not srv.start():
            raise SystemExit("Le serveur n'a pas demarre ; voir " + str(work / "server.log"))
        import cstl_llm_agent as m
        from cstl_guard import AuthorizationToken, CstlGuard, Unauthorized
        server_key = (work / "cstl_adn.db.server_key.pub").read_text().strip()
        guard = CstlGuard(server_key)    # dans le systeme qui execute : il ne croit que cette cle

        builtins.print = lambda *a, **k: None   # le SDK est bavard ; la demo parle seule
        owner = m.CstlAgent("responsable", "none", port=srv.port)
        bot = m.CstlAgent("agent_support", "none", port=srv.port)
        registered = owner.register() and bot.register()
        builtins.print = quiet_print

        say(1, "Deux agents s'enregistrent avec leur cle Ed25519 ; le responsable ecrit la politique")
        check(registered, "agents enregistres (messages signes)", "enregistrement refuse")
        owner.send_relations("server", [
            {"type": "PERFORM", "subject": "agent_support", "object": "wipe_db", "modality": "MUST_NOT",
             "reason": "base de production"},
            {"type": "PERFORM", "subject": "agent_support", "object": "refund_order", "modality": "MUST_NOT",
             "amount_above": "100"}])
        ok("interdit : wipe_db ; interdit : refund_order au-dela de 100")

        say(2, "Action permise : jeton signe par le serveur, verifie par le systeme qui execute")
        token = bot.authorize("read_logs")
        done = guard.run(token, "read_logs", lambda: "journaux lus", agent="agent_support")
        check(done == "journaux lus", f"read_logs execute (jeton valide {token.expires_at - token.issued_at} s)",
              "read_logs non execute")

        say(3, "Action interdite")
        try:
            bot.authorize("wipe_db")
            failures += 1
            bad("wipe_db autorise")
        except m.ActionDenied as e:
            ok(f"wipe_db refuse : raison {e.reason}, regle {e.rule}")

        say(4, "Plafond de montant")
        t50 = bot.authorize("refund_order", amount=50)
        guard.run(t50, "refund_order", lambda: None, agent="agent_support", amount=50)
        ok("remboursement de 50 : autorise et execute")
        try:
            bot.authorize("refund_order", amount=500)
            failures += 1
            bad("remboursement de 500 autorise")
        except m.ActionDenied as e:
            ok(f"remboursement de 500 : refuse ({e.reason})")

        say(5, "Le systeme qui execute refuse un jeton falsifie ou reutilise")
        t = bot.authorize("refund_order", amount=80)
        for label, fn in [
            ("montant change dans le jeton (80 -> 8000)",
             lambda: guard.run(AuthorizationToken(**{**t.__dict__, "amount": "8000"}), "refund_order",
                               lambda: None, agent="agent_support", amount=8000)),
            ("jeton de refund_order presente pour wipe_db",
             lambda: guard.run(t, "wipe_db", lambda: None, agent="agent_support")),
            ("sans jeton", lambda: guard.run(None, "wipe_db", lambda: None)),
        ]:
            try:
                fn()
                failures += 1
                bad(f"{label} : accepte")
            except Unauthorized as e:
                ok(f"{label} : refuse ({e.reason})")
        guard.run(t, "refund_order", lambda: None, agent="agent_support", amount=80)
        try:
            guard.run(t, "refund_order", lambda: None, agent="agent_support", amount=80)
            failures += 1
            bad("jeton reutilise : accepte")
        except Unauthorized as e:
            ok(f"meme jeton une deuxieme fois : refuse ({e.reason})")

        say(6, "Usurpation : une autre cle au nom de agent_support")
        builtins.print = lambda *a, **k: None
        shutil.rmtree(work / ".cstl")            # l'usurpateur n'a pas la cle : il en cree une autre
        thief = m.CstlAgent("agent_support", "none", port=srv.port)
        thief.registered = True
        allowed, resp = thief.request_action("read_logs")
        builtins.print = quiet_print
        check(not allowed, f"refuse ({resp.purpose} {resp.fields.get('reason', '')})".strip(),
              "l'usurpateur a ete autorise")

        say(7, "Quelqu'un modifie le journal d'audit dans la base (remboursement 50 -> 5)")
        srv.stop()
        db = sqlite3.connect(work / "cstl_adn.db")
        row = db.execute("SELECT hash, payload FROM adn_store WHERE CAST(payload AS TEXT) LIKE '%amount=50%' "
                         "ORDER BY rowid LIMIT 1").fetchone()
        forged = bytes(row[1]).decode().replace("amount=50", "amount=5", 1)
        db.execute("UPDATE adn_store SET payload=? WHERE hash=?", (forged.encode(), row[0]))
        db.commit()
        db.close()
        ok("une ligne de la base SQLite modifiee a la main")
        restarted = srv.start()
        log = (work / "server.log").read_text(errors="replace").splitlines()
        reason = next((x for x in reversed(log) if "Demarrage impossible" in x or "❌" in x), "")
        check(not restarted, f"le serveur refuse de redemarrer : {reason.strip()[:150]}",
              "le serveur a redemarre sur une base falsifiee")
    finally:
        builtins.print = quiet_print
        srv.stop()
        shutil.rmtree(work, ignore_errors=True)

    print("\n" + ("Tout est conforme : 7/7." if failures == 0 else f"{failures} verification(s) en echec."))
    return failures


def main(argv=None):
    ap = argparse.ArgumentParser(description="Demonstration de CSTL contre le vrai serveur")
    ap.add_argument("--binary", help="chemin du binaire cstl_parser")
    args = ap.parse_args(argv)
    sys.exit(1 if run(find_binary(args.binary)) else 0)


if __name__ == "__main__":
    main()
