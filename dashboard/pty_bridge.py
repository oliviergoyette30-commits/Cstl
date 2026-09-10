#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
dashboard/pty_bridge.py -- serveur WebSocket qui donne un VRAI terminal
interactif (pseudo-terminal complet, pas un mode "print" one-shot) pour
Hermes/Ollama, Claude Code et OpenClaw, directement depuis les onglets du
dashboard. Ajoute le 07/09/2026 a la demande explicite de l'utilisateur :
"j'aimerais avoir la vraie interface" -- reponse choisie apres clarification
("terminal complet interactif pour chaque outil").

Ce que ceci fait REELLEMENT : ouvre un pseudo-terminal (module stdlib
`pty`, POSIX -- macOS inclus), y lance le VRAI process interactif de
chaque outil (`ollama run <modele>`, `claude` SANS -p, `openclaw chat`),
et relaie les octets bruts (sequences ANSI, effacements, positionnement du
curseur...) entre ce PTY et une connexion WebSocket. Cote navigateur,
xterm.js affiche ces octets a l'identique -- c'est le VRAI terminal de
chaque outil, pas une reimplementation ou une simulation. Si le CLI
plante, demande un mot de passe ou affiche une erreur, l'utilisateur le
voit tel quel, comme dans une vraie fenetre Terminal.

Dependance externe REELLE (pas stdlib) : `websockets` (asyncio). Si
absente, start_server() retourne False et le reste du dashboard continue
de fonctionner normalement -- ce module est optionnel, jamais un
prerequis dur, meme philosophie que resolve_brain()/HermesAgentBrain
ailleurs dans ce projet (degradation propre, jamais un crash).

Securite : le serveur WebSocket est lie a 127.0.0.1 UNIQUEMENT -- meme
modele de confiance que le reste de ce dashboard (machine locale,
mono-utilisateur, pas d'authentification supplementaire ajoutee ici,
puisque le dashboard HTTP lui-meme n'en a pas non plus).

Limite honnete : chaque connexion WebSocket spawn son PROPRE process --
deux onglets/clients connectes au meme outil en meme temps sont deux vrais
process independants (deux vrais terminaux), jamais un etat partage.
"""
from __future__ import annotations

import asyncio
import base64
import fcntl
import json
import os
import pty
import shutil
import signal
import struct
import termios
import threading
from pathlib import Path

try:
    import websockets
except ImportError:
    websockets = None


def _resolve_binary(name, extra_candidates=()):
    """Cherche un binaire reel -- PATH d'abord (shutil.which, coherent
    avec run_claude_code() dans server.py), puis une liste explicite
    d'emplacements connus sur cette machine/plateforme en repli. Jamais de
    chemin invente si rien n'est trouve."""
    found = shutil.which(name)
    if found:
        return found
    for candidate in extra_candidates:
        p = Path(candidate).expanduser()
        if p.exists():
            return str(p)
    return None


def _first_ollama_model():
    """Interroge le VRAI serveur Ollama local (meme endpoint que
    check_hermes_connection() dans server.py) pour trouver un modele
    reellement installe -- jamais un nom code en dur qui pourrait ne pas
    exister sur cette machine."""
    try:
        import urllib.request
        with urllib.request.urlopen("http://127.0.0.1:11434/api/tags", timeout=2.0) as resp:
            data = json.loads(resp.read())
            models = data.get("models", [])
            if models:
                return models[0].get("name")
    except Exception:
        pass
    return None


