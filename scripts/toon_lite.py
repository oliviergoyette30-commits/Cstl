"""
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
