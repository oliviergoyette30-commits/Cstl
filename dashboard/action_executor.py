#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
dashboard/action_executor.py -- bras d'EXECUTION de l'OS CSTL agentic.

Contexte (voir dashboard/README.md pour le flux complet) : CSTL (le serveur
Rust, port 5050) reste la couche de DECISION/AUDIT/GOUVERNANCE -- un agent
(LLM, humain, script) propose une action via un payload
`purpose=action_request` (champs `tool=` et `command=` dans INTENT_PAYLOAD),
et CE payload doit etre COMMITTE par le RestrictedCouncil (quorum humain deja
existant, Ed25519, notifie par Telegram -- voir src/restricted_council.rs et
src/telegram_council.rs) exactement comme n'importe quelle autre entree de
l'adn_store. Ce fichier ne committe RIEN et ne vote RIEN -- il se contente de
repondre a un fait deja etabli cote serveur : "ce hash est committed=1 et son
purpose est action_request" -> executer reellement la commande, une seule
fois, et rapporter le resultat comme un nouvel evenement CSTL trace
(`purpose=action_result`).

LIMITE HONNETE, ASSUMEE DES LE DEPART (pas a decouvrir plus tard) :
  Le champ `tool=` (claude-code / openclaw / hermes / shell) est
  INFORMATIF SEULEMENT dans cette premiere passe -- il est loggue, stocke
  dans dashboard_actions.db et renvoye dans le payload action_result, mais
  TOUTE commande, quel que soit `tool`, passe par le MEME shell partage
  (`bash -lc <command>`), jamais par une session PTY interactive specifique
  a l'outil (voir dashboard/pty_bridge.py). Router reellement vers la bonne
  session PTY par outil est un vrai morceau de travail a part, non fait ici
  -- voir la section dediee de dashboard/README.md.

