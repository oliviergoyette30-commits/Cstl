#!/usr/bin/env python3
"""
cstl_openclaw_userbot_bridge.py -- variante du pont OpenClaw<->CSTL qui lit
et repond directement dans le DM Telegram personnel d'Olivier avec le bot
OpenClaw, plutot que de router les invites vers un groupe separe
(2026-09-07).

POURQUOI CETTE VARIANTE EXISTE : `channels.telegram.execApprovals.target
= "both"` et `approvals.exec.mode = "targets"` ont ete essayes en direct
sur la machine d'Olivier pour faire apparaitre les invites dans un
groupe contenant un second bot dedie -- aucun des deux n'a fait
apparaitre le message dans le groupe (verifie: le groupe ne contient
que les messages de test manuels, jamais l'invite reelle). Plutot que
de continuer a deviner des cles de config non confirmees par la doc,
ce script contourne le probleme : il n'a pas besoin que l'invite sorte
du DM, puisqu'il AGIT COMME Olivier lui-meme sur Telegram (via
Telethon, protocole MTProto avec le compte personnel, pas un bot).

DIFFERENCE DE NATURE avec cstl_openclaw_bridge.py (a comprendre avant
de lancer ceci) :
  - cstl_openclaw_bridge.py pilote un bot Telegram DEDIE, avec un
    pouvoir limite a ce que ce bot peut faire.
  - CE script pilote le compte Telegram PERSONNEL d'Olivier -- il peut
    faire tout ce qu'Olivier peut faire sur Telegram (lire tous ses
    chats, envoyer des messages en son nom, etc.), meme si en pratique
    il ne regarde que les messages du bot OpenClaw configure. Le
    fichier de session cree au premier lancement (`--session`) est un
    secret d'authentification complet pour le compte Telegram
    d'Olivier -- a proteger comme un mot de passe, jamais commite,
    jamais partage.

Dependance externe (pas stdlib, contrairement a cstl_openclaw_bridge.py) :
    pip3 install telethon

Premiere utilisation (interactif -- demande le numero de telephone puis
un code recu dans Telegram, une seule fois -- le fichier de session
sauvegarde ensuite la connexion) :
    python3 sdk/python/cstl_openclaw_userbot_bridge.py \
        --api-id <API_ID> --api-hash <API_HASH> --peer CSTL1_bot

Verifie: PAS teste en direct sur la machine d'Olivier au moment de
l'ecriture de ce fichier (Telethon necessite un vrai numero de
telephone + code recu sur son compte -- non simulable depuis ce
sandbox). La logique de parsing/jugement reutilisee
(parse_approval_message, judge_command) EST testee, voir
cstl_openclaw_bridge.py et son historique de session.
"""

from __future__ import annotations

import argparse
import sys

try:
    from telethon import TelegramClient, events
except ImportError:
    print(
        "Telethon n'est pas installe. Lance: pip3 install telethon",
        file=sys.stderr,
    )
    raise

# Reutilise la logique deja ecrite et testee -- aucune duplication de
# regex ou de politique de jugement entre les deux ponts.
from cstl_openclaw_bridge import parse_approval_message, judge_command


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--api-id", required=True, type=int, help="API ID depuis my.telegram.org")
    parser.add_argument("--api-hash", required=True, help="API hash depuis my.telegram.org")
    parser.add_argument(
        "--peer",
        required=True,
        help="Username du bot OpenClaw a surveiller (sans @), ex: CSTL1_bot",
    )
    parser.add_argument(
        "--session",
        default="cstl_userbot",
        help="Nom du fichier de session Telethon (secret -- ne pas commiter). Defaut: cstl_userbot.session dans le repertoire courant.",
    )
    args = parser.parse_args()

    client = TelegramClient(args.session, args.api_id, args.api_hash)

    @client.on(events.NewMessage(incoming=True))
    async def handler(event):
        sender = await event.get_sender()
        sender_username = getattr(sender, "username", None) or ""
        if sender_username.lower() != args.peer.lower():
            return

        text = event.raw_text or ""
        request = parse_approval_message(text)
        if request is None:
            return

        decision = judge_command(request.command_text)
        reply = f"/approve {request.approval_id} {decision}"
        print(
            f"[cstl_openclaw_userbot_bridge] invite id={request.approval_id} "
            f"commande={request.command_text!r} -> decision={decision}"
        )
        await event.respond(reply)

    print(
        "[cstl_openclaw_userbot_bridge] demarrage -- POLITIQUE PLACEHOLDER "
        "(voir judge_command dans cstl_openclaw_bridge.py). Ecoute les DM "
        f"de @{args.peer}.",
        file=sys.stderr,
    )
    client.start()
    client.run_until_disconnected()


if __name__ == "__main__":
    main()
