#!/usr/bin/env python3
"""
cstl_essai_tout_en_un.py -- essayer l'arbre indente CSTL avec N'IMPORTE QUEL
modele (ChatGPT, Claude.ai, Gemini, Mistral, modele local), par copier-coller.
Un seul fichier, Python 3.8+, aucune installation.

FICHIER GENERE par scripts/make_standalone_try.py (depot CSTL) : ne pas
modifier a la main. Les decodeurs inclus sont les vrais (cstl_compact.py,
toon_lite.py) : rien n'est devine.

1. Ecrire toutes les consignes d'un coup dans un dossier :
     python3 cstl_essai_tout_en_un.py consignes essais/
   -> un fichier par test ; coller chacun dans une NOUVELLE conversation.
2. Enregistrer chaque reponse du modele dans le meme dossier, sous le meme
   nom avec .reponse.txt a la place de .txt
   (ex. ecriture_cstl_indent.reponse.txt).
3. Tout verifier d'un coup :
     python3 cstl_essai_tout_en_un.py verifier essais/

Tests :
  ecriture_<format>  le modele convertit un JSON imbrique (24 contrats) dans
                     le format ; verifie : relu ? identique ? (strict, puis
                     tolerant) ; % de valeurs justes
  lecture_<format>   3 questions sur les memes donnees ; bonnes reponses
Formats : cstl_indent (comptes en tete), cstl_indent_fin (comptes a la fin,
@end), toon, json (reference).
Option : --kind commandes|organisation|contrats --size 8|24 --seed 1|2
(les memes pour consignes et verifier).
"""
import argparse
import json
import random
import re
import sys
import types
import zlib
from pathlib import Path


def _module(name, src):
    mod = types.ModuleType(name)
    exec(compile(src, f"<{name}>", "exec"), mod.__dict__)
    return mod


