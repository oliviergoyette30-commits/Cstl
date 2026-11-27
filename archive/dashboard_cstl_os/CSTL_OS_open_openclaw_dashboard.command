#!/bin/bash
# CSTL_OS_open_openclaw_dashboard.command -- ouvre la VRAIE console web
# d'OpenClaw (Control UI native du gateway, pas quelque chose invente ici),
# avec le token courant genere par OpenClaw lui-meme. Cree le 07/09/2026 a
# la demande de l'utilisateur ("j'aimerais avoir acces a l'interface
# openclaw"). Trouve via `openclaw --help` -- commande deja existante,
# jamais utilisee dans ce depot avant aujourd'hui :
#   openclaw dashboard   "Open the Control UI with your current token"
# Meme correctif de PATH que CSTL_OS (une .app/.command lancee du Finder
# n'herite pas de ~/.zshrc) -- openclaw et node doivent etre trouvables.
set -u
export PATH="$HOME/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if ! command -v openclaw >/dev/null 2>&1; then
    osascript -e 'display notification "commande openclaw introuvable dans PATH" with title "CSTL -- OpenClaw Dashboard"' >/dev/null 2>&1 || true
    echo "openclaw introuvable dans PATH -- rien a ouvrir."
    exit 1
fi

echo "Ouverture de la console OpenClaw (openclaw dashboard)..."
openclaw dashboard
