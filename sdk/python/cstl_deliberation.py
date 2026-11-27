#!/usr/bin/env python3
"""
cstl_deliberation.py -- moteur de deliberation CSTL v6 (2026-11-26).
Specification : docs/CSTL_DELIBERATION_v6.md.

Plusieurs modeles deliberent sur un sujet SANS copier-coller et SANS relire
tout l'historique :

- ETAT : chaque proposition devient un POINT numerote (p1, p2, p2.1 pour un
  amendement de p2). Le moteur tient la position de chaque participant sur
  chaque point (supports / rejects / abstains, avec sigma).
- DELTA : a chaque tour, un modele recoit seulement l'ETAT COURANT (points
  ouverts et positions, points fermes depuis son dernier tour, ses erreurs a
  corriger), jamais l'historique ni les regles recopiees. La taille du
  message depend du nombre de points ouverts, pas de la longueur du debat.
- VERIFICATION : chaque reponse est verifiee localement (aucune prose, sigma
  entre 0 et 1, positions sur des points ouverts, limites), puis envoyee au
  VRAI serveur CSTL, signee par la cle du participant, ou elle est validee
  (operateurs, contradictions) et inscrite dans l'audit. Une erreur est
  renvoyee au modele, qui a `max_retries` essais pour corriger.
- CLOTURE : un point est CONSENSUS quand tous les participants actifs le
  soutiennent (sigma >= min_confidence) ; DEADLOCK quand il reste un refus ou
  une abstention et que les positions n'ont pas bouge pendant `stall_rounds`
  tours, ou a la limite de tours. Le debat s'arrete quand il n'y a plus de
  point ouvert, quand un tour entier n'a rien change, ou a `max_rounds`.
  Garde-fous contre la « politesse structuree » : au plus `max_new_points`
  propositions par tour, `max_points` au total, amendements limites a
  `max_depth` niveaux (p3.1.1 oui, p3.1.1.1 non).
- EXECUTION : les points d'ACTION en consensus (PERFORM ... modality=MUST,
  MUST_NOT ou MAY) sont compiles en regles signees par le moderateur (un
  auteur de politique du serveur) avec `publish_policy` : recu signe du
  serveur, puis la couche de controle accorde ou refuse les jetons.
- ESCALADE : un point d'action en DEADLOCK passe a un humain par l'accord
  humain du serveur (action `deliberate.<sujet>.<point>`, a lister dans
  CSTL_APPROVAL_ACTIONS) ; s'il approuve, le point est compile comme une
  decision humaine.

Les modeles sont interchangeables : `ScriptedModel` (tests), `ManualModel`
(copier-coller avec des chatbots, le moteur calcule les deltas),
`OpenRouterModel` (API).
"""
from __future__ import annotations

import json
import math
import os
import re
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

STANCES = ("supports", "rejects", "abstains")
MODALITIES = ("MUST", "MUST_NOT", "MAY")
_REL = re.compile(r"^RELATION\s*\[(.*)\]\s*$")
_NAME = re.compile(r"^[A-Za-z0-9_.\-]{1,128}$")
_SIGMA = re.compile(r"^(0(\.\d+)?|1(\.0+)?|\.\d+)$")


def estimate_tokens(text: str) -> int:
    """Estimation grossiere (4 caracteres par token), la meme pour toutes les
    formes comparees : elle sert aux comparaisons, pas a la facturation."""
    return max(1, math.ceil(len(text) / 4))


# ------------------------------------------------------------------ modeles
class ScriptedModel:
    """Reponses fixees a l'avance, ou calculees par une fonction (tests)."""

    def __init__(self, replies):
        self.replies = replies
        self.calls = []

    def reply(self, system: str, prompt: str) -> str:
        self.calls.append(prompt)
        if callable(self.replies):
            return self.replies(prompt, len(self.calls))
        return self.replies[min(len(self.calls), len(self.replies)) - 1]


class ManualModel:
    """Un humain recopie le message dans un chatbot et colle la reponse
    (terminee par ---END---). Les instructions ne sont donnees qu'une fois :
    le chatbot garde sa conversation."""

    def __init__(self, name, read=input, write=print):
        self.name, self.read, self.write, self.started = name, read, write, False

    def reply(self, system: str, prompt: str) -> str:
        text = prompt if self.started else system + "\n\n" + prompt
        self.started = True
        self.write(f"\n===== A COLLER DANS {self.name.upper()} =====\n{text}\n"
                   f"===== COLLE SA REPONSE (jusqu'a ---END---) =====")
        lines = []
        while True:
            line = self.read()
            lines.append(line)
            if line.strip() == "---END---":
                return "\n".join(lines)


