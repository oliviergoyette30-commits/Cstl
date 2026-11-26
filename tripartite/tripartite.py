#!/usr/bin/env python3
"""
tripartite/tripartite.py -- test MANUEL d'une conversation a trois (ChatGPT,
Gemini, Claude) en CSTL, recopiee a la main d'un chatbot a l'autre
(2026-11-26). Guide : tripartite/GUIDE.md.

  python3 tripartite/tripartite.py kit
      ecrit les messages prets a coller : amorce_chatgpt.md, amorce_gemini.md,
      amorce_claude.md, 1_sujet.cstl, 2_decision.cstl (a partir de regles.cstl).

  python3 tripartite/tripartite.py check transcript.txt [--port 5050] [--out rapport.md]
      verifie chaque message du transcript (les reponses collees les unes apres
      les autres, blocs de code acceptes) :
        - forme : #!CSTL en tete, ---END--- a la fin, aucune prose ;
        - regles recopiees, sender et receiver attendus (ordre de parole) ;
        - sigma sur chaque relation, entre 0 et 1 ; operateurs officiels ;
        - au moins 3 relations nouvelles ; blocs d'accord / desaccord ;
        - VERDICT DU VRAI SERVEUR CSTL (demarre en mode developpement sur un
          port libre, ou celui donne par --port), avertissements compris ;
      puis les desaccords deontiques entre participants (l'un dit MUST,
      l'autre MUST_NOT sur la meme action) et un score par modele.
"""
import argparse
import os
import re
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
ORDER = ["chatgpt", "gemini", "claude"]
OFFICIAL = {
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS", "INTENT", "MAINTAIN", "TRANSFORM", "RESIST",
    "AMP", "INH", "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER", "COMMAND", "ASK", "STATE",
    "PERFORM", "RECOMMEND", "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES", "COMPARES", "ENTAILS",
    "CONTRADICTS", "KNOWS", "BELIEVES", "ASSUMES", "DOUBTS", "DISBELIEVES", "BEFORE", "AFTER", "DURING",
    "EITHER_OR", "REACTS", "INVOLVES", "SATISFIES", "MUTUAL"}
DISSENT = {"STRENGTH", "GAP", "DISPUTE", "PARTIAL_DISPUTE", "CONCERN", "CAUTION", "ALTERNATIVE", "REJECT",
           "VETO", "SELF_CRITIQUE", "AGREEMENT", "RECOMMEND"}


def rules():
    return (HERE / "regles.cstl").read_text().strip().splitlines()


