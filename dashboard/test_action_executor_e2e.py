#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
dashboard/test_action_executor_e2e.py -- verification REELLE, bout-en-bout,
de action_executor.py contre un VRAI serveur CSTL (target/release/cstl_parser)
lance dans un repertoire jetable, avec un VRAI fichier ADN SQLite jetable.

AUCUNE PARTIE DE CE SCENARIO N'EST SIMULEE :
  1. Lance reellement le vrai binaire compile (cargo build --release deja
     execute avant ce script), dans un repertoire de travail jetable (donc
     ecrit dans <scratch>/cstl_adn.db -- jamais le cstl_adn.db du depot).
  2. Enregistre reellement l'agent "Olivier" (purpose=agent_register,
     auto-signe Ed25519) -- reutilise sdk/python/cstl_llm_agent.py
     (load_or_create_keypair, register_agent, sign_intent) et
     sdk/python/cstl_client.py (CstlClient), rien de cryptographique
     reimplemente ici.
  3. Envoie un vrai payload purpose=action_request (tool=shell,
     command="echo CSTL_ACTION_REAL_TEST_12345") sur le vrai socket TCP,
     recupere le hash REEL retourne dans AUDIT [hash=...].
  4. Committe REELLEMENT ce hash via un vrai council_decision signe par
     "Olivier" (le seul membre autorise par defaut de RestrictedCouncil,
     voir src/restricted_council.rs::from_env() -- CSTL_COUNCIL_MEMBERS non
     definie ici, donc single_member("Olivier")). Confirme apres coup en
     RELISANT cstl_adn.db (committed=1), pas une supposition.
  5. Appelle action_executor.run_action_poll_cycle() une seule fois pour de
     vrai, et confirme:
       (a) une ligne apparait dans dashboard_actions.db avec le bon hash
       (b) stdout contient reellement "CSTL_ACTION_REAL_TEST_12345"
       (c) un nouveau payload purpose=action_result a bien ete envoye et
           apparait dans le VRAI audit_trail du serveur (relu depuis le
           fichier SQLite, jamais suppose).

Usage:
    cargo build --release   # une fois, avant ce script
    python3 dashboard/test_action_executor_e2e.py
