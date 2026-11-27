#!/usr/bin/env python3
"""
cstl_guard.py -- cote SYSTEME VISE : refuser toute action sans autorisation
signee par le serveur CSTL (couche 9 obligatoire, 2026-11-20).

Avant : la couche 9 repondait allowed/denied, mais c'etait l'agent qui
choisissait de demander. Un agent qui agissait sans demander n'etait pas
bloque. Ici, c'est le systeme qui execute l'action (base de donnees, API,
outil) qui exige la preuve :

    guard = CstlGuard(open("cstl_adn.db.server_key.pub").read())  # cle epinglee
    guard.run(token, "wipe_db", wipe_db)   # leve Unauthorized sinon

et l'agent obtient le jeton par `CstlAgent.authorize("wipe_db")`.

Le jeton est verifie contre la cle PUBLIQUE du serveur, distribuee hors bande
(fichier `<base>.server_key.pub`) : la cle `server_key` recopiee dans la
reponse du serveur n'est JAMAIS utilisee pour verifier (un faux serveur
pourrait y mettre la sienne). Message canonique identique a
`src/server/server_key.rs::Authorization::message`.

Verifie : signature, action, agent (optionnel), validite dans le temps, et
usage unique (un request_id deja execute est refuse). Le registre d'usage
est en memoire, ou dans un fichier SQLite (`replay_db=`) pour survivre a un
redemarrage du systeme vise.
"""
import os
import sqlite3
import threading
import time
from dataclasses import dataclass
from typing import Callable, Optional

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

AUTHZ_VERSION = "cstl-authz-v1"


class Unauthorized(Exception):
    """Action refusee par le systeme vise : jeton absent, invalide ou deja utilise."""

    def __init__(self, reason: str):
        self.reason = reason
        super().__init__(f"action refusee : {reason}")


@dataclass(frozen=True)
class AuthorizationToken:
    agent: str
    action: str
    request_id: str
    issued_at: int
    expires_at: int
    audit_hash: str
    signature: str
    # Montant autorise (2026-11-21), tel qu'ecrit par le serveur, ou None.
    amount: Optional[str] = None

    def message(self) -> bytes:
        text = (f"{AUTHZ_VERSION}\nagent={self.agent}\naction={self.action}\nrequest_id={self.request_id}\n"
                f"issued_at={self.issued_at}\nexpires_at={self.expires_at}\naudit_hash={self.audit_hash}")
        if self.amount is not None:
            text += f"\namount={self.amount}"
        return text.encode()

    @classmethod
    def from_response(cls, resp) -> Optional["AuthorizationToken"]:
        """Jeton d'une reponse `action_authorization` (CstlResponse), ou None
        si la decision n'est pas `allowed` ou si un champ manque."""
        f = resp.fields
        if f.get("decision") != "allowed":
            return None
        try:
            return cls(agent=f["receiver"], action=f["action"], request_id=f["request_id"],
                       issued_at=int(f["issued_at"]), expires_at=int(f["expires_at"]),
                       audit_hash=f["audit_hash"], signature=f["authorization"], amount=f.get("amount"))
        except (KeyError, ValueError):
            return None