# ------------------------------------------------------------------ kit
TEMPLATE = """Tu participes a un test : une conversation a trois entre ChatGPT, Gemini et Claude, ecrite en CSTL, un format structure pour la communication entre IA. Un humain recopie les messages d'un chatbot a l'autre, et un serveur CSTL les verifie ensuite.

Ton nom dans la conversation : {nom}
Ordre de parole : chatgpt, puis gemini, puis claude, puis de nouveau chatgpt. Tu ecris donc a : {suivant}
Le moderateur (moderateur) ouvre le debat et demande la decision finale ; au tour de decision, tu ecris au moderateur.

A partir de maintenant, CHACUNE de tes reponses suit ces regles, sans exception :

```
{regles}
```

Forme d'un message :

```
#!CSTL v5.0.0 MODE=A
<les {n} lignes (RULE) ci-dessus, recopiees telles quelles>
META [encoder={nom}, produced_by={nom}, turn=<numero du tour>]
INTENT_PAYLOAD [purpose=<propose|critique|answer|decide>, sender={nom}, receiver={suivant}]
RELATION [type=<OPERATEUR>, subject=<sujet>, object=<objet>, sigma=<0 a 1>]
RELATION [type=PERFORM, subject=<qui>, object=<action>, modality=<MUST|MUST_NOT|MAY>, sigma=<0 a 1>]
AGREEMENT_BLOCK [
STRENGTH <point_d_accord> [sigma=<0 a 1>]
]
DISAGREEMENT_BLOCK [
CONCERN <point_de_desaccord> [sigma=<0 a 1>]
]
DECISION: <decision> [sigma=<0 a 1>]
---END---
```
(La ligne DECISION seulement au tour de decision.)

Details :
- Identifiants : minuscules_avec_soulignes, sans accents ni espaces. Une valeur avec des espaces ou des virgules va entre guillemets : reason="texte libre".
- sigma = ta confiance, entre 0 et 1, sur CHAQUE relation.
- Operateurs officiels (en majuscules) :
  - affirmer : STATE, KNOWS (sigma au moins 0.8), BELIEVES, ASSUMES, DOUBTS (sigma au plus 0.5), DISBELIEVES
  - agir, proposer : PERFORM (avec modality=MUST, MUST_NOT ou MAY), RECOMMEND, ASK, COMMAND
  - logique : ENTAILS (implique), CONTRADICTS, EITHER_OR
  - liens : EQUALS, POSSESSES, RESEMBLES, OPPOSES, COMPARES, INVOLVES, SATISFIES
  - causes et dynamiques : ARR (cause), AMP (amplifie), INH (freine), PRESSURE, CATALYZE
  - temps : BEFORE, AFTER, DURING
  - en minuscules, tu peux creer ton propre predicat (agrees_with, depends_on).
- Types des blocs d'accord et de desaccord : STRENGTH, AGREEMENT, RECOMMEND, GAP, DISPUTE, PARTIAL_DISPUTE, CONCERN, CAUTION, ALTERNATIVE, REJECT, VETO, SELF_CRITIQUE.
- Jamais MUST et MUST_NOT sur la meme action pour le meme sujet dans un message.
- A chaque tour, tu recois les derniers messages des autres participants. Reponds a leurs relations (accord, desaccord, nuance) et ajoute au moins 3 relations nouvelles.

Exemple de message valide :

```
#!CSTL v5.0.0 MODE=A
{regles}
META [encoder=exemple, produced_by=exemple, turn=2]
INTENT_PAYLOAD [purpose=critique, sender=exemple, receiver=autre]
RELATION [type=BELIEVES, subject=exemple, object=pilot_reduces_delays, sigma=0.7]
RELATION [type=PERFORM, subject=ai_agent, object=order_routine_blood_test, modality=MAY, sigma=0.75, reason="with daily human review"]
RELATION [type=DOUBTS, subject=exemple, object=liability_is_clear, sigma=0.3]
AGREEMENT_BLOCK [
STRENGTH faster_results [sigma=0.8]
]
DISAGREEMENT_BLOCK [
CONCERN alert_fatigue [sigma=0.6]
]
---END---
```

Pour confirmer, reponds uniquement par ce message, dans un bloc de code :

```
#!CSTL v5.0.0 MODE=A
{regles}
META [encoder={nom}, produced_by={nom}, turn=0]
INTENT_PAYLOAD [purpose=ready, sender={nom}, receiver=moderateur]
RELATION [type=STATE, subject={nom}, object=ready_for_cstl_debate, sigma=1.0]
---END---
```
"""

SUJET = """#!CSTL v5.0.0 MODE=A
{regles}
META [encoder=moderateur, produced_by=moderateur, turn=1, topic=ai_agent_orders_lab_tests]
INTENT_PAYLOAD [purpose=open_debate, sender=moderateur, receiver=chatgpt, context=tripartite_debate]
RELATION [type=ASK, subject=moderateur, object=should_hospital_let_ai_agent_order_lab_tests_alone, sigma=1.0]
RELATION [type=STATE, subject=debate, object=three_rounds_then_decision, sigma=1.0]
RELATION [type=STATE, subject=speaking_order, object="chatgpt then gemini then claude", sigma=1.0]
RELATION [type=PERFORM, subject=each_participant, object=propose_conditions_and_limits, modality=MUST, sigma=1.0]
RELATION [type=PERFORM, subject=each_participant, object=answer_the_other_two, modality=MUST, sigma=1.0]
---END---
"""

DECISION = """#!CSTL v5.0.0 MODE=A
{regles}
META [encoder=moderateur, produced_by=moderateur, turn=4]
INTENT_PAYLOAD [purpose=request_decision, sender=moderateur, receiver=all]
RELATION [type=ASK, subject=moderateur, object=final_decision_with_conditions, sigma=1.0]
RELATION [type=PERFORM, subject=each_participant, object=write_one_decision_line, modality=MUST, sigma=1.0]
RELATION [type=PERFORM, subject=each_participant, object=send_to_moderateur, modality=MUST, sigma=1.0]
---END---
"""