"""

import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DASHBOARD_DIR = REPO_ROOT / "dashboard"
SDK_DIR = REPO_ROOT / "sdk" / "python"
SERVER_BIN = REPO_ROOT / "target" / "release" / "cstl_parser"
PORT = 5050  # port en dur cote Rust (main.rs) -- aucune variable d'env pour le changer

FAILURES = []


def check(label, condition, detail=""):
    status = "OK" if condition else "ECHEC"
    print(f"  [{status}] {label}" + (f" -- {detail}" if detail else ""))
    if not condition:
        FAILURES.append(f"{label} -- {detail}")


def main():
    if not SERVER_BIN.exists():
        print(f"binaire introuvable: {SERVER_BIN} -- lance `cargo build --release` d'abord.")
        return 1

    scratch = Path(tempfile.mkdtemp(prefix="cstl_action_e2e_"))
    adn_db_path = scratch / "cstl_adn.db"
    actions_db_path = scratch / "dashboard_actions_test.db"
    keyfile = scratch / "olivier_ed25519.key"
    print(f"[setup] repertoire jetable: {scratch}")
    print(f"[setup] fichier ADN jetable: {adn_db_path}")
    print(f"[setup] base dashboard_actions.db jetable: {actions_db_path}")

    # Variables d'env AVANT tout import de dashboard/action_executor.py et
    # dashboard/server.py (constantes module-level lues a l'import).
    os.environ["CSTL_SERVER_HOST"] = "127.0.0.1"
    os.environ["CSTL_SERVER_PORT"] = str(PORT)
    os.environ["CSTL_ADN_DB_PATH"] = str(adn_db_path)
    os.environ["CSTL_DASHBOARD_ACTIONS_DB"] = str(actions_db_path)
    os.environ.pop("CSTL_COUNCIL_MEMBERS", None)  # -> single_member("Olivier"), config par defaut

    sys.path.insert(0, str(DASHBOARD_DIR))
    sys.path.insert(0, str(SDK_DIR))

    import action_executor  # noqa: E402
    import server as dashboard_server  # noqa: E402
    from cstl_client import CstlClient  # noqa: E402
    from cstl_llm_agent import load_or_create_keypair, register_agent, sign_intent  # noqa: E402

    print(f"\n[1/6] Lancement reel du serveur CSTL ({SERVER_BIN}) dans {scratch}, port {PORT}...")
    proc = subprocess.Popen(
        [str(SERVER_BIN)], cwd=str(scratch),
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        connected = False
        for _ in range(50):
            try:
                import socket
                with socket.create_connection(("127.0.0.1", PORT), timeout=0.5):
                    connected = True
                    break
            except OSError:
                time.sleep(0.2)
        check("serveur CSTL accepte une connexion TCP reelle", connected)
        if not connected:
            print(proc.stdout.read() if proc.stdout else "(pas de sortie capturee)")
            return 1

        client = CstlClient(host="127.0.0.1", port=PORT, timeout=15.0)

        print("\n[2/6] Enregistrement reel de l'agent 'Olivier' (agent_register auto-signe)...")
        sk, pubkey_hex = load_or_create_keypair(keyfile)
        reg = register_agent(client, sk, pubkey_hex, "Olivier", capabilities=["council"])
        print(f"      purpose={reg.purpose!r} fields={reg.fields}")
        check("agent_register_ack recu pour Olivier", reg.purpose == "agent_register_ack", reg.raw)

        print("\n[3/6] Envoi reel d'un vrai payload purpose=action_request (tool=shell)...")
        action_payload = (
            "#!CSTL v5.0.0 MODE=A\n"
            "META [encoder=E2ETest, produced_by=E2ETest]\n"
            "INTENT_PAYLOAD [purpose=action_request, sender=dashboard_user, receiver=server, "
            "tool=shell, command=\"echo CSTL_ACTION_REAL_TEST_12345\"]\n"
            "---END---\n"
        )
        resp = client.send_raw(action_payload)
        print(f"      purpose={resp.purpose!r} audit_hash={resp.audit_hash!r}")
        target_hash = resp.audit_hash
        check("purpose=action_request accepte (hash AUDIT recu)", target_hash is not None, resp.raw)
        if not target_hash:
            return 1

        print("\n[4/6] Commit REEL via council_decision signe par 'Olivier'...")
        meta = {"encoder": "E2ETest", "produced_by": "Olivier", "public_key": pubkey_hex}
        intent = {
            "purpose": "council_decision", "sender": "Olivier", "receiver": "server",
            "target_hash": target_hash, "decision": "commit",
        }
        sig_hex = sign_intent(sk, pubkey_hex, version="v5.0.0", mode="A",
                               meta=meta, intent=intent, relations=[])
        intent["signature"] = sig_hex
        council_payload = client.build_payload(
            encoder=meta["encoder"], produced_by=meta["produced_by"], purpose="council_decision",
            sender="Olivier", receiver="server",
            extra_meta={"public_key": pubkey_hex},
            extra_intent={k: v for k, v in intent.items() if k not in ("purpose", "sender", "receiver")},
        )
        council_resp = client.send_raw(council_payload)
        print(f"      purpose={council_resp.purpose!r} fields={council_resp.fields}")
        check(
            "council_decision_applied avec committed=true",
            council_resp.purpose == "council_decision_applied" and council_resp.fields.get("committed") == "true",
            council_resp.raw,
        )

        client.close()

        print("\n[4b/6] Relecture DIRECTE du fichier ADN (readonly) pour confirmer committed=1...")
        uri = f"file:{adn_db_path.as_posix()}?mode=ro"
        conn = sqlite3.connect(uri, uri=True, timeout=2.0)
        row = conn.execute("SELECT committed, committed_by FROM adn_store WHERE hash=?", (target_hash,)).fetchone()
        conn.close()
        print(f"      row lue dans adn_store: committed={row[0] if row else None} committed_by={row[1] if row else None}")
        check("committed=1 confirme par lecture directe de adn_store (pas une supposition)",
              row is not None and row[0] == 1)

        print("\n[5/6] Appel reel de action_executor.run_action_poll_cycle() (un seul cycle)...")
        cycle_result = action_executor.run_action_poll_cycle()
        print(f"      ok={cycle_result.get('ok')} processed={cycle_result.get('processed')}")
        check("run_action_poll_cycle a traite exactement 1 action", cycle_result.get("processed") == 1, cycle_result)

        matching = [r for r in cycle_result.get("results", []) if r["hash"] == target_hash]
        check("l'action traitee correspond bien au hash committe", len(matching) == 1)
        if matching:
            exec_result = matching[0]["execution"]
            print(f"      execution: exit_code={exec_result.get('exit_code')} stdout={exec_result.get('stdout')!r}")
            check("exit_code == 0", exec_result.get("exit_code") == 0)
            check("stdout contient CSTL_ACTION_REAL_TEST_12345",
                  "CSTL_ACTION_REAL_TEST_12345" in (exec_result.get("stdout") or ""), exec_result.get("stdout"))
            submit_result = matching[0]["submit_result"]
            print(f"      submit_result: ok={submit_result.get('ok')} round_trip_ms={submit_result.get('round_trip_ms')}")
            check("le payload action_result a bien ete envoye (ok=True)", submit_result.get("ok") is True, submit_result)

        print("\n[5b/6] Confirmation (a) : ligne dans dashboard_actions.db avec le bon hash...")
        exec_conn = sqlite3.connect(str(actions_db_path))
        exec_row = exec_conn.execute(
            "SELECT hash, tool, command, exit_code, stdout, executed_at FROM executed_actions WHERE hash=?",
            (target_hash,),
        ).fetchone()
        exec_conn.close()
        print(f"      ligne dashboard_actions.db: {exec_row}")
        check("ligne presente dans executed_actions (dashboard_actions.db)", exec_row is not None)
        if exec_row:
            check("stdout stocke localement contient bien CSTL_ACTION_REAL_TEST_12345",
                  "CSTL_ACTION_REAL_TEST_12345" in (exec_row[4] or ""))

        print("\n[6/6] Confirmation (c) : purpose=action_result present dans le VRAI audit_trail du serveur...")
        conn2 = sqlite3.connect(uri, uri=True, timeout=2.0)
        conn2.row_factory = sqlite3.Row
        audit_rows = conn2.execute(
            "SELECT hash, sender, receiver, purpose, parent_hash FROM audit_trail "
            "WHERE purpose='action_result' ORDER BY seq DESC LIMIT 5"
        ).fetchall()
        conn2.close()
        print(f"      lignes audit_trail purpose=action_result: {[dict(r) for r in audit_rows]}")
        check("au moins une ligne purpose=action_result dans audit_trail", len(audit_rows) >= 1)

        print("\n[bonus] Re-execution du meme cycle -- ne doit PAS re-executer la meme action deux fois...")
        cycle_result_2 = action_executor.run_action_poll_cycle()
        print(f"      ok={cycle_result_2.get('ok')} processed={cycle_result_2.get('processed')}")
        check("2e appel de run_action_poll_cycle() ne retraite pas la meme action (processed=0)",
              cycle_result_2.get("processed") == 0, cycle_result_2)

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        shutil.rmtree(scratch, ignore_errors=True)

    print()
    if FAILURES:
        print(f"XXX {len(FAILURES)} ECHEC(S):")
        for f in FAILURES:
            print(f"   - {f}")
        return 1
    print("Tous les scenarios de verification reelle sont conformes.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