class CstlGuard:
    def __init__(self, server_public_key_hex: str, *, skew: int = 5,
                 replay_db: Optional[str] = None, clock: Callable[[], float] = time.time,
                 online: Optional[tuple] = None, online_timeout: float = 5.0):
        """`online=(hote, port)` (2026-11-25) : avant d'executer, demander au
        serveur CSTL si le jeton vaut ENCORE (purpose=token_status). Refus si
        la cle de l'agent a ete revoquee (`revoked`), si un arret d'urgence le
        couvre (`stopped`), si le serveur ne le connait pas (`unknown`) ou s'il
        ne repond pas (`status_unavailable`, ferme par defaut). La reponse est
        verifiee sous la cle du serveur, avec un nonce neuf."""
        try:
            self._key = Ed25519PublicKey.from_public_bytes(bytes.fromhex(server_public_key_hex.strip()))
        except ValueError as e:
            raise ValueError(f"cle publique du serveur invalide : {e}") from e
        self.skew, self.clock = skew, clock
        if online is None and os.environ.get("CSTL_GUARD_ONLINE"):
            # Pour les proxys MCP/A2A et cstl-gate, sans changer leur code.
            host, _, port = os.environ["CSTL_GUARD_ONLINE"].rpartition(":")
            online = (host or "127.0.0.1", int(port))
        self.online, self.online_timeout = online, online_timeout
        self._lock = threading.Lock()
        self._used: set[str] = set()
        self._db = None
        if replay_db:
            self._db = sqlite3.connect(replay_db, check_same_thread=False)
            self._db.execute("CREATE TABLE IF NOT EXISTS used_authorizations (request_id TEXT PRIMARY KEY, action TEXT, used_at INTEGER)")
            self._db.commit()

    def check(self, token: Optional[AuthorizationToken], action: str, agent: Optional[str] = None,
              amount=None) -> None:
        """Leve Unauthorized si le jeton ne permet pas `action` maintenant (et
        pour `amount`, si le systeme vise execute un montant). Ne consomme PAS
        le jeton (voir `run`)."""
        if token is None:
            raise Unauthorized("no_token")
        try:
            self._key.verify(bytes.fromhex(token.signature), token.message())
        except (InvalidSignature, ValueError):
            raise Unauthorized("bad_signature")
        if token.action != action:
            raise Unauthorized("wrong_action")
        if agent is not None and token.agent != agent:
            raise Unauthorized("wrong_agent")
        # Montant (2026-11-21) : le jeton autorise UN montant precis. Executer
        # un autre montant, ou un montant avec un jeton qui n'en porte pas
        # (obtenu sans le declarer), est refuse.
        if amount is not None or token.amount is not None:
            try:
                same = amount is not None and token.amount is not None and float(amount) == float(token.amount)
            except ValueError:
                same = False
            if not same:
                raise Unauthorized("wrong_amount")
        now = self.clock()
        if token.issued_at > now + self.skew:
            raise Unauthorized("not_yet_valid")
        if now > token.expires_at + self.skew:
            raise Unauthorized("expired")
        if self.online is not None:
            status = self.live_status(token)
            if status != "valid":
                raise Unauthorized(status)

    def live_status(self, token: AuthorizationToken) -> str:
        """Etat du jeton selon le serveur, en direct : valid, expired, revoked,
        stopped, unknown, ou status_unavailable (pas de reponse signee)."""
        import secrets
        import socket
        nonce = secrets.token_hex(16)
        text = ("#!CSTL v5.0.0 MODE=A\nMETA [encoder=cstl_guard, produced_by=cstl_guard]\n"
                f"INTENT_PAYLOAD [purpose=token_status, sender=cstl_guard, receiver=server, "
                f"audit_hash={token.audit_hash}, nonce={nonce}]\n---END---\n")
        try:
            with socket.create_connection(self.online, timeout=self.online_timeout) as s:
                s.sendall(text.encode())
                buf = b""
                while b"---END---" not in buf:
                    chunk = s.recv(65536)
                    if not chunk:
                        break
                    buf += chunk
        except OSError:
            return "status_unavailable"
        fields = {}
        for line in buf.decode(errors="replace").splitlines():
            if line.startswith("INTENT_PAYLOAD ["):
                for part in line[len("INTENT_PAYLOAD ["):].rstrip("]").split(", "):
                    k, _, v = part.partition("=")
                    fields[k.strip()] = v.strip()
        status = fields.get("status", "")
        msg = (f"cstl-token-status-v1\naudit_hash={token.audit_hash}\nstatus={status}\n"
               f"nonce={nonce}\nchecked_at={fields.get('checked_at', '')}")
        try:
            if fields.get("purpose") != "token_status" or fields.get("nonce") != nonce \
                    or fields.get("audit_hash") != token.audit_hash:
                return "status_unavailable"
            self._key.verify(bytes.fromhex(fields.get("signature", "")), msg.encode())
        except (InvalidSignature, ValueError):
            return "status_unavailable"
        return status if status in ("valid", "expired", "revoked", "stopped", "unknown") else "status_unavailable"

    def _consume(self, token: AuthorizationToken) -> None:
        with self._lock:
            if token.request_id in self._used:
                raise Unauthorized("already_used")
            if self._db is not None:
                try:
                    self._db.execute("INSERT INTO used_authorizations VALUES (?, ?, ?)",
                                     (token.request_id, token.action, int(self.clock())))
                    self._db.commit()
                except sqlite3.IntegrityError:
                    raise Unauthorized("already_used")
            self._used.add(token.request_id)

    def run(self, token: Optional[AuthorizationToken], action: str, func: Callable, *args,
            agent: Optional[str] = None, amount=None, **kwargs):
        """Execute `func` SEULEMENT avec un jeton valide, jamais utilise, pour
        `action` (et pour `amount` : le montant que `func` va reellement
        executer). Le jeton est consomme AVANT l'execution : une action qui
        echoue en cours de route demande un nouveau jeton."""
        self.check(token, action, agent, amount)
        self._consume(token)
        return func(*args, **kwargs)
