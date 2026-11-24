"""
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
    """Mots d'une ligne, guillemets compris (pour relire « nom=valeur »)."""
    out, cur, quoted, i = [], [], False, 0
    while i < len(line):
        ch = line[i]
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
