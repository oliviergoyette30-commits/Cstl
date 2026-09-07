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
  - Telegram et Obsidian : cables cote serveur Rust (voir
    src/telegram_council.rs, src/obsidian_escalation.rs) mais ce dashboard
    ne lit aucun etat de ces integrations -- affiche comme "cable cote
    serveur, pas encore visible ici", jamais comme "connecte".

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

DASHBOARD_PORT = int(os.environ.get("CSTL_DASHBOARD_PORT", "5099"))
CSTL_SERVER_HOST = os.environ.get("CSTL_SERVER_HOST", "127.0.0.1")
CSTL_SERVER_PORT = int(os.environ.get("CSTL_SERVER_PORT", "5050"))
ADN_DB_PATH = Path(os.environ.get("CSTL_ADN_DB_PATH", str(REPO_ROOT / "cstl_adn.db")))
GRAPHIFY_OUT = Path(os.environ.get("CSTL_GRAPHIFY_OUT", str(REPO_ROOT / "graphify-out")))

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
    # src/telegram_council.rs et src/obsidian_escalation.rs cette session),
    # mais ce dashboard ne lit aucun etat en direct de ces integrations.
    for name, src_file in [("Telegram", "src/telegram_council.rs"),
                            ("Obsidian", "src/obsidian_escalation.rs")]:
        wired = (REPO_ROOT / src_file).exists()
        systems.append({
            "name": name,
            "status": "cable_cote_serveur_pas_visible_ici" if wired else "fichier_source_introuvable",
            "detail": f"{src_file} present dans src/" if wired else f"{src_file} absent",
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
