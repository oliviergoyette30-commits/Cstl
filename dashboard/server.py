#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
dashboard/server.py -- petit serveur web local (bibliotheque standard
uniquement, aucune dependance a installer) qui sert de tableau de bord pour
l'OS CSTL agentic kernel, a cote du vrai serveur Rust CSTL (`cargo run`,
port 5050 par defaut).

LANCEMENT (macOS, depuis la racine du depot) :

    python3 dashboard/server.py
    # puis ouvrir http://127.0.0.1:5099/ dans un navigateur

PREREQUIS : Python 3 (deja present sur macOS). Aucune dependance externe --
tout ce fichier utilise uniquement `http.server`, `sqlite3` et `socket`, qui
font partie de la bibliotheque standard. Rien a installer avec pip.

VARIABLES D'ENVIRONNEMENT (toutes optionnelles) :
    CSTL_DASHBOARD_PORT   port d'ecoute de CE dashboard (defaut 5099 --
                          different du port 5050 du serveur CSTL lui-meme)
    CSTL_SERVER_HOST      hote du serveur CSTL a interroger (defaut 127.0.0.1)
    CSTL_SERVER_PORT      port du serveur CSTL a interroger (defaut 5050)
    CSTL_ADN_DB_PATH      chemin vers le fichier SQLite de l'ADN store
                          (defaut cstl_adn.db, relatif au repertoire courant
                          -- c'est le meme fichier que celui ouvert par
                          `src/main.rs` cote Rust)
    CSTL_GRAPHIFY_OUT     chemin vers le dossier graphify-out/ (defaut
                          graphify-out/ relatif au repertoire courant)

CE QUE CE DASHBOARD FAIT REELLEMENT (honnete, pas de simulation) :
  - Il ouvre le fichier SQLite `cstl_adn.db` DIRECTEMENT en lecture seule
    (`file:...?mode=ro`, URI SQLite) pendant que le serveur Rust tourne et
    y ecrit. SQLite gere nativement plusieurs lecteurs simultanes tant
    qu'aucune connexion n'est en mode WAL-incompatible exclusif -- le mode
    par defaut (rollback journal) autorise un lecteur pendant qu'un
    ecrivain travaille, sauf pendant les quelques millisecondes ou
    l'ecrivain detient le verrou EXCLUSIVE au moment du COMMIT (auquel cas
    ce dashboard recoit `sqlite3.OperationalError: database is locked` --
    ce n'est pas un bug, c'est SQLite qui refuse honnetement plutot que de
    lire des donnees a moitie ecrites ; le endpoint le rapporte tel quel).
  - Il ouvre une VRAIE connexion TCP vers 127.0.0.1:5050 (ou l'hote/port
    configure), envoie EXACTEMENT le payload CSTL construit a partir des
    champs du formulaire, et retourne la reponse brute telle quelle,
    sans reformattage ni interpretation -- ce que l'utilisateur voit dans
    la zone "reponse brute" est un copier-coller de ce que le process Rust
    a ecrit sur le socket, rien de plus.
  - Le statut "serveur CSTL" est une vraie tentative de connexion TCP avec
    timeout court (1s) a chaque appel du endpoint /api/status -- jamais un
    booleen en dur. Si le process Rust est tue, ce statut passe a down des
    le prochain rafraichissement (bouton manuel, pas de polling automatique).

