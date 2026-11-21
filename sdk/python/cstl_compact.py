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


def encode(relations):
    """Relations (dicts, comme pour CstlAgent.send_relations) -> texte compact."""
    lines = []
    for r in relations:
        r = {k: str(v) for k, v in r.items()}
        t = r.get("type", "")
        if t == "POSSESSES" and r.get("object", "").startswith("role:") and set(r) == {"type", "subject", "object"}:
            lines.append(f"ROLE {_token(r['subject'])} {_token(r['object'][5:])}")
        elif t == "STATE" and r.get("object") in ("true", "false") and set(r) == {"type", "subject", "object"}:
            lines.append(f"FACT {_token(r['subject'])} {r['object']}")
        elif t == "PERFORM" and r.get("modality") in MODALITIES and r.get("subject") and r.get("object"):
            parts = [_token(r["subject"]), r["modality"], _token(r["object"])]
            for k, v in r.items():
                if k in ("type", "subject", "object", "modality", "reason"):
                    continue
                parts.append(SHORT[k] + _token(v) if k in SHORT else f"{k}={_token(v)}")
            if "reason" in r:
                parts.append(_quote(r["reason"]))
            lines.append(" ".join(parts))
        else:
            rest = [f"{k}={_token(v)}" for k, v in r.items() if k not in ("type", "subject", "object")]
            lines.append(" ".join(["REL", _token(t), _token(r.get("subject", "")), _token(r.get("object", ""))] + rest))
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


def decode(text):
    """Texte compact -> relations completes. Leve CompactError sur une ligne
    illisible (jamais de devinette)."""
    out = []
    for n, line in enumerate(text.splitlines(), 1):
        words = _split(line)
        if not words:
            continue
        toks = [w for w, _ in words]
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
            r = {"type": "PERFORM", "subject": toks[0], "object": toks[2], "modality": toks[1]}
            rest = toks[3:]
            if rest and words[-1][1]:
                r["reason"] = rest.pop()
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
            out.append(r)
    return out


GUIDE = (
    "Format CSTL compact (une ligne par element) :\n"
    "- ROLE <agent> <role> : l'agent a ce role.\n"
    "- FACT <fait> true|false : le fait est vrai ou faux.\n"
    "- <qui> <MUST|MUST_NOT|MAY> <action> [conditions] [\"raison\"] : une regle. <qui> est un agent ou role:<nom>.\n"
    "  Conditions : >X (montant au-dela de X), <=X (montant au plus X), total>X / total<=X (total deja autorise "
    "plus ce montant), if:F (seulement si le fait F est vrai), unless:F (seulement si F n'est pas vrai), "
    "from:D / until:D (periode de validite)."
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