class OpenRouterModel:
    """N'importe quel modele d'OpenRouter (GPT, Gemini, Claude...). Cle :
    OPENROUTER_API_KEY (dans l'environnement cloud, le proxy l'ajoute)."""

    URL = "https://openrouter.ai/api/v1/chat/completions"

    def __init__(self, model: str, max_tokens: int = 1500):
        self.model, self.max_tokens = model, max_tokens
        self.key = os.environ.get("OPENROUTER_API_KEY", "proxy-injected")
        self.usage = {"prompt_tokens": 0, "completion_tokens": 0}

    def reply(self, system: str, prompt: str) -> str:
        body = json.dumps({"model": self.model, "max_tokens": self.max_tokens, "temperature": 0,
                           "messages": [{"role": "system", "content": system},
                                        {"role": "user", "content": prompt}]}).encode()
        delay = 5
        attempts = int(os.environ.get("CSTL_OPENROUTER_ATTEMPTS", "5"))
        for attempt in range(attempts):
            req = urllib.request.Request(self.URL, data=body, headers={
                "Content-Type": "application/json", "Authorization": f"Bearer {self.key}",
                "X-Title": "CSTL deliberation"})
            try:
                with urllib.request.urlopen(req, timeout=300) as r:
                    d = json.load(r)
                for k in self.usage:
                    self.usage[k] += int((d.get("usage") or {}).get(k) or 0)
                content = ((d.get("choices") or [{}])[0].get("message") or {}).get("content")
                if content:
                    return content
            except urllib.error.HTTPError as e:
                if e.code not in (429, 500, 502, 503, 504) or attempt == attempts - 1:
                    raise RuntimeError(f"OpenRouter {e.code} : {e.read()[:200]!r}") from None
            except (urllib.error.URLError, TimeoutError, OSError, ValueError, __import__("http.client").client.HTTPException) as e:
                # Connexion coupee pendant la lecture, reponse tronquee ou
                # illisible (2026-11-26, banc de deliberation) : on reessaie.
                if attempt == attempts - 1:
                    raise RuntimeError(f"OpenRouter injoignable : {e}") from None
            time.sleep(delay)
            delay = min(delay * 2, 90)
        raise RuntimeError("OpenRouter : reponse vide")


# ------------------------------------------------------------------- etat
@dataclass
class Point:
    id: str
    kind: str                      # "action" ou "claim"
    relation: dict                 # la relation proposee (sans sigma)
    proposer: str
    round: int
    status: str = "open"           # open, consensus, deadlock, resolved_by_human, rejected_by_human
    reason: str = ""
    amends: Optional[str] = None
    history: list = field(default_factory=list)   # vecteurs de positions a chaque fin de tour

    def key(self):
        r = self.relation
        if self.kind == "action":
            return ("action", r["subject"], r["object"], r["modality"])
        return ("claim", r["type"], r["subject"], r["object"])

    def describe(self):
        r = self.relation
        if self.kind == "action":
            return f"{r['subject']} {r['modality']} {r['object']}"
        return f"{r['subject']} {r['type']} {r['object']}"


class ReplyError(ValueError):
    def __init__(self, code, detail):
        self.code, self.detail = code, detail
        super().__init__(f"{code}: {detail}")


def _attrs(inner):
    import cstl_client
    return cstl_client._parse_kv_block(inner)


def parse_reply(text: str) -> list[dict]:
    """Relations d'une reponse de modele. Leve ReplyError (code, detail) sur
    tout ce qui n'est pas une relation : rien n'est devine. Un bloc de code
    autour est accepte ; les lignes META / INTENT_PAYLOAD sont ignorees (le
    moteur les ecrit lui-meme)."""
    lines = [ln.strip() for ln in text.strip().splitlines()]
    lines = [ln for ln in lines if ln and not ln.startswith("```")]
    if not lines or not lines[0].startswith("#!CSTL"):
        raise ReplyError("E_FORMAT", "la reponse doit commencer par #!CSTL v5.0.0 MODE=A")
    try:
        end = lines.index("---END---")
    except ValueError:
        raise ReplyError("E_FORMAT", "la reponse doit finir par ---END---") from None
    if any(lines[end + 1:]):
        raise ReplyError("E_PROSE", f"texte apres ---END--- : {lines[end + 1][:80]!r}")
    rels = []
    for ln in lines[1:end]:
        if ln.startswith(("META [", "INTENT_PAYLOAD [")):
            continue
        m = _REL.match(ln)
        if not m:
            raise ReplyError("E_PROSE", f"ligne hors format : {ln[:80]!r} (seulement des lignes RELATION [...])")
        rels.append({k: str(v) for k, v in _attrs(m.group(1)).items()})
    # Une reponse sans relation veut dire « rien a changer » : ce n'est pas
    # une erreur (avant le 2026-11-26, E_EMPTY forcait un nouvel essai).
    return rels


