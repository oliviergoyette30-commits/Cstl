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
import re

MODALITIES = ("MUST", "MUST_NOT", "MAY", "REQUIRE", "FORBID", "PERMIT")
PREFIXES = [("total<=", "total_at_most"), ("total>", "total_above"), ("<=", "amount_at_most"), (">", "amount_above"),
            ("window=", "total_window_secs"), ("if:", "if_fact"), ("unless:", "unless_fact"),
            ("from:", "valid_from"), ("until:", "valid_until")]
SHORT = {attr: prefix for prefix, attr in PREFIXES}


class CompactError(ValueError):
    pass


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


def encode(relations, table=False):
    """Relations (dicts, comme pour CstlAgent.send_relations) -> texte compact.
    `table=True` : mode tableau (en-tetes de colonnes, voir la doc du module)."""
    rels = [{k: str(v) for k, v in r.items()} for r in relations]
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
        lines.append(f"{t}[{len(rs)}] subject object" + "".join(f" {_token(c)}" for c in cols))
        lines += ["  " + " ".join([_cell(r["subject"]) if r["subject"] != "" else '""',
                                   _cell(r["object"]) if r["object"] != "" else '""']
                                  + [_cell(r.get(c)) if r.get(c) != "" else '""' for c in cols]) for r in rs]
    return "\n".join(lines)


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
                out.append(_rule_from(toks[6:], words[6:], n, line, r))
            continue
        m = SECTION.match(line.strip()) if not indented else None
        if m:
            close()
            section = [m.group(1), int(m.group(2)) if m.group(2) else None, 0]
            continue
        g = GENERIC.match(line.strip()) if not indented else None
        if g:
            close()
            cols = [w for w, _ in _split(g.group(3))] if g.group(3).strip() else []
            section = ["GENERIC", int(g.group(2)), 0, cols, g.group(1)]
            continue
        close()
        section = None
        head = toks[0]
        if head == "ROLE":
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
            out.append(_rule_from(toks[3:], words[3:], n, line, base))
    close()
    return out, problems


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
                         "if_fact", "unless_fact", "valid_from", "valid_until"):
                if e.get(attr):
                    parts.append(SHORT[attr] + _token(e[attr]))
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
