#!/usr/bin/env python3
"""
cstl_openclaw_bridge.py -- pont entre les invites d'approbation exec
d'OpenClaw et une couche de jugement CSTL, via un second bot Telegram
dedie (2026-09-07). Stdlib uniquement (urllib, re, json, time).

CE QUE CE PONT FAIT REELLEMENT (verifie en direct sur la machine
d'Olivier avant d'ecrire ce fichier -- voir les captures d'ecran de la
session) :

  - OpenClaw, quand `tools.exec.ask` exige une approbation, poste un
    message TEXTE (pas de bouton cache, pas de callback_data) dans le
    DM Telegram de l'owner ET, si `channels.telegram.execApprovals.
    target` vaut "both" ou "channel", dans un groupe configure. Ce
    message contient, en clair :
        "Approval required."
        "Run:" / "Pending command:" suivi de la commande shell
        "/approve <uuid> allow-once"
        "/approve <uuid> deny"
        "Full id:\n<uuid>"
  - N'importe quel expediteur reconnu comme approbateur
    (`channels.telegram.execApprovals.approvers`, qui retombe sur
    `commands.ownerAllowFrom` si vide) peut resoudre l'invite en
    repondant EXACTEMENT `/approve <uuid> allow-once|allow-always|deny`
    dans le meme chat.
  - Deux bots Telegram ne peuvent pas se DM directement -- c'est
    pourquoi ce pont exige un GROUPE contenant le bot OpenClaw
    (@CSTL1_bot) et un second bot dedie (le "bot approbateur") dont ce
    script controle le token. Voir le README / le compte-rendu de
    session pour la procedure de creation (BotFather, ajout au groupe,
    `channels.telegram.execApprovals.target=both`, ajout de l'ID
    numerique du bot approbateur a `execApprovals.approvers`).

CE QUE CE PONT NE FAIT PAS (limite v1 assumee, a documenter) :

  - AUCUNE politique deontique MUST/MUST_NOT reelle de CSTL n'existe
    aujourd'hui pour des commandes shell -- les sémantiques
    deontiques de `src/semantic.rs` portent sur des performatifs de
    message (INFORM/REQUEST/PROPOSE/...), pas sur de l'exec systeme.
    La fonction `judge_command()` ci-dessous est un PLACEHOLDER
    explicite : liste blanche minimale de commandes en lecture seule
    evidentes, refus par defaut pour tout le reste. Ce n'est PAS une
    politique CSTL verifiee -- juste un point de depart sur lequel
    brancher la vraie logique plus tard.
  - Aucune verification cryptographique de l'origine du message
    d'invite -- ce pont fait confiance a "ca vient du chat_id
    configure" pour decider de parser un message comme une invite
    d'approbation. Un attaquant qui controlerait ce groupe Telegram
    pourrait forger de faux textes d'invite. Le groupe doit rester
    prive, avec uniquement Olivier + les deux bots dedans.
  - Une fois qu'une decision "allow" part, CSTL EXECUTE REELLEMENT une
    commande shell sur la machine d'Olivier via OpenClaw. Un bug dans
    `judge_command()` qui repond "allow" a tort est un vrai risque
    d'execution non desiree -- ce n'est plus une demonstration de
    protocole a ce stade.

Usage :
    python3 sdk/python/cstl_openclaw_bridge.py \
        --bot-token <TOKEN_DU_BOT_APPROBATEUR> \
        --chat-id <ID_DU_GROUPE>

    Si --chat-id est omis, le script imprime les chat_id vus dans les
    premiers updates recus et s'arrete -- utile pour le decouvrir une
    fois le bot ajoute au groupe (ecrire un message dans le groupe
    d'abord, puis lancer le script sans --chat-id).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.request
import urllib.error
from dataclasses import dataclass
from typing import Optional

TELEGRAM_API = "https://api.telegram.org"
POLL_TIMEOUT_S = 30  # long-polling Telegram cote getUpdates

# ---------------------------------------------------------------------------
# Parsing du texte d'invite -- format confirme en direct le 2026-09-07
# (voir docstring plus haut). Regex volontairement tolerantes a l'ordre
# EN/FR puisque OpenClaw peut localiser certains libelles.
# ---------------------------------------------------------------------------

APPROVAL_MARKER_RE = re.compile(r"Approval required\.", re.IGNORECASE)
FULL_ID_RE = re.compile(r"Full id:\s*\n?\s*([0-9a-fA-F-]{36})")
# Fallback : extraire l'id depuis une des deux lignes /approve pretes-a-copier
# si jamais "Full id:" disparait d'une version future d'OpenClaw.
APPROVE_LINE_RE = re.compile(
    r"/approve\s+([0-9a-fA-F-]{36})\s+(allow-once|allow-always|deny)"
)
# La commande en attente : entre "Run:" ou "Pending command:" et le prochain
# bloc vide/label. On capture la premiere ligne non vide qui suit.
COMMAND_BLOCK_RE = re.compile(
    r"(?:Run:|Pending command:)\s*\n(?:Sh\s*\n)?([^\n]+)", re.IGNORECASE
)


@dataclass
class ApprovalRequest:
    approval_id: str
    command_text: Optional[str]
    raw_text: str


def parse_approval_message(text: str) -> Optional[ApprovalRequest]:
    """Retourne une ApprovalRequest si `text` est une invite d'approbation
    exec OpenClaw reconnaissable, sinon None. Ne devine jamais un id --
    si aucun id n'est trouve, retourne None plutot que de risquer de
    repondre a la mauvaise invite."""
    if not APPROVAL_MARKER_RE.search(text):
        return None

    id_match = FULL_ID_RE.search(text)
    approval_id = id_match.group(1) if id_match else None
    if approval_id is None:
        approve_match = APPROVE_LINE_RE.search(text)
        if approve_match:
            approval_id = approve_match.group(1)
    if approval_id is None:
        return None

    cmd_match = COMMAND_BLOCK_RE.search(text)
    command_text = cmd_match.group(1).strip() if cmd_match else None

    return ApprovalRequest(approval_id=approval_id, command_text=command_text, raw_text=text)


# ---------------------------------------------------------------------------
# Jugement -- PLACEHOLDER EXPLICITE, voir avertissement dans la docstring
# du module. A remplacer par la vraie politique deontique CSTL des que
# celle-ci existe pour des commandes shell.
# ---------------------------------------------------------------------------

# Prefixes de commandes considerees lecture-seule / sans effet de bord
# destructeur evident. Correspondance sur le premier mot de la commande
# uniquement -- ne protege PAS contre `cmd1 && rm -rf /` ou une injection
# de shell dans les arguments. C'est un point de depart, pas une sandbox.
_READONLY_PREFIXES = (
    "uptime", "date", "whoami", "pwd", "ls", "df", "ps", "uname",
    "openclaw status", "openclaw exec-policy show", "openclaw config get",
    "cat ", "echo ",
)

# Motifs jamais approuves automatiquement, quel que soit le prefixe --
# verifies en dernier, gagnent toujours sur la liste blanche ci-dessus.
_HARD_DENY_SUBSTRINGS = (
    "rm -rf", "mkfs", "dd if=", "shutdown", "reboot", ":(){:|:&};:",
    "> /dev/sd", "chmod -R 777 /", "curl ", "wget ",
)


def judge_command(command_text: Optional[str]) -> str:
    """Retourne 'allow-once' ou 'deny'. PLACEHOLDER -- voir docstring
    module. Refuse par defaut si la commande est absente/illisible."""
    if not command_text:
        return "deny"
    lowered = command_text.strip().lower()
    if any(bad in lowered for bad in _HARD_DENY_SUBSTRINGS):
        return "deny"
    if any(lowered.startswith(prefix) for prefix in _READONLY_PREFIXES):
        return "allow-once"
    return "deny"


# ---------------------------------------------------------------------------
# Client Telegram minimal -- stdlib uniquement, pas de dependance externe.
# ---------------------------------------------------------------------------

class TelegramError(Exception):
    pass


def _telegram_call(token: str, method: str, params: dict, timeout: int = 10) -> dict:
    url = f"{TELEGRAM_API}/bot{token}/{method}"
    data = json.dumps(params).encode("utf-8")
    req = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except urllib.error.URLError as exc:
        raise TelegramError(f"appel {method} echoue: {exc}") from exc
    if not body.get("ok"):
        raise TelegramError(f"{method} a renvoye ok=false: {body}")
    return body["result"]


def get_updates(token: str, offset: Optional[int]) -> list:
    params = {"timeout": POLL_TIMEOUT_S}
    if offset is not None:
        params["offset"] = offset
    return _telegram_call(token, "getUpdates", params, timeout=POLL_TIMEOUT_S + 10)


def send_message(token: str, chat_id: int, text: str) -> dict:
    return _telegram_call(token, "sendMessage", {"chat_id": chat_id, "text": text})


# ---------------------------------------------------------------------------
# Boucle principale
# ---------------------------------------------------------------------------

def run_bridge(token: str, chat_id: Optional[int]) -> None:
    print(
        "[cstl_openclaw_bridge] demarrage -- POLITIQUE PLACEHOLDER, "
        "voir avertissement en tete de fichier.",
        file=sys.stderr,
    )
    offset = None
    seen_chat_ids = set()

    while True:
        try:
            updates = get_updates(token, offset)
        except TelegramError as exc:
            print(f"[cstl_openclaw_bridge] erreur getUpdates: {exc}", file=sys.stderr)
            time.sleep(5)
            continue

        for update in updates:
            offset = update["update_id"] + 1
            message = update.get("message") or update.get("channel_post")
            if not message:
                continue
            text = message.get("text")
            msg_chat_id = message.get("chat", {}).get("id")

            if chat_id is None:
                if msg_chat_id not in seen_chat_ids:
                    seen_chat_ids.add(msg_chat_id)
                    print(
                        f"[cstl_openclaw_bridge] chat_id vu: {msg_chat_id} "
                        f"(type={message.get('chat', {}).get('type')}, "
                        f"titre={message.get('chat', {}).get('title')})"
                    )
                continue

            if msg_chat_id != chat_id:
                continue
            if not text:
                continue

            request = parse_approval_message(text)
            if request is None:
                continue

            decision = judge_command(request.command_text)
            reply = f"/approve {request.approval_id} {decision}"
            print(
                f"[cstl_openclaw_bridge] invite id={request.approval_id} "
                f"commande={request.command_text!r} -> decision={decision}"
            )
            try:
                send_message(token, chat_id, reply)
            except TelegramError as exc:
                print(
                    f"[cstl_openclaw_bridge] echec envoi decision: {exc}",
                    file=sys.stderr,
                )

        if chat_id is None and seen_chat_ids:
            print(
                "[cstl_openclaw_bridge] chat_id non fourni -- relance avec "
                "--chat-id <un des ids ci-dessus> pour activer le jugement.",
                file=sys.stderr,
            )
            return


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bot-token", required=True, help="Token du bot approbateur dedie (PAS celui d'OpenClaw)")
    parser.add_argument("--chat-id", type=int, default=None, help="chat_id du groupe (omettre pour le decouvrir)")
    args = parser.parse_args()
    run_bridge(args.bot_token, args.chat_id)


if __name__ == "__main__":
    main()