class Deliberation:
    """Une deliberation entre `participants` (noms d'agents) sur `topic`."""

    def __init__(self, topic, question, participants, models, *, host="127.0.0.1", port=5050,
                 moderator="moderateur", server_key=None, max_rounds=6, stall_rounds=2,
                 min_confidence=0.5, max_new_points=2, max_points=20, max_depth=2, max_retries=2,
                 max_failures=2, initial_points=(), escalate=True, log=None, context="", claims=True, quorum=None):
        if not _NAME.match(topic):
            raise ValueError("topic : identifiant [A-Za-z0-9_.-]")
        import cstl_llm_agent as m
        self.m = m
        self.topic, self.question = topic, question
        # Dossier du debat (texte libre : faits, regles), donne avec les
        # instructions a chaque appel ; jamais envoye au serveur.
        self.context = context
        # claims=False : deliberation de DECISION, seules les propositions
        # d'action deviennent des points ; une affirmation est verifiee et
        # auditee, mais ne garde pas le debat ouvert.
        self.claims = claims
        self.participants = list(participants)
        self.models = dict(models)
        self.cfg = dict(max_rounds=max_rounds, stall_rounds=stall_rounds, min_confidence=min_confidence,
                        max_new_points=max_new_points, max_points=max_points, max_depth=max_depth,
                        max_retries=max_retries, max_failures=max_failures)
        # Quorum : nombre minimal de participants actifs pour qu'une unanimite
        # compte. Par defaut, la majorite des participants inscrits, et au moins
        # 2 : un seul participant restant (les autres exclus) ne fait pas un
        # consensus.
        n = len(self.participants)
        self.cfg["quorum"] = max(2, n // 2 + 1) if quorum is None else int(quorum)
        if self.cfg["quorum"] < 2 or self.cfg["quorum"] > n:
            raise ValueError(f"quorum doit etre entre 2 et {n}")
        self.server_key = server_key
        self.escalate_enabled = escalate
        self.log = log or (lambda *_: None)
        quiet = __import__("contextlib").redirect_stdout(__import__("io").StringIO())
        with quiet:
            self.agents = {p: m.CstlAgent(p, "none", host=host, port=port) for p in self.participants}
            self.moderator = m.CstlAgent(moderator, "none", host=host, port=port)
            for a in [*self.agents.values(), self.moderator]:
                if not a.register():
                    raise RuntimeError(f"enregistrement refuse : {a.name}")
        # Sans CSTL_POLICY_AUTHORS, toute relation PERFORM d'un participant
        # deviendrait une regle active avant meme d'etre adoptee : refus.
        pol = self.moderator._send_signed("policy_query", "server", [], {"request_id": f"delib_{int(time.time() * 1e6)}"})
        if pol.fields.get("authors_enforced") != "true":
            raise RuntimeError("le serveur doit avoir CSTL_POLICY_AUTHORS (avec le moderateur) : sinon les "
                               "propositions des participants deviendraient des regles avant d'etre adoptees")
        self.points: dict[str, Point] = {}
        self.stances: dict[str, dict[str, tuple]] = {}      # point -> agent -> (stance, sigma, reason)
        self.active = list(self.participants)
        self.failures = {p: 0 for p in self.participants}
        self.round = 0
        self.events = []          # (round, kind, detail) : fermetures, erreurs, exclusions
        self.turns = []           # un enregistrement par appel de modele
        self.last_seen = {p: 0 for p in self.participants}   # index d'evenement vu au dernier tour
        self.compiled, self.escalations = [], []
        self.stop_reason = None
        for rel in initial_points:
            self._add_point(dict(rel), moderator, 0)

    # ------------------------------------------------------------ points
    def _next_id(self, amends=None):
        if amends:
            k = 1 + sum(1 for p in self.points if p.startswith(amends + ".") and p.count(".") == amends.count(".") + 1)
            return f"{amends}.{k}"
        return f"p{1 + sum(1 for p in self.points if '.' not in p)}"

    def _add_point(self, rel, proposer, rnd, sigma=None):
        amends = rel.pop("amends", None)
        sig = rel.pop("sigma", None)
        reason = rel.get("reason")
        kind = "action" if rel.get("type") == "PERFORM" and rel.get("modality") in MODALITIES else "claim"
        clean = {k: v for k, v in rel.items() if k in ("type", "subject", "object", "modality", "reason")}
        pid = self._next_id(amends)
        pt = Point(pid, kind, clean, proposer, rnd, amends=amends)
        self.points[pid] = pt
        self.stances[pid] = {}
        if proposer in self.participants:
            self.stances[pid][proposer] = ("supports", float(sig if sig is not None else sigma or 0.7), reason)
        self.events.append((rnd, "new_point", pid))
        return pt

    def _find(self, key):
        return next((p for p in self.points.values() if p.key() == key), None)

    # ------------------------------------------------------------ messages
    def system_prompt(self, agent):
        return (
            f"Tu es {agent}, participant a une deliberation CSTL avec {', '.join(p for p in self.participants if p != agent)}. "
            "Un moteur tient l'etat du debat : chaque proposition est un POINT numerote (p1, p2, p2.1 pour un amendement de p2). "
            "A chaque tour, tu recois l'etat courant ; reponds UNIQUEMENT par un message CSTL, sans aucune prose :\n"
            "#!CSTL v5.0.0 MODE=A\n"
            f"RELATION [type=supports, subject={agent}, object=p1, sigma=0.8]\n"
            f"RELATION [type=rejects, subject={agent}, object=p2, sigma=0.7, reason=\"pourquoi\"]\n"
            f"RELATION [type=abstains, subject={agent}, object=p3, sigma=0.5]\n"
            "RELATION [type=PERFORM, subject=<qui>, object=<action>, modality=MUST|MUST_NOT|MAY, sigma=0.8]   (nouvelle proposition d'action)\n"
            "RELATION [type=BELIEVES|KNOWS|DOUBTS|ASSUMES|STATE|ENTAILS|RECOMMEND, subject=<s>, object=<o>, sigma=0.6]   (nouvelle affirmation)\n"
            "RELATION [type=PERFORM, subject=<qui>, object=<action_modifiee>, modality=MAY, sigma=0.7, amends=p2]   (amendement de p2)\n"
            "---END---\n"
            "Regles : (1) donne ta position (supports, rejects ou abstains) sur chaque point ouvert ou tu n'en as pas, ou si tu changes d'avis ; "
            f"(2) au plus {self.cfg['max_new_points']} nouvelles propositions par tour, seulement si elles apportent quelque chose ; "
            "(3) sigma entre 0 et 1 sur chaque ligne ; (4) identifiants en minuscules_avec_soulignes ; "
            "(5) operateurs en majuscules : seulement ceux de l'exemple ; (6) un point est adopte quand tous le soutiennent, "
            "et le debat s'arrete quand plus rien ne bouge : ne repete pas une position inchangee."
            + (f"\n\nDOSSIER :\n{self.context}" if self.context else "")
        )

    def state_message(self, agent, errors=None):
        """L'ETAT pour `agent` : points ouverts avec toutes les positions,
        points fermes depuis son dernier tour, ses erreurs a corriger."""
        lines = ["#!CSTL v5.0.0 MODE=A",
                 f"META [encoder=cstl_deliberation, produced_by={self.moderator.name}, round={self.round}]",
                 f"INTENT_PAYLOAD [purpose=deliberate, sender={self.moderator.name}, receiver={agent}, topic={self.topic}, "
                 f"round={self.round}, max_rounds={self.cfg['max_rounds']}]"]
        if self.round == 1 and self.last_seen[agent] == 0:
            lines.append(f"RELATION [type=ASK, subject={self.moderator.name}, object={self.question}, sigma=1.0]")
        for p in self.points.values():
            if p.status != "open":
                continue
            r = p.relation
            # Une ligne par point, positions incluses : supports=gpt:0.8;gem:0.9
            # (raison seulement pour un refus, c'est elle qu'il faut discuter).
            groups = {s: [] for s in STANCES}
            why = []
            for who, (st, sg, reason) in sorted(self.stances[p.id].items()):
                groups[st].append(f"{who}:{sg:g}")
                if st == "rejects" and reason:
                    why.append(f"{who}: {_clean(reason)}")
            head = (f"{r['subject']} {r['modality']} {r['object']}" if p.kind == "action"
                    else f"{r['subject']} {r['type']} {r['object']}")
            parts = [f"type=point, subject={p.id}, object=\"{head}\""]
            if p.amends:
                parts.append(f"amends={p.amends}")
            parts += [f"{k}={';'.join(v)}" for k, v in groups.items() if v]
            if why:
                parts.append(f'why="{_clean(" / ".join(why))}"')
            lines.append("RELATION [" + ", ".join(parts) + "]")
        for rnd, kind, detail in self.events[self.last_seen[agent]:]:
            if kind in ("consensus", "deadlock"):
                lines.append(f"RELATION [type=closed, subject={detail}, object={kind}, round={rnd}]")
        for e in errors or []:
            lines.append(f"RELATION [type=error, subject={agent}, object={e.code}, detail=\"{_clean(e.detail)}\"]")
        lines.append("---END---")
        return "\n".join(lines)

    # ------------------------------------------------------------ un tour
    def _apply(self, agent, rels):
        """Verifie localement, puis renvoie (positions, nouveaux points) sans
        rien modifier ; leve ReplyError."""
        stances, new = [], []
        for r in rels:
            s = r.get("sigma")
            if s is None or not _SIGMA.match(s):
                raise ReplyError("E_SIGMA", f"sigma absent ou hors de 0 a 1 : {r}")
            t = r.get("type", "")
            if t in STANCES:
                if r.get("subject") != agent:
                    raise ReplyError("E_STANCE", f"une position doit avoir subject={agent}")
                pid = r.get("object", "")
                if pid not in self.points:
                    raise ReplyError("E_POINT", f"point inconnu : {pid}")
                if self.points[pid].status != "open":
                    continue                                 # point deja ferme : ignore
                stances.append((pid, t, float(s), r.get("reason")))
            else:
                for k in ("type", "subject", "object"):
                    if not _NAME.match(r.get(k, "")):
                        raise ReplyError("E_NAME", f"{k} manquant ou non identifiant : {r.get(k)!r}")
                if t == "PERFORM" and r.get("modality") not in MODALITIES:
                    raise ReplyError("E_MODALITY", "PERFORM exige modality=MUST, MUST_NOT ou MAY")
                if r.get("amends"):
                    parent = r["amends"]
                    if parent not in self.points:
                        raise ReplyError("E_AMEND", f"amendement d'un point inconnu : {parent}")
                    if parent.count(".") + 1 > self.cfg["max_depth"]:
                        raise ReplyError("E_DEPTH", f"amendement trop profond (au plus {self.cfg['max_depth']} niveaux) : {parent}")
                new.append(r)
        counted = [r for r in new if self.claims or (r.get("type") == "PERFORM" and r.get("modality") in MODALITIES)]
        if len(counted) > self.cfg["max_new_points"]:
            raise ReplyError("E_LIMIT", f"{len(counted)} nouvelles propositions, au plus {self.cfg['max_new_points']} par tour")
        return stances, new

    def _server_check(self, agent, rels):
        """Envoie les relations au vrai serveur, signees par l'agent. Renvoie
        None si accepte, sinon ReplyError avec l'erreur du serveur."""
        resp = self.agents[agent]._send_signed("inform", "server", rels,
                                               {"topic": self.topic, "round": str(self.round)})
        if resp.is_error or resp.purpose not in ("acknowledgement",):
            detail = resp.fields.get("detail") or resp.fields.get("errors") or resp.fields.get("reason") or resp.purpose
            return ReplyError("E_SERVER", f"{resp.purpose} : {detail}")
        return None

    def turn(self, agent):
        errors, record = [], {"round": self.round, "agent": agent, "attempts": []}
        system = self.system_prompt(agent)
        for attempt in range(1 + self.cfg["max_retries"]):
            prompt = self.state_message(agent, errors)
            t0 = time.time()
            try:
                text = self.models[agent].reply(system, prompt)
            except Exception as e:  # noqa: BLE001 -- un modele en panne est un participant defaillant
                text, err = "", ReplyError("E_MODEL", str(e)[:200])
            else:
                err = None
            att = {"prompt": prompt, "reply": text, "prompt_tokens": estimate_tokens(system + prompt),
                   "reply_tokens": estimate_tokens(text), "seconds": round(time.time() - t0, 2)}
            record["attempts"].append(att)
            if err is None:
                try:
                    rels = parse_reply(text)
                    stances, new = self._apply(agent, rels)
                    to_server = [{k: v for k, v in r.items() if k != "amends"} for r in rels]
                    err = self._server_check(agent, to_server) if to_server else None
                except ReplyError as e:
                    err = e
            if err is None:
                self._commit(agent, stances, new)
                att["ok"] = True
                self.failures[agent] = 0
                self.last_seen[agent] = len(self.events)
                self.turns.append(record)
                return True
            att["error"] = f"{err.code}: {err.detail}"
            errors = [err]
            self.log(f"  {agent} : {err.code} {err.detail[:120]}")
        self.failures[agent] += 1
        self.events.append((self.round, "failed_turn", agent))
        if self.failures[agent] >= self.cfg["max_failures"] and agent in self.active:
            self.active.remove(agent)
            self.events.append((self.round, "excluded", agent))
        self.last_seen[agent] = len(self.events)
        self.turns.append(record)
        return False

    def _commit(self, agent, stances, new):
        for pid, st, sg, why in stances:
            self.stances[pid][agent] = (st, sg, why)
        for r in new:
            base = dict(r)
            sig = base.get("sigma")
            probe = Point("?", "action" if base.get("type") == "PERFORM" and base.get("modality") in MODALITIES else "claim",
                          {k: base.get(k) for k in ("type", "subject", "object", "modality")}, agent, self.round)
            dup = self._find(probe.key()) if not base.get("amends") else None
            if dup is not None:
                if dup.status == "open":
                    self.stances[dup.id][agent] = ("supports", float(sig), base.get("reason"))
                continue
            if probe.kind == "claim" and not self.claims:
                self.events.append((self.round, "claim_ignored", agent))
                continue
            if len(self.points) >= self.cfg["max_points"]:
                self.events.append((self.round, "point_limit", agent))
                continue
            self._add_point(base, agent, self.round)

    # ------------------------------------------------------------ cloture
    def _vector(self, pid):
        return tuple(sorted((a, *self._effective(pid, a)) for a in self.active))

    def _effective(self, pid, agent):
        st, sg, _ = self.stances[pid].get(agent, ("none", 0.0, None))
        if st == "supports" and sg < self.cfg["min_confidence"]:
            st = "abstains"
        return (st,)

    def close_round(self):
        changed = False
        for p in self.points.values():
            if p.status != "open":
                continue
            vec = self._vector(p.id)
            p.history.append(vec)
            stances = [v[1] for v in vec]
            if len(self.active) >= self.cfg["quorum"] and all(s == "supports" for s in stances):
                p.status, p.reason = "consensus", "unanimity"
                self.events.append((self.round, "consensus", p.id))
                changed = True
                continue
            n = self.cfg["stall_rounds"]
            if self.round - p.round >= n and len(p.history) > n and len(set(p.history[-(n + 1):])) == 1:
                p.status = "deadlock"
                p.reason = "stalled_with_rejections" if "rejects" in stances else "stalled_without_quorum"
                self.events.append((self.round, "deadlock", p.id))
                changed = True
        return changed

    def run(self):
        while self.round < self.cfg["max_rounds"]:
            self.round += 1
            self.log(f"tour {self.round}")
            before = (len(self.points), json.dumps({k: sorted(v.items()) for k, v in self.stances.items()}, default=str))
            for agent in list(self.active):
                self.turn(agent)
            closed = self.close_round()
            self._publish_state()
            after = (len(self.points), json.dumps({k: sorted(v.items()) for k, v in self.stances.items()}, default=str))
            if len(self.active) < self.cfg["quorum"]:
                self.stop_reason = "not_enough_active_participants"
                break
            if self.points and not any(p.status == "open" for p in self.points.values()):
                self.stop_reason = "all_points_closed"
                break
            if before == after and not closed and self.round > 1:
                self.stop_reason = "no_change_in_a_full_round"
                break
        else:
            self.stop_reason = "max_rounds"
        for p in self.points.values():
            if p.status == "open":
                p.status, p.reason = "deadlock", self.stop_reason
                self.events.append((self.round, "deadlock", p.id))
        self._publish_state()
        return self.summary()

    # ------------------------------------------------------------ execution
    def state(self):
        return {"topic": self.topic, "round": self.round, "stop_reason": self.stop_reason,
                "active": self.active, "quorum": self.cfg["quorum"], "min_confidence": self.cfg["min_confidence"],
                "points": [{"id": p.id, "kind": p.kind, "text": p.describe(), "status": p.status, "reason": p.reason,
                            "proposer": p.proposer, "amends": p.amends,
                            "stances": {a: {"stance": s, "sigma": g} for a, (s, g, _) in sorted(self.stances[p.id].items())}}
                           for p in self.points.values()]}

    def _publish_state(self):
        """L'etat, signe par le moderateur et inscrit dans l'audit (arbre)."""
        try:
            self.moderator.send_tree("server", self.state(), tree_id=f"delib_{self.topic}")
        except Exception as e:  # noqa: BLE001 -- l'etat reste en memoire ; on le signale
            self.events.append((self.round, "state_not_published", str(e)[:100]))

    def compile(self):
        """Points d'action en consensus -> regles signees (publish_policy).
        Deux modalites differentes adoptees pour la meme action et le meme
        acteur : rien n'est compile pour cette action, elle est escaladee."""
        actions = [p for p in self.points.values() if p.kind == "action" and p.status in ("consensus", "resolved_by_human")]
        groups = {}
        for p in actions:
            groups.setdefault((p.relation["subject"], p.relation["object"]), []).append(p)
        rules, conflicts = [], []
        for (subj, obj), ps in groups.items():
            if len({p.relation["modality"] for p in ps}) > 1:
                conflicts.append((subj, obj, [p.id for p in ps]))
                continue
            p = ps[0]
            how = "consensus" if p.status == "consensus" else "human_decision"
            rules.append({"type": "PERFORM", "subject": subj, "object": obj, "modality": p.relation["modality"],
                          "reason": f"{how} deliberation {self.topic} {p.id}"})
        receipt = None
        if rules:
            if not self.server_key:
                raise RuntimeError("server_key requis pour verifier le recu de la politique compilee")
            receipt = self.moderator.publish_policy(rules, self.server_key)
        self.compiled = rules
        for subj, obj, ids in conflicts:
            self.events.append((self.round, "compile_conflict", f"{subj} {obj} {','.join(ids)}"))
        return {"rules": rules, "receipt": receipt, "conflicts": conflicts}

    def escalate(self):
        """Chaque point d'action en DEADLOCK -> demande d'accord humain
        (action deliberate.<sujet>.<point>). Renvoie les approval_id."""
        out = []
        for p in self.points.values():
            if p.kind != "action" or p.status != "deadlock":
                continue
            action = f"deliberate.{self.topic}.{p.id}"
            ok, resp = self.moderator.request_action(action)
            out.append({"point": p.id, "action": action, "approval_id": resp.fields.get("approval_id"),
                        "reason": resp.fields.get("reason"), "allowed_without_human": ok})
        self.escalations = out
        return out

    def resolve_escalations(self, wait: float = 0, poll: float = 2.0):
        """Applique les decisions humaines : un point approuve devient
        resolved_by_human (compile ensuite), un point refuse rejected_by_human."""
        for e in self.escalations:
            p = self.points[e["point"]]
            if p.status != "deadlock" or not e.get("approval_id"):
                continue
            deadline = time.time() + wait
            while True:
                ok, resp = self.moderator.request_action(e["action"], approval_id=e["approval_id"])
                reason = resp.fields.get("reason")
                if ok:
                    p.status, p.reason = "resolved_by_human", f"approved_by {resp.fields.get('approved_by', '?')}"
                    break
                if reason == "approval_rejected":
                    p.status, p.reason = "rejected_by_human", "approval_rejected"
                    break
                if reason != "approval_pending" or time.time() >= deadline:
                    break
                time.sleep(poll)
        return self.state()

    def summary(self):
        by = {}
        for p in self.points.values():
            by.setdefault(p.status, []).append(p.id)
        prompt_t = sum(a["prompt_tokens"] for t in self.turns for a in t["attempts"])
        reply_t = sum(a["reply_tokens"] for t in self.turns for a in t["attempts"])
        return {"topic": self.topic, "rounds": self.round, "stop_reason": self.stop_reason, "status": by,
                "turns": len(self.turns), "failed_turns": sum(1 for e in self.events if e[1] == "failed_turn"),
                "retries": sum(len(t["attempts"]) - 1 for t in self.turns),
                "excluded": [e[2] for e in self.events if e[1] == "excluded"],
                "tokens": {"prompts": prompt_t, "replies": reply_t,
                           "full_history_baseline": self.full_history_baseline()}}

    def full_history_baseline(self):
        """Ce qu'auraient coute les memes tours si chaque modele relisait tout
        l'historique (toutes les reponses precedentes, dont les regles) a
        chaque appel, comme dans le test manuel : base de comparaison."""
        total, history = 0, ""
        for t in self.turns:
            for a in t["attempts"]:
                total += estimate_tokens(self.system_prompt(t["agent"]) + history)
                if a.get("ok"):
                    history += "\n" + a["reply"]
        return total


def _clean(text):
    return re.sub(r'["\[\]\n\r\\]', " ", str(text or ""))[:200].strip()


# ------------------------------------------------------------------- CLI
def verify_state(state, me=None, sent=None):
    """Verification INDEPENDANTE de l'etat publie par le moderateur
    (2026-11-27). N'importe qui le relit (`fetch_tree("delib_<topic>")`,
    signe et audite) et recalcule chaque fermeture a partir des positions
    publiees, avec les regles publiees (quorum, min_confidence) :
      - un CONSENSUS doit avoir au moins `quorum` participants actifs, qui
        soutiennent tous le point avec sigma >= min_confidence ;
      - un point ouvert ou en impasse ne doit PAS remplir cette condition
        (sinon le moderateur a etouffe un consensus).
    Un participant (`me`) donne aussi les positions qu'il a lui-meme
    envoyees (`sent` = {point: (stance, sigma)}) : elles doivent figurer
    telles quelles dans l'etat. Renvoie la liste des anomalies ([] = conforme).

    Limite : l'impasse par stagnation depend de l'historique des tours, qui
    n'est pas publie ; seule sa compatibilite avec les positions finales est
    verifiee. Les positions des AUTRES participants sont celles que publie
    le moderateur : chacun verifie les siennes."""
    issues = []
    quorum, min_conf = state.get("quorum"), state.get("min_confidence")
    if not isinstance(quorum, int) or not isinstance(min_conf, (int, float)):
        return ["etat sans quorum ni min_confidence : regles de fermeture non publiees"]
    active = list(state.get("active") or [])
    for p in state.get("points", []):
        st = p.get("stances") or {}
        unanimous = len(active) >= quorum and all(
            (st.get(a) or {}).get("stance") == "supports" and float((st.get(a) or {}).get("sigma", 0)) >= min_conf
            for a in active)
        if p.get("status") == "consensus" and not unanimous:
            issues.append(f"{p['id']} : marque consensus sans unanimite de {len(active)} actifs (quorum {quorum})")
        if p.get("status") in ("open", "deadlock") and unanimous:
            issues.append(f"{p['id']} : unanimite atteinte mais marque {p['status']}")
    if me is not None:
        points = {p["id"]: p for p in state.get("points", [])}
        for pid, (stance, sigma) in (sent or {}).items():
            got = ((points.get(pid) or {}).get("stances") or {}).get(me)
            if got is None:
                issues.append(f"{pid} : position de {me} absente de l'etat")
            elif got.get("stance") != stance or abs(float(got.get("sigma", -1)) - float(sigma)) > 1e-9:
                issues.append(f"{pid} : position de {me} publiee {got}, envoyee {(stance, sigma)}")
    return issues


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description="Deliberation CSTL v6 entre modeles (docs/CSTL_DELIBERATION_v6.md).")
    ap.add_argument("--topic", required=True, help="identifiant du sujet")
    ap.add_argument("--question", required=True, help="la question, en identifiant (minuscules_soulignes)")
    ap.add_argument("--model", action="append", required=True,
                    help="nom=manual ou nom=openrouter:<modele>, une fois par participant")
    ap.add_argument("--host", default=os.environ.get("CSTL_HOST", "127.0.0.1"))
    ap.add_argument("--port", type=int, default=int(os.environ.get("CSTL_PORT", "5050")))
    ap.add_argument("--server-key", help="cle publique du serveur (fichier .pub ou hex), pour compiler")
    ap.add_argument("--max-rounds", type=int, default=6)
    ap.add_argument("--quorum", type=int, help="participants actifs requis pour un consensus "
                    "(defaut : la majorite, au moins 2)")
    ap.add_argument("--out", default="deliberation.json")
    args = ap.parse_args(argv)
    models, names = {}, []
    for spec in args.model:
        name, _, kind = spec.partition("=")
        names.append(name)
        models[name] = ManualModel(name) if kind == "manual" else OpenRouterModel(kind.split(":", 1)[1])
    key = args.server_key
    if key and Path(key).is_file():
        key = Path(key).read_text().strip()
    d = Deliberation(args.topic, args.question, names, models, host=args.host, port=args.port, server_key=key,
                     max_rounds=args.max_rounds, quorum=args.quorum, log=lambda s: print(s, file=sys.stderr))
    summary = d.run()
    result = {"summary": summary, "state": d.state()}
    if key:
        result["compiled"] = d.compile()
        result["escalations"] = d.escalate()
    Path(args.out).write_text(json.dumps({**result, "turns": d.turns}, indent=1, ensure_ascii=False, default=str))
    print(json.dumps(result, indent=1, ensure_ascii=False, default=str))


def cli():
    main()


if __name__ == "__main__":
    cli()
