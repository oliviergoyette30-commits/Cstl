#!/bin/bash
# CSTL_OS_stop.command -- arrete le serveur Rust CSTL et le dashboard Python
# demarres par CSTL_OS.app (2026-09-07). Double-clic dans le Finder (ou
# `bash CSTL_OS_stop.command` en terminal).
#
# Historique : la premiere version tuait le PID enregistre par CSTL_OS.app
# (capture via `$!` sur un `nohup cargo run --release &`) -- ce PID est
# celui du wrapper cargo, pas du vrai binaire `cstl_parser` qu'il lance en
# enfant. Le tuer laissait le vrai serveur tourner, orphelin, invisible.
# Trouve et corrige en verifiant reellement les process apres coup (`ps
# aux` montrait toujours `target/release/cstl_parser` actif apres "arret").
# Methode corrigee : chercher par le nom du VRAI binaire/script (pkill -f),
# pas par un PID capture au demarrage.

set -u

kill_by_pattern() {
    local pattern="$1" label="$2"
    local pids
    pids="$(pgrep -f "$pattern" 2>/dev/null || true)"
    if [ -n "$pids" ]; then
        echo "Arret de $label (pattern: $pattern) -- pid(s): $pids"
        pkill -f "$pattern" 2>/dev/null || true
    else
        echo "$label : rien a arreter (aucun process ne correspond a: $pattern)"
    fi
}

# Le vrai binaire compile -- c'est LUI qui ecoute reellement sur le port
# 5050, pas le process `cargo run` qui l'a lance (celui-ci se termine tout
# seul une fois son enfant tue).
kill_by_pattern "target/release/cstl_parser" "serveur CSTL (binaire)"
kill_by_pattern "cargo run --release" "serveur CSTL (wrapper cargo, si encore present)"
kill_by_pattern "dashboard/server.py" "dashboard Python"

echo "Termine."