def build_command(tool, repo_root):
    """Retourne (argv: list[str] | None, cwd: str | None, error: str | None)
    pour l'outil demande. Jamais de commande fabriquee : si le binaire ou
    un prerequis manque, error dit precisement quoi, argv/cwd restent
    None."""
    if tool == "claude-code":
        bin_path = _resolve_binary("claude", ["~/.npm-global/bin/claude"])
        if not bin_path:
            return None, None, "commande 'claude' introuvable (PATH ou ~/.npm-global/bin)."
        return [bin_path], str(repo_root), None

    if tool == "openclaw":
        bin_path = _resolve_binary("openclaw", ["~/.npm-global/bin/openclaw"])
        if not bin_path:
            return None, None, "commande 'openclaw' introuvable (PATH ou ~/.npm-global/bin)."
        return [bin_path, "chat"], str(repo_root), None

    if tool == "openclaw-logs":
        # Lecture seule, ajoute le 07/09/2026 a la demande explicite de
        # l'utilisateur : "une copy de openclaw qu'on ne peut pas interagir
        # avec mais qu'on voit ce qui se passe dans openclaw". `openclaw
        # chat` (le tool "openclaw" ci-dessus) est le runtime LOCAL embarque
        # (alias de `openclaw tui --local`) -- il essaie de reprendre le
        # meme port que la vraie Gateway et echoue si elle tourne deja.
        # `openclaw logs --follow` est different : documente
        # (docs/logging.md) comme un tail RPC du log de la Gateway --
        # fonctionne PRECISEMENT quand la Gateway tourne, ne bind aucun
        # port, ne peut rien modifier cote Gateway. Aucun forward de stdin
        # n'est cable cote frontend pour cet outil (voir index.html) --
        # lecture seule appliquee au niveau UI, pas seulement documentee.
        bin_path = _resolve_binary("openclaw", ["~/.npm-global/bin/openclaw"])
        if not bin_path:
            return None, None, "commande 'openclaw' introuvable (PATH ou ~/.npm-global/bin)."
        return [bin_path, "logs", "--follow"], str(repo_root), None

    if tool == "hermes":
        bin_path = _resolve_binary("ollama", [
            "/usr/local/bin/ollama", "/opt/homebrew/bin/ollama",
            "/Applications/Ollama.app/Contents/Resources/ollama",
        ])
        if not bin_path:
            return None, None, "commande 'ollama' introuvable -- Ollama est-il installe sur cette machine ?"
        model = _first_ollama_model()
        if not model:
            return None, None, ("aucun modele Ollama detecte via http://127.0.0.1:11434/api/tags -- "
                                 "le serveur Ollama tourne-t-il, et au moins un modele est-il installe "
                                 "(`ollama pull ...`) ?")
        return [bin_path, "run", model], str(repo_root), None

    return None, None, f"outil inconnu: {tool!r}"


class PtySession:
    """Une VRAIE session PTY -- un process enfant reel, un fd maitre reel.
    Relaye vers UNE connexion WebSocket a la fois."""

    def __init__(self, argv, cwd):
        self.argv = argv
        self.cwd = cwd
        self.pid = None
        self.fd = None

    def spawn(self):
        pid, fd = pty.fork()
        if pid == 0:
            # Cote enfant : remplace cette image de process par le vrai
            # binaire demande -- os.execvpe ne revient jamais si ca reussit.
            try:
                os.chdir(self.cwd)
            except OSError:
                pass
            env = os.environ.copy()
            env.setdefault("TERM", "xterm-256color")
            try:
                os.execvpe(self.argv[0], self.argv, env)
            except Exception:
                os._exit(127)
        else:
            self.pid = pid
            self.fd = fd
            os.set_blocking(fd, False)

    def resize(self, cols, rows):
        if self.fd is None:
            return
        try:
            winsize = struct.pack("HHHH", rows, cols, 0, 0)
            fcntl.ioctl(self.fd, termios.TIOCSWINSZ, winsize)
        except OSError:
            pass

    def write(self, data: bytes):
        if self.fd is None:
            return
        try:
            os.write(self.fd, data)
        except OSError:
            pass

    def close(self):
        # pty.fork() appelle setsid() cote enfant -- il est chef de son
        # propre groupe de process, donc killpg atteint aussi les
        # eventuels sous-process qu'il aurait lances.
        if self.pid:
            try:
                os.killpg(os.getpgid(self.pid), signal.SIGTERM)
            except (ProcessLookupError, PermissionError, OSError):
                try:
                    os.kill(self.pid, signal.SIGTERM)
                except (ProcessLookupError, OSError):
                    pass
            try:
                os.waitpid(self.pid, os.WNOHANG)
            except (ChildProcessError, OSError):
                pass
        if self.fd is not None:
            try:
                os.close(self.fd)
            except OSError:
                pass