def kit():
    r = "\n".join(rules())
    for i, nom in enumerate(ORDER):
        suivant = ORDER[(i + 1) % 3]
        (HERE / f"amorce_{nom}.md").write_text(TEMPLATE.format(nom=nom, suivant=suivant, regles=r, n=len(rules())))
    (HERE / "1_sujet.cstl").write_text(SUJET.format(regles=r))
    (HERE / "2_decision.cstl").write_text(DECISION.format(regles=r))
    print("ecrit : amorce_chatgpt.md, amorce_gemini.md, amorce_claude.md, 1_sujet.cstl, 2_decision.cstl")


# ---------------------------------------------------------------- check
FENCE = re.compile(r"^\s*```")
BLOCK = re.compile(r"^([A-Z_]+) \[(.*)\]\s*$")
ATTR = re.compile(r'(\w[\w.]*)=("(?:[^"\\]|\\.)*"|[^,\]]*)')


SEPARATOR = re.compile(r"^\s*={5,}\s*$")


def split_messages(text):
    """Les reponses sont separees par une ligne de « ===== » : tout ce qui est
    dans un morceau (prose comprise) appartient a cette reponse. Sans
    separateur, chaque message commence a une ligne #!CSTL. Un morceau sans
    #!CSTL est une reponse entierement en prose."""
    lines = text.splitlines()
    if any(SEPARATOR.match(ln) for ln in lines):
        msgs, chunk = [], []
        for ln in lines + ["====="]:
            if SEPARATOR.match(ln):
                if any(x.strip() for x in chunk):
                    head = next((i for i, x in enumerate(chunk) if x.strip().startswith("#!CSTL")), None)
                    if head is None:
                        msgs.append({"lines": ["(aucune ligne #!CSTL)"], "before": chunk})
                    else:
                        msgs.append({"lines": [chunk[head].strip()] + chunk[head + 1:], "before": chunk[:head]})
                chunk = []
            else:
                chunk.append(ln)
        return msgs
    msgs, cur, before = [], None, []
    for line in lines:
        if line.strip().startswith("#!CSTL"):
            if cur is not None:
                msgs.append(cur)
            cur = {"lines": [line.strip()], "before": before}
            before = []
        elif cur is None:
            before.append(line)
        else:
            cur["lines"].append(line)
    if cur is not None:
        msgs.append(cur)
    return msgs


def attrs(inner):
    return {k: v.strip().strip('"') for k, v in ATTR.findall(inner)}


def analyse(msg, expected_rules):
    """Champs, relations et problemes de forme d'un message."""
    lines = [ln.rstrip() for ln in msg["lines"]]
    # Fin : ---END---, puis eventuellement une cloture ``` et de la prose.
    end = next((i for i, ln in enumerate(lines) if ln.strip() == "---END---"), None)
    body = lines[1:end] if end is not None else lines[1:]
    after = [ln for ln in (lines[end + 1:] if end is not None else []) if ln.strip() and not FENCE.match(ln)]
    out = {"meta": {}, "intent": {}, "relations": [], "rules": [], "prose": [], "agree": 0, "disagree": 0,
           "decision": None, "has_end": end is not None, "text": "\n".join(lines[:end + 1] if end is not None else lines)}
    in_block = None
    for ln in body:
        s = ln.strip()
        if not s or FENCE.match(s):
            continue
        if in_block:
            if s == "]":
                in_block = None
                continue
            if in_block == "RELATIONS":
                m = re.match(r"^\((\S+)\)\s+(\S+)\s+(\S+)(?:\s+\[(.*)\])?$", s)
                if m:
                    out["relations"].append({"type": m.group(2), "subject": m.group(1), "object": m.group(3),
                                             **attrs(m.group(4) or "")})
                else:
                    out["prose"].append(s)
            else:
                word = s.split()[0]
                if word in DISSENT:
                    out["agree" if in_block == "AGREEMENT_BLOCK" else "disagree"] += 1
                else:
                    out["prose"].append(s)
            continue
        if s.startswith("(RULE)"):
            out["rules"].append(s)
        elif s in ("RELATIONS [", "AGREEMENT_BLOCK [", "DISAGREEMENT_BLOCK [", "CONSTRAINTS [", "UNCERTAINTY ["):
            in_block = s[:-2]
        elif s.startswith("DECISION"):
            out["decision"] = s
        elif BLOCK.match(s):
            name, inner = BLOCK.match(s).groups()
            a = attrs(inner)
            if name == "META":
                out["meta"] = a
            elif name == "INTENT_PAYLOAD":
                out["intent"] = a
            elif name == "RELATION":
                out["relations"].append(a)
        else:
            out["prose"].append(s)
    out["prose"] += [ln.strip() for ln in msg["before"] if ln.strip() and not FENCE.match(ln)] + after
    out["missing_rules"] = [r for r in expected_rules if r not in out["rules"]]
    return out