cstl_compact = _module('cstl_compact', r'''"""
cstl_compact.py -- CSTL compact : n'ecrire que l'imprevisible (2026-11-21).

Une relation par ligne, sans noms de champs pour ce qui est previsible. Un
programme reconstruit les relations completes, que le SDK signe et envoie
au serveur comme d'habitude. Mesure (scripts/format_short_benchmark.py) :
2,5 fois moins de tokens que le CSTL complet ou le JSON compact, sans perte
de comprehension.

GRAMMAIRE (une ligne = une relation ; « # » commence un commentaire) :
  ROLE <agent> <role>              POSSESSES subject=<agent> object=role:<role>
  FACT <fait> true|false           STATE subject=<fait> object=true|false
  <subject> <MODALITE> <action> [conditions] ["raison"]
                                   PERFORM, MODALITE = MUST, MUST_NOT, MAY,
                                   REQUIRE, FORBID ou PERMIT
  REL <type> <subject> <object> [cle=valeur ...]   toute autre relation

MODE TABLEAU (`encode(..., table=True)`, 2026-11-21) : une section par type
d'element, avec un en-tete qui nomme les colonnes et annonce le nombre de
lignes ; les lignes de la section sont indentees, « - » marque une case vide.
  ROLES[2] agent role
    agent_a finance
    agent_b ops
  FACTS[1] fait valeur
    audit_open true
  RULES[2] qui modalite action au_dela si sauf_si
    agent_a MUST_NOT wipe_db 100 - audit_open
    role:finance MUST log_transaction - - - "tracabilite"
Les autres relations (taches, commandes, evenements...) sont regroupees par
type dans une section generique, dont l'en-tete commence par « subject
object » puis nomme les autres colonnes (2026-11-22) :
  ORDER[2] subject object amount status
    o1 client_3 450 paid
    o2 client_1 80 -
Apres les 6 colonnes d'une regle viennent les autres conditions (prefixes
ci-dessous) et la raison. Un nombre [N] faux est une erreur : c'est ce qui
permet de reperer une ligne oubliee au lieu de l'accepter en silence.
Les sections en tableau s'inspirent des tableaux de TOON (Token-Oriented Object
Notation, github.com/toon-format/toon, licence MIT).
Pourquoi : dans scripts/compact_writing_benchmark.py, Haiku oubliait des
regles et confondait if: et unless: en mode ligne, alors qu'il ecrivait
bien TOON, dont les colonnes sont nommees une fois dans l'en-tete.

CONDITIONS d'une regle (dans n'importe quel ordre) :
  >X           amount_above=X        <=X          amount_at_most=X
  total>X      total_above=X         total<=X     total_at_most=X
  window=N     total_window_secs=N   if:F         if_fact=F
  unless:F     unless_fact=F         from:D       valid_from=D
  until:D      valid_until=D         cle=valeur   tout autre attribut
"""
import base64
import json
import re

MODALITIES = ("MUST", "MUST_NOT", "MAY", "REQUIRE", "FORBID", "PERMIT")
PREFIXES = [("total<=", "total_at_most"), ("total>", "total_above"), ("<=", "amount_at_most"), (">", "amount_above"),
            ("window=", "total_window_secs"), ("if:", "if_fact"), ("unless:", "unless_fact"),
            ("from:", "valid_from"), ("until:", "valid_until")]
SHORT = {attr: prefix for prefix, attr in PREFIXES}


class CompactError(ValueError):
    pass


# ---- typage (2026-11-22), memes types que le serveur (src/server/schema.rs) ----
TYPES = ("num", "int", "date", "bool", "str")
_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?)?$")


def check_value(ty, v):
    """La valeur (chaine) est-elle du type `ty` ?"""
    v = str(v).strip()
    if ty == "num":
        try:
            f = float(v)
            return f == f and f not in (float("inf"), float("-inf"))
        except ValueError:
            return False
    if ty == "int":
        return re.fullmatch(r"[+-]?\d+", v) is not None
    if ty == "date":
        if not _DATE.match(v):
            return False
        import datetime
        try:
            datetime.date.fromisoformat(v[:10])
            return True
        except ValueError:
            return False
    if ty == "bool":
        return v in ("true", "false", "1", "0")
    return ty == "str"


# Conditions de regle dont la valeur a un type impose (verifie au decodage,
# comme le fait le serveur : un montant « beaucoup » est refuse, pas devine).
CONDITION_TYPES = {"amount_above": "num", "amount_at_most": "num", "total_above": "num", "total_at_most": "num",
                   "total_window_secs": "int", "valid_from": "date", "valid_until": "date", "priority": "int"}


def _check_rule(r, n):
    for attr, ty in CONDITION_TYPES.items():
        if attr in r and not check_value(ty, r[attr]):
            raise CompactError(f"ligne {n} : {attr}={r[attr]!r} n'est pas de type {ty}")
    return r


def _check_typed(rels, types):
    """Refuse a l'encodage ce que le decodage refuserait : on n'ecrit jamais un
    message illisible."""
    for i, r in enumerate(rels, 1):
        if r.get("type") == "PERFORM":
            _check_rule(r, i)
        for col, ty in (types.get(r.get("type")) or {}).items():
            if col in r and r[col] != "" and not check_value(ty, r[col]):
                raise CompactError(f"relation {i} : {r['type']}.{col}={r[col]!r} n'est pas de type {ty}")


def _schema_types(rels):
    """Types declares par les relations SCHEMA du message : {type: {col: ty}}."""
    out = {}
    for r in rels:
        if r.get("type") == "SCHEMA" and r.get("subject"):
            out[r["subject"]] = {k: v for k, v in r.items() if k not in ("type", "subject", "object") and v in TYPES}
    return out


def _col(c, types):
    ty = (types or {}).get(c)
    return f"{c}:{ty}" if ty and ty != "str" else c


def _parse_cols(names, n):
    cols, types = [], {}
    for raw in names:
        name, sep, ty = raw.partition(":")
        if sep:
            if ty not in TYPES:
                raise CompactError(f"ligne {n} : type inconnu {ty!r} pour la colonne {name} (types : {', '.join(TYPES)})")
            types[name] = ty
        cols.append(name)
    return cols, types


def _quote(text):
    return '"' + str(text).replace("\\", "\\\\").replace('"', '\\"') + '"'


def _token(value):
    v = str(value)
    return v if v and not re.search(r'[\s"#]', v) else _quote(v)


def _rule_extras(r, skip):
    parts = []
    for k, v in r.items():
        if k in skip or k == "reason":
            continue
        parts.append(SHORT[k] + _token(v) if k in SHORT else f"{_key(k)}={_token(v)}")
    if "reason" in r:
        parts.append(_quote(r["reason"]))
    return parts


def _key(k):
    """Nom d'attribut ecrit en ligne (cle=valeur). Corrige le 2026-11-22 : un
    nom avec un espace n'etait pas mis entre guillemets, et la ligne devenait
    illisible (trouve par test_generic_random_round_trips)."""
    if "=" in k:
        raise CompactError(f"nom d'attribut avec « = » non representable : {k!r}")
    return _token(k)


def _is_role(r):
    return r.get("type") == "POSSESSES" and r.get("object", "").startswith("role:") and set(r) == {"type", "subject", "object"}


def _is_fact(r):
    return r.get("type") == "STATE" and r.get("object") in ("true", "false") and set(r) == {"type", "subject", "object"}


def _is_rule(r):
    return r.get("type") == "PERFORM" and r.get("modality") in MODALITIES and bool(r.get("subject")) and bool(r.get("object"))


def _line(r):
    if _is_role(r):
        return f"ROLE {_token(r['subject'])} {_token(r['object'][5:])}"
    if _is_fact(r):
        return f"FACT {_token(r['subject'])} {r['object']}"
    if _is_rule(r):
        return " ".join([_token(r["subject"]), r["modality"], _token(r["object"])]
                        + _rule_extras(r, ("type", "subject", "object", "modality")))
    rest = [f"{_key(k)}={_token(v)}" for k, v in r.items() if k not in ("type", "subject", "object")]
    return " ".join(["REL", _token(r.get("type", "")), _token(r.get("subject", "")), _token(r.get("object", ""))] + rest)


def _cell(value):
    return "-" if value in (None, "") else (_quote(value) if str(value) == "-" else _token(value))


def encode(relations, table=False, types=None):
    """Relations (dicts, comme pour CstlAgent.send_relations) -> texte compact.
    `table=True` : mode tableau (en-tetes de colonnes, voir la doc du module).
    `types` : {type de relation: {colonne: num|int|date|bool|str}}, ecrits dans
    les en-tetes (`amount:num`) et verifies au decodage ; les declarations
    SCHEMA presentes dans `relations` sont reprises automatiquement."""
    rels = [{k: str(v) for k, v in r.items()} for r in relations]
    types = {**_schema_types(rels), **(types or {})}
    _check_typed(rels, types)
    if not table:
        return "\n".join(_line(r) for r in rels)
    roles, facts, rules = [r for r in rels if _is_role(r)], [r for r in rels if _is_fact(r)], [r for r in rels if _is_rule(r)]
    others = [r for r in rels if not (_is_role(r) or _is_fact(r) or _is_rule(r))]
    lines = []
    if roles:
        lines.append(f"ROLES[{len(roles)}] agent role")
        lines += [f"  {_cell(r['subject'])} {_cell(r['object'][5:])}" for r in roles]
    if facts:
        lines.append(f"FACTS[{len(facts)}] fait valeur")
        lines += [f"  {_cell(r['subject'])} {r['object']}" for r in facts]
    if rules:
        lines.append(f"RULES[{len(rules)}] qui modalite action au_dela si sauf_si")
        for r in rules:
            cells = [_cell(r["subject"]), r["modality"], _cell(r["object"]), _cell(r.get("amount_above")),
                     _cell(r.get("if_fact")), _cell(r.get("unless_fact"))]
            extras = _rule_extras(r, ("type", "subject", "object", "modality", "amount_above", "if_fact", "unless_fact"))
            lines.append("  " + " ".join(cells + extras))
    # Section generique (2026-11-22) : les autres relations, regroupees par
    # type, en tableau. Les noms de colonnes ne s'ecrivent qu'une fois ; « - »
    # marque une case vide. Une relation sans subject ou object reste en ligne.
    groups = {}
    for r in others:
        if r.get("subject") is None or r.get("object") is None or not GENERIC_TYPE.match(r.get("type", "")) \
                or r.get("type") in ("ROLES", "FACTS", "RULES") \
                or not all(GENERIC_TYPE.match(k) for k in r if k not in ("type", "subject", "object")):
            lines.append(_line(r))
            continue
        groups.setdefault(r["type"], []).append(r)
    for t, rs in groups.items():
        cols = []
        for r in rs:
            cols += [k for k in r if k not in ("type", "subject", "object") and k not in cols]
        lines.append(f"{t}[{len(rs)}] subject object" + "".join(f" {_col(c, types.get(t))}" for c in cols))
        lines += ["  " + " ".join([_cell(r["subject"]) if r["subject"] != "" else '""',
                                   _cell(r["object"]) if r["object"] != "" else '""']
                                  + [_cell(r.get(c)) if r.get(c) != "" else '""' for c in cols]) for r in rs]
    return "\n".join(lines)


SYMBOLS = {"MUST": "*", "MUST_NOT": "!", "MAY": "?"}
SYMBOL_MODALITY = {v: k for k, v in SYMBOLS.items()}
HEADER_SYM = re.compile(r"^([A-Za-z_][A-Za-z0-9_.]*)\[(\d+)\]:((?:\s+\S+)*)\s*$")
PLAIN = re.compile(r"^[A-Za-z0-9_.:\-]+$")


def _sym_line(r):
    """Une relation de politique sans mot-cle, ou None si elle ne s'y prete pas."""
    if _is_role(r) and PLAIN.match(r["subject"]) and PLAIN.match(r["object"][5:]):
        return f"{r['subject']} @{r['object'][5:]}"
    if _is_fact(r) and PLAIN.match(r["subject"]):
        return f"{r['subject']}={'1' if r['object'] == 'true' else '0'}"
    if _is_rule(r) and r["modality"] in SYMBOLS and PLAIN.match(r["subject"]) and PLAIN.match(r["object"]) \
            and r["object"][:1] not in "*!?@" and "=" not in r["subject"]:
        parts = [r["subject"], SYMBOLS[r["modality"]] + r["object"]]
        if "amount_above" in r:
            parts.append(">" + _token(r["amount_above"]))
        for attr, sym in (("if_fact", "+"), ("unless_fact", "-")):
            if attr in r:
                if not PLAIN.match(r[attr]):
                    return None
                parts.append(sym + r[attr])
        return " ".join(parts + _rule_extras(r, ("type", "subject", "object", "modality", "amount_above", "if_fact",
                                                 "unless_fact")))
    return None


def encode_symbols(relations, types=None):
    """CSTL compact SANS MOT-CLE (2026-11-22). Politiques, une ligne chacune :
      agent_a @finance                         role
      kyc_done=1  /  kyc_done=0                fait vrai / faux
      agent_f !wipe_db >100 +audit_open -kyc   regle : * MUST, ! MUST_NOT, ? MAY ;
                                               >X au-dela de X, +F si F, -F sauf si F
    Autres relations, en tableau par type ; les 2 premieres valeurs d'une ligne
    sont subject et object, puis les colonnes annoncees :
      ORDER[2]: amount status
        o1 client_3 450 paid
        o2 client_1 80 -
    Ce qui ne s'y prete pas (noms avec espaces, autres modalites...) reste en
    CSTL compact ordinaire, que `decode` lit aussi."""
    rels = [{k: str(v) for k, v in r.items()} for r in relations]
    types = {**_schema_types(rels), **(types or {})}
    _check_typed(rels, types)
    lines, groups = [], {}
    for r in rels:
        sym = _sym_line(r) if (_is_role(r) or _is_fact(r) or _is_rule(r)) else None
        if sym is not None:
            lines.append(sym)
        elif not (_is_role(r) or _is_fact(r) or _is_rule(r)) and _generic_ok(r):
            groups.setdefault(r["type"], []).append(r)
        else:
            lines.append(_line(r))
    for t, rs in groups.items():
        cols = []
        for r in rs:
            cols += [k for k in r if k not in ("type", "subject", "object") and k not in cols]
        lines.append(f"{t}[{len(rs)}]:" + "".join(f" {_col(c, types.get(t))}" for c in cols))
        lines += ["  " + " ".join([_cell(r["subject"]) if r["subject"] != "" else '""',
                                   _cell(r["object"]) if r["object"] != "" else '""']
                                  + [_cell(r.get(c)) if r.get(c) != "" else '""' for c in cols]) for r in rs]
    return "\n".join(lines)


def _generic_ok(r):
    return (r.get("subject") is not None and r.get("object") is not None and GENERIC_TYPE.match(r.get("type", ""))
            and r["type"] not in ("ROLES", "FACTS", "RULES")
            and all(GENERIC_TYPE.match(k) for k in r if k not in ("type", "subject", "object")))


def _split(line):
    """Mots d'une ligne : [(texte, entierement_entre_guillemets)]. Un mot peut
    contenir une partie entre guillemets (if:"a b") ; « # » hors guillemets
    commence un commentaire."""
    toks, cur, quoted_whole, in_tok, i = [], [], False, False, 0
    while i < len(line):
        ch = line[i]
        if ch == '"':
            if not in_tok:
                in_tok, quoted_whole = True, True
            i += 1
            while i < len(line) and line[i] != '"':
                if line[i] == "\\" and i + 1 < len(line):
                    i += 1
                cur.append(line[i])
                i += 1
            if i >= len(line):
                raise CompactError(f"guillemets mal fermes : {line!r}")
            i += 1
            continue
        if ch.isspace() or ch == "#":
            if in_tok:
                toks.append(("".join(cur), quoted_whole))
                cur, quoted_whole, in_tok = [], False, False
            if ch == "#":
                break
            i += 1
            continue
        if not in_tok:
            in_tok, quoted_whole = True, False
        elif quoted_whole:
            quoted_whole = False   # "abc"def : pas une raison
        cur.append(ch)
        i += 1
    if in_tok:
        toks.append(("".join(cur), quoted_whole))
    return toks


SECTION = re.compile(r"^(ROLES|FACTS|RULES)(?:\[(\d+)\])?(?:\s.*)?$")
GENERIC_TYPE = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")
GENERIC = re.compile(r"^([A-Za-z_][A-Za-z0-9_.]*)\[(\d+)\]\s+subject\s+object((?:\s+\S+)*)\s*$")


def _empty(word):
    text, quoted = word
    return None if (text == "-" and not quoted) else text


def _rule_from(toks, words, n, line, base):
    r = dict(base)
    rest, rest_words = toks, words
    if rest and rest_words[-1][1] and len(rest) > 0:
        r["reason"] = rest[-1]
        rest = rest[:-1]
    for t in rest:
        for prefix, attr in PREFIXES:
            if t.startswith(prefix) and len(t) > len(prefix):
                r[attr] = t[len(prefix):]
                break
        else:
            k, sep, v = t.partition("=")
            if not sep or not k:
                raise CompactError(f"ligne {n} : condition inconnue : {t!r}")
            r[k] = v
    return r


def decode_report(text):
    """Comme `decode`, mais renvoie (relations, problemes) : un nombre [N]
    d'en-tete qui ne correspond pas aux lignes est signale au lieu de lever
    une erreur. Une ligne illisible leve toujours CompactError."""
    out, problems = [], []
    section = None   # [nom, nombre annonce ou None, lignes lues]

    def close():
        if section and section[1] is not None and section[1] != section[2]:
            name = section[4] if section[0] == "GENERIC" else section[0]
            problems.append(f"{name}[{section[1]}] annonce, {section[2]} ligne(s) ecrite(s)")

    for n, line in enumerate(text.splitlines(), 1):
        words = _split(line)
        if not words:
            continue
        toks = [w for w, _ in words]
        indented = line[:1].isspace()
        if section and indented:
            section[2] += 1
            kind = section[0]
            if kind == "ROLES":
                if len(toks) != 2:
                    raise CompactError(f"ligne {n} : ROLES attend <agent> <role>")
                out.append({"type": "POSSESSES", "subject": toks[0], "object": f"role:{toks[1]}"})
            elif kind == "FACTS":
                if len(toks) != 2 or toks[1] not in ("true", "false"):
                    raise CompactError(f"ligne {n} : FACTS attend <fait> true|false")
                out.append({"type": "STATE", "subject": toks[0], "object": toks[1]})
            elif kind == "GENERIC":
                cols = section[3]
                if len(words) != len(cols) + 2:
                    raise CompactError(f"ligne {n} : {section[4]} attend {len(cols) + 2} valeurs, {len(words)} lues : {line!r}")
                r = {"type": section[4], "subject": toks[0], "object": toks[1]}
                for c, w in zip(cols, words[2:]):
                    v = _empty(w)
                    if v is not None:
                        ty = section[5].get(c)
                        if ty and not check_value(ty, v):
                            raise CompactError(f"ligne {n} : {section[4]}.{c}={v!r} n'est pas de type {ty}")
                        r[c] = v
                out.append(r)
            else:
                if len(toks) < 6 or toks[1] not in MODALITIES:
                    raise CompactError(f"ligne {n} : RULES attend <qui> <modalite> <action> <au_dela> <si> <sauf_si> : {line!r}")
                r = {"type": "PERFORM", "subject": toks[0], "object": toks[2], "modality": toks[1]}
                for attr, w in zip(("amount_above", "if_fact", "unless_fact"), words[3:6]):
                    v = _empty(w)
                    if v is not None:
                        r[attr] = v
                out.append(_check_rule(_rule_from(toks[6:], words[6:], n, line, r), n))
            continue
        m = SECTION.match(line.strip()) if not indented else None
        if m:
            close()
            section = [m.group(1), int(m.group(2)) if m.group(2) else None, 0]
            continue
        hs = HEADER_SYM.match(line.strip()) if not indented else None
        if hs:
            close()
            cols, ctypes = _parse_cols(hs.group(3).split(), n)
            section = ["GENERIC", int(hs.group(2)), 0, cols, hs.group(1), ctypes]
            continue
        g = GENERIC.match(line.strip()) if not indented else None
        if g:
            close()
            cols, ctypes = _parse_cols([w for w, _ in _split(g.group(3))] if g.group(3).strip() else [], n)
            section = ["GENERIC", int(g.group(2)), 0, cols, g.group(1), ctypes]
            continue
        close()
        section = None
        head = toks[0]
        sym = _decode_symbol_line(toks, words, n, line)
        if sym is not None:
            out.append(sym)
        elif head == "ROLE":
            if len(toks) != 3:
                raise CompactError(f"ligne {n} : ROLE <agent> <role>")
            out.append({"type": "POSSESSES", "subject": toks[1], "object": f"role:{toks[2]}"})
        elif head == "FACT":
            if len(toks) != 3 or toks[2] not in ("true", "false"):
                raise CompactError(f"ligne {n} : FACT <fait> true|false")
            out.append({"type": "STATE", "subject": toks[1], "object": toks[2]})
        elif head == "REL":
            if len(toks) < 4:
                raise CompactError(f"ligne {n} : REL <type> <subject> <object> [cle=valeur ...]")
            r = {"type": toks[1], "subject": toks[2], "object": toks[3]}
            for t in toks[4:]:
                k, sep, v = t.partition("=")
                if not sep:
                    raise CompactError(f"ligne {n} : attribut sans « = » : {t!r}")
                r[k] = v
            out.append(r)
        else:
            if len(toks) < 3 or toks[1] not in MODALITIES:
                raise CompactError(f"ligne {n} : <subject> <MODALITE> <action> attendu : {line!r}")
            base = {"type": "PERFORM", "subject": toks[0], "object": toks[2], "modality": toks[1]}
            out.append(_check_rule(_rule_from(toks[3:], words[3:], n, line, base), n))
    close()
    return out, problems


def _decode_symbol_line(toks, words, n, line):
    """Ligne sans mot-cle (`encode_symbols`), ou None si ce n'en est pas une."""
    if len(toks) == 1 and not words[0][1]:
        k, sep, v = toks[0].partition("=")
        if sep and v in ("0", "1") and k:
            return {"type": "STATE", "subject": k, "object": "true" if v == "1" else "false"}
        return None
    if len(toks) == 2 and toks[1].startswith("@") and len(toks[1]) > 1 and not words[1][1]:
        return {"type": "POSSESSES", "subject": toks[0], "object": f"role:{toks[1][1:]}"}
    if len(toks) >= 2 and toks[1][:1] in SYMBOL_MODALITY and len(toks[1]) > 1 and not words[1][1]:
        r = {"type": "PERFORM", "subject": toks[0], "object": toks[1][1:], "modality": SYMBOL_MODALITY[toks[1][0]]}
        rest, rest_words = list(toks[2:]), list(words[2:])
        keep, keep_words = [], []
        for t, w in zip(rest, rest_words):
            if not w[1] and t[:1] in "+-" and len(t) > 1 and PLAIN.match(t[1:]):
                r["if_fact" if t[0] == "+" else "unless_fact"] = t[1:]
            else:
                keep.append(t)
                keep_words.append(w)
        return _check_rule(_rule_from(keep, keep_words, n, line, r), n)
    return None


def decode(text):
    """Texte compact (mode ligne ou tableau) -> relations completes. Leve
    CompactError sur une ligne illisible ou un nombre [N] faux (jamais de
    devinette : une regle oubliee ne doit pas passer en silence)."""
    out, problems = decode_report(text)
    if problems:
        raise CompactError("nombre de lignes faux : " + " ; ".join(problems))
    return out


GUIDE = (
    "Format CSTL compact (une ligne par element ; les crochets [ ] ci-dessous marquent ce qui est facultatif, "
    "ne les ecris pas) :\n"
    "- ROLE <agent> <role> : l'agent a ce role.\n"
    "- FACT <fait> true|false : le fait est vrai ou faux.\n"
    "- <qui> <MUST|MUST_NOT|MAY> <action> [conditions] [\"raison\"] : une regle. <qui> est un agent ou role:<nom>.\n"
    "  Conditions : >X (montant au-dela de X), <=X (montant au plus X), total>X / total<=X (total deja autorise "
    "plus ce montant), if:F (seulement si le fait F est vrai), unless:F (sauf si le fait F est vrai), "
    "from:D / until:D (periode de validite).\n"
    "Exemple :\n"
    "ROLE agent_a finance\n"
    "FACT audit_open true\n"
    "agent_a MUST_NOT wipe_db >100 unless:audit_open\n"
    "role:finance MUST log_transaction \"tracabilite\""
)


GUIDE_TABLE = (
    "Format CSTL compact en tableaux : une section par type d'element. L'en-tete donne le nombre exact de lignes "
    "[N] et les colonnes ; chaque ligne de la section commence par deux espaces ; « - » marque une case vide.\n"
    "- ROLES[N] agent role : une ligne par role d'un agent.\n"
    "- FACTS[N] fait valeur : une ligne par fait, valeur true ou false.\n"
    "- RULES[N] qui modalite action au_dela si sauf_si : une ligne par regle. qui = un agent ou role:<nom> ; "
    "modalite = MUST, MUST_NOT ou MAY ; au_dela = la regle ne vaut que si le montant depasse ce nombre ; "
    "si = la regle ne vaut que si ce fait est vrai ; sauf_si = la regle ne vaut pas si ce fait est vrai. "
    "Une raison facultative peut suivre, entre guillemets.\n"
    "Exemple :\n"
    "ROLES[1] agent role\n"
    "  agent_a finance\n"
    "FACTS[1] fait valeur\n"
    "  audit_open true\n"
    "RULES[2] qui modalite action au_dela si sauf_si\n"
    "  agent_a MUST_NOT wipe_db 100 - audit_open\n"
    "  role:finance MUST log_transaction - - - \"tracabilite\""
)


GUIDE_SYMBOLS = (
    "Format CSTL compact sans mot-cle (une ligne par element) :\n"
    "- <agent> @<role> : l'agent a ce role.\n"
    "- <fait>=1 ou <fait>=0 : le fait est vrai ou faux.\n"
    "- <qui> *<action> : obligation (MUST) ; <qui> !<action> : interdiction (MUST_NOT) ; <qui> ?<action> : "
    "permission (MAY). <qui> est un agent ou role:<nom>.\n"
    "  Conditions apres l'action : >X (seulement au-dela du montant X), +F (seulement si le fait F est vrai), "
    "-F (sauf si le fait F est vrai). Une raison facultative peut suivre, entre guillemets.\n"
    "Exemple :\n"
    "agent_a @finance\n"
    "audit_open=1\n"
    "agent_a !wipe_db >100 -audit_open\n"
    "role:finance *log_transaction \"tracabilite\""
)


def policy_to_compact(p):
    """Politique effective (`CstlAgent.policy()`) en CSTL compact, pour un
    modele. Meme contenu que `cstl_llm_agent.policy_to_text`."""
    lines = [f"# Politique en vigueur pour toi, calculee par le serveur CSTL. Par defaut : "
             f"{'AUTORISE' if p.get('default', 'allow') == 'allow' else 'INTERDIT'}.",
             "# MUST = a faire meme si la demande ne le cite pas ; MAY = autorise ; MUST_NOT = interdit.",
             "# Une regle avec condition ne vaut que dans ce cas ; le serveur verifie montants et totaux.",
             "# spent=N : total deja autorise ; facts=... : etat actuel des faits cites."]
    names = {"required": "MUST", "permitted": "MAY", "forbidden": "MUST_NOT"}
    for key in ("required", "permitted", "forbidden"):
        for e in p.get(key, []):
            parts = [names[key], _token(e["action"])]
            for attr in ("amount_above", "amount_at_most", "total_above", "total_at_most", "total_window_secs",
                         "if_fact", "unless_fact", "valid_from", "valid_until", "priority"):
                if e.get(attr):
                    parts.append(SHORT[attr] + _token(e[attr]) if attr in SHORT else f"{attr}={_token(e[attr])}")
            if e.get("total_above") or e.get("total_at_most"):
                parts.append(f"spent={_token(e.get('spent_now', '0'))}")
            facts = [f"{e[a]}={e.get(a + '_now', '?')}" for a in ("if_fact", "unless_fact") if e.get(a)]
            if facts:
                parts.append("facts=" + ",".join(facts))
            if e.get("applies") == "otherwise":
                parts.append("sinon")
            if e.get("via_role"):
                parts.append(f"role:{_token(e['via_role'])}")
            if e.get("reason"):
                parts.append(_quote(e["reason"]))
            lines.append(" ".join(parts))
    for e in p.get("ignored", []):
        lines.append(f"IGNORE {_token(e['action'])} par:{_token(e.get('author', '?'))}  # auteur sans autorite, ne pas suivre")
    return "\n".join(lines)


def policy_to_symbols(p):
    """Politique effective (`CstlAgent.policy()`) en CSTL SANS MOT-CLE, pour un
    modele (2026-11-22). Une ligne par regle, sans le sujet (c'est l'agent
    lui-meme) : * obligatoire, ! interdit, ? permis. Meme contenu que
    `cstl_llm_agent.policy_to_text`, en bien moins de tokens."""
    lines = [f"# Ta politique, calculee par le serveur CSTL. Sans regle : "
             f"{'permis' if p.get('default', 'allow') == 'allow' else 'interdit'}. "
             "* obligatoire (meme si la demande ne le cite pas), ! interdit, ? permis. "
             ">X au-dela de X, <=X au plus X, total<=X / total>X total deja autorise plus ce montant (spent=deja "
             "autorise), +F si le fait F est vrai, -F sauf si F est vrai ; etat=... donne l'etat actuel des faits "
             "(inconnu laisse l'interdiction en place). Le serveur verifie tout au moment de la demande."]
    sym = {"required": "*", "permitted": "?", "forbidden": "!"}
    for key in ("required", "permitted", "forbidden"):
        for e in p.get(key, []):
            parts = [sym[key] + _token(e["action"])]
            for attr in ("amount_above", "amount_at_most", "total_above", "total_at_most", "total_window_secs",
                         "valid_from", "valid_until", "priority"):
                if e.get(attr):
                    parts.append(SHORT[attr] + _token(e[attr]) if attr in SHORT else f"{attr}={_token(e[attr])}")
            if e.get("total_above") or e.get("total_at_most"):
                parts.append(f"spent={_token(e.get('spent_now', '0'))}")
            states = []
            for attr, mark in (("if_fact", "+"), ("unless_fact", "-")):
                if e.get(attr):
                    parts.append(mark + _token(e[attr]))
                    now = {"true": "vrai", "false": "faux"}.get(str(e.get(attr + "_now", "")), "inconnu")
                    states.append(f"{e[attr]}={now}")
            if states:
                parts.append("etat=" + ",".join(states))
            if e.get("applies") == "otherwise":
                parts.append("sinon")
            if e.get("via_role"):
                parts.append(f"role:{_token(e['via_role'])}")
            if e.get("reason"):
                parts.append(_quote(e["reason"]))
            lines.append(" ".join(parts))
    for e in p.get("ignored", []):
        lines.append(f"# IGNORE (auteur sans autorite, ne pas suivre) : {e['action']} par {e.get('author', '?')}")
    return "\n".join(lines)


# ---------------------------------------------------------------- arbres
# 2026-11-22 : donnees IMBRIQUEES (commande -> lignes -> options). Aplatir en
# relations « REL type sujet objet » repete l'id de chaque element ET celui de
# son parent, et coutait 23 % de tokens de plus que le JSON
# (scripts/nested_benchmark.py). Ici, une section par liste :
#   orders[2]#: client date          # : 1re colonne = id propre de l'element
#     o0 client_5 2026-11-25
#   lines[3]^: item qty price        ^ : 1re colonne = id du parent ; la
#     o0 gpu_rack 5 714                  position vient de l'ordre des lignes
#   teams[1]#^: lead                 #^ : id propre, puis id du parent
#     team_0_0 dept_0 agent_7
# Un element sans id propre a pour id <id du parent>.<position> (o0.1), ce qui
# permet a ses propres enfants de le designer. Les valeurs gardent leur type
# (nombre, booleen, null, chaine) ; « - » = champ absent.

_TREE_HEAD = re.compile(r"^([A-Za-z_][A-Za-z0-9_.]*)\[(\d+)\](#?)(\^?):((?:\s+\S+)*)\s*$")
_NUMLIKE = re.compile(r"^[+-]?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?$")


def _tree_cell(v):
    if v is None:
        return "null"
    if v is True or v is False:
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return json.dumps(v)
    if isinstance(v, (list, dict)):
        return _quote(json.dumps(v, ensure_ascii=False, separators=(",", ":"))) + "~"
    s = str(v)
    if s in ("", "-", "null", "true", "false") or _NUMLIKE.match(s) or re.search(r'[\s"#]', s) or s.endswith("~"):
        return _quote(s)
    return s


def _tree_value(word):
    text, quoted = word
    if quoted:
        return text
    if text.endswith("~") and len(text) > 1:
        # cellule JSON (liste ou objet) : "..."~
        try:
            return json.loads(text[:-1])
        except ValueError:
            raise CompactError(f"valeur JSON illisible : {text!r}") from None
    if text == "null":
        return None
    if text in ("true", "false"):
        return text == "true"
    if _NUMLIKE.match(text):
        return json.loads(text) if not re.search(r"^[+-]?0\d", text) else text
    return text


def _child_lists(o):
    return [(k, v) for k, v in o.items() if isinstance(v, list) and v and all(isinstance(x, dict) for x in v)]


def encode_tree(data):
    """Arbre JSON {liste: [objets...]} -> CSTL compact en sections. Sans perte
    (`decode_tree` redonne exactement `data`), y compris quand un meme nom de
    liste revient a plusieurs niveaux."""
    if not isinstance(data, dict) or not all(isinstance(v, list) for v in data.values()):
        raise CompactError("encode_tree attend {nom: [objets]} a la racine")
    order, own, nested, cols = [], {}, {}, {}

    def shape(key, objs, is_nested):
        if key not in own:
            order.append(key)
            own[key], nested[key], cols[key] = True, False, []
        nested[key] = nested[key] or is_nested
        for o in objs:
            if not isinstance(o, dict):
                raise CompactError(f"{key} : liste d'objets attendue")
            if not (isinstance(o.get("id"), str) and o["id"] and "." not in o["id"]):
                own[key] = False
            kids = {k for k, _ in _child_lists(o)}
            cols[key] += [k for k in o if k not in kids and k not in cols[key]]
            for k, v in _child_lists(o):
                shape(k, v, True)

    for key, objs in data.items():
        shape(key, objs, False)
    rows = {k: [] for k in order}

    def assign(key, objs, parent):
        for i, o in enumerate(objs):
            oid = o["id"] if own[key] else (f"{parent}.{i}" if parent is not None else str(i))
            rows[key].append((o, parent))
            for k, v in _child_lists(o):
                assign(k, v, oid)

    for key, objs in data.items():
        assign(key, objs, None)
    lines = []
    for key in order:
        c = [x for x in cols[key] if not (own[key] and x == "id")]
        lines.append(f"{key}[{len(rows[key])}]{'#' if own[key] else ''}{'^' if nested[key] else ''}:" + "".join(f" {x}" for x in c))
        for o, parent in rows[key]:
            cells = ([_tree_cell(o["id"])] if own[key] else []) + ([_tree_cell(parent) if parent is not None else '""'] if nested[key] else [])
            lines.append("  " + " ".join(cells + [_tree_cell(o[x]) if x in o else "-" for x in c]))
    return "\n".join(lines)


def decode_tree(text):
    """Inverse de `encode_tree`. Leve CompactError si une ligne, un nombre de
    lignes ou un parent ne colle pas."""
    sections, cur = [], None
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        if not line[:1].isspace():
            m = _TREE_HEAD.match(line.strip())
            if not m:
                raise CompactError(f"ligne {n} : en-tete de section attendu : {line!r}")
            cur = {"key": m.group(1), "n": int(m.group(2)), "own": bool(m.group(3)), "nested": bool(m.group(4)),
                   "cols": m.group(5).split(), "rows": []}
            sections.append(cur)
            continue
        if cur is None:
            raise CompactError(f"ligne {n} : ligne de donnees hors section")
        cur["rows"].append((n, _split(line)))
    # 1re passe : chaque element et son id ; 2e passe : rattachement au parent.
    nodes, links, root = {}, [], {}
    for sec in sections:
        if len(sec["rows"]) != sec["n"]:
            raise CompactError(f"{sec['key']}[{sec['n']}] annonce, {len(sec['rows'])} ligne(s) ecrite(s)")
        width = len(sec["cols"]) + sec["own"] + sec["nested"]
        pos = {}
        for n, words in sec["rows"]:
            if len(words) != width:
                raise CompactError(f"ligne {n} : {sec['key']} attend {width} valeurs, {len(words)} lues")
            i, obj, parent = 0, {}, None
            if sec["own"]:
                obj["id"] = _tree_value(words[0])
                i = 1
            if sec["nested"]:
                parent = str(_tree_value(words[i])) if words[i] != ("", True) else None
                i += 1
            for c, w in zip(sec["cols"], words[i:]):
                if w != ("-", False):
                    obj[c] = _tree_value(w)
            k = pos.get(parent, 0)
            pos[parent] = k + 1
            oid = str(obj["id"]) if sec["own"] else (f"{parent}.{k}" if parent is not None else str(k))
            if oid in nodes:
                raise CompactError(f"ligne {n} : id en double {oid!r}")
            nodes[oid] = obj
            links.append((sec["key"], obj, parent, n))
    for key, obj, parent, n in links:
        if parent is None:
            root.setdefault(key, []).append(obj)
        elif parent in nodes:
            nodes[parent].setdefault(key, []).append(obj)
        else:
            raise CompactError(f"ligne {n} : parent inconnu {parent!r}")
    return root


# ------------------------------------------------- arbres <-> serveur CSTL
# 2026-11-22 : pour qu'un arbre soit SIGNE, VERIFIE (SCHEMA) et AUDITE, il est
# envoye en relations : chaque element devient
#   RELATION [type=<nom de sa liste>, subject=<son id>, object=<id du parent ou ->, tree=<id de l'arbre>, ...]
# Tout ce qu'il faut pour le reconstruire a l'identique voyage avec lui :
#   tree_id=1        l'id est un vrai champ « id » (sinon <parent>.<position>) ;
#   tree_types=...   types des champs qui ne sont pas des chaines
#                    (qty:int,price:num,flag:bool,note:null) ;
#   f_<nom>          un champ qui porte le nom d'un attribut reserve de CSTL
#                    (type, subject, object, modality, id, sigma...) ou qui
#                    commence par f_ ou tree.
# Ce que le serveur ne pourrait pas rendre intact est REFUSE ici (jamais perdu
# en silence) : nom de champ hors [A-Za-z0-9_.-], valeur avec « " », « [ », « ] »
# ou un saut de ligne, valeur de plus de 300 caracteres, liste de valeurs
# simples ou objet dans une cellule.

_RESERVED_REL = {"type", "subject", "object", "modality", "id", "sigma", "tau", "layer", "weight", "coref_with"}
_NAME_OK = re.compile(r"^[A-Za-z0-9_.\-]{1,128}$")


def _field_name(k):
    if not _NAME_OK.match(k):
        raise CompactError(f"nom de champ non transmissible au serveur : {k!r}")
    return f"f_{k}" if k in _RESERVED_REL or k.startswith(("f_", "tree")) else k


def _unfield(k):
    return k[2:] if k.startswith("f_") else k


def _b64(text):
    return base64.urlsafe_b64encode(text.encode()).decode().rstrip("=")


def _unb64(text):
    return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4)).decode()


def _wire_value(where, v):
    """Valeur -> (texte transmissible, type). Une liste, un objet ou un texte
    que le format filaire ne peut pas porter (guillemet, crochet, retour a la
    ligne) part en JSON encode en base64url, de type « json » (2026-11-22)."""
    if v is None:
        return "", "null"
    if v is True or v is False:
        return ("true" if v else "false"), "bool"
    if isinstance(v, int):
        return str(v), "int"
    if isinstance(v, float):
        return json.dumps(v), "num"
    if isinstance(v, (list, dict)) or any(c in str(v) for c in '"[]\n\r\\'):
        return _b64(json.dumps(v, ensure_ascii=False, separators=(",", ":"))), "json"
    return str(v), None


# Une valeur longue est decoupee en morceaux de WIRE_CHUNK caracteres
# (2026-11-23) : le premier sous son nom, les suivants sous tree_c<i>_<nom>,
# et son type note le nombre de morceaux (nom:str/3, nom:json/3). Le serveur
# rend chaque attribut jusqu'a 300 caracteres ; un document entier (1 Mio par
# message) passe ainsi.
WIRE_CHUNK = 250


def _put_wire(rel, types, name, where, v):
    out, t = _wire_value(where, v)
    parts = [out[i:i + WIRE_CHUNK] for i in range(0, len(out), WIRE_CHUNK)] or [""]
    rel[name] = parts[0]
    for i, part in enumerate(parts[1:], 1):
        rel[f"tree_c{i}_{name}"] = part
    if len(parts) > 1:
        t = f"{t or 'str'}/{len(parts)}"
    if t:
        types.append(f"{name}:{t}")


def _put_types(rel, types):
    """Liste des types d'une relation, decoupee comme une valeur longue :
    tree_types, puis tree_t1, tree_t2... (un mot n'est jamais coupe)."""
    chunks, cur = [], ""
    for t in types:
        if cur and len(cur) + 1 + len(t) > WIRE_CHUNK:
            chunks.append(cur)
            cur = t
        else:
            cur = f"{cur} {t}" if cur else t
    if cur:
        chunks.append(cur)
    for i, c in enumerate(chunks):
        rel["tree_types" if i == 0 else f"tree_t{i}"] = c


def _from_wire(v, t):
    if t == "json":
        return json.loads(_unb64(v))
    return None if t == "null" else v == "true" if t == "bool" else int(v) if t == "int" else float(v) if t == "num" else v


def _get_wire(rel, name, t):
    """Inverse de `_put_wire` : recolle les morceaux (type nom:<t>/<n>)."""
    v = rel[name]
    if t and "/" in t:
        t, n = t.split("/", 1)
        try:
            v += "".join(rel[f"tree_c{i}_{name}"] for i in range(1, int(n)))
        except (KeyError, ValueError):
            raise CompactError(f"valeur longue incomplete : {name} ({n} morceaux annonces)") from None
        if t == "str":
            t = None
    return _from_wire(v, t)


def tree_hash(rels, tree_id):
    """Empreinte canonique d'un arbre (2026-11-23), identique a celle du serveur
    (`src/server/tree_check.rs::canonical_hash`) : relations tree=<tree_id>
    dans l'ordre, `cle=valeur` triees par cle, separees par 0x1f, relations
    separees par 0x1e ; SHA-256, prefixe sha256:."""
    import hashlib
    body = "\x1e".join("\x1f".join(f"{k}={r[k]}" for k in sorted(r) if k != "block")
                       for r in rels if r.get("tree") == str(tree_id))
    return "sha256:" + hashlib.sha256(body.encode()).hexdigest()


def tree_to_relations(data, tree_id, source=None, nonce=None):
    """Document JSON -> relations CSTL, toutes marquees tree=<tree_id>. Les
    listes d'objets deviennent des relations (une par objet) ; le reste de la
    racine (champs simples, liste d'objets en racine, valeur seule) part dans
    une relation « tree_root » (tree_kind=fields|list|value|tables).

    Comptes (2026-11-23) : chaque element qui a des enfants porte tree_n=<nombre
    d'enfants directs>, et tree_root porte tree_n=<nombre d'elements a la
    racine>. Le SERVEUR les verifie a la reception et refuse un arbre
    incomplet ; `relations_to_tree` les reverifie a la relecture.

    source=<agent> : l'arbre se reclame de cette source (tree_source sur
    tree_root). Le serveur exige alors un engagement signe de la source
    (tree_commit) dont l'empreinte (`tree_hash`) est celle de l'arbre recu."""
    if not _NAME_OK.match(str(tree_id)):
        raise CompactError(f"id d'arbre invalide : {tree_id!r}")
    rels, seen = [], set()
    kind, extra = None, {}
    if isinstance(data, list) and data and all(isinstance(x, dict) for x in data):
        kind, data = "list", {"_": data}
    elif isinstance(data, dict):
        extra = {k: v for k, v in data.items()
                 if not (_NAME_OK.match(k) and isinstance(v, list) and v and all(isinstance(x, dict) for x in v))}
        data = {k: v for k, v in data.items() if k not in extra}
        kind = "fields" if extra else "tables"
    else:
        kind, extra, data = "value", {"v": data}, {}
    rel = {"type": "tree_root", "subject": "tree_root", "object": "-", "tree": str(tree_id), "tree_kind": kind}
    if source is not None:
        if not _NAME_OK.match(str(source)):
            raise CompactError(f"nom de source invalide : {source!r}")
        rel["tree_source"] = str(source)
    if nonce is not None:
        # Sel de l'engagement (2026-11-23) : sans lui, l'empreinte d'un petit
        # document se devine en essayant les contenus plausibles.
        if not re.fullmatch(r"[0-9a-f]{32,128}", str(nonce)):
            raise CompactError("nonce : 32 a 128 chiffres hexadecimaux")
        rel["tree_nonce"] = str(nonce)
    types = []
    for k, v in extra.items():
        name = _field_name(k)
        _put_wire(rel, types, name, f"racine.{k}", v)
    if types:
        _put_types(rel, types)
    rel["tree_n"] = str(sum(len(v) for v in data.values()))
    rels.append(rel)

    def walk(key, objs, parent):
        if not _NAME_OK.match(key):
            raise CompactError(f"nom de liste non transmissible : {key!r}")
        for i, o in enumerate(objs):
            if not isinstance(o, dict):
                raise CompactError(f"{key} : liste d'objets attendue")
            own = isinstance(o.get("id"), str) and o["id"] != "" and "." not in o["id"]
            oid = o["id"] if own else (f"{parent}.{i}" if parent != "-" else f"{key}.{i}")
            if oid in seen:
                raise CompactError(f"id en double dans l'arbre : {oid!r}")
            seen.add(oid)
            rel = {"type": key, "subject": oid, "object": parent, "tree": str(tree_id)}
            if own:
                rel["tree_id"] = "1"
            types, kids = [], []
            for k, v in o.items():
                if own and k == "id":
                    continue
                if isinstance(v, list) and v and all(isinstance(x, dict) for x in v):
                    kids.append((k, v))
                    continue
                name = _field_name(k)
                _put_wire(rel, types, name, f"{key}.{k}", v)
            if types:
                _put_types(rel, types)
            if kids:
                rel["tree_n"] = str(sum(len(v) for _, v in kids))
            rels.append(rel)
            for k, v in kids:
                walk(k, v, oid)

    for key, objs in data.items():
        walk(key, objs, "-")
    return rels


def relations_to_tree(rels, tree_id=None):
    """Inverse de `tree_to_relations` (relations dans l'ordre d'envoi)."""
    nodes, root, kind, extra = {}, {}, None, {}
    for r in rels:
        if tree_id is not None and r.get("tree") != str(tree_id):
            continue
        all_types = " ".join([r.get("tree_types", "")] + [r[f"tree_t{i}"] for i in range(1, 1000) if f"tree_t{i}" in r])
        types = dict(t.split(":", 1) for t in all_types.split() if ":" in t)
        o = {"id": r["subject"]} if r.get("tree_id") == "1" else {}
        for k, v in r.items():
            if k in ("type", "subject", "object", "tree", "tree_id", "tree_types", "tree_kind", "tree_n", "tree_source",
                     "tree_nonce", "block") or k.startswith(("tree_c", "tree_t")):
                continue
            o[_unfield(k)] = _get_wire(r, k, types.get(k))
        if r.get("tree_kind"):
            kind, extra = r["tree_kind"], o
            continue
        nodes[r["subject"]] = o
        if r["object"] == "-":
            root.setdefault(r["type"], []).append(o)
        elif r["object"] in nodes:
            nodes[r["object"]].setdefault(r["type"], []).append(o)
        else:
            raise CompactError(f"parent inconnu {r['object']!r} pour {r['subject']!r}")
    # Comptes annonces (tree_n) : verifies aussi a la relecture.
    kids = {}
    for r in rels:
        if (tree_id is None or r.get("tree") == str(tree_id)) and not r.get("tree_kind"):
            kids[r["object"]] = kids.get(r["object"], 0) + 1
    for r in rels:
        if (tree_id is not None and r.get("tree") != str(tree_id)) or "tree_n" not in r:
            continue
        got = kids.get("-" if r.get("tree_kind") else r["subject"], 0)
        if str(got) != str(r["tree_n"]):
            raise CompactError(f"arbre incomplet : {r['subject']} annonce {r['tree_n']} enfant(s), {got} relu(s)")
    if kind == "value":
        return extra.get("v")
    if kind == "list":
        return root.get("_", [])
    return {**extra, **root}


# ------------------------------------------------- arbres indentes (v2)
# 2026-11-22. Le mode arbre par identifiants (`encode_tree`) etait le plus
# court, mais Gemma le suivait moins bien que le JSON (32/36 contre 32/33) :
# il fallait relier chaque element a son parent par id, et un champ absent
# note « - » etait parfois pris pour une valeur. Ici :
#   - les colonnes de chaque liste sont declarees UNE fois en tete :
#       @lines[3]^orders: item qty price
#     (3 elements en tout, enfants des elements de « orders ») ;
#   - l'arbre est ecrit par INDENTATION (2 espaces par niveau), sans id : un
#     element est sous son parent, comme en JSON indente ou en YAML ;
#   - un champ present dans TOUS les elements de la liste est une colonne ;
#     les autres s'ecrivent « nom=valeur », seulement s'ils existent ;
#   - si un parent peut avoir plusieurs listes d'enfants, la ligne commence par
#     « nom> » ; un element sans aucun champ s'ecrit « . ».

_TREE2_HEAD = re.compile(r"^@([A-Za-z_][A-Za-z0-9_.]*)(?:\[(\d+)\])?(?:\^([A-Za-z0-9_.,-]+))?:((?:\s+\S+)*)\s*$")


def _raw_tokens(line):
    """Mots d'une ligne, guillemets compris (pour relire « nom=valeur »).

    Une valeur en ligne ([...], {...}, ?{...}) peut contenir des espaces autour
    de sa ponctuation : `{"a": 1, "b": [1, 2]}` est un seul mot (2026-11-24) --
    du JSON pur est donc toujours une valeur en ligne valide. Un espace ailleurs
    dans une valeur en ligne (`[hello world]`) est une erreur, pas un mot
    recolle en silence."""
    out, cur, quoted, depth, i = [], [], False, 0, 0
    while i < len(line):
        ch = line[i]
        if depth and not quoted and ch.isspace():
            prev = next((c for c in reversed(cur) if not c.isspace()), "")
            j = i
            while j < len(line) and line[j].isspace():
                j += 1
            nxt = line[j] if j < len(line) else ""
            if prev in "[{,:" or nxt in "]},:":
                i = j
                continue
            raise CompactError(f"espace dans une valeur en ligne (mettre le texte entre guillemets) : {line!r}")
        if not quoted and ch in "[{" and (depth or not cur or cur[-1] in "=?"):
            depth += 1
        elif not quoted and ch in "]}" and depth:
            depth -= 1
        if quoted:
            cur.append(ch)
            if ch == "\\" and i + 1 < len(line):
                cur.append(line[i + 1])
                i += 2
                continue
            if ch == '"':
                quoted = False
        elif ch == '"':
            quoted = True
            cur.append(ch)
        elif ch.isspace():
            if cur:
                out.append("".join(cur))
                cur = []
        else:
            cur.append(ch)
        i += 1
    if quoted:
        raise CompactError(f"guillemets mal fermes : {line!r}")
    if cur:
        out.append("".join(cur))
    return out


# Extensions (2026-11-22), pour que tout document JSON passe sans perte :
#   - listes et objets EN LIGNE dans une cellule : tags=[a,b,3]
#     geo={lat:45.5,lon:-73.6}, matrice [[1,2],[3,4]] ; une chaine y est
#     ecrite entre guillemets (avec les echappements JSON) si elle contient
#     un espace, une virgule, « : », un crochet, une accolade ou un guillemet ;
#   - texte sur PLUSIEURS lignes : la cellule vaut « |N » et les N lignes
#     suivantes, sous l'element, commencent par « | » :
#       t1 desc=|2
#         | premiere ligne
#         | seconde ligne
#   - racine LIBRE : « @@ nom=valeur ... » pour les champs simples de la
#     racine, « @@list » si la racine est une liste d'objets (ecrite comme la
#     liste « _ »), « @@= valeur » pour toute autre valeur (nombre, texte,
#     liste de valeurs) ;
#   - une liste presente a la racine ET sous un parent : « ^.,parent » (« . »
#     designe la racine).
# Un nom de champ hors de [A-Za-z0-9_.-] s'ecrit entre guillemets ("a b"=1).

_IKEY_OK = re.compile(r"^[A-Za-z0-9_.\-]+$")
_ITABLE_OK = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")
_IBLOCK = re.compile(r"^\|(\d+)$")


# Le VIDE QUI PARLE (2026-11-23) : une valeur INCONNUE, distincte de null
# (connue : pas de valeur) et d'un champ absent (sans objet). En JSON :
# {"$unknown": {}} ou {"$unknown": {"sigma": 0.3, ...}} (son incertitude, son
# statut, son plan de resolution). Dans l'arbre indente : `?` ou
# `?{sigma:0.3,pending:next_checkup}`, en cellule comme en ligne.
UNKNOWN = "$unknown"


def unknown(**details):
    """Valeur inconnue, avec ses details eventuels : unknown(sigma=0.3)."""
    return {UNKNOWN: dict(details)}


def is_unknown(v):
    return isinstance(v, dict) and len(v) == 1 and isinstance(v.get(UNKNOWN), dict)


def _ikids(o):
    return [(k, v) for k, v in o.items()
            if _ITABLE_OK.match(k) and isinstance(v, list) and v and all(isinstance(x, dict) for x in v)]


def _iscalar_out(v):
    if v is None:
        return "null"
    if v is True or v is False:
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        if isinstance(v, float) and (v != v or v in (float("inf"), float("-inf"))):
            raise CompactError(f"nombre non representable en JSON : {v!r}")
        return json.dumps(v)
    return None


def _inline(v):
    """Valeur -> texte en ligne, sans espace hors guillemets."""
    out = _iscalar_out(v)
    if out is not None:
        return out
    if is_unknown(v):
        return "?" + (_inline(v[UNKNOWN]) if v[UNKNOWN] else "")
    if isinstance(v, list):
        return "[" + ",".join(_inline(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{" + ",".join((k if _IKEY_OK.match(k) else json.dumps(k, ensure_ascii=False)) + ":" + _inline(x)
                              for k, x in v.items()) + "}"
    s = str(v)
    if (s == "" or s in ("null", "true", "false") or _NUMLIKE.match(s) or s[0] == "?"
            or re.search(r'[\s",\[\]{}:\\]', s)):
        return json.dumps(s, ensure_ascii=False)
    return s


def _parse_inline(s, i=0):
    """Texte en ligne -> (valeur, position suivante)."""
    if i >= len(s):
        raise CompactError(f"valeur en ligne incomplete : {s!r}")
    ch = s[i]
    if ch == "?" and (i + 1 == len(s) or s[i + 1] in ",]}{"):
        if i + 1 < len(s) and s[i + 1] == "{":
            details, j = _parse_inline(s, i + 1)
            return {UNKNOWN: details}, j
        return {UNKNOWN: {}}, i + 1
    if ch == '"':
        j = i + 1
        while j < len(s) and s[j] != '"':
            j += 2 if s[j] == "\\" else 1
        if j >= len(s):
            raise CompactError(f"guillemets mal fermes : {s!r}")
        try:
            return json.loads(s[i:j + 1]), j + 1
        except ValueError:
            raise CompactError(f"chaine illisible : {s[i:j + 1]!r}") from None
    if ch in "[{":
        close, out, i = ("]" if ch == "[" else "}"), ([] if ch == "[" else {}), i + 1
        if i < len(s) and s[i] == close:
            return out, i + 1
        while True:
            if ch == "{":
                if i < len(s) and s[i] == '"':
                    key, i = _parse_inline(s, i)
                else:
                    j = i
                    while j < len(s) and s[j] not in ":,]}":
                        j += 1
                    key, i = s[i:j], j
                if not isinstance(key, str) or i >= len(s) or s[i] != ":":
                    raise CompactError(f"« cle:valeur » attendu : {s!r}")
                val, i = _parse_inline(s, i + 1)
                out[key] = val
            else:
                val, i = _parse_inline(s, i)
                out.append(val)
            if i < len(s) and s[i] == ",":
                i += 1
                continue
            if i < len(s) and s[i] == close:
                return out, i + 1
            raise CompactError(f"« , » ou « {close} » attendu : {s!r}")
    j = i
    while j < len(s) and s[j] not in ",]}":
        j += 1
    word = s[i:j]
    if not word:
        raise CompactError(f"valeur vide en ligne : {s!r}")
    return _iscalar_in(word, False), j


def _iscalar_in(text, quoted):
    if quoted:
        return text
    if text == "null":
        return None
    if text in ("true", "false"):
        return text == "true"
    if _NUMLIKE.match(text) and not re.search(r"^[+-]?0\d", text):
        return json.loads(text)
    return text


def _icell(v, blocks):
    """Valeur -> cellule ; un texte multiligne ajoute ses lignes a `blocks`."""
    out = _iscalar_out(v)
    if out is not None:
        return out
    if isinstance(v, (list, dict)):
        return _inline(v)
    s = str(v)
    if "\n" in s:
        part = s.split("\n")
        blocks.append(part)
        return f"|{len(part)}"
    if (s in ("", "-", ".", "null", "true", "false") or _NUMLIKE.match(s) or re.search(r'[\s"#=]', s)
            or s[0] in "[{|@/?"):
        return _quote(s)
    return s


def _iname(k):
    return k if _IKEY_OK.match(k) else json.dumps(k, ensure_ascii=False)


def _icell_in(raw):
    """Cellule brute -> (valeur, nombre de lignes de bloc attendues ou None)."""
    m = _IBLOCK.match(raw)
    if m:
        return None, int(m.group(1))
    if raw[0] in "[{" or raw == "?" or raw.startswith("?{"):
        val, j = _parse_inline(raw)
        if j != len(raw):
            raise CompactError(f"texte en trop apres la valeur en ligne : {raw!r}")
        return val, None
    words = _split(raw)
    return (_iscalar_in(*words[0]) if words else ""), None


def _inamed(tok):
    """« nom=valeur » (nom eventuellement entre guillemets) -> (nom, brut)."""
    if tok.startswith('"'):
        name, j = _parse_inline(tok)
        if not isinstance(name, str) or tok[j:j + 1] != "=":
            raise CompactError(f"« nom=valeur » attendu : {tok!r}")
        return name, tok[j + 1:]
    name, sep, raw = tok.partition("=")
    if not sep or not name:
        raise CompactError(f"« nom=valeur » attendu : {tok!r}")
    return name, raw


def encode_tree_indented(data, counts="head"):
    """Document JSON -> CSTL compact en arbre indente. Sans perte
    (`decode_tree_indented` redonne exactement `data`), quelle que soit la
    racine (objet, liste, valeur).

    counts="head" : nombre d'elements en en-tete (`@orders[8]: ...`).
    counts="end"  : en-tetes sans nombre, et une derniere ligne
    `@end orders=8 lines=21` (2026-11-22). Pour qui ECRIT de gauche a droite
    (un modele), compter apres coup est plus sur qu'annoncer avant ; et
    `@end` seul suffit deja a detecter un document coupe.
    counts="line" : chaque element qui a des enfants finit sa ligne par
    `/N`, le nombre de ses enfants directs (`ctr_1 party_9 /3`), et le
    document finit par `@end`. Un modele compte bien 1 a 5 elements qu'il
    vient d'ecrire, mal 129 ; et un compte local dit OU il manque une ligne."""
    if counts not in ("head", "end", "line"):
        raise CompactError(f"counts : head, end ou line, pas {counts!r}")
    head = []
    if isinstance(data, list) and data and all(isinstance(x, dict) for x in data):
        head, tables, fields = ["@@list"], {"_": data}, {}
    elif isinstance(data, dict):
        tables = {k: v for k, v in data.items() if (k, v) in _ikids(data)}
        fields = {k: v for k, v in data.items() if k not in tables}
        if not tables and not fields:
            return "@@= {}"
    else:
        return "@@= " + _inline(data)

    order, parents, counts_of, dense, kids_of = [], {}, {}, {}, {None: list(tables)}

    def shape(key, objs, parent_key):
        if key not in counts_of:
            order.append(key)
            counts_of[key], parents[key], dense[key] = 0, [], None
        if parent_key not in parents[key]:
            parents[key].append(parent_key)
        for o in objs:
            counts_of[key] += 1
            kids = _ikids(o)
            cols = [k for k in o if (k, o[k]) not in kids and _IKEY_OK.match(k)]
            dense[key] = cols if dense[key] is None else [c for c in dense[key] if c in cols]
            for k, v in kids:
                kids_of.setdefault(key, [])
                if k not in kids_of[key]:
                    kids_of[key].append(k)
                shape(k, v, key)

    for key, objs in tables.items():
        shape(key, objs, None)
    lines = list(head)

    def emit(prefix, cells, blocks, depth):
        lines.append(prefix + " ".join(cells))
        for part in blocks:
            lines.extend("  " * (depth + 1) + ("| " + t if t else "|") for t in part)

    if fields:
        blocks = []
        emit("@@ ", [f"{_iname(k)}={_icell(v, blocks)}" for k, v in fields.items()], blocks, -1)
    for key in order:
        ps = parents[key]
        tag = "" if ps == [None] else "^" + ",".join("." if p is None else p for p in ps)
        n = f"[{counts_of[key]}]" if counts == "head" else ""
        lines.append(f"@{key}{n}{tag}:" + "".join(f" {c}" for c in dense[key] or []))

    def rows(key, objs, parent_key, depth):
        tag = f"{key}> " if len(kids_of.get(parent_key, [])) > 1 else ""
        for o in objs:
            kids = _ikids(o)
            blocks = []
            cells = [_icell(o[c], blocks) for c in dense[key] or []]
            cells += [f"{_iname(k)}={_icell(v, blocks)}" for k, v in o.items()
                      if k not in (dense[key] or []) and (k, v) not in kids]
            if counts == "line" and kids:
                cells = (cells or ["."]) + [f"/{sum(len(v) for _, v in kids)}"]
            emit("  " * depth + tag, cells or ["."], blocks, depth)
            for k, v in kids:
                rows(k, v, key, depth + 1)

    for key, objs in tables.items():
        rows(key, objs, None, 0)
    if counts == "end":
        lines.append("@end" + "".join(f" {k}={counts_of[k]}" for k in order))
    elif counts == "line":
        lines.append("@end")
    return "\n".join(lines)


def _is_named(tok):
    """Mot de la forme nom=valeur (nom eventuellement entre guillemets) ?"""
    if tok.startswith('"'):
        try:
            name, j = _parse_inline(tok)
        except CompactError:
            return False
        return isinstance(name, str) and tok[j:j + 1] == "="
    head = tok.split("=", 1)[0]
    return "=" in tok and bool(head) and _IKEY_OK.match(head) is not None


def decode_tree_indented(text, lenient=False, warnings=None):
    """Inverse de `encode_tree_indented`. Leve CompactError si une ligne, un
    nombre d'elements, une indentation ou un bloc ne colle pas.

    `[N]` est facultatif dans un en-tete (absent : pas de verification).

    lenient=True (2026-11-22), pour un texte ECRIT PAR UN MODELE : rien n'est
    devine, seules trois regles de forme sont assouplies, parce que
    l'information reste complete sans elles :
      - un `[N]` faux devient un avertissement (l'indentation suffit a
        reconstruire l'arbre) ;
      - une ligne peut omettre ses DERNIERES colonnes : ces champs sont
        absents (le modele a mis en colonne un champ facultatif) ;
      - l'indentation peut etre de n'importe quelle largeur (tabulation = 4
        espaces), pourvu qu'elle soit coherente.
    Les avertissements sont ajoutes a la liste `warnings` si elle est donnee."""
    heads, kids_of, root_keys, root = {}, {}, [], {}
    stack = []          # [profondeur, objet, nom de sa liste, enfants annonces (/N), enfants vus]
    seen, as_list, value = {}, False, None
    ended = False
    widths = [0]        # mode tolerant : largeur d'indentation de chaque niveau
    lines = text.split("\n")
    if lenient:   # tabulations de tete seulement : une valeur peut en contenir
        lines = [(lambda body: line[:len(line) - len(body)].replace("\t", "    ") + body)(line.lstrip(" \t"))
                 for line in lines]
    i = 0

    def take_blocks(n, pending):
        """Lit les lignes « | ... » des cellules multilignes de la ligne n."""
        nonlocal i
        for setter, count in pending:
            part = []
            for _ in range(count):
                if i >= len(lines):
                    raise CompactError(f"ligne {n} : bloc « | » incomplet")
                body = lines[i].lstrip(" ")
                if body == "|":
                    part.append("")
                elif body.startswith("| "):
                    part.append(body[2:])
                else:
                    raise CompactError(f"ligne {i + 1} : ligne de bloc « | » attendue : {lines[i]!r}")
                i += 1
            setter("\n".join(part))

    def close(entry):
        """Verifie le compte local `/N` d'un element dont on quitte les enfants."""
        if entry[3] is not None and entry[3] != entry[4]:
            msg = f"ligne {entry[5]} : /{entry[3]} annonce, {entry[4]} enfant(s) ecrit(s)"
            if not lenient:
                raise CompactError(msg)
            if warnings is not None:
                warnings.append(msg)

    def fill(obj, toks, cols, n):
        pending = []
        for c, t in zip(cols, toks):
            val, blk = _icell_in(t)
            obj[c] = val
            if blk is not None:
                pending.append((lambda v, c=c: obj.__setitem__(c, v), blk))
        for t in toks[len(cols):]:
            name, raw = _inamed(t)
            val, blk = _icell_in(raw) if raw else ("", None)
            obj[name] = val
            if blk is not None:
                pending.append((lambda v, name=name: obj.__setitem__(name, v), blk))
        take_blocks(n, pending)

    while i < len(lines):
        line, n = lines[i], i + 1
        i += 1
        if not line.strip():
            continue
        if ended:
            raise CompactError(f"ligne {n} : contenu apres « @end »")
        if re.match(r"^@end(\s|$)", line):
            ended = True
            for tok in line.split()[1:]:
                name, sep, num = tok.partition("=")
                if not sep or not num.isdigit():
                    raise CompactError(f"ligne {n} : « @end nom=nombre ... » attendu : {tok!r}")
                if name not in heads:
                    raise CompactError(f"ligne {n} : liste inconnue dans « @end » : {name!r}")
                if heads[name]["n"] is not None and heads[name]["n"] != int(num):
                    raise CompactError(f"ligne {n} : {name} compte deux fois, differemment")
                heads[name]["n"] = int(num)
            continue
        if line.startswith("@@"):
            rest = line[2:]
            if rest.strip() == "list":
                as_list = True
            elif rest.startswith("="):
                body = rest[1:].strip()
                value, j = _parse_inline(body) if body else (None, 0)
                if not body or j != len(body):
                    raise CompactError(f"ligne {n} : valeur de racine illisible : {line!r}")
                as_list = "value"
            else:
                fill(root, _raw_tokens(rest.strip()), [], n)
            continue
        if line.startswith("@"):
            m = _TREE2_HEAD.match(line.strip())
            if not m:
                raise CompactError(f"ligne {n} : en-tete illisible : {line!r}")
            key = m.group(1)
            heads[key] = {"n": int(m.group(2)) if m.group(2) else None, "cols": m.group(4).split()}
            for p in (m.group(3).split(",") if m.group(3) else ["."]):
                if p == ".":
                    root_keys.append(key)
                else:
                    kids_of.setdefault(p, []).append(key)
            continue
        indent = len(line) - len(line.lstrip(" "))
        if not lenient:
            if indent % 2:
                raise CompactError(f"ligne {n} : indentation impaire")
            depth = indent // 2
        else:
            popped = False
            while len(widths) > 1 and indent < widths[-1]:
                widths.pop()
                popped = True
            if indent > widths[-1] and popped:
                raise CompactError(f"ligne {n} : indentation incoherente")
            if indent > widths[-1]:
                widths.append(indent)
            elif indent != widths[-1]:
                raise CompactError(f"ligne {n} : indentation incoherente")
            depth = len(widths) - 1
        while stack and stack[-1][0] >= depth:
            close(stack.pop())
        if depth and (not stack or stack[-1][0] != depth - 1):
            raise CompactError(f"ligne {n} : element sans parent (indentation)")
        cands = kids_of.get(stack[-1][2], []) if stack else root_keys
        toks = _raw_tokens(line.strip())
        expected = None
        if toks and re.fullmatch(r"/\d+", toks[-1]):
            expected = int(toks.pop()[1:])
            if not toks:
                toks = ["."]
        if len(cands) > 1:
            if not toks or not toks[0].endswith(">") or toks[0][:-1] not in cands:
                raise CompactError(f"ligne {n} : « nom> » attendu parmi {cands}")
            key = toks.pop(0)[:-1]
        elif len(cands) == 1:
            key = cands[0]
        else:
            raise CompactError(f"ligne {n} : aucune liste d'enfants declaree ici")
        cols = heads[key]["cols"]
        if toks == ["."] and not cols:
            toks = []
        if not lenient and len(toks) < len(cols):
            raise CompactError(f"ligne {n} : {key} attend {len(cols)} valeurs")
        npos = next((j for j, t in enumerate(toks) if _is_named(t)), len(toks)) if lenient else len(cols)
        named = {_inamed(t)[0] for t in toks[npos:] if _is_named(t)} if lenient else set()
        if lenient and [c for c in cols[npos:] if c not in named] and warnings is not None:
            # Colonnes manquantes acceptees, mais jamais en silence (2026-11-24) :
            # un enfant oublie de l'indentation se lit aussi comme un parent a
            # qui il manque des colonnes (vu dans une critique ecrite par un
            # modele). L'avertissement nomme la ligne a verifier.
            warnings.append(f"ligne {n} : {key} sans {', '.join(c for c in cols[npos:] if c not in named)} "
                            "(colonnes absentes)")
        obj = {}
        fill(obj, toks, cols[:npos], n)
        (stack[-1][1].setdefault(key, []) if stack else root.setdefault(key, [])).append(obj)
        seen[key] = seen.get(key, 0) + 1
        if stack:
            stack[-1][4] += 1
        stack.append([depth, obj, key, expected, 0, n])
    while stack:
        close(stack.pop())
    for key, h in heads.items():
        if h["n"] is not None and seen.get(key, 0) != h["n"]:
            msg = f"{key}[{h['n']}] annonce, {seen.get(key, 0)} element(s) ecrit(s)"
            if not lenient:
                raise CompactError(msg)
            if warnings is not None:
                warnings.append(msg)
    if as_list == "value":
        if root or heads:
            raise CompactError("« @@= » : la racine est une valeur, rien d'autre n'est attendu")
        return value
    if as_list:
        if set(root) - {"_"}:
            raise CompactError("« @@list » : seule la liste « _ » est attendue")
        return root.get("_", [])
    return root


# ------------------------------------------------- CSTL polymorphe (2026-11-24)
# CSTL a plusieurs formes, chacune bonne pour un type de donnees :
#   complet    RELATION [type=..., ...]          le format filaire du serveur
#   lignes     REL / ROLE / FACT, une par ligne   relations, avec mots-cles
#   tableau    ROLES[N] / FACTS[N] / RULES[N]     relations regulieres
#   symboles   bot !refund >100, @role, fait=1    politiques : le plus court
#   arbre      @liste^parent: colonnes, indente   documents imbriques
#   arbre_n    le meme, avec /N en fin de ligne   documents lus par un modele
#   arbre_ids  liste[N]#^: avec ids de parents    documents (ancienne forme)
# Le CSTL polymorphe essaie TOUTES les formes capables de representer les
# donnees, garde celles qui se relisent a l'identique, et prend la moins
# chere en tokens. Le texte commence par « #cstl <forme> » : le lecteur sait
# quelle forme lire, sans rien deviner.

_TOKEN_RE = re.compile(r" ?[A-Za-z]+| ?\d{1,3}|[^\sA-Za-z\d]+|\s+")

FORMS_RELATIONS = ("complet", "lignes", "tableau", "symboles")
# « strict » (2026-11-25, ancien nom « objets_json ») : la structure CSTL
# indentee « cle: valeur », chaque valeur ecrite en JSON. C'est la forme que
# les modeles ecrivent sans erreur aujourd'hui (48/48 sur donnees reelles),
# parce qu'ils connaissent deja les valeurs JSON. Elle reste la forme d'ecriture
# des modeles jusqu'a ce qu'ils soient entraines sur les formes CSTL plus
# compactes (arbre, relationnel, mixte) ; elle ressemble a un sous-ensemble du
# YAML, sans ses pieges (no -> false, 1.10 -> 1.1). `#cstl objets_json` reste lu.
FORMS_DOCUMENT = ("arbre", "arbre_n", "arbre_ids", "objets", "strict")

# FIABILITE MESUREE (2026-11-24) de chaque forme, par sorte de donnees et par
# tache, quand un MODELE lit ou ecrit : (reussites, essais), sur les memes
# modeles et les memes donnees pour toutes les formes d'une meme ligne.
# Sources : docs/BENCHMARK_CSTL.md, scripts/*_results / *_benchmark_*.json.
# Une forme absente d'une ligne n'a pas ete mesuree pour cette tache : elle ne
# sera choisie que si aucune forme mesuree ne represente les donnees.
RELIABILITY = {
    # 12 politiques (10 a 60 regles, roles, faits, conditions), GPT-5 mini,
    # Claude Haiku 5.5, Llama 4 Maverick, Mistral Medium 3.5 ; lecture : toute
    # la politique juste ; ecriture : politique exacte (format_short_benchmark,
    # compact_writing_benchmark).
    ("relations", "lire"): {"lignes": (26, 48), "symboles": (25, 48), "complet": (22, 48), "tableau": (18, 48)},
    ("relations", "ecrire"): {"complet": (24, 48), "symboles": (21, 48), "lignes": (17, 48), "tableau": (15, 48)},
    # Donnees tabulaires imbriquees : lecture sur les 4 modeles ou les deux
    # formes d'arbre ont ete posees (Llama, Mistral, Flash-Lite, GPT-5 mini).
    # L'arbre par ids n'a ete mesure QUE sur Gemma 4 31B (31/36 contre 36/36
    # pour l'arbre) : ramene a la meme base que les autres par ce rapport
    # (99/144 x 31/36 = 85/144). Ne jamais comparer des taux mesures sur des
    # modeles differents.
    ("tabulaire", "lire"): {"arbre_n": (108, 144), "arbre": (99, 144), "arbre_ids": (85, 144)},
    # Ecriture stricte : arbre_n sur 10 modeles (107/120). Arbre a comptes en
    # tete mesure sur Gemma seulement (9/12 contre 12/12 pour arbre_n) : ramene
    # par ce rapport (107 x 9/12 = 80/120).
    ("tabulaire", "ecrire"): {"arbre_n": (107, 120), "arbre": (80, 120)},
    # Donnees irregulieres (12 reponses d'API reelles) : ecriture sur GPT-5
    # mini, Claude Haiku, Llama 4, Mistral (real_data_benchmark). Lecture :
    # arbre a egalite avec JSON, questions trop faciles (effet plafond).
    # mixte (tables a cellules JSON) : 37/48 -- les tables ou un champ manque
    # dans certains elements sont mal remplies (valeurs decalees).
    # relationnel : 40/48 (v1 mesuree ; v2 36/48 mesuree, 40/48 relue avec le
    # lecteur qui accepte les cles a espaces). Echecs restants : du contenu
    # (champs inventes ou omis, elements manquants), pas de la syntaxe.
    ("irregulier", "ecrire"): {"strict": (48, 48), "relationnel": (40, 48), "mixte": (37, 48),
                               "objets": (29, 48), "arbre_n": (25, 48)},
    ("irregulier", "lire"): {"arbre": (75 * 7 - 6, 75 * 7), "arbre_n": (75 * 7 - 8, 75 * 7)},
}


def estimate_tokens(text):
    """Estimation des tokens d'un texte, proche des tokenizers BPE : mots,
    nombres par groupes de 3 chiffres, ponctuation par suites, espaces par
    suites. Sur les bancs d'essai, elle donne -46 % / -6 % la ou les 6
    tokenizers mesures donnent -39 a -49 % / -8 a -13 % (arbre contre JSON)."""
    return len(_TOKEN_RE.findall(text))


def _is_relations(data):
    return (isinstance(data, list) and data and all(
        isinstance(r, dict) and all(isinstance(r.get(k), str) for k in ("type", "subject", "object"))
        and all(isinstance(v, str) for v in r.values()) for r in data))


def _encode_full(rels):
    import cstl_client
    return "".join(cstl_client._format_kv_block("RELATION", r) for r in rels).rstrip("\n")


def _decode_full(text):
    import cstl_client
    return [{k: v for k, v in r.items() if k != "block"} for r in cstl_client.parse_response(text).relations]


_ENCODERS = {
    "complet": _encode_full,
    "lignes": lambda r: encode(r),
    "tableau": lambda r: encode(r, table=True),
    "symboles": encode_symbols,
    "arbre": lambda d: encode_tree_indented(d),
    "arbre_n": lambda d: encode_tree_indented(d, counts="line"),
    "arbre_ids": encode_tree,
    "objets": lambda d: encode_objects(d),
    "strict": lambda d: encode_objects_json(d),
}
_DECODERS = {
    "complet": _decode_full,
    "lignes": decode, "tableau": decode, "symboles": decode,
    "arbre": decode_tree_indented, "arbre_n": decode_tree_indented,
    "arbre_ids": decode_tree,
    "objets": lambda t: decode_objects(t),
    "strict": lambda t: decode_objects_json(t),
    "objets_json": lambda t: decode_objects_json(t),   # ancien nom de « strict »
}


def candidates(data, forms=None):
    """{forme: texte} pour chaque forme CSTL qui represente ces donnees SANS
    PERTE (verifie : la forme relue redonne exactement les donnees, ordre
    compris -- l'ordre des regles compte pour la couche 9)."""
    pool = forms or (FORMS_RELATIONS + FORMS_DOCUMENT if _is_relations(data) else FORMS_DOCUMENT)
    out = {}
    for f in pool:
        try:
            text = _ENCODERS[f](data)
            if _DECODERS[f](text) == data:
                out[f] = text
        except Exception:  # noqa: BLE001 -- une forme qui ne sait pas representer ces donnees est ecartee
            continue
    return out


def data_kind(data):
    """« relations », « tabulaire » (document dont l'arbre indente fait gagner
    au moins 25 % de tokens estimes : listes d'objets reguliers) ou
    « irregulier » (objets, configurations, metadonnees)."""
    if _is_relations(data):
        return "relations"
    try:
        j = estimate_tokens(json.dumps(data, ensure_ascii=False, separators=(",", ":")))
        t = estimate_tokens(encode_tree_indented(data))
    except CompactError:
        return "irregulier"
    return "tabulaire" if j and (j - t) / j >= 0.25 else "irregulier"


def _rate(cell):
    ok, n = cell
    return ok / n if n else 0.0


def choose_format(data, task=None, forms=None):
    """La forme CSTL a utiliser pour ces donnees. Renvoie (forme, texte, detail).

    task=None (entre machines) : toutes les formes sans perte sont aussi
    fiables (decodeurs deterministes) -> la moins chere en tokens.
    task="lire" ou "ecrire" (un MODELE lit, ou doit ecrire, ces donnees) : la
    forme la plus FIABLE mesuree pour cette sorte de donnees et cette tache
    (RELIABILITY) ; a fiabilite egale a 2 points pres, la moins chere. Une forme
    non mesuree n'est retenue que si aucune forme mesuree ne convient.
    detail = {"sorte", "tache", "fiabilite": {forme: taux}, "tokens": {forme: n}}."""
    kind = globals()["data_kind"](data)
    if forms is None:
        forms = FORMS_RELATIONS + ("arbre", "arbre_n") if kind == "relations" else FORMS_DOCUMENT
    cands = candidates(data, forms)
    if not cands:
        raise CompactError("aucune forme CSTL ne represente ces donnees sans perte")
    costs = {f: estimate_tokens(t) for f, t in cands.items()}
    order = list(_ENCODERS)
    if task is None:
        best = min(costs, key=lambda f: (costs[f], order.index(f)))
        return best, cands[best], {"sorte": kind, "tache": None, "fiabilite": {}, "tokens": costs}
    if task not in ("lire", "ecrire"):
        raise CompactError(f"tache inconnue : {task!r} (lire ou ecrire)")
    table = RELIABILITY.get((kind, task), {})
    rates = {f: _rate(table[f]) for f in cands if f in table}
    if rates:
        top = max(rates.values())
        pool = [f for f, r in rates.items() if r >= top - 0.02]
    else:
        pool = list(cands)
    best = min(pool, key=lambda f: (costs[f], order.index(f)))
    return best, cands[best], {"sorte": kind, "tache": task, "fiabilite": rates, "tokens": costs}


def encode_auto(data, task=None, forms=None):
    """CSTL polymorphe : la forme choisie par choose_format, precedee de
    « #cstl <forme> »."""
    form, text, _ = choose_format(data, task, forms)
    return f"#cstl {form}\n{text}"


def decode_auto(text, lenient=False, warnings=None):
    """Relit un texte ecrit par encode_auto (« #cstl <forme> » en tete). Sans
    cette ligne : arbre indente s'il commence par « @ », format complet s'il
    contient « RELATION [ »."""
    # Rien n'est retire apres l'en-tete : la fin du texte peut faire partie
    # des donnees (dernier message termine par un espace, 2026-11-24).
    m = re.match(r"\s*#cstl (\w+)\n", text)
    body = text[m.end():] if m else text.strip()
    if m:
        form = m.group(1)
    elif body.startswith("@"):
        form = "arbre"
    elif "RELATION [" in body:
        form = "complet"
    else:
        raise CompactError("forme CSTL inconnue : il manque « #cstl <forme> »")
    if form not in _DECODERS:
        raise CompactError(f"forme CSTL inconnue : {form!r}")
    if form in ("arbre", "arbre_n"):
        return decode_tree_indented(body, lenient=lenient, warnings=warnings)
    return _DECODERS[form](body)


# ------------------------------------------------- forme « objets » (2026-11-24)
# Pour les donnees IRREGULIERES riches en objets (configurations, metadonnees,
# reponses d'API), ou l'arbre indente s'est montre difficile a ecrire pour un
# modele (GPT-5 mini 5/12, Llama 3/12 sur 12 reponses d'API reelles) : trop de
# regles propres (@@, ^., nom>, guillemets des objets en ligne). Ici, le moins
# de regles possible, dans une syntaxe que les modeles connaissent (proche de
# YAML), mais sans ses ambiguites :
#   cle: valeur                 une valeur simple (memes regles que les cellules)
#   cle:                        un objet : ses champs dessous, indentes de 2
#   cle: [a,b] / {} / []        liste de valeurs, objet ou liste vides : en ligne
#   cle:                        une liste dont un element n'est pas une valeur
#     - valeur                    simple : un element par ligne « - » ;
#     - cle: valeur               un objet : ses champs apres « - », puis
#       cle2: valeur              alignes dessous
#   cle: |N                     texte de N lignes, en bloc « | »
#   cle: ? / ?{...}             valeur inconnue
# Une cle hors [A-Za-z0-9_.-] s'ecrit en JSON ("docs:dev": ...). Racine : un
# objet (ses champs), une liste (des « - ») ou « = valeur ».

def _okey(k):
    """Cle de la forme objets. Le separateur est « : » suivi d'un espace (ou
    la fin de ligne), comme en YAML : une cle peut contenir « : » (lint:fix,
    docs:dev), cas frequent dans les vraies API, que les modeles n'entourent
    pas de guillemets. Entre guillemets seulement si elle est vide, contient
    « : » suivi d'un espace ou un retour a la ligne, finit par « : », commence
    par « - », « \" », « ? », « = » ou un espace, ou finit par un espace."""
    if (not k or ": " in k or "\n" in k or "\r" in k or k.endswith(":") or k[0] in "-\"?= \t"
            or k[-1] in " \t" or "#" in k):
        return json.dumps(k, ensure_ascii=False)
    return k


def _osimple(v):
    """Valeur ecrite sur la ligne de sa cle (sinon : None, en bloc dessous)."""
    if isinstance(v, dict):
        return "{}" if not v else (_inline(v) if is_unknown(v) else None)
    if isinstance(v, list):
        if not v:
            return "[]"
        return _inline(v) if all(_iscalar_out(x) is not None or isinstance(x, str) for x in v) else None
    return None


def encode_objects(data, json_values=False):
    """Document JSON -> CSTL forme « objets ». Sans perte (decode_objects).

    json_values=True (forme « strict », ex-« objets_json », 2026-11-24) : la meme structure
    (indentation, cles sans guillemets, ni accolades ni virgules), mais chaque
    valeur ecrite EXACTEMENT comme en JSON ("texte", 27, false, null, [..]).
    Les modeles ecrivent les valeurs JSON sans erreur ; les regles de
    guillemets propres a CSTL etaient la premiere source d'echecs d'ecriture
    sur des donnees reelles."""
    lines = []
    if json_values:
        dumps = lambda v: json.dumps(v, ensure_ascii=False, separators=(",", ":"))  # noqa: E731

        def scalar_cell(v, ind):
            return dumps(v), []
    else:
        dumps = None

    def scalar_cell_cstl(v, ind):
        blocks = []
        cell = _icell(v, blocks)
        return cell, [ind + "  " + ("| " + t if t else "|") for part in blocks for t in part]

    if not json_values:
        scalar_cell = scalar_cell_cstl

    def simple_of(v):
        if json_values:
            if isinstance(v, dict) and v and not is_unknown(v):
                return None
            if isinstance(v, list) and v and not all(not isinstance(x, (dict, list)) for x in v):
                return None
            return dumps(v) if isinstance(v, (dict, list)) else None
        return _osimple(v)

    def field(k, v, ind):
        simple = simple_of(v)
        if simple is not None:
            lines.append(f"{ind}{_okey(k)}: {simple}")
        elif isinstance(v, dict):
            lines.append(f"{ind}{_okey(k)}:")
            obj(v, ind + "  ")
        elif isinstance(v, list):
            lines.append(f"{ind}{_okey(k)}:")
            items(v, ind + "  ")
        else:
            cell, extra = scalar_cell(v, ind)
            lines.append(f"{ind}{_okey(k)}: {cell}")
            lines.extend(extra)

    def obj(o, ind):
        for k, v in o.items():
            field(k, v, ind)

    def items(seq, ind):
        for x in seq:
            if isinstance(x, dict) and x and not is_unknown(x):
                first = True
                for k, v in x.items():
                    if first:
                        before = len(lines)
                        field(k, v, ind + "  ")
                        lines[before] = ind + "- " + lines[before][len(ind) + 2:]
                        first = False
                    else:
                        field(k, v, ind + "  ")
            elif isinstance(x, list) and x and simple_of(x) is None:
                lines.append(f"{ind}-")
                items(x, ind + "  ")
            else:
                simple = simple_of(x) if isinstance(x, (dict, list)) else None
                if simple is not None:
                    lines.append(f"{ind}- {simple}")
                else:
                    cell, extra = scalar_cell(x, ind)
                    lines.append(f"{ind}- {cell}")
                    lines.extend(extra)

    if isinstance(data, dict) and data and not (json_values and is_unknown(data)):
        obj(data, "")
    elif isinstance(data, list) and data and simple_of(data) is None:
        items(data, "")
    else:
        simple = simple_of(data) if isinstance(data, (dict, list)) else None
        return "= " + (simple if simple is not None else (dumps(data) if json_values else _inline(data)))
    return "\n".join(lines)


def encode_strict(data):
    """CSTL strict : alias de encode_objects_json."""
    return encode_objects_json(data)


def decode_strict(text):
    """CSTL strict : alias de decode_objects_json."""
    return decode_objects_json(text)


def encode_objects_json(data):
    """Forme « strict » (ex-« objets_json ») : voir encode_objects(json_values=True)."""
    return encode_objects(data, json_values=True)


def decode_objects_json(text):
    return decode_objects(text, json_values=True)


def decode_objects(text, json_values=False):
    """Inverse de encode_objects. Leve CompactError si une ligne ne colle pas."""
    _json_values = json_values
    lines = [ln for ln in text.split("\n")]
    pos = 0

    def indent(ln):
        return len(ln) - len(ln.lstrip(" "))

    def value_of(raw, ind, n):
        """Cellule d'une ligne, plus ses lignes de bloc « | » eventuelles."""
        nonlocal pos
        raw = raw.strip()
        if raw and raw[0] in "[{" and not _json_values:
            raw = "".join(_raw_tokens(raw))          # [a, b] : espaces apres les virgules permis
        if _json_values:
            if _IBLOCK.match(raw):
                pass
            else:
                try:
                    return json.loads(raw)
                except ValueError as e:
                    raise CompactError(f"ligne {n} : valeur JSON attendue : {raw[:60]!r} ({e})") from None
        if raw and raw[0] not in '"[{?|' and len(_raw_tokens(raw)) > 1:
            # Valeur sans guillemets avec des espaces : toute la fin de ligne est
            # le texte (comme en YAML). Jamais le premier mot seul en silence.
            return raw
        val, blk = _icell_in(raw) if raw else ("", None)
        if blk is not None:
            part = []
            for _ in range(blk):
                if pos >= len(lines):
                    raise CompactError(f"ligne {n} : bloc « | » incomplet")
                body = lines[pos].lstrip(" ")
                if body == "|":
                    part.append("")
                elif body.startswith("| "):
                    part.append(body[2:])
                else:
                    raise CompactError(f"ligne {pos + 1} : ligne de bloc « | » attendue")
                pos += 1
            val = "\n".join(part)
        return val

    def split_key(body, n):
        if body.startswith('"'):
            key, j = _parse_inline(body)
            if not isinstance(key, str) or body[j:j + 1] != ":":
                raise CompactError(f"ligne {n} : « cle: valeur » attendu")
            return key, body[j + 1:]
        m = re.match(r"(.*?):(?: |$)", body)
        if not m or not m.group(1) or m.group(1)[0] in "-\"?= \t":
            raise CompactError(f"ligne {n} : « cle: valeur » attendu : {body!r}")
        return m.group(1), body[m.end():]

    def skip_blank():
        nonlocal pos
        while pos < len(lines) and not lines[pos].strip():
            pos += 1

    def block(ind):
        """Bloc a l'indentation ind : objet (cle:) ou liste (- )."""
        skip_blank()
        if pos >= len(lines) or indent(lines[pos]) != ind:
            raise CompactError(f"ligne {pos + 1} : bloc indente de {ind} espaces attendu")
        return items(ind) if lines[pos].lstrip(" ").startswith("-") else obj(ind)

    def obj(ind, first_body=None):
        nonlocal pos
        out = {}
        while True:
            if first_body is not None:
                body, n, first_body = first_body, pos, None
            else:
                skip_blank()
                if pos >= len(lines) or indent(lines[pos]) < ind:
                    return out
                if indent(lines[pos]) > ind:
                    raise CompactError(f"ligne {pos + 1} : indentation inattendue")
                body = lines[pos][ind:]
                if body.startswith("-"):
                    return out
                pos += 1
                n = pos
            key, rest = split_key(body, n)
            if key in out:
                raise CompactError(f"ligne {n} : cle en double {key!r}")
            if rest.strip():
                out[key] = value_of(rest, ind, n)
            else:
                out[key] = block(ind + 2)

    def items(ind):
        nonlocal pos
        out = []
        while True:
            skip_blank()
            if pos >= len(lines) or indent(lines[pos]) != ind or not lines[pos][ind:].startswith("-"):
                if pos < len(lines) and lines[pos].strip() and indent(lines[pos]) > ind:
                    raise CompactError(f"ligne {pos + 1} : indentation inattendue")
                return out
            body = lines[pos][ind + 1:]
            pos += 1
            n = pos
            if not body.strip():
                out.append(block(ind + 2))
            elif not body.startswith(" "):
                raise CompactError(f"ligne {n} : « - » suivi d'un espace attendu")
            else:
                body = body[1:]
                # Un champ « cle: valeur » ; une valeur simple non entre guillemets
                # ne contient jamais d'espace, donc jamais « : ».
                looks_field = (body.startswith('"') and re.match(r'"(?:[^"\\]|\\.)*":', body)) or \
                    (body[0] not in '"[{?' and re.match(r"[^ ].*?:(?: |$)", body))
                if looks_field:
                    out.append(obj(ind + 2, first_body=body))
                else:
                    out.append(value_of(body, ind, n))

    text_body = text.strip("\n")
    if text_body.startswith("= "):
        if json_values:
            return json.loads(text_body[2:])
        val, j = _parse_inline(text_body[2:].strip())
        return val
    if not text_body.strip():
        return {}
    first = text_body.lstrip(" ")
    result = items(0) if first.startswith("-") else obj(0)
    skip_blank()
    if pos < len(lines):
        raise CompactError(f"ligne {pos + 1} : texte en trop")
    return result


# ------------------------------------------- formes specialisees (2026-11-24)
# serie, graphe, conversation, appels, matrice : voir cstl_formes.py. Elles
# rejoignent le CSTL polymorphe ; une forme qui ne convient pas aux donnees
# leve CompactError et n'est simplement pas candidate.
_data_kind_base = data_kind


def data_kind(data):  # noqa: F811 -- etend la version de base
    """Sorte de donnees : relations, appels, conversation, serie, graphe,
    matrice (quand la forme specialisee les represente sans perte), sinon
    tabulaire ou irregulier."""
    if _is_relations(data):
        return "relations"
    for form in ("changements", "appels", "conversation", "serie", "graphe", "matrice"):
        try:
            text = _ENCODERS[form](data)
            if _DECODERS[form](text) == data:
                return form
        except Exception:  # noqa: BLE001
            continue
    return _data_kind_base(data)


# Enregistrement des formes specialisees : fait par cstl_formes lui-meme (a la
# fin de son chargement), quel que soit le module importe en premier.
try:
    import cstl_formes  # noqa: E402,F401
except ImportError:      # cstl_compact seul (ex. fichier tout-en-un) : formes de base uniquement
    pass
''')
toon_lite = _module('toon_lite', r'''"""
scripts/toon_lite.py -- encodeur et decodeur TOON (Token-Oriented Object
Notation) limites a ce dont les bancs d'essai ont besoin (2026-11-21).

Suit la specification officielle (github.com/toon-format/spec, SPEC.md v4.4) :
  - objet : « cle: valeur », 2 espaces d'indentation par niveau ;
  - tableau uniforme d'objets : en-tete « cle[N]{c1,c2}: » puis une ligne par
    objet, cellules separees par des virgules (delimiteur par defaut) ;
  - true, false, null en minuscules ; nombres en decimal simple ;
  - une chaine est entre guillemets si elle est vide, a un espace au bord,
    vaut true/false/null, ressemble a un nombre, contient « : », « " »,
    « \\ », « [ ] { } », un caractere de controle ou le delimiteur, ou
    commence par « - » ou « # » ;
  - pas de ligne vide, pas d'espace en fin de ligne, pas de saut de ligne
    final.
RESULTAT (2026-11-21, `format_short_benchmark.py` et `compact_writing_benchmark.py`,
3 passes x 12 politiques ; fichiers scripts/toon_lecture_*.json, toon_ecriture_*.json) :
  LECTURE              tokens donnees   tout juste Haiku   tout juste Opus
    CSTL compact             782            24/36              36/36
    JSON abrege            1 033            22/36              36/36
    TOON                   1 046            20/36              36/36
  ECRITURE (Haiku)     exactes   tokens ecrits
    CSTL compact        18/36        785
    JSON                30/36      2 143
    TOON                32/36      1 045
  ECRITURE (Opus) : TOON non mesure (credit epuise). Serie precedente : CSTL
  compact 36/36, JSON 30/36.
- En lecture, le CSTL compact coute 25 % de tokens de moins que TOON, a
  comprehension egale (Opus 36/36 partout ; Haiku 24, 22, 20 sur 36).
- En ecriture, Haiku ecrit mieux le TOON que le CSTL compact (32/36 contre
  18/36), et presque aussi court (1 045 tokens contre 2 143 en JSON). Le
  CSTL compact reste le plus court (785), mais Haiku y oublie des regles et
  confond if: et unless:.
- Scores TOON d'ecriture recalcules : Haiku ecrivait souvent « role:support »
  dans la colonne role, que la premiere lecture transformait en
  role:role:support. C'etait une ambiguite du schema, pas une faute du format.
- TOON a des tableaux a en-tete : il paie « null » pour chaque condition
  absente et des guillemets autour de role:<nom> (le « : » l'impose).

Le paquet officiel `toon-format` n'a pas pu etre installe dans cet
environnement ; cet encodeur n'en couvre qu'une partie (objets, tableaux
uniformes d'objets a cellules primitives).
"""
import json
import re

NUMERIC = re.compile(r"^[+-]?[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$", re.I)
KEY = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")


def _needs_quotes(s):
    return (s == "" or s != s.strip() or s in ("true", "false", "null") or NUMERIC.match(s) is not None
            or any(c in s for c in ':"\\[]{},') or any(ord(c) < 0x20 for c in s) or s[0] in "-#")


def _quote(s):
    out = s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
    return f'"{out}"'


def _prim(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, (int, float)):
        if isinstance(v, float) and v.is_integer():
            v = int(v)
        return str(v)
    s = str(v)
    return _quote(s) if _needs_quotes(s) else s


def _key(k):
    return k if KEY.match(k) else _quote(k)


def encode(obj, indent=0):
    """Objet {cle: primitive | objet | liste uniforme d'objets} -> TOON."""
    pad = "  " * indent
    lines = []
    for k, v in obj.items():
        if isinstance(v, dict):
            lines.append(f"{pad}{_key(k)}:")
            if v:
                lines.append(encode(v, indent + 1))
        elif isinstance(v, list):
            if not v:
                lines.append(f"{pad}{_key(k)}: []")
                continue
            fields = list(v[0].keys())
            assert all(isinstance(x, dict) and set(x) == set(fields) for x in v), "tableau non uniforme"
            lines.append(f"{pad}{_key(k)}[{len(v)}]{{{','.join(_key(f) for f in fields)}}}:")
            lines += [f"{pad}  " + ",".join(_prim(x[f]) for f in fields) for x in v]
        else:
            lines.append(f"{pad}{_key(k)}: {_prim(v)}")
    return "\n".join(lines)


class ToonError(ValueError):
    pass


def _split_cells(row):
    cells, cur, quoted, i = [], [], False, 0
    while i < len(row):
        c = row[i]
        if quoted:
            if c == "\\" and i + 1 < len(row):
                cur.append({"n": "\n", "r": "\r", "t": "\t"}.get(row[i + 1], row[i + 1]))
                i += 2
                continue
            if c == '"':
                quoted = False
            else:
                cur.append(c)
        elif c == '"':
            quoted = True
            cur.append("\0")   # marque : la cellule etait entre guillemets
        elif c == ",":
            cells.append("".join(cur))
            cur = []
        else:
            cur.append(c)
        i += 1
    if quoted:
        raise ToonError(f"guillemets mal fermes : {row!r}")
    cells.append("".join(cur))
    return cells


def _value(cell):
    cell = cell.strip()
    if cell.startswith("\0"):
        return cell.replace("\0", "")
    if cell in ("true", "false"):
        return cell == "true"
    if cell == "null":
        return None
    if NUMERIC.match(cell):
        n = float(cell)
        return int(n) if n.is_integer() else n
    return cell


HEADER = re.compile(r'^(\s*)("?[^"\[]+"?)\[(\d+)\]\{(.*)\}:\s*$')
FIELD = re.compile(r'^(\s*)("?[^":]+"?):\s*(.*)$')


def decode(text):
    """TOON (le sous-ensemble ci-dessus) -> objet. Renvoie (objet, erreurs de
    longueur) : une longueur [N] fausse est signalee, pas fatale."""
    root, stack, count_errors = {}, [(-1, None)], []
    table = None
    for raw in text.splitlines():
        if not raw.strip():
            continue
        depth = len(raw) - len(raw.lstrip(" "))
        if table is not None and depth > table["depth"]:
            cells = [_value(c) for c in _split_cells(raw.strip())]
            if len(cells) != len(table["fields"]):
                raise ToonError(f"ligne de tableau a {len(cells)} cellules au lieu de {len(table['fields'])} : {raw!r}")
            table["rows"].append(dict(zip(table["fields"], cells)))
            continue
        if table is not None:
            if len(table["rows"]) != table["n"]:
                count_errors.append(f"{table['key']}[{table['n']}] mais {len(table['rows'])} lignes")
            table = None
        while stack and stack[-1][0] >= depth:
            stack.pop()
        parent = stack[-1][1] if stack[-1][1] is not None else root
        h = HEADER.match(raw)
        if h:
            key = h.group(2).strip('"')
            rows = []
            parent[key] = rows
            table = {"depth": depth, "fields": [f.strip().strip('"') for f in h.group(4).split(",")], "rows": rows,
                     "n": int(h.group(3)), "key": key}
            continue
        f = FIELD.match(raw)
        if not f:
            raise ToonError(f"ligne illisible : {raw!r}")
        key, rest = f.group(2).strip('"'), f.group(3)
        if rest == "":
            child = {}
            parent[key] = child
            stack.append((depth, child))
        elif rest == "[]":
            parent[key] = []
        else:
            parent[key] = _value(_split_cells(rest)[0])
    if table is not None and len(table["rows"]) != table["n"]:
        count_errors.append(f"{table['key']}[{table['n']}] mais {len(table['rows'])} lignes")
    return root, count_errors


def policy_object(rels):
    """Les relations d'une politique, sous la forme la plus favorable a TOON :
    trois tableaux uniformes (null quand une condition n'existe pas)."""
    roles = [{"agent": r["subject"], "role": r["object"][5:]} for r in rels if r["type"] == "POSSESSES"]
    facts = {r["subject"]: r["object"] == "true" for r in rels if r["type"] == "STATE"}
    rules = [{"subject": r["subject"], "modality": r["modality"], "action": r["object"],
              "amount_above": int(r["amount_above"]) if "amount_above" in r else None,
              "if_fact": r.get("if_fact"), "unless_fact": r.get("unless_fact")}
             for r in rels if r["type"] == "PERFORM"]
    return {"roles": roles, "facts": facts, "rules": rules}


def relations_of(obj):
    """Inverse de `policy_object` : objet decode -> relations."""
    out = []
    for r in obj.get("roles", []) or []:
        role = str(r["role"])
        role = role[5:] if role.startswith("role:") else role   # « role:x » ou « x » : les deux se lisent
        out.append({"type": "POSSESSES", "subject": str(r["agent"]), "object": f"role:{role}"})
    for f, v in (obj.get("facts") or {}).items():
        out.append({"type": "STATE", "subject": f, "object": "true" if v is True or v == "true" else "false"})
    for r in obj.get("rules", []) or []:
        rel = {"type": "PERFORM", "subject": str(r["subject"]), "object": str(r["action"]), "modality": str(r["modality"])}
        for k in ("amount_above", "if_fact", "unless_fact"):
            if r.get(k) not in (None, ""):
                rel[k] = str(r[k])
        out.append(rel)
    return out


if __name__ == "__main__":
    # Exemple de la specification (README) : doit sortir a l'identique.
    users = {"users": [{"id": 1, "name": "Ada", "role": "admin"}, {"id": 2, "name": "Bob", "role": "user"}]}
    assert encode(users) == "users[2]{id,name,role}:\n  1,Ada,admin\n  2,Bob,user", encode(users)
    assert encode({"user": {"id": 123, "name": "Ada"}}) == "user:\n  id: 123\n  name: Ada"
    assert decode(encode(users))[0] == users
    print(json.dumps(decode(encode(users))[0]))


# ---------------------------------------------------------------- imbrication
# 2026-11-22 : donnees imbriquees (scripts/nested_benchmark.py), meme
# specification (v4.4) :
#   - tableau de primitives : cle[N]: a,b,c
#   - tableau d'objets aux MEMES cles et aux valeurs toutes primitives : forme
#     tabulaire cle[N]{c1,c2}: puis une ligne par objet ;
#   - sinon, forme liste : cle[N]: puis un element par ligne « - » au niveau
#     suivant ; pour un objet, son premier champ va sur la ligne du tiret et
#     les suivants un niveau plus loin ;
#   - objet imbrique : cle: puis ses champs un niveau plus loin.

def _is_prim(v):
    return v is None or isinstance(v, (str, int, float, bool))


def _field_lines(k, v, depth):
    pad = "  " * depth
    if isinstance(v, dict):
        return [f"{pad}{_key(k)}:"] + _object_lines(v, depth + 1)
    if isinstance(v, list):
        if not v:
            return [f"{pad}{_key(k)}: []"]
        if all(_is_prim(x) for x in v):
            return [f"{pad}{_key(k)}[{len(v)}]: " + ",".join(_prim(x) for x in v)]
        if all(isinstance(x, dict) and x for x in v):
            fields = list(v[0].keys())
            if all(set(x) == set(fields) and all(_is_prim(y) for y in x.values()) for x in v):
                return ([f"{pad}{_key(k)}[{len(v)}]{{{','.join(_key(f) for f in fields)}}}:"]
                        + [f"{pad}  " + ",".join(_prim(x[f]) for f in fields) for x in v])
        out = [f"{pad}{_key(k)}[{len(v)}]:"]
        for x in v:
            out += _item_lines(x, depth + 1)
        return out
    return [f"{pad}{_key(k)}: {_prim(v)}"]


def _item_lines(x, depth):
    pad = "  " * depth
    if not isinstance(x, dict):
        return [f"{pad}- {_prim(x)}"]
    items = list(x.items())
    k0, v0 = items[0]
    assert _is_prim(v0), "premier champ d'un element de liste : primitive attendue"
    return [f"{pad}- {_key(k0)}: {_prim(v0)}"] + [ln for k, v in items[1:] for ln in _field_lines(k, v, depth + 1)]


def _object_lines(obj, depth):
    return [ln for k, v in obj.items() for ln in _field_lines(k, v, depth)]


def encode_nested(obj):
    """Objet JSON quelconque (objets, listes, primitives) -> TOON."""
    return "\n".join(_object_lines(obj, 0))


_HEAD = re.compile(r'^("(?:[^"\\]|\\.)*"|[^\s:\[]+)\[(\d+)\](?:\{(.*)\})?:\s*(.*)$')
_KV = re.compile(r'^("(?:[^"\\]|\\.)*"|[^\s:\[]+):\s*(.*)$')


def _unkey(k):
    return json.loads(k) if k.startswith('"') else k


def decode_nested(text):
    """Inverse de `encode_nested` (meme sous-ensemble de la specification)."""
    lines = [(len(ln) - len(ln.lstrip(" ")), ln.strip()) for ln in text.splitlines() if ln.strip()]
    pos = [0]

    def field(obj, depth, content):
        h = _HEAD.match(content)
        if h:
            key, n, fields, rest = _unkey(h.group(1)), int(h.group(2)), h.group(3), h.group(4)
            if fields is not None:
                names = [_unkey(f) for f in fields.split(",")]
                rows = []
                while pos[0] < len(lines) and lines[pos[0]][0] > depth * 2:
                    rows.append(dict(zip(names, [_value(c) for c in _split_cells(lines[pos[0]][1])])))
                    pos[0] += 1
                obj[key] = rows
            elif rest:
                obj[key] = [_value(c) for c in _split_cells(rest)]
            else:
                items = []
                while pos[0] < len(lines) and lines[pos[0]][0] == (depth + 1) * 2 and lines[pos[0]][1].startswith("- "):
                    first = lines[pos[0]][1][2:]
                    pos[0] += 1
                    m = _KV.match(first)
                    if m and not first.startswith('"') or (m and first.startswith('"') and m.group(2)):
                        item = {}
                        field(item, depth + 2, first)
                        while pos[0] < len(lines) and lines[pos[0]][0] == (depth + 2) * 2 and not lines[pos[0]][1].startswith("- "):
                            c = lines[pos[0]][1]
                            pos[0] += 1
                            field(item, depth + 2, c)
                        items.append(item)
                    else:
                        items.append(_value(_split_cells(first)[0]))
                if len(items) != n:
                    raise ToonError(f"{key}[{n}] annonce, {len(items)} elements")
                obj[key] = items
            return
        m = _KV.match(content)
        if not m:
            raise ToonError(f"ligne illisible : {content!r}")
        key, rest = _unkey(m.group(1)), m.group(2)
        if rest == "[]":
            obj[key] = []
        elif rest:
            obj[key] = _value(_split_cells(rest)[0])
        else:
            child = {}
            while pos[0] < len(lines) and lines[pos[0]][0] == (depth + 1) * 2:
                c = lines[pos[0]][1]
                pos[0] += 1
                field(child, depth + 1, c)
            obj[key] = child

    root = {}
    while pos[0] < len(lines):
        c = lines[pos[0]][1]
        pos[0] += 1
        field(root, 0, c)
    return root
''')