async def _pump_pty_to_ws(loop, session, ws):
    """Lit le PTY (non-bloquant, via loop.add_reader) et pousse chaque
    paquet d'octets recu vers le websocket. Base64 pour rester dans un
    frame texte JSON sans se soucier de l'encodage -- un vrai PTY peut
    emettre des octets non-UTF8 valides (couleurs, dessin de boite...)."""
    queue: asyncio.Queue = asyncio.Queue()

    def on_readable():
        try:
            chunk = os.read(session.fd, 65536)
        except OSError:
            chunk = b""
        queue.put_nowait(chunk)
        if not chunk:
            try:
                loop.remove_reader(session.fd)
            except (ValueError, OSError):
                pass

    loop.add_reader(session.fd, on_readable)
    try:
        while True:
            chunk = await queue.get()
            if not chunk:
                await ws.send(json.dumps({"type": "exit"}))
                break
            await ws.send(json.dumps({"type": "stdout", "data": base64.b64encode(chunk).decode("ascii")}))
    finally:
        try:
            loop.remove_reader(session.fd)
        except (ValueError, OSError):
            pass


async def _handle_client(ws, tool, repo_root):
    argv, cwd, error = build_command(tool, repo_root)
    if error:
        await ws.send(json.dumps({"type": "error", "message": error}))
        await ws.close()
        return

    session = PtySession(argv, cwd)
    try:
        session.spawn()
    except OSError as e:
        await ws.send(json.dumps({"type": "error", "message": f"echec du lancement reel: {e}"}))
        await ws.close()
        return

    await ws.send(json.dumps({"type": "started", "argv": argv}))

    loop = asyncio.get_event_loop()
    pump_task = asyncio.ensure_future(_pump_pty_to_ws(loop, session, ws))
    try:
        async for message in ws:
            try:
                msg = json.loads(message)
            except json.JSONDecodeError:
                continue
            if msg.get("type") == "stdin":
                session.write(base64.b64decode(msg.get("data", "")))
            elif msg.get("type") == "resize":
                try:
                    session.resize(int(msg.get("cols", 80)), int(msg.get("rows", 24)))
                except (TypeError, ValueError):
                    pass
    except Exception:
        pass
    finally:
        pump_task.cancel()
        session.close()


def start_server(repo_root, host="127.0.0.1", port=5100):
    """Demarre le serveur WebSocket dans un thread dedie avec sa propre
    boucle asyncio (le reste du dashboard reste un ThreadingHTTPServer
    synchrone -- ce module ne le modifie pas). Retourne True si demarre
    reellement, False si le paquet `websockets` est absent -- le dashboard
    HTTP continue de fonctionner sans cette fonctionnalite, honnetement
    signale cote /api/terminal-info plutot que de planter."""
    if websockets is None:
        return False

    async def handler(ws, path=None):
        # Compat entre versions de `websockets` : les anciennes passent
        # `path` en 2e argument positionnel, les recentes (v11+) le
        # deplacent dans ws.request.path et n'appellent le handler
        # qu'avec un seul argument -- on gere les deux sans savoir a
        # l'avance laquelle est installee sur cette machine.
        if path is None:
            path = getattr(ws, "path", None)
            if path is None:
                req = getattr(ws, "request", None)
                path = getattr(req, "path", "/") if req is not None else "/"
        tool = path.strip("/").split("/")[-1]
        await _handle_client(ws, tool, repo_root)

    def run():
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        async def main():
            async with websockets.serve(handler, host, port):
                await asyncio.Future()  # tourne indefiniment

        try:
            loop.run_until_complete(main())
        except OSError as e:
            print(f"[pty_bridge] echec du bind {host}:{port} -- {e}")

    thread = threading.Thread(target=run, daemon=True, name="pty-bridge")
    thread.start()
    return True