LIMITES HONNETES DE CETTE V1 :
  - Pas de push/notifications : il faut cliquer sur "Rafraichir" pour revoir
    l'etat courant (statut serveur + ADN store). Aucun WebSocket, aucun
    polling automatique en arriere-plan.
  - Le registre d'agents (`AgentRegistry`, alice/bob) vit UNIQUEMENT en
    memoire dans le process Rust -- rien ne l'expose depuis l'exterieur du
    process aujourd'hui (pas d'endpoint HTTP/TCP dedie cote serveur Rust
    pour le lister). Ce dashboard ne peut donc PAS afficher la liste des
    agents enregistres ; il peut seulement envoyer un payload et montrer
    ce que le routage a produit (champ `receiver=` de l'accusé de reception,
    visible dans la reponse brute).
  - "Utiliser le multi agent" via ce dashboard signifie : le payload est
    route par le serveur Rust vers un nom d'agent enregistre en memoire
    (alice/bob par defaut, capability="communication") et le serveur
    repond par un accuse de reception structure (INTENT_PAYLOAD purpose=
    acknowledgement, RELATION type=received, AUDIT hash=...) -- PAS par
    un texte genere par un vrai modele de langage. Un agent LLM reel existe
    dans ce depot (`sdk/python/cstl_llm_agent.py`, Anthropic/Gemini) mais
    tourne comme un process Python separe, hors de ce dashboard -- ce
    dashboard ne le lance pas et ne s'y connecte pas.
  - Hermes/Ollama et OpenClaw : toujours non integres cote serveur Rust
    (confirme par recherche dans src/ -- aucune reference dans le code
    Rust), mais CE dashboard sait desormais tenter une vraie connexion vers
    chacun (voir /api/generate-relation et /api/openclaw-check ci-dessous) :
      * /api/generate-relation (POST {"topic": "..."}) essaie dans l'ordre
        Hermes/Ollama local (127.0.0.1:11434) -> Gemini -> Anthropic via
        sdk/python/cstl_llm_agent.py::resolve_brain("auto") (code reutilise
        tel quel, jamais duplique). Dans CE sandbox, aucun des trois n'est
        disponible (Ollama absent, confirme par `which ollama` et
        `curl http://localhost:11434/api/tags`; ni GEMINI_API_KEY ni
        ANTHROPIC_API_KEY dans l'environnement) -- l'endpoint le rapporte
        honnetement plutot que d'inventer une relation.
      * /api/openclaw-check tente une connexion (WebSocket si le paquet
        `websockets` est installe, sinon TCP brut) vers
        ws://127.0.0.1:19001 -- AUCUN protocole applicatif OpenClaw n'est
        parle (aucune documentation disponible pour l'ecrire), seulement un
        test de connectivite generique. Dans ce sandbox, retourne toujours
        "inaccessible" puisqu'OpenClaw tourne sur la machine macOS de
        l'utilisateur, jamais ici -- resultat ATTENDU, pas une erreur.
  - Graphify : /api/graphify-summary lit le VRAI fichier
    graphify-out/graph.json (pas de "topology.json" dans ce depot) et
    retourne noeuds/liens/communautes reels + comparaison honnete de
    built_at_commit contre HEAD (perime si ce n'est pas un ancetre de HEAD).
  - Telegram et Obsidian : /api/telegram-obsidian-status (voir
    check_telegram_obsidian_status() ci-dessous) remplace desormais la ligne
    statique "cable cote serveur, pas visible ici" par une VRAIE verification
    en deux temps, honnetement bornee :
      1. Ce que CE process Python voit dans SON PROPRE os.environ pour
         TELEGRAM_BOT_TOKEN/TELEGRAM_CHAT_ID/OBSIDIAN_VAULT_PATH -- affiche
         explicitement comme "vu par le dashboard, PAS une preuve de ce que
         voit le process Rust" (deux process = deux espaces memoire = deux
         os.environ distincts, jamais partages).
      2. Preuve indirecte REELLE via lecture directe de cstl_adn.db :
         - priority=critical (handler.rs STEP 3-priority) n'ecrit RIEN en
           base -- seul effet: un tokio::spawn fire-and-forget vers l'API
           Telegram (jamais persiste) + un eprintln sur stderr Rust
           (inaccessible ici). Le seul signal recuperable est que le
           payload texte contenant "priority=critical" a bien ete stocke
           tel quel dans adn_store.payload -- preuve que le SERVEUR a recu
           la demande d'escalade, PAS que Telegram l'a effectivement recue
           (ca depend de TELEGRAM_BOT_TOKEN/CHAT_ID cote Rust, invisible
           d'ici).
         - Le circuit-breaker de gouvernance (STEP 3c-governance) est un
           mecanisme DIFFERENT qui, lui, persiste dans governance_alerts
           (last_alert_ts) et declenche le meme genre d'envoi Telegram --
           preuve indirecte plus solide (table dediee), affichee separement.
         - Obsidian (STEP 3c-bis) ecrit dans un fichier markdown du vault,
           jamais dans SQLite -- proxy retenu : governance_events.
           inconsistency=1, car l'escalade Obsidian se declenche exactement
           sur la meme condition (!consistency.consistent).
    Honnetete assumee : si aucune de ces preuves indirectes n'existe, le
    endpoint dit "aucune escalade dans l'historique", jamais "connecte" ni
    "deconnecte" -- ce ne sont pas des etats que ce dashboard peut observer.

JARVIS (interface vocale, voir index.html section "=== JARVIS ===") :
  - La reconnaissance et la synthese vocales tournent ENTIEREMENT cote
    navigateur (Web Speech API : SpeechRecognition / SpeechSynthesis) --
    aucun endpoit HTTP dedie ici, ce fichier ne recoit et n'envoie jamais
    d'audio. Jarvis reutilise tel quel /api/generate-relation (transcription
    -> topic) puis /api/send (meme flux que le compositeur manuel).
  - Le seul changement serveur pour Jarvis est ADDITIF : /api/send retourne
    desormais aussi "human_summary" (voir summarize_cstl_response_for_speech
    ci-dessous), une phrase francaise courte extraite par regex de
    raw_response, pensee pour etre lue a voix haute -- jamais le wire-format
    brut. Toutes les cles deja existantes de la reponse /api/send restent
    inchangees ; le compositeur manuel les ignore et continue de marcher.
"""

import json
import os
import re
import socket
import sqlite3
import subprocess
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

REPO_ROOT = Path(__file__).resolve().parent.parent

# sdk/python/cstl_llm_agent.py expose deja resolve_brain() (Hermes/Ollama ->
# Gemini -> Anthropic, degradation propre a None si aucun n'est disponible)
# -- reutilise tel quel ici, jamais duplique. Import a l'interieur d'un
# try/except: ce module importe lui-meme cstl_client.py (stdlib pur, ok) et
# tente d'importer `cryptography` en haut de fichier (deja gere en interne
# par un try/except la-bas) -- donc cet import ne devrait jamais echouer
# dans un environnement Python standard, mais si sdk/python/ est deplace ou
# absent, ce dashboard degrade proprement plutot que de planter au demarrage.
sys.path.insert(0, str(REPO_ROOT / "sdk" / "python"))
try:
    from cstl_llm_agent import resolve_brain  # noqa: E402
    _LLM_AGENT_IMPORT_ERROR = None
except Exception as e:  # pragma: no cover - filet honnete, pas un cas attendu
    resolve_brain = None
    _LLM_AGENT_IMPORT_ERROR = str(e)

# === ORCHESTRATEUR ===
# Meme patron d'import honnete que ci-dessus, pour le relais multi-tours reel
# (voir run_orchestrated_relay() plus bas). Reutilise TEL QUEL :
#   - resolve_brain (deja importe ci-dessus, meme fonction)
#   - load_or_create_keypair / register_agent / send_signed_relation / sign_intent
#     de sdk/python/cstl_llm_agent.py (identite Ed25519, enregistrement
#     dynamique aupres du serveur, signature -- AUCUNE de ces logiques n'est
#     redecrite ici)
#   - CstlClient de sdk/python/cstl_client.py (connexion TCP reelle vers le
#     serveur CSTL, construction/serialisation du payload wire-format)
# Import separe (pas dans le try/except ci-dessus) uniquement pour un message
# d'erreur distinct si jamais cette partie manque alors que resolve_brain
# seul suffirait encore a /api/generate-relation existant.
try:
    from cstl_llm_agent import load_or_create_keypair, register_agent, send_signed_relation, sign_intent
    from cstl_client import CstlClient
    _ORCHESTRATOR_IMPORT_ERROR = None
except Exception as e:  # pragma: no cover - filet honnete, pas un cas attendu
    load_or_create_keypair = None
    register_agent = None
    send_signed_relation = None
    sign_intent = None
    CstlClient = None
    _ORCHESTRATOR_IMPORT_ERROR = str(e)

DASHBOARD_PORT = int(os.environ.get("CSTL_DASHBOARD_PORT", "5099"))
CSTL_SERVER_HOST = os.environ.get("CSTL_SERVER_HOST", "127.0.0.1")
CSTL_SERVER_PORT = int(os.environ.get("CSTL_SERVER_PORT", "5050"))
ADN_DB_PATH = Path(os.environ.get("CSTL_ADN_DB_PATH", str(REPO_ROOT / "cstl_adn.db")))
GRAPHIFY_OUT = Path(os.environ.get("CSTL_GRAPHIFY_OUT", str(REPO_ROOT / "graphify-out")))
ORCHESTRATOR_KEYFILE = Path(os.environ.get("CSTL_ORCHESTRATOR_KEYFILE",
                                            str(Path.home() / ".cstl" / "dashboard_orchestrator_ed25519.key")))

TCP_TIMEOUT_S = 5.0
STATUS_TIMEOUT_S = 1.0
END_MARKER = b"---END---"


def check_server_status():
    """Vraie tentative de connexion TCP a 127.0.0.1:5050 (timeout court).
    Retourne un dict honnete: jamais un statut fixe."""
    started = time.monotonic()
    try:
        with socket.create_connection((CSTL_SERVER_HOST, CSTL_SERVER_PORT), timeout=STATUS_TIMEOUT_S):
            elapsed_ms = round((time.monotonic() - started) * 1000, 1)
            return {"up": True, "host": CSTL_SERVER_HOST, "port": CSTL_SERVER_PORT,
                    "latency_ms": elapsed_ms, "checked_at": int(time.time())}
    except OSError as e:
        elapsed_ms = round((time.monotonic() - started) * 1000, 1)
        return {"up": False, "host": CSTL_SERVER_HOST, "port": CSTL_SERVER_PORT,
                "error": str(e), "latency_ms": elapsed_ms, "checked_at": int(time.time())}


def read_adn_state():
    """Lecture directe et en lecture seule du vrai fichier SQLite pendant
    que le serveur Rust tourne. Aucune donnee inventee : si le fichier
    n'existe pas encore (serveur jamais lance), on le dit explicitement."""
    if not ADN_DB_PATH.exists():
        return {"ok": False, "reason": "db_not_found", "path": str(ADN_DB_PATH)}

    uri = f"file:{ADN_DB_PATH.as_posix()}?mode=ro"
    try:
        conn = sqlite3.connect(uri, uri=True, timeout=2.0)
        conn.row_factory = sqlite3.Row
    except sqlite3.OperationalError as e:
        return {"ok": False, "reason": "open_failed", "detail": str(e), "path": str(ADN_DB_PATH)}

    result = {"ok": True, "path": str(ADN_DB_PATH)}
    try:
        cur = conn.cursor()

        cur.execute("SELECT COUNT(*) AS total, "
                    "SUM(CASE WHEN committed=1 THEN 1 ELSE 0 END) AS committed "
                    "FROM adn_store")
        row = cur.fetchone()
        total = row["total"] or 0
        committed = row["committed"] or 0
        result["stats"] = {"total": total, "committed": committed, "pending": total - committed}

        cur.execute("SELECT hash, sigma, committed, created_at, produced_by "
                    "FROM adn_store ORDER BY created_at DESC LIMIT 20")
        result["recent_entries"] = [dict(r) for r in cur.fetchall()]

        cur.execute("SELECT COUNT(*) AS n FROM audit_trail")
        result["audit_count"] = cur.fetchone()["n"]

        # governance_events / governance_alerts : tables optionnelles selon
        # la version du schema -- verifie leur existence reelle avant de lire,
        # jamais un bloc vide fabrique.
        cur.execute("SELECT name FROM sqlite_master WHERE type='table' "
                    "AND name IN ('governance_events','governance_alerts')")
        existing_tables = {r["name"] for r in cur.fetchall()}

        if "governance_events" in existing_tables:
            cur.execute("SELECT sender, ts, inconsistency, semantic_warning "
                        "FROM governance_events ORDER BY ts DESC LIMIT 20")
            result["governance_events"] = [dict(r) for r in cur.fetchall()]
        else:
            result["governance_events"] = None

        if "governance_alerts" in existing_tables:
            cur.execute("SELECT sender, last_alert_ts FROM governance_alerts")
            result["governance_alerts"] = [dict(r) for r in cur.fetchall()]
        else:
            result["governance_alerts"] = None

    except sqlite3.OperationalError as e:
        result["ok"] = False
        result["reason"] = "query_failed"
        result["detail"] = str(e)
    finally:
        conn.close()
    return result


def build_cstl_payload(fields):
    """Construit un payload CSTL v5.0.0 valide a partir des champs fournis
    par l'utilisateur. Aucun champ invente : purpose/sender/receiver
    viennent tels quels du formulaire ; les relations (liste de
    dict subject/predicate/object) sont serialisees une par ligne."""
    purpose = fields.get("purpose", "dashboard_test").strip() or "dashboard_test"
    sender = fields.get("sender", "dashboard_user").strip() or "dashboard_user"
    receiver = fields.get("receiver", "server").strip() or "server"
    relations = fields.get("relations", [])

    # META [encoder=..., produced_by=...] est obligatoire cote serveur
    # (validator.rs E301/E302) -- constate en direct lors de la premiere
    # verification de ce dashboard (reponse purpose=validation_error).
    lines = ["#!CSTL v5.0.0 MODE=A",
             "META [encoder=CstlDashboard, produced_by=dashboard]",
             f"INTENT_PAYLOAD [purpose={purpose}, sender={sender}, receiver={receiver}]"]
    for rel in relations:
        subj = rel.get("subject", "").strip()
        pred = rel.get("predicate", "").strip()
        obj = rel.get("object", "").strip()
        if subj and pred and obj:
            lines.append(f"RELATION [type={pred}, subject={subj}, object={obj}]")
    lines.append("---END---")
    return "\n".join(lines) + "\n"


def send_cstl_payload(payload_text):
    """Ouvre une vraie connexion TCP, envoie le payload, lit jusqu'a
    ---END--- (ou fermeture/timeout), retourne la reponse brute + metadata
    honnete sur ce qui s'est reellement passe."""
    started = time.monotonic()
    try:
        with socket.create_connection((CSTL_SERVER_HOST, CSTL_SERVER_PORT), timeout=TCP_TIMEOUT_S) as sock:
            sock.sendall(payload_text.encode("utf-8"))
            sock.settimeout(TCP_TIMEOUT_S)
            buf = b""
            while END_MARKER not in buf:
                try:
                    chunk = sock.recv(4096)
                except socket.timeout:
                    break
                if not chunk:
                    break
                buf += chunk
            elapsed_ms = round((time.monotonic() - started) * 1000, 1)
            return {
                "ok": True,
                "sent": payload_text,
                "raw_response": buf.decode("utf-8", errors="replace"),
                "bytes_received": len(buf),
                "round_trip_ms": elapsed_ms,
            }
    except OSError as e:
        elapsed_ms = round((time.monotonic() - started) * 1000, 1)
        return {"ok": False, "sent": payload_text, "error": str(e), "round_trip_ms": elapsed_ms}


def check_telegram_obsidian_status():
    """Statut REEL (autant que possible depuis ce process) de Telegram et
    Obsidian -- remplace la ligne statique "cable cote serveur, pas visible
    ici". Voir le commentaire du module (en-tete du fichier) pour la
    justification complete de chaque proxy utilise ici.

    Ne retourne JAMAIS "connecte"/"deconnecte" pour Telegram/Obsidian: ce ne
    sont pas des etats que ce process peut observer directement (process
    Rust separe). Retourne a la place des preuves indirectes datees, ou dit
    explicitement qu'aucune n'existe."""
    result = {
        "limite_architecturale": (
            "ce dashboard tourne dans un PROCESS PYTHON SEPARE du serveur Rust "
            "(cargo run) -- il ne peut lire QUE ses propres variables "
            "d'environnement (os.environ de CE process), jamais celles du "
            "process Rust voisin (deux process = deux espaces memoire distincts). "
            "'dashboard_process_env' ci-dessous ne prouve donc RIEN sur la "
            "configuration reelle du serveur Rust, et son absence ici ne prouve "
            "pas non plus que le serveur Rust en soit prive."
        ),
        "dashboard_process_env": {
            "TELEGRAM_BOT_TOKEN": bool(os.environ.get("TELEGRAM_BOT_TOKEN")),
            "TELEGRAM_CHAT_ID": bool(os.environ.get("TELEGRAM_CHAT_ID")),
            "OBSIDIAN_VAULT_PATH": bool(os.environ.get("OBSIDIAN_VAULT_PATH")),
        },
    }

    if not ADN_DB_PATH.exists():
        no_db = {"evidence": "base_absente", "detail": f"{ADN_DB_PATH} n'existe pas -- serveur Rust jamais lance ici, ou base ailleurs."}
        result["telegram"] = no_db
        result["obsidian"] = no_db
        return result

    uri = f"file:{ADN_DB_PATH.as_posix()}?mode=ro"
    try:
        conn = sqlite3.connect(uri, uri=True, timeout=2.0)
        conn.row_factory = sqlite3.Row
    except sqlite3.OperationalError as e:
        err = {"evidence": "lecture_impossible", "detail": str(e)}
        result["telegram"] = err
        result["obsidian"] = err
        return result

    try:
        cur = conn.cursor()

        # Preuve n1 (Telegram, priority=critical) : handler.rs STEP 3-priority
        # n'ecrit RIEN dans une table dediee -- le seul effet cote donnees est
        # que le payload texte complet (contenant "priority=critical") est
        # stocke tel quel dans adn_store.payload par le put() normal, qui a
        # lieu independamment de la priorite. Ce n'est PAS une preuve que
        # Telegram a recu le message (ca depend de TELEGRAM_BOT_TOKEN/CHAT_ID
        # cote process Rust, invisible d'ici) -- seulement que le SERVEUR a
        # traite une demande d'escalade immediate.
        cur.execute(
            "SELECT hash, created_at FROM adn_store WHERE payload LIKE '%priority=critical%' "
            "ORDER BY created_at DESC LIMIT 5"
        )
        critical_payloads = [dict(r) for r in cur.fetchall()]

        cur.execute(
            "SELECT name FROM sqlite_master WHERE type='table' "
            "AND name IN ('governance_events','governance_alerts')"
        )
        existing_tables = {r["name"] for r in cur.fetchall()}

        # Preuve n2 (Telegram, circuit-breaker de gouvernance) : mecanisme
        # DIFFERENT de priority=critical (STEP 3c-governance, pas STEP
        # 3-priority) mais qui, lui, persiste reellement dans
        # governance_alerts -- preuve indirecte plus solide (table dediee),
        # meme politique d'envoi Telegram fire-and-forget en arriere-plan.
        governance_alerts = []
        if "governance_alerts" in existing_tables:
            cur.execute("SELECT sender, last_alert_ts FROM governance_alerts ORDER BY last_alert_ts DESC")
            governance_alerts = [dict(r) for r in cur.fetchall()]

        # Preuve (Obsidian) : STEP 3c-bis ecrit dans un fichier markdown du
        # vault (jamais dans SQLite) -- ce dashboard ne peut PAS lire ce
        # fichier sans connaitre OBSIDIAN_VAULT_PATH cote process Rust
        # (invisible ici). Proxy retenu : governance_events.inconsistency=1,
        # car l'escalade Obsidian se declenche EXACTEMENT sur la meme
        # condition (!consistency.consistent) que celle qui alimente ce
        # champ -- proxy honnete, pas une lecture directe du vault.
        inconsistency_events = []
        if "governance_events" in existing_tables:
            cur.execute(
                "SELECT sender, ts FROM governance_events WHERE inconsistency=1 "
                "ORDER BY ts DESC LIMIT 5"
            )
            inconsistency_events = [dict(r) for r in cur.fetchall()]
    except sqlite3.OperationalError as e:
        conn.close()
        err = {"evidence": "requete_echouee", "detail": str(e)}
        result["telegram"] = err
        result["obsidian"] = err
        return result
    conn.close()

    if critical_payloads or governance_alerts:
        result["telegram"] = {
            "evidence": "escalade_detectee_indirecte",
            "priority_critical_payloads_recents": critical_payloads,
            "governance_alerts_recents": governance_alerts,
            "detail": (
                "au moins un signal indirect trouve dans l'ADN store. "
                "priority_critical_payloads_recents = payloads recus par le "
                "serveur avec priority=critical (preuve que le SERVEUR a traite "
                "la demande, PAS que Telegram l'a recue -- aucune table ne "
                "persiste le resultat de cet envoi fire-and-forget). "
                "governance_alerts_recents = alertes du circuit-breaker de "
                "gouvernance (mecanisme different, mais meme canal Telegram, "
                "et celui-ci EST persiste)."
            ),
        }
    else:
        result["telegram"] = {
            "evidence": "aucune_escalade_dans_historique",
            "detail": (
                "aucun payload priority=critical et aucune ligne dans "
                "governance_alerts dans cet ADN store -- soit aucune escalade "
                "n'a ete demandee, soit la base a ete videe/recreee depuis."
            ),
        }

    if inconsistency_events:
        result["obsidian"] = {
            "evidence": "proxy_inconsistance_detectee",
            "governance_events_inconsistency_recents": inconsistency_events,
            "detail": (
                "proxy indirect (governance_events.inconsistency=1) -- ce dashboard "
                "ne lit jamais le vault Obsidian lui-meme (chemin inconnu, "
                "process Rust separe). Une inconsistance detectee ici correspond "
                "exactement a la condition qui declenche ObsidianEscalation::escalate() "
                "cote serveur, mais ne prouve pas que le fichier a reellement ete "
                "ecrit (ca depend de OBSIDIAN_VAULT_PATH cote Rust, invisible ici)."
            ),
        }
    else:
        result["obsidian"] = {
            "evidence": "aucune_inconsistance_dans_historique",
            "detail": "aucune ligne governance_events.inconsistency=1 dans cet ADN store.",
        }

    return result


# === ORCHESTRATEUR ===
# Relais reel de N tours entre deux "positions" d'un meme agent LLM contre le
# vrai serveur CSTL (port 5050) -- reutilise EXCLUSIVEMENT les briques deja
# ecrites et testees de sdk/python/cstl_llm_agent.py et cstl_client.py
# (resolve_brain, load_or_create_keypair, register_agent, send_signed_relation,
# sign_intent, CstlClient) : rien de cryptographique, rien de protocolaire
# n'est reimplemente ici. La seule chose ajoutee est un petit assemblage
# ("glue") qui capture, pour chaque tour, le texte du payload REELLEMENT
# envoye ET la reponse REELLEMENT recue -- necessaire parce que
# send_signed_relation() (voir cstl_llm_agent.py) ne retourne QUE la reponse
# parsee, jamais le payload qu'elle a construit et envoye. Ce petit assemblage
# appelle les MEMES fonctions (sign_intent, client.build_payload,
# client.send_raw) avec les MEMES arguments que send_signed_relation --
# aucune signature, canonicalisation ou regle de wire-format n'est
# redecrite : seule la capture du texte intermediaire differe.
def _send_signed_relation_with_payload(client, sk, pubkey_hex, *, name, receiver,
                                        purpose, relations, extra_intent=None):
    meta = {"encoder": "LlmAgent", "produced_by": name, "public_key": pubkey_hex}
    intent = {"purpose": purpose, "sender": name, "receiver": receiver}
    if extra_intent:
        intent.update(extra_intent)
    sig_hex = sign_intent(sk, pubkey_hex, version="v5.0.0", mode="A",
                           meta=meta, intent=intent, relations=relations)
    intent["signature"] = sig_hex
    payload_text = client.build_payload(
        encoder=meta["encoder"], produced_by=meta["produced_by"], purpose=purpose,
        sender=name, receiver=receiver, relations=relations,
        extra_meta={"public_key": pubkey_hex},
        extra_intent={k: v for k, v in intent.items() if k not in ("purpose", "sender", "receiver")},
    )
    resp = client.send_raw(payload_text)
    return payload_text, resp


def run_orchestrated_relay(turns, topic, brain=None):
    """Lance un relais reel de `turns` tours contre le vrai serveur CSTL
    (CSTL_SERVER_HOST:CSTL_SERVER_PORT). A chaque tour : le brain genere une
    relation (resolve_brain("auto") en production -- Hermes local > Gemini >
    Anthropic), la relation est signee Ed25519 et envoyee au VRAI serveur par
    une VRAIE connexion TCP, la VRAIE reponse est capturee.

    `brain` est injectable : None (defaut, chemin de production) declenche un
    VRAI resolve_brain("auto") -- si aucun brain reel n'est disponible (cas de
    ce sandbox), la fonction retourne honnetement une erreur plutot que
    d'inventer un relais. Passer un objet brain factice (duck-typing
    generate_relation(topic, peer_message)->dict, comme AnthropicAgentBrain/
    GeminiAgentBrain/HermesAgentBrain) permet de prouver que la MECANIQUE
    (boucle de tours, enregistrement reel, envois TCP reels, capture de
    trace) fonctionne independamment de la disponibilite d'un vrai modele --
    voir dashboard/test_orchestrator_stub.py.

    Retourne toujours un dict JSON-serialisable, jamais une exception non
    geree."""
    if None in (load_or_create_keypair, register_agent, send_signed_relation, sign_intent, CstlClient):
        return {"ok": False, "error": f"import de sdk/python impossible ({_ORCHESTRATOR_IMPORT_ERROR}) -- orchestration indisponible."}

    topic = (topic or "").strip()
    if not topic:
        return {"ok": False, "error": "sujet vide -- rien a faire discuter aux agents."}

    try:
        turns = int(turns)
    except (TypeError, ValueError):
        return {"ok": False, "error": f"turns invalide (attendu un entier): {turns!r}"}
    if turns < 1:
        return {"ok": False, "error": f"turns doit etre >= 1 (recu: {turns})"}
    if turns > 20:
        return {"ok": False, "error": f"turns limite a 20 par appel pour ce dashboard v1 (recu: {turns})"}

    real_brain = brain is None
    if real_brain:
        if resolve_brain is None:
            return {"ok": False, "error": f"import de resolve_brain impossible ({_LLM_AGENT_IMPORT_ERROR})."}
        try:
            brain = resolve_brain("auto")
        except Exception as e:
            return {"ok": False, "error": f"resolve_brain('auto') a leve une exception: {e}"}
        if brain is None:
            return {
                "ok": False,
                "error": (
                    "aucun agent LLM disponible pour orchestrer un relais (ni Hermes/Ollama "
                    "local sur 127.0.0.1:11434, ni GEMINI_API_KEY, ni ANTHROPIC_API_KEY dans "
                    "l'environnement de CE process dashboard) -- voir stderr du dashboard pour "
                    "le detail de chaque tentative. Aucun texte n'est invente a la place."
                ),
            }

    try:
        sk, pubkey_hex = load_or_create_keypair(ORCHESTRATOR_KEYFILE)
    except Exception as e:
        return {"ok": False, "error": f"generation/chargement de la cle Ed25519 impossible: {e}"}

    agent_name = "dashboard_orchestrator"
    client = CstlClient(host=CSTL_SERVER_HOST, port=CSTL_SERVER_PORT, timeout=15.0)
    trace = []
    try:
        try:
            reg = register_agent(client, sk, pubkey_hex, agent_name)
        except OSError as e:
            return {"ok": False, "error": f"connexion TCP au serveur CSTL ({CSTL_SERVER_HOST}:{CSTL_SERVER_PORT}) echouee: {e}", "trace": trace}
        trace.append({
            "turn": 0, "kind": "agent_register",
            "response_purpose": reg.purpose, "response_fields": reg.fields,
            "audit_hash": reg.audit_hash, "raw_response": reg.raw,
        })
        if reg.purpose != "agent_register_ack":
            return {"ok": False, "error": f"enregistrement de l'agent refuse par le serveur (purpose={reg.purpose!r})", "trace": trace}

        peer_message = None
        for turn in range(1, turns + 1):
            try:
                relation = brain.generate_relation(topic, peer_message)
            except Exception as e:
                trace.append({"turn": turn, "kind": "generation_error",
                               "error": f"{type(brain).__name__}.generate_relation a leve une exception: {e}"})
                return {"ok": False, "error": "generation LLM echouee en cours de relais -- voir trace.", "trace": trace}

            try:
                payload_text, resp = _send_signed_relation_with_payload(
                    client, sk, pubkey_hex, name=agent_name, receiver="server",
                    purpose="llm_relay_turn", relations=[relation],
                )
            except OSError as e:
                trace.append({"turn": turn, "kind": "tcp_error", "relation_generated": relation, "error": str(e)})
                return {"ok": False, "error": f"envoi TCP au serveur CSTL echoue au tour {turn}: {e}", "trace": trace}

            trace.append({
                "turn": turn,
                "kind": "relay_turn",
                "brain": type(brain).__name__,
                "model": getattr(brain, "model", None),
                "relation_generated": relation,
                "payload_sent": payload_text,
                "response_purpose": resp.purpose,
                "response_status": resp.status,
                "audit_hash": resp.audit_hash,
                "raw_response": resp.raw,
            })
            peer_message = json.dumps(relation, ensure_ascii=False)
    finally:
        client.close()

    return {
        "ok": True,
        "brain": type(brain).__name__,
        "model": getattr(brain, "model", None),
        "real_brain": real_brain,
        "turns_completed": turns,
        "trace": trace,
    }


# === JARVIS === (voir aussi index.html, section "=== JARVIS ===")
# Ajout ADDITIF pour l'interface vocale Jarvis : construit un resume court,
# lisible A VOIX HAUTE (via SpeechSynthesis cote navigateur), a partir des
# CHAMPS DEJA PRESENTS dans la reponse de /api/send -- jamais un texte
# invente, jamais le wire-format brut lu mot a mot (illisible a l'oral).
# N'AJOUTE qu'une cle "human_summary" au dict retourne par send_cstl_payload()
# (voir do_POST /api/send ci-dessous) -- ne modifie ni ne supprime aucune cle
# existante, donc le compositeur manuel (qui ignore ce champ) continue de
# fonctionner exactement comme avant.
_RE_CONSISTENCY = re.compile(
    r"CONSISTENCY \[consistent=(?P<consistent>true|false), "
    r"contradictions=(?P<contradictions>\d+), cycles=(?P<cycles>\d+), sigma=(?P<sigma>[0-9.]+)\]"
)
_RE_PRIORITY = re.compile(r"PRIORITY \[value=(?P<value>[a-z]+), escalated=(?P<escalated>true|false)\]")
_RE_VALIDATION_ERROR = re.compile(r"purpose=validation_error, errors=(?P<errors>.*)\]")
_RE_ACK = re.compile(r"INTENT_PAYLOAD \[purpose=acknowledgement")


def summarize_cstl_response_for_speech(send_result):
    """Construit une phrase francaise courte a partir des champs deja
    extraits par regex de raw_response (jamais de champ fabrique). Retourne
    None si rien d'exploitable n'a ete trouve (ex. reponse vide, timeout) --
    le cote client doit alors se rabattre sur un message generique plutot que
    d'inventer un contenu."""
    if not send_result.get("ok"):
        return f"Echec de connexion au serveur CSTL : {send_result.get('error', 'erreur inconnue')}."

    raw = send_result.get("raw_response") or ""

    err_match = _RE_VALIDATION_ERROR.search(raw)
    if err_match:
        return f"Le serveur a rejete la relation. Erreur de validation : {err_match.group('errors')}."

    if not _RE_ACK.search(raw):
        return "Reponse recue mais non reconnue -- consulte la reponse brute a l'ecran."

    parts = ["Le serveur a accepte la relation."]
    cons_match = _RE_CONSISTENCY.search(raw)
    if cons_match:
        sigma = cons_match.group("sigma")
        consistent = cons_match.group("consistent") == "true"
        contradictions = cons_match.group("contradictions")
        parts.append(f"Coherence sigma {sigma}" + (", sans contradiction" if contradictions == "0"
                     else f", {contradictions} contradiction(s) detectee(s)") + ".")
        if not consistent:
            parts.append("Attention, le graphe est marque incoherent.")
    prio_match = _RE_PRIORITY.search(raw)
    if prio_match:
        value = prio_match.group("value")
        escalated = prio_match.group("escalated") == "true"
        parts.append(f"Priorite {value}" + (", escalade declenchee." if escalated else "."))
    return " ".join(parts)


def generate_relation_via_llm(topic):
    """Essaie dans l'ordre Hermes/Ollama (local, gratuit) -> Gemini ->
    Anthropic via resolve_brain("auto") de sdk/python/cstl_llm_agent.py --
    AUCUNE logique de generation dupliquee ici, seulement l'appel et la mise
    en forme honnete du resultat pour le compositeur du dashboard.

    Retourne un dict JSON-serialisable, jamais une exception non geree:
    {"ok": False, "brain": None, "message": "..."} si aucun brain n'est
    disponible (le cas normal dans ce sandbox), ou
    {"ok": True, "brain": "HermesAgentBrain", "model": "...",
     "relation": {"type":..., "subject":..., "object":...}} en cas de
    succes reel."""
    if resolve_brain is None:
        return {
            "ok": False, "brain": None,
            "message": ("import de sdk/python/cstl_llm_agent.py impossible "
                        f"({_LLM_AGENT_IMPORT_ERROR}) -- generation LLM indisponible."),
        }

    topic = (topic or "").strip()
    if not topic:
        return {"ok": False, "brain": None, "message": "sujet vide -- rien a demander a un agent LLM."}

    try:
        brain = resolve_brain("auto")
    except Exception as e:
        return {"ok": False, "brain": None, "message": f"resolve_brain('auto') a leve une exception: {e}"}

    if brain is None:
        return {
            "ok": False, "brain": None,
            "message": ("aucun agent LLM disponible (ni Hermes local, ni cle "
                        "Gemini/Anthropic) -- voir stderr du process dashboard "
                        "pour le detail de chaque tentative (Hermes: serveur "
                        "Ollama sur 127.0.0.1:11434 ? Gemini: GEMINI_API_KEY ? "
                        "Anthropic: ANTHROPIC_API_KEY ?)."),
        }

    brain_name = type(brain).__name__
    model = getattr(brain, "model", None)
    try:
        relation = brain.generate_relation(topic, None)
    except Exception as e:
        return {
            "ok": False, "brain": brain_name, "model": model,
            "message": f"{brain_name} disponible mais la generation a echoue reellement: {e}",
        }

    if not isinstance(relation, dict) or not all(k in relation for k in ("type", "subject", "object")):
        return {
            "ok": False, "brain": brain_name, "model": model,
            "message": f"reponse du modele mal formee (attendu type/subject/object): {relation!r}",
        }

    return {"ok": True, "brain": brain_name, "model": model, "relation": relation}


def read_graphify_summary():
    """Lit le VRAI fichier graphify-out/graph.json s'il existe (le format
    reellement produit par cet outil -- pas de fichier "topology.json"
    trouve dans ce depot) et retourne un resume honnete: nombre de noeuds,
    nombre de communautes distinctes, commit sur lequel le graphe a ete
    construit. Aucun nombre invente: si le fichier est absent, le dit
    explicitement ; si le commit d'origine (`built_at_commit`) n'est pas un
    ancetre de HEAD, le signale comme possiblement perime plutot que de
    pretendre que le graphe est a jour."""
    graph_file = GRAPHIFY_OUT / "graph.json"
    manifest_file = GRAPHIFY_OUT / "manifest.json"

    if not graph_file.exists():
        return {
            "ok": False, "reason": "non_genere",
            "detail": f"{graph_file} absent -- regenere avec `graphify update .`",
        }

    try:
        raw = graph_file.read_text(encoding="utf-8")
        data = json.loads(raw)
    except (OSError, json.JSONDecodeError) as e:
        return {"ok": False, "reason": "lecture_impossible", "detail": str(e), "path": str(graph_file)}

    nodes = data.get("nodes", [])
    links = data.get("links", [])
    communities = {n.get("community") for n in nodes if isinstance(n, dict) and "community" in n}
    built_at_commit = data.get("built_at_commit")

    result = {
        "ok": True,
        "path": str(graph_file),
        "node_count": len(nodes),
        "link_count": len(links),
        "community_count": len(communities),
        "built_at_commit": built_at_commit,
        "generated_at": None,
    }

    # Date de generation: prise du mtime reel du fichier sur disque (le
    # format graph.json lui-meme ne porte pas de champ de date explicite,
    # verifie sur cette instance) -- jamais fabriquee.
    try:
        result["generated_at"] = int(graph_file.stat().st_mtime)
    except OSError:
        pass

    # Perime ? Comparaison SIMPLE et honnete: built_at_commit est-il un
    # ancetre de HEAD dans CE depot git ? Si `git` ou le repo sont
    # indisponibles, on le dit plutot que de deviner.
    result["head_commit"] = None
    result["stale"] = None
    result["stale_reason"] = None
    try:
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=str(REPO_ROOT),
            capture_output=True, text=True, timeout=5,
        )
        if head.returncode == 0:
            head_commit = head.stdout.strip()
            result["head_commit"] = head_commit
            if not built_at_commit:
                result["stale"] = None
                result["stale_reason"] = "graph.json ne porte pas de built_at_commit -- impossible de comparer."
            elif built_at_commit == head_commit:
                result["stale"] = False
                result["stale_reason"] = "built_at_commit == HEAD exactement."
            else:
                anc = subprocess.run(
                    ["git", "merge-base", "--is-ancestor", built_at_commit, "HEAD"],
                    cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=5,
                )
                if anc.returncode == 0:
                    result["stale"] = True
                    result["stale_reason"] = ("built_at_commit est un ancetre de HEAD mais pas HEAD lui-meme -- "
                                               "des commits sont arrives depuis la generation du graphe.")
                else:
                    result["stale"] = True
                    result["stale_reason"] = (f"built_at_commit ({built_at_commit[:12]}...) n'est PAS un ancetre "
                                               "de HEAD dans l'historique courant -- le graphe a ete construit sur "
                                               "une autre branche/ligne d'historique, possiblement perime.")
        else:
            result["stale_reason"] = f"`git rev-parse HEAD` a echoue: {head.stderr.strip()}"
    except (OSError, subprocess.SubprocessError) as e:
        result["stale_reason"] = f"comparaison git impossible: {e}"

    return result


def check_openclaw_connection():
    """Test de connectivite GENERIQUE vers ws://127.0.0.1:19001 -- PAS le
    protocole applicatif d'OpenClaw (aucune documentation de ce protocole
    n'est disponible pour cette tache, donc aucun format de message n'est
    invente ici). Deux niveaux, selon ce qui est disponible:
      1. Si le paquet `websockets` est installe: tente une vraie poignee de
         main WebSocket (upgrade HTTP compris) -- plus proche de ce
         qu'OpenClaw attend reellement s'il ecoute en WebSocket.
      2. Sinon: simple connexion TCP brute sur le port -- suffisant pour
         savoir si quelque chose ecoute la, pas pour parler le protocole.
    Dans CE sandbox, OpenClaw tourne (s'il tourne) sur la machine macOS de
    l'utilisateur, pas ici -- "inaccessible depuis ici" est donc le resultat
    ATTENDU et correct, pas un signe de bug."""
    host, port = "127.0.0.1", 19001
    started = time.monotonic()

    try:
        import websockets.sync.client as ws_sync  # type: ignore
        try:
            conn = ws_sync.connect(f"ws://{host}:{port}/", open_timeout=2.0, close_timeout=1.0)
            conn.close()
            elapsed_ms = round((time.monotonic() - started) * 1000, 1)
            return {
                "reachable": True, "method": "websocket_handshake",
                "host": host, "port": port, "latency_ms": elapsed_ms,
                "detail": "poignee de main WebSocket reussie sur ce port.",
            }
        except Exception as e:
            elapsed_ms = round((time.monotonic() - started) * 1000, 1)
            return {
                "reachable": False, "method": "websocket_handshake",
                "host": host, "port": port, "latency_ms": elapsed_ms,
                "detail": (f"echec de la poignee de main WebSocket ({e}) -- attendu dans ce sandbox, "
                           "OpenClaw tourne sur la machine de l'utilisateur, pas ici."),
            }
    except ImportError:
        pass

    try:
        with socket.create_connection((host, port), timeout=2.0):
            elapsed_ms = round((time.monotonic() - started) * 1000, 1)
            return {
                "reachable": True, "method": "tcp_raw",
                "host": host, "port": port, "latency_ms": elapsed_ms,
                "detail": ("port TCP ouvert (paquet 'websockets' absent, test TCP brut seulement -- "
                           "ne prouve pas qu'un serveur WebSocket ou OpenClaw ecoute reellement)."),
            }
    except OSError as e:
        elapsed_ms = round((time.monotonic() - started) * 1000, 1)
        return {
            "reachable": False, "method": "tcp_raw",
            "host": host, "port": port, "latency_ms": elapsed_ms,
            "detail": (f"connexion TCP echouee ({e}) -- attendu dans ce sandbox, OpenClaw tourne "
                       "sur la machine de l'utilisateur, pas ici."),
        }


def check_other_systems():
    """Statuts honnetes des "autres systemes" -- jamais une case verte
    fabriquee. Chaque ligne dit precisement sur quoi son statut est base."""
    systems = []

    # Telegram / Obsidian : cables cote Rust (verifie par lecture de
    # src/telegram_council.rs et src/obsidian_escalation.rs cette session).
    # Plus une simple ligne statique -- voir check_telegram_obsidian_status()
    # et le bouton dedie "Statut Telegram / Obsidian (preuve indirecte reelle)"
    # cote index.html pour une VRAIE verification (preuve indirecte via ADN
    # store), interrogee a la demande via /api/telegram-obsidian-status.
    for name, src_file in [("Telegram", "src/telegram_council.rs"),
                            ("Obsidian", "src/obsidian_escalation.rs")]:
        wired = (REPO_ROOT / src_file).exists()
        systems.append({
            "name": name,
            "status": "cable_cote_serveur_voir_bouton_statut_dedie" if wired else "fichier_source_introuvable",
            "detail": (f"{src_file} present dans src/ -- clique 'Statut Telegram / Obsidian' "
                       "plus bas pour une preuve indirecte reelle (ADN store)." if wired
                       else f"{src_file} absent"),
        })

    # Hermes/Ollama : toujours non-integre cote serveur Rust lui-meme (grep
    # src/ ne trouve aucune reference), mais depuis cette session le
    # dashboard SAIT parler a un serveur Ollama local via HermesAgentBrain
    # (sdk/python/cstl_llm_agent.py) -- voir /api/generate-relation. Le
    # statut ci-dessous reste honnete: pas d'integration Rust, endpoint
    # dashboard present.
    systems.append({
        "name": "Hermes/Ollama",
        "status": "non_integre",
        "detail": ("aucune reference dans src/ (Rust) -- mais /api/generate-relation de CE dashboard "
                   "sait parler a un serveur Ollama local (127.0.0.1:11434) via HermesAgentBrain "
                   "s'il est installe et lance."),
    })

    # OpenClaw : voir /api/openclaw-check -- test de connectivite generique
    # uniquement (aucun protocole applicatif documente disponible).
    systems.append({
        "name": "OpenClaw",
        "status": "non_integre",
        "detail": ("aucune reference dans src/ ; /api/openclaw-check de CE dashboard teste seulement "
                   "l'ouverture du port ws://127.0.0.1:19001, sans parler le protocole applicatif "
                   "(non documente) -- clique 'Tester la connexion OpenClaw' pour un resultat en direct."),
    })

    # Graphify : plus de ligne statique ici -- remplace par un resume REEL
    # de graphify-out/graph.json, lu a la demande via /api/graphify-summary
    # (voir read_graphify_summary()) et affiche par le bouton dedie du
    # panneau "Autres systemes" cote index.html.

    # Claude Code : cette session elle-meme, honnete: pas d'etat verifiable
    # depuis un serveur externe, juste une mention.
    systems.append({
        "name": "Claude Code",
        "status": "hors_cadre_serveur",
        "detail": "outil d'edition/agent local, pas un service a interroger en TCP/HTTP",
    })

    return systems


class DashboardHandler(BaseHTTPRequestHandler):
    server_version = "CSTLDashboard/1.0"

    def log_message(self, fmt, *args):
        # Log minimal sur stderr, garde le defaut de BaseHTTPRequestHandler
        # mais evite le bruit habituel dans le cas courant.
        pass

    def _send_json(self, obj, status=200):
        body = json.dumps(obj, ensure_ascii=False, indent=2).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/" or parsed.path == "/index.html":
            self._serve_static("index.html", "text/html; charset=utf-8")
        elif parsed.path == "/api/status":
            self._send_json(check_server_status())
        elif parsed.path == "/api/adn":
            self._send_json(read_adn_state())
        elif parsed.path == "/api/other-systems":
            self._send_json(check_other_systems())
        elif parsed.path == "/api/telegram-obsidian-status":
            self._send_json(check_telegram_obsidian_status())
        elif parsed.path == "/api/graphify-summary":
            self._send_json(read_graphify_summary())
        elif parsed.path == "/api/openclaw-check":
            self._send_json(check_openclaw_connection())
        else:
            self.send_error(404, "Not found")

    def do_POST(self):
        parsed = urlparse(self.path)
        if parsed.path == "/api/send":
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length) if length else b"{}"
            try:
                fields = json.loads(raw.decode("utf-8"))
            except json.JSONDecodeError:
                self._send_json({"ok": False, "error": "corps JSON invalide"}, status=400)
                return
            payload_text = build_cstl_payload(fields)
            result = send_cstl_payload(payload_text)
            # === JARVIS === champ ADDITIF (voir summarize_cstl_response_for_speech
            # ci-dessus) -- le compositeur manuel existant ignore cette cle et
            # continue de fonctionner a l'identique.
            result["human_summary"] = summarize_cstl_response_for_speech(result)
            self._send_json(result)
        elif parsed.path == "/api/generate-relation":
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length) if length else b"{}"
            try:
                fields = json.loads(raw.decode("utf-8"))
            except json.JSONDecodeError:
                self._send_json({"ok": False, "error": "corps JSON invalide"}, status=400)
                return
            self._send_json(generate_relation_via_llm(fields.get("topic", "")))
        elif parsed.path == "/api/orchestrate":
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length) if length else b"{}"
            try:
                fields = json.loads(raw.decode("utf-8"))
            except json.JSONDecodeError:
                self._send_json({"ok": False, "error": "corps JSON invalide"}, status=400)
                return
            turns = fields.get("turns", 4)
            topic = fields.get("topic", "Est-ce que Montreal est au Canada?")
            self._send_json(run_orchestrated_relay(turns, topic))
        else:
            self.send_error(404, "Not found")

    def _serve_static(self, filename, content_type):
        path = Path(__file__).resolve().parent / filename
        try:
            body = path.read_bytes()
        except FileNotFoundError:
            self.send_error(404, "Not found")
            return
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    addr = ("127.0.0.1", DASHBOARD_PORT)
    httpd = ThreadingHTTPServer(addr, DashboardHandler)
    print(f"[dashboard] CSTL Dashboard sur http://127.0.0.1:{DASHBOARD_PORT}/")
    print(f"[dashboard] Serveur CSTL cible: {CSTL_SERVER_HOST}:{CSTL_SERVER_PORT}")
    print(f"[dashboard] ADN store lu depuis: {ADN_DB_PATH}")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\n[dashboard] Arret.")
        httpd.shutdown()


if __name__ == "__main__":
    main()