KINDS = ('commandes', 'organisation', 'contrats')
NOTES = {'cstl_indent': "Format : CSTL compact en arbre indente. En tete, une ligne par liste : « @nom[N]^parent: colonnes » (N elements en tout ; ^parent = liste dont ce sont les enfants). Puis l'arbre : un element par ligne, ses enfants en dessous, indentes de 2 espaces. Les valeurs suivent les colonnes ; un champ facultatif s'ecrit nom=valeur, et seulement s'il existe.", 'cstl_indent_fin': "Format : CSTL compact en arbre indente. En tete, une ligne par liste : « @nom^parent: colonnes » (^parent = liste dont ce sont les enfants). Puis l'arbre : un element par ligne, ses enfants en dessous, indentes de 2 espaces. Les colonnes sont les champs presents dans TOUS les elements de la liste ; un autre champ s'ecrit nom=valeur, et seulement s'il existe. Derniere ligne : « @end nom=N ... », le nombre d'elements ecrits dans chaque liste.", 'toon': 'Format : TOON. Indentation = imbrication ; « cle[N]{champs}: » puis une ligne par element ; « - » = element de liste.', 'json': 'Format : JSON imbrique.'}


def gen(kind, n, seed):
    rnd = random.Random(zlib.crc32(f"{kind}/{n}/{seed}".encode()))
    if kind == "commandes":
        items = ["gpu_rack", "license_pack", "support_plan", "storage_tb", "router"]
        orders = []
        for i in range(n):
            o = {"id": f"o{i}", "client": f"client_{rnd.randint(1, 6)}", "date": f"2026-11-{rnd.randint(1, 28):02d}"}
            if rnd.random() < 0.3:
                o["note"] = rnd.choice(["urgent", "fragile", "vip"])
            o["lines"] = []
            for _ in range(rnd.randint(1, 4)):
                line = {"item": rnd.choice(items), "qty": rnd.randint(1, 5), "price": rnd.randint(10, 900)}
                if rnd.random() < 0.4:
                    line["options"] = [{"name": nm, "price": rnd.randint(5, 80)}
                                       for nm in rnd.sample(["warranty", "express", "install"], rnd.randint(1, 2))]
                o["lines"].append(line)
            orders.append(o)
        return {"orders": orders}
    if kind == "organisation":
        depts = []
        for i in range(max(2, n // 4)):
            d = {"id": f"dept_{i}", "name": rnd.choice(["finance", "support", "ops", "legal", "rnd"]) + f"_{i}",
                 "budget": rnd.randint(10, 500) * 1000, "teams": []}
            for j in range(rnd.randint(1, 3)):
                t = {"id": f"team_{i}_{j}", "lead": f"agent_{rnd.randint(1, 40)}", "members": []}
                for _ in range(rnd.randint(1, 5)):
                    m = {"agent": f"agent_{rnd.randint(1, 40)}", "role": rnd.choice(["analyst", "operator", "reviewer"]),
                         "clearance": rnd.choice(["low", "medium", "high"])}
                    if rnd.random() < 0.3:
                        m["since"] = f"202{rnd.randint(3, 6)}-0{rnd.randint(1, 9)}-01"
                    t["members"].append(m)
                d["teams"].append(t)
            depts.append(d)
        return {"departments": depts}
    contracts = []
    actions = ["terminate", "renew", "audit", "disclose", "pay", "notify"]
    for i in range(n):
        c = {"id": f"ctr_{i}", "party": f"party_{rnd.randint(1, 9)}", "sections": []}
        for j in range(rnd.randint(1, 3)):
            s = {"title": rnd.choice(["payment", "termination", "privacy", "audit", "liability"]) + f"_{j}", "clauses": []}
            for k in range(rnd.randint(1, 4)):
                cl = {"ref": f"{i}.{j}.{k}", "modality": rnd.choice(["MUST", "MUST_NOT", "MAY"]), "action": rnd.choice(actions)}
                if rnd.random() < 0.4:
                    cl["amount_above"] = rnd.choice([100, 500, 1000, 5000])
                s["clauses"].append(cl)
            c["sections"].append(s)
        contracts.append(c)
    return {"contracts": contracts}


def questions(kind, data, seed):
    rnd = random.Random(seed * 97 + len(json.dumps(data)))
    if kind == "commandes":
        o = rnd.choice(data["orders"])
        total = sum(ln["qty"] * (ln["price"] + sum(op["price"] for op in ln.get("options", []))) for ln in o["lines"])
        return [(f"Valeur totale de la commande {o['id']} : somme, sur ses lignes, de qty x (price + prix de toutes les "
                 "options de la ligne). (un nombre)", total),
                ("Combien de lignes, toutes commandes confondues, ont une option nommee warranty ?",
                 sum(any(op["name"] == "warranty" for op in ln.get("options", [])) for o2 in data["orders"] for ln in o2["lines"])),
                ("Quelles commandes contiennent une ligne gpu_rack avec qty d'au moins 3 ? (liste d'id)",
                 sorted({o2["id"] for o2 in data["orders"] for ln in o2["lines"] if ln["item"] == "gpu_rack" and ln["qty"] >= 3},
                        key=lambda x: int(x[1:])))]
    if kind == "organisation":
        d = rnd.choice(data["departments"])
        members = [(t, m) for d2 in data["departments"] for t in d2["teams"] for m in t["members"]]
        a = rnd.choice(members)[1]["agent"]
        return [(f"Combien de membres ont la clearance high dans le departement {d['id']} ?",
                 sum(m["clearance"] == "high" for t in d["teams"] for m in t["members"])),
                (f"Quels sont les lead des equipes dont {a} est membre ? (liste, sans doublon)",
                 sorted({t["lead"] for t, m in members if m["agent"] == a})),
                ("Quels departements ont un budget superieur a 200000 ET au moins une equipe de plus de 3 membres ? (liste d'id)",
                 sorted(d2["id"] for d2 in data["departments"] if d2["budget"] > 200000 and any(len(t["members"]) > 3 for t in d2["teams"])))]
    c = rnd.choice(data["contracts"])
    cl = rnd.choice([x for c2 in data["contracts"] for s in c2["sections"] for x in s["clauses"]])
    return [(f"Combien de clauses MUST_NOT le contrat {c['id']} contient-il, toutes sections confondues ?",
             sum(x["modality"] == "MUST_NOT" for s in c["sections"] for x in s["clauses"])),
            ("Quels contrats ont une clause d'action terminate avec un amount_above inferieur a 1000 ? (liste d'id)",
             sorted({c2["id"] for c2 in data["contracts"] for s in c2["sections"] for x in s["clauses"]
                     if x["action"] == "terminate" and x.get("amount_above") is not None and x["amount_above"] < 1000},
                    key=lambda x: int(x.split("_")[1]))),
            (f"Quel est le title de la section qui contient la clause de ref {cl['ref']} ? (une chaine)",
             next(s["title"] for c2 in data["contracts"] for s in c2["sections"] if cl in s["clauses"]))]


def same(answer, truth):
    if isinstance(truth, list):
        return isinstance(answer, list) and sorted(str(x).strip() for x in answer) == sorted(str(x) for x in truth)
    if isinstance(truth, (int, float)) and not isinstance(truth, bool):
        try:
            return abs(float(answer) - float(truth)) < 0.006
        except (TypeError, ValueError):
            return False
    return str(answer).strip() == str(truth)


def leaves(v, path=""):
    """Valeurs feuilles : {(chemin, valeur JSON)}."""
    if isinstance(v, dict):
        return set().union(*(leaves(x, f"{path}/{k}") for k, x in v.items())) if v else {(path, "{}")}
    if isinstance(v, list):
        return set().union(*(leaves(x, f"{path}[{i}]") for i, x in enumerate(v))) if v else {(path, "[]")}
    return {(path, json.dumps(v))}


def strip_fences(raw):
    m = re.search(r"```[a-zA-Z]*\n(.*?)```", raw, re.S)
    return (m.group(1) if m else raw).strip("\n")


def render(fmt, data):
    if fmt == "json":
        return json.dumps(data, ensure_ascii=False, separators=(",", ":"))
    if fmt == "toon":
        return toon_lite.encode_nested(data)
    return cstl_compact.encode_tree_indented(data, counts="end" if fmt == "cstl_indent_fin" else "head")


def decode(fmt, text, lenient=False):
    if fmt == "json":
        return json.loads(text)
    if fmt == "toon":
        return toon_lite.decode_nested(text)
    return cstl_compact.decode_tree_indented(text, lenient=lenient)


def prompt_ecriture(fmt, data, kind):
    ex = gen(KINDS[(KINDS.index(kind) + 1) % len(KINDS)], 3, 99)
    return (f"Convertis le document JSON ci-dessous dans le format decrit.\n{NOTES[fmt]}\n\n"
            f"Exemple (autres donnees).\nJSON :\n{json.dumps(ex, ensure_ascii=False)}\n"
            f"Meme document dans le format :\n{render(fmt, ex)}\n\n"
            f"Document a convertir :\n{json.dumps(data, ensure_ascii=False)}\n\n"
            "Reponds uniquement par le document converti, sans explication, dans UN SEUL bloc de code (```), "
            "pour garder l'indentation.")


def prompt_lecture(fmt, data, kind, seed):
    lines = "\n".join(f"{i + 1}. {q}" for i, (q, _) in enumerate(questions(kind, data, seed)))
    return (f"Voici des donnees echangees entre agents.\n{NOTES[fmt]}\n\nDonnees :\n{render(fmt, data)}\n\n"
            f"Questions :\n{lines}\n\nReponds uniquement par {{\"r1\": ..., \"r2\": ..., \"r3\": ...}}.")


def check_ecriture(fmt, data, raw):
    text = strip_fences(raw)
    out = []
    try:
        back, err = decode(fmt, text), None
    except Exception as e:  # noqa: BLE001 -- toute erreur du decodeur = illisible
        back, err = None, f"{type(e).__name__}: {e}"
    if err and fmt.startswith("cstl_indent"):
        warn = []
        try:
            soft = cstl_compact.decode_tree_indented(text, lenient=True, warnings=warn)
        except Exception:  # noqa: BLE001
            soft = None
        if soft == data:
            out.append(f"  STRICT : refuse ({err[:120]})")
            out.append("  TOLERANT : IDENTIQUE au JSON de depart" + "".join(f"\n    avertissement : {w}" for w in warn))
            return out, 1.0, "tolerant"
    truth = leaves(data)
    got = leaves(back) if err is None else set()
    score = len(truth & got) / max(len(truth), len(got), 1)
    if err:
        out.append(f"  ILLISIBLE : {err[:200]}")
        verdict = "illisible"
    elif back == data:
        out.append("  IDENTIQUE au JSON de depart")
        verdict = "identique"
    else:
        verdict = "different"
        out.append("  LISIBLE mais DIFFERENT. Premieres differences :")
        out += [f"    manque ou faux : {p} = {v}" for p, v in sorted(truth - got)[:6]]
        out += [f"    en trop        : {p} = {v}" for p, v in sorted(got - truth)[:6]]
    return out, score, verdict


def check_lecture(data, kind, seed, raw):
    a, b = raw.find("{"), raw.rfind("}")
    try:
        ans = json.loads(raw[a:b + 1])
    except ValueError:
        return ["  ILLISIBLE : JSON {\"r1\": ..., \"r2\": ..., \"r3\": ...} attendu"], 0
    out, ok = [], 0
    for i, (q, truth) in enumerate(questions(kind, data, seed)):
        got = ans.get(f"r{i + 1}")
        good = same(got, truth)
        ok += good
        out.append(f"  {'OK  ' if good else 'FAUX'} r{i + 1} : modele {got!r} | attendu {truth!r}")
    return out, ok


TESTS = [f"ecriture_{f}" for f in ("cstl_indent", "cstl_indent_fin", "toon", "json")] + \
        [f"lecture_{f}" for f in ("cstl_indent", "toon", "json")]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["consignes", "verifier"])
    ap.add_argument("dossier")
    ap.add_argument("--kind", default="contrats", choices=KINDS)
    ap.add_argument("--size", type=int, default=24)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    d = Path(args.dossier)
    data = gen(args.kind, args.size, args.seed)
    if args.action == "consignes":
        d.mkdir(parents=True, exist_ok=True)
        for t in TESTS:
            kind, fmt = t.split("_", 1)
            text = prompt_ecriture(fmt, data, args.kind) if kind == "ecriture" else prompt_lecture(fmt, data, args.kind, args.seed)
            (d / f"{t}.txt").write_text(text + "\n", encoding="utf-8")
        print(f"{len(TESTS)} consignes ecrites dans {d}/ :")
        for t in TESTS:
            print(f"  {t}.txt  ->  reponse attendue dans {t}.reponse.txt")
        print("Une NOUVELLE conversation par consigne. Puis : python3 cstl_essai_tout_en_un.py verifier", d)
        return
    rows = []
    for t in TESTS:
        f = d / f"{t}.reponse.txt"
        if not f.exists():
            continue
        raw = f.read_text(encoding="utf-8")
        kind, fmt = t.split("_", 1)
        print(f"\n{t}")
        if kind == "ecriture":
            lines, score, verdict = check_ecriture(fmt, data, raw)
            print("\n".join(lines))
            print(f"  valeurs justes : {100 * score:.1f} %")
            rows.append((t, f"{verdict}, {100 * score:.1f} % des valeurs"))
        else:
            lines, ok = check_lecture(data, args.kind, args.seed, raw)
            print("\n".join(lines))
            rows.append((t, f"{ok}/3"))
    if not rows:
        sys.exit(f"Aucune reponse trouvee dans {d}/ (fichiers <test>.reponse.txt).")
    print("\nRESUME")
    for t, r in rows:
        print(f"  {t:26} {r}")


if __name__ == "__main__":
    main()