def server(port=None):
    if port:
        return None, port
    binary = REPO / "target" / "debug" / "cstl_parser"
    if not binary.exists():
        sys.exit("cargo build --bin cstl_parser d'abord (ou --port d'un serveur en CSTL_DEV_MODE=1)")
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    env = {**os.environ, "CSTL_DEV_MODE": "1", "CSTL_PORT": str(port), "CSTL_KB_VERIFY": "off",
           "CSTL_REST_PORT": str(port + 1 if port < 65535 else port - 1)}
    proc = subprocess.Popen([str(binary)], cwd=tempfile.mkdtemp(prefix="cstl_tripartite_"), env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            return proc, port
        except OSError:
            time.sleep(0.25)
    proc.terminate()
    sys.exit("le serveur CSTL n'a pas demarre")


def send(port, text):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            s.sendall((text.rstrip("\n") + "\n").encode())
            buf = b""
            while b"---END---" not in buf:
                chunk = s.recv(65536)
                if not chunk:
                    break
                buf += chunk
        return buf.decode(errors="replace")
    except OSError as e:
        return f"INTENT_PAYLOAD [purpose=unreachable, detail={e}]"


def verdict(raw):
    intent = next((ln for ln in raw.splitlines() if ln.startswith("INTENT_PAYLOAD")), "")
    purpose = re.search(r"purpose=([^,\]]+)", intent)
    trace = re.search(r"verdict=(\w+)", raw)
    warnings = re.findall(r"SEMANTIC_WARNING \[detail=([^\]]+)\]", raw)
    detail = re.search(r"(?:detail|errors)=([^\]]+)", intent)
    p = purpose.group(1) if purpose else "?"
    ok = p == "acknowledgement" and (trace is None or trace.group(1) == "PASS")
    return ok, p, (detail.group(1) if detail else ""), warnings


def check(path, port=None, out=None, start="chatgpt"):
    text = Path(path).read_text()
    expected = rules()
    msgs = [m for m in split_messages(text)]
    proc, port = server(port)
    rows, per_model, deontic = [], {}, {}
    # Ordre de parole a partir de `start` ; un message renvoye apres
    # correction (meme expediteur que le precedent) ne fait pas avancer le tour.
    speakers = ORDER[ORDER.index(start):] + ORDER[:ORDER.index(start)]
    turn, previous = 0, None
    try:
        for i, m in enumerate(msgs, 1):
            a = analyse(m, expected)
            sender = a["intent"].get("sender", "?")
            receiver = a["intent"].get("receiver", "?")
            if sender == "moderateur":
                continue
            retry = previous is not None and sender == previous
            if retry:
                turn -= 1
            exp_sender = speakers[turn % 3]
            exp_receivers = {speakers[(turn + 1) % 3], "moderateur", "all"}
            turn += 1
            previous = sender
            ok_srv, purpose, detail, warns = verdict(send(port, a["text"]))
            is_decision = receiver == "moderateur" or a["intent"].get("purpose") in ("decide", "decision")
            rels = a["relations"]
            sigmas = [r.get("sigma") for r in rels]
            bad_sigma = [s for s in sigmas if s is None or not re.fullmatch(r"(0(\.\d+)?|1(\.0+)?)", s or "")]
            bad_ops = [r.get("type") for r in rels if r.get("type", "") and r["type"] != r["type"].lower()
                       and r["type"] not in OFFICIAL]
            checks = {
                "debut #!CSTL": a["text"].startswith("#!CSTL v5.0.0"),
                "fin ---END---": a["has_end"],
                "aucune prose": not a["prose"],
                "regles recopiees": not a["missing_rules"],
                "sender attendu": sender == exp_sender,
                "receiver attendu": receiver in exp_receivers,
                "sigma 0-1 partout": bool(rels) and not bad_sigma,
                "operateurs officiels": not bad_ops,
                ">= 3 relations": len(rels) >= 3 or a["intent"].get("purpose") == "ready",
                "accord/desaccord": (a["agree"] + a["disagree"]) > 0 or a["intent"].get("purpose") == "ready",
                # Au tour de decision : une ligne DECISION: avec un identifiant
                # (pas une phrase), comme le demande l'amorce.
                "decision (si demandee)": not is_decision or bool(
                    a["decision"] and re.fullmatch(r"DECISION\s*[:=]\s*[a-z0-9_.]+(\s*\[.*\])?", a["decision"].strip())),
                "serveur PASS": ok_srv,
            }
            notes = []
            if a["prose"]:
                notes.append("prose : " + " | ".join(a["prose"][:2])[:160])
            if a["missing_rules"]:
                notes.append(f"{len(a['missing_rules'])} regle(s) manquante(s)")
            if bad_sigma:
                notes.append(f"sigma absents ou hors 0-1 : {len(bad_sigma)}")
            if bad_ops:
                notes.append("operateurs inconnus : " + ", ".join(bad_ops))
            if not ok_srv:
                notes.append(f"serveur : {purpose} {detail}"[:200])
            notes += [f"avertissement : {w}"[:160] for w in warns]
            if is_decision and not a["decision"]:
                notes.append("aucune ligne DECISION: au tour de decision")
            elif is_decision and not checks["decision (si demandee)"]:
                notes.append("DECISION ecrite en phrase, pas en identifiant")
            if sender != exp_sender:
                notes.append(f"sender {sender}, attendu {exp_sender}")
            for r in rels:
                if r.get("type") == "PERFORM" and r.get("modality") in ("MUST", "MUST_NOT", "MAY"):
                    deontic.setdefault((r.get("subject"), r.get("object")), {}).setdefault(r["modality"], set()).add(sender)
            rows.append((i, sender, a["intent"].get("purpose", "?"), len(rels), checks, notes, a["decision"]))
            pm = per_model.setdefault(sender, [0, 0])
            pm[0] += sum(checks.values())
            pm[1] += len(checks)
    finally:
        if proc:
            proc.terminate()
            proc.wait(timeout=10)

    lines = ["# Test tripartite CSTL : rapport", "", f"Transcript : `{path}` ; {len(rows)} message(s) de modeles.", ""]
    names = list(rows[0][4].keys()) if rows else []
    lines.append("| # | modele | purpose | relations | " + " | ".join(names) + " |")
    lines.append("|---" * (4 + len(names)) + "|")
    for i, sender, purpose, n, checks, _, _ in rows:
        lines.append(f"| {i} | {sender} | {purpose} | {n} | " + " | ".join("ok" if v else "**NON**" for v in checks.values()) + " |")
    lines += ["", "## Score par modele", ""]
    for model, (good, total) in per_model.items():
        lines.append(f"- **{model}** : {good}/{total} verifications reussies ({100 * good // max(total, 1)} %)")
    lines += ["", "## Details", ""]
    for i, sender, _, _, _, notes, dec in rows:
        if notes or dec:
            lines.append(f"- message {i} ({sender}) : " + " ; ".join(notes + ([dec] if dec else [])))
    conflicts = [(k, v) for k, v in deontic.items() if len(v) > 1]
    lines += ["", "## Desaccords deontiques entre participants", ""]
    if not conflicts:
        lines.append("Aucun : personne n'a ecrit une modalite opposee a celle d'un autre sur la meme action.")
    for (subj, obj), mods in conflicts:
        lines.append(f"- `{subj}` / `{obj}` : " + " ; ".join(f"{mod} par {', '.join(sorted(who))}" for mod, who in mods.items()))
    report = "\n".join(lines) + "\n"
    if out:
        Path(out).write_text(report)
    print(report)
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("kit")
    c = sub.add_parser("check")
    c.add_argument("transcript")
    c.add_argument("--port", type=int)
    c.add_argument("--out")
    c.add_argument("--start", default="chatgpt", choices=ORDER, help="premier modele a parler dans le transcript")
    args = ap.parse_args(argv)
    if args.cmd == "kit":
        kit()
    else:
        check(args.transcript, args.port, args.out, args.start)


if __name__ == "__main__":
    main()