Base de donnees : ce module possede sa PROPRE base SQLite
(dashboard/dashboard_actions.db par defaut, voir CSTL_DASHBOARD_ACTIONS_DB)
-- separee du fichier ADN de CSTL (cstl_adn.db). Ce module n'ecrit JAMAIS
dans le fichier SQLite de CSTL: il l'ouvre uniquement en lecture seule
(`file:...?mode=ro`), exactement comme dashboard/server.py::read_adn_state().
Son unique role est d'eviter de re-executer deux fois la meme action
committee (table `executed_actions`, cle primaire = hash).
"""

import os
import sqlite3
import subprocess
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Meme convention que dashboard/server.py::ADN_DB_PATH -- delibcrement
# redefinie ici (pas importee de server.py) pour que ce module reste
# utilisable/testable seul, sans dependre de l'import de server.py (qui,
# lui, importe CE module -- voir submit_action_result() plus bas pour
# comment le cycle d'import est evite dans l'autre sens).
ADN_DB_PATH = Path(os.environ.get("CSTL_ADN_DB_PATH", str(REPO_ROOT / "cstl_adn.db")))

# Base SEPAREE, proprietaire de ce module seul -- jamais le fichier ADN de CSTL.
EXECUTED_DB_PATH = Path(os.environ.get(
    "CSTL_DASHBOARD_ACTIONS_DB", str(Path(__file__).resolve().parent / "dashboard_actions.db")
))

MAX_OUTPUT_CHARS = 4000
DEFAULT_TIMEOUT_S = 60


# ---------------------------------------------------------------------------
# Parsing d'un payload brut CSTL -- MEME convention que src/server/parser.rs
# (split_top_level_commas puis split_once('=') puis trim_matches('"')),
# reimplementee ici en Python car ce module n'a pas acces au parser Rust
# directement (le parser Rust n'est pas expose comme lib appelable depuis
# Python -- seulement via le protocole TCP). Voir src/server/parser.rs
# (fonctions split_top_level_commas / parse_block) pour la grammaire de
# reference que ce code doit imiter.
# ---------------------------------------------------------------------------

def _split_top_level_commas(content):
    """Coupe sur les virgules de premier niveau uniquement -- une virgule a
    l'interieur d'une valeur entre guillemets doubles n'est pas un
    separateur. Miroir exact de parser.rs::split_top_level_commas."""
    parts = []
    start = 0
    in_quotes = False
    for i, ch in enumerate(content):
        if ch == '"':
            in_quotes = not in_quotes
        elif ch == "," and not in_quotes:
            parts.append(content[start:i])
            start = i + 1
    parts.append(content[start:])
    return parts


def _parse_block_fields(block_text):
    """Parse le contenu entre le premier '[' et le DERNIER ']' de
    `block_text` en dict cle->valeur, comme parser.rs::parse_block.
    Retourne None (jamais une exception) si les crochets sont absents ou
    si un fragment n'a pas de '=' -- un payload malforme ne doit jamais
    faire planter la boucle appelante."""
    start = block_text.find("[")
    end = block_text.rfind("]")
    if start == -1 or end == -1 or end <= start:
        return None
    content = block_text[start + 1:end]
    fields = {}
    for pair in _split_top_level_commas(content):
        pair = pair.strip()
        if not pair:
            continue
        if "=" not in pair:
            return None
        key, _, value = pair.partition("=")
        fields[key.strip()] = value.strip().strip('"')
    return fields


def quote_intent_value(value):
    """Cite une valeur si elle contient une virgule ou un crochet fermant --
    meme regle que sdk/python/cstl_client.py::_quote_if_needed, reprise ici
    a l'identique (pas importee, pour ne pas lier ce module au SDK agent).
    Sans ca, une `command=` contenant une virgule casserait le split
    top-level cote serveur AVANT meme la validation."""
    value = str(value)
    if "," in value or "]" in value:
        escaped = value.replace('"', '\\"')
        return f'"{escaped}"'
    return value


def parse_action_request(raw_payload_text):
    """Extrait {"tool": ..., "command": ...} de la ligne INTENT_PAYLOAD d'un
    payload CSTL brut. Retourne None proprement (jamais d'exception) si la
    ligne INTENT_PAYLOAD est absente, malformee, ou si `tool`/`command`
    manquent -- un payload qui n'est structurellement pas une action ne doit
    jamais casser la boucle de poll appelante."""
    try:
        for line in (raw_payload_text or "").splitlines():
            stripped = line.strip()
            if stripped.startswith("INTENT_PAYLOAD"):
                fields = _parse_block_fields(stripped)
                if not fields:
                    return None
                tool = fields.get("tool")
                command = fields.get("command")
                if not tool or not command:
                    return None
                return {"tool": tool, "command": command}
        return None
    except Exception:
        return None


# ---------------------------------------------------------------------------
# Base locale dashboard_actions.db
# ---------------------------------------------------------------------------

def _init_executed_db(path):
    conn = sqlite3.connect(str(path))
    conn.execute(
        """CREATE TABLE IF NOT EXISTS executed_actions (
            hash TEXT PRIMARY KEY,
            tool TEXT,
            command TEXT,
            exit_code INTEGER,
            stdout TEXT,
            stderr TEXT,
            executed_at INTEGER
        )"""
    )
    conn.commit()
    return conn


def _record_executed(executed_db_path, hash_, tool, command, exit_code, stdout, stderr):
    conn = _init_executed_db(executed_db_path)
    try:
        conn.execute(
            "INSERT OR REPLACE INTO executed_actions "
            "(hash, tool, command, exit_code, stdout, stderr, executed_at) "
            "VALUES (?, ?, ?, ?, ?, ?, ?)",
            (hash_, tool, command, exit_code, stdout, stderr, int(time.time())),
        )
        conn.commit()
    finally:
        conn.close()


def find_newly_committed_actions(adn_db_path=None, executed_db_path=None):
    """Lecture SQLite READONLY du fichier ADN de CSTL: jointure
    adn_store/audit_trail sur hash, filtre committed=1 AND
    purpose='action_request', exclut les hash deja dans executed_actions.
    Retourne toujours une liste (jamais une exception) -- []
    si le fichier ADN n'existe pas encore, ne s'ouvre pas, ou si la requete
    echoue (base verrouillee par une ecriture concurrente, par exemple)."""
    adn_db_path = Path(adn_db_path) if adn_db_path else ADN_DB_PATH
    executed_db_path = Path(executed_db_path) if executed_db_path else EXECUTED_DB_PATH

    if not adn_db_path.exists():
        return []

    uri = f"file:{adn_db_path.as_posix()}?mode=ro"
    try:
        conn = sqlite3.connect(uri, uri=True, timeout=2.0)
        conn.row_factory = sqlite3.Row
    except sqlite3.OperationalError:
        return []

    try:
        cur = conn.cursor()
        cur.execute(
            "SELECT a.hash AS hash, a.payload AS payload, a.created_at AS created_at "
            "FROM adn_store a JOIN audit_trail t ON a.hash = t.hash "
            "WHERE a.committed = 1 AND t.purpose = 'action_request'"
        )
        rows = [dict(r) for r in cur.fetchall()]
    except sqlite3.OperationalError:
        return []
    finally:
        conn.close()

    already_executed = set()
    try:
        exec_conn = _init_executed_db(executed_db_path)
        try:
            already_executed = {r[0] for r in exec_conn.execute("SELECT hash FROM executed_actions").fetchall()}
        finally:
            exec_conn.close()
    except sqlite3.OperationalError:
        pass

    result = []
    for row in rows:
        if row["hash"] in already_executed:
            continue
        parsed = parse_action_request(row.get("payload") or "")
        if parsed is None:
            # Entree committee avec purpose=action_request mais tool=/command=
            # absents ou payload malforme -- ignoree silencieusement plutot
            # que de faire planter tout le cycle de poll pour les autres.
            continue
        result.append({
            "hash": row["hash"],
            "tool": parsed["tool"],
            "command": parsed["command"],
            "created_at": row.get("created_at"),
        })
    return result


# ---------------------------------------------------------------------------
# Execution reelle
# ---------------------------------------------------------------------------

def execute_action(tool, command, timeout_s=DEFAULT_TIMEOUT_S):
    """Execute REELLEMENT `command` via bash -lc, capture stdout/stderr/
    exit_code. `tool` est INFORMATIF SEULEMENT ici (voir limite honnete en
    tete de fichier) -- pas de routage vers une session PTY specifique a
    l'outil dans cette passe. Tronque stdout/stderr a MAX_OUTPUT_CHARS
    chacun. Ne leve jamais d'exception non geree -- un timeout ou une
    erreur systeme produit un dict {"ok": False, ...} exploitable."""
    started = time.time()
    try:
        proc = subprocess.run(
            ["bash", "-lc", command],
            capture_output=True, timeout=timeout_s, text=True,
        )
        return {
            "ok": True,
            "tool": tool,
            "command": command,
            "exit_code": proc.returncode,
            "stdout": (proc.stdout or "")[:MAX_OUTPUT_CHARS],
            "stderr": (proc.stderr or "")[:MAX_OUTPUT_CHARS],
            "elapsed_s": round(time.time() - started, 3),
        }
    except subprocess.TimeoutExpired as e:
        return {
            "ok": False,
            "tool": tool,
            "command": command,
            "exit_code": None,
            "stdout": (e.stdout or "")[:MAX_OUTPUT_CHARS] if e.stdout else "",
            "stderr": f"timeout apres {timeout_s}s -- processus interrompu.",
            "elapsed_s": round(time.time() - started, 3),
        }
    except OSError as e:
        return {
            "ok": False,
            "tool": tool,
            "command": command,
            "exit_code": None,
            "stdout": "",
            "stderr": f"echec de lancement du processus: {e}",
            "elapsed_s": round(time.time() - started, 3),
        }


def submit_action_result(target_hash, tool, exit_code, stdout=None, stderr=None):
    """Construit et envoie un vrai payload `purpose=action_result` (plus une
    RELATION type=EXECUTED) sur le VRAI socket TCP du serveur CSTL, via
    dashboard/server.py::send_cstl_payload -- REUTILISE, jamais duplique.
    Import fait ICI (a l'appel, pas en tete de fichier) parce que
    server.py importe ce module au niveau module -- un import en tete de
    fichier creerait un cycle d'import Python (server -> action_executor ->
    server). Ferme la boucle d'audit: l'execution elle-meme devient un
    evenement CSTL trace, avec in_reply_to=<hash de la demande d'origine>."""
    from server import send_cstl_payload  # import tardif -- casse le cycle, voir docstring

    exit_code_str = "null" if exit_code is None else str(exit_code)
    lines = [
        "#!CSTL v5.0.0 MODE=A",
        "META [encoder=CstlDashboard, produced_by=dashboard_executor]",
        (
            "INTENT_PAYLOAD [purpose=action_result, sender=dashboard_executor, "
            f"receiver=server, in_reply_to={target_hash}, tool={quote_intent_value(tool)}, "
            f"exit_code={exit_code_str}]"
        ),
        f"RELATION [type=EXECUTED, subject={quote_intent_value(tool)}, object={target_hash}]",
        "---END---",
    ]
    payload_text = "\n".join(lines) + "\n"
    return send_cstl_payload(payload_text)


def run_action_poll_cycle(adn_db_path=None, executed_db_path=None, timeout_s=DEFAULT_TIMEOUT_S):
    """Un cycle COMPLET, une seule fois (pas de boucle infinie ici -- la
    boucle/thread periodique vit dans dashboard/server.py) : trouve les
    actions fraichement committees -> les execute reellement -> enregistre
    localement (dashboard_actions.db) -> renvoie le resultat au serveur CSTL
    comme purpose=action_result. Retourne toujours un dict JSON-serialisable,
    jamais une exception non geree -- testable un cycle a la fois, comme le
    reste des fonctions de server.py."""
    try:
        newly_committed = find_newly_committed_actions(adn_db_path, executed_db_path)
    except Exception as e:  # pragma: no cover - filet honnete, pas un cas attendu
        return {"ok": False, "error": f"find_newly_committed_actions a leve une exception inattendue: {e}", "results": []}

    executed_db_path_resolved = Path(executed_db_path) if executed_db_path else EXECUTED_DB_PATH

    results = []
    for action in newly_committed:
        hash_ = action["hash"]
        tool = action["tool"]
        command = action["command"]

        exec_result = execute_action(tool, command, timeout_s=timeout_s)

        try:
            _record_executed(
                executed_db_path_resolved, hash_, tool, command,
                exec_result.get("exit_code"), exec_result.get("stdout", ""), exec_result.get("stderr", ""),
            )
            record_error = None
        except Exception as e:  # pragma: no cover - filet honnete, pas un cas attendu
            record_error = str(e)

        try:
            submit_result = submit_action_result(
                hash_, tool, exec_result.get("exit_code"),
                exec_result.get("stdout"), exec_result.get("stderr"),
            )
        except Exception as e:  # pragma: no cover - filet honnete, pas un cas attendu
            submit_result = {"ok": False, "error": f"submit_action_result a leve une exception inattendue: {e}"}

        results.append({
            "hash": hash_,
            "tool": tool,
            "command": command,
            "execution": exec_result,
            "record_error": record_error,
            "submit_result": submit_result,
        })

    return {"ok": True, "processed": len(results), "results": results}
