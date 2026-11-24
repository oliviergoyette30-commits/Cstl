"""
cstl_formes.py -- formes CSTL specialisees, une par type de donnees
(2026-11-24). Le CSTL polymorphe (cstl_compact.choose_format) les essaie avec
les autres : une forme qui ne convient pas aux donnees leve CompactError et
n'est pas candidate ; une forme candidate n'est retenue que si elle se relit a
l'identique.

  serie         series temporelles : debut, pas, puis seulement les valeurs
                  @serie mesures[3] t de=2026-11-24T10:00:00Z pas=60: temp hum
                  21.5 40
  graphe        graphes : noeuds en table, aretes « a>b »
                  @noeuds nodes[2]: id label
                  n1 Alice
                  @aretes edges[1] source>target: type
                  n1>n2 knows
  conversation  messages {role, content} : « #role » puis le texte brut
                  #user
                  Bonjour, peux-tu...
  appels        appels d'outils {name, arguments} : syntaxe d'appel
                  @appels
                  rembourser(commande="o1", montant=80)
  matrice       listes de nombres : une rangee par ligne
                  @matrice 2x3
                  1 2 3
                  4 5 6

Les autres types sont couverts par les formes de cstl_compact : evenements et
journaux, plans et taches, tables -> arbre ; objets irreguliers, donnees
geographiques -> objets_json ; regles et faits -> symboles, lignes, complet ;
texte long -> blocs |N. Les medias ne se transmettent que par reference.
"""
import datetime as _dt
import json
import re

from cstl_compact import (CompactError, _icell, _icell_in, _iname, _inamed, _is_named, _inline,
                          _iscalar_out, _raw_tokens, _split, _iscalar_in)

_J = lambda v: json.dumps(v, ensure_ascii=False, separators=(",", ":"))  # noqa: E731
_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")


def _cells(o, cols):
    """Une ligne : les colonnes, puis les autres champs « nom=valeur »."""
    blocks = []
    cells = [_icell(o[c], blocks) for c in cols]
    cells += [f"{_iname(k)}={_icell(v, blocks)}" for k, v in o.items() if k not in cols]
    if blocks:
        raise CompactError("texte multiligne : forme non adaptee")
    return " ".join(cells) if cells else "."


def _uncells(line, cols, n):
    toks = _raw_tokens(line.strip())
    if toks == ["."]:
        toks = []
    if len(toks) < len(cols):
        raise CompactError(f"ligne {n} : {len(cols)} valeurs attendues")
    obj = {}
    for c, t in zip(cols, toks):
        val, blk = _icell_in(t)
        if blk is not None:
            raise CompactError(f"ligne {n} : bloc non permis ici")
        obj[c] = val
    for t in toks[len(cols):]:
        name, raw = _inamed(t)
        obj[name] = _icell_in(raw)[0] if raw else ""
    return obj


def _root_fields(d, skip):
    extra = {k: v for k, v in d.items() if k not in skip}
    if not extra:
        return []
    blocks = []
    line = "@@ " + " ".join(f"{_iname(k)}={_icell(v, blocks)}" for k, v in extra.items())
    if blocks:
        raise CompactError("texte multiligne a la racine : forme non adaptee")
    return [line]


def _read_root_fields(line, n):
    out = {}
    for t in _raw_tokens(line[3:].strip()):
        name, raw = _inamed(t)
        out[name] = _icell_in(raw)[0] if raw else ""
    return out


# ------------------------------------------------------------ serie temporelle

_TIME_KEYS = ("t", "time", "timestamp", "ts", "date", "datetime", "heure", "instant")
_ISO_FORMATS = ("%Y-%m-%dT%H:%M:%SZ", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S+00:00",
                "%Y-%m-%dT%H:%MZ", "%Y-%m-%dT%H:%M", "%Y-%m-%d")


def _iso_format(s):
    for f in _ISO_FORMATS:
        try:
            if _dt.datetime.strptime(s, f).strftime(f) == s:
                return f
        except (ValueError, TypeError):
            continue
    return None


def _series_parts(data):
    """(nom de liste, liste, champs de racine) d'un document serie."""
    if isinstance(data, list):
        return "_", data, {}
    if isinstance(data, dict):
        lists = [k for k, v in data.items() if isinstance(v, list) and len(v) >= 3 and all(isinstance(x, dict) for x in v)]
        if len(lists) == 1 and _NAME.match(lists[0]):
            return lists[0], data[lists[0]], {k: v for k, v in data.items() if k != lists[0]}
    raise CompactError("pas une serie")


def encode_serie(data):
    name, pts, root = _series_parts(data)
    if len(pts) < 3 or not all(isinstance(p, dict) for p in pts):
        raise CompactError("pas une serie")
    keys = list(pts[0])
    if any(list(p) != keys for p in pts):
        raise CompactError("serie : champs differents d'un point a l'autre")
    tkey = next((k for k in keys if k.lower() in _TIME_KEYS), None)
    if tkey is None:
        raise CompactError("serie : pas de champ de temps")
    ts = [p[tkey] for p in pts]
    if all(isinstance(t, int) and not isinstance(t, bool) for t in ts):
        start, step, fmt = ts[0], ts[1] - ts[0], None
        if any(ts[i] != start + i * step for i in range(len(ts))):
            raise CompactError("serie : pas irregulier")
        de = _J(start)
    elif all(isinstance(t, str) for t in ts):
        fmt = _iso_format(ts[0])
        if not fmt or any(_iso_format(t) != fmt for t in ts):
            raise CompactError("serie : dates de formats differents")
        d0 = _dt.datetime.strptime(ts[0], fmt)
        step = int((_dt.datetime.strptime(ts[1], fmt) - d0).total_seconds())
        if any((d0 + _dt.timedelta(seconds=i * step)).strftime(fmt) != t for i, t in enumerate(ts)):
            raise CompactError("serie : pas irregulier")
        de = ts[0]
    else:
        raise CompactError("serie : temps ni entier ni date ISO")
    cols = [k for k in keys if k != tkey]
    # le « : » qui ouvre les colonnes est un mot a lui seul : jamais colle a une valeur
    head = (f"@serie {name}[{len(pts)}] {_iname(tkey)} de={_icell(de, []) if fmt else de} pas={step}"
            + (f" format={_icell(fmt, [])}" if fmt else "") + " :" + "".join(f" {_iname(c)}" for c in cols))
    lines = _root_fields(root, ()) + [head]
    for p in pts:
        lines.append(_cells({c: p[c] for c in cols}, cols))
    return "\n".join(lines)


def decode_serie(text):
    lines = text.split("\n")
    root, i = {}, 0
    if lines and lines[0].startswith("@@ "):
        root, i = _read_root_fields(lines[0], 1), 1
    head = lines[i] if i < len(lines) else ""
    if not head.startswith("@serie ") or ":" not in head:
        raise CompactError(f"ligne {i + 1} : en-tete de serie illisible")
    toks = _raw_tokens(head[len("@serie "):])
    if ":" not in toks:
        raise CompactError(f"ligne {i + 1} : « : » attendu avant les colonnes")
    colon = toks.index(":")
    cols = [_icell_in(c)[0] for c in toks[colon + 1:]]
    mm = re.match(r"^(\S+)\[(\d+)\]$", toks[0])
    if not mm or len(toks) < 4:
        raise CompactError(f"ligne {i + 1} : en-tete de serie illisible")
    name, n, tkey = mm.group(1), int(mm.group(2)), _icell_in(toks[1])[0]
    named = dict(_inamed(t) for t in toks[2:colon])
    step = int(named["pas"])
    fmt = _icell_in(named["format"])[0] if "format" in named else None
    rows = lines[i + 1:]
    if len(rows) != n:
        raise CompactError(f"serie : {n} points annonces, {len(rows)} ecrits")
    if fmt:
        d0 = _dt.datetime.strptime(_icell_in(named["de"])[0], fmt)
        times = [(d0 + _dt.timedelta(seconds=k * step)).strftime(fmt) for k in range(n)]
    else:
        start = json.loads(named["de"])
        times = [start + k * step for k in range(n)]
    pts = []
    for k, (t, row) in enumerate(zip(times, rows)):
        vals = _uncells(row, cols, i + 2 + k)
        pts.append({tkey: t, **vals})
    if name == "_":
        return pts
    return {**root, name: pts} if not root else _reorder(root, name, pts)


def _reorder(root, name, pts):
    out = dict(root)
    out[name] = pts
    return out


# ------------------------------------------------------------------- graphe

_NODE_KEYS = ("nodes", "noeuds", "vertices")
_EDGE_KEYS = ("edges", "links", "aretes")
_ENDS = (("source", "target"), ("from", "to"), ("src", "dst"))


def encode_graphe(data):
    if not isinstance(data, dict):
        raise CompactError("pas un graphe")
    nk = next((k for k in _NODE_KEYS if isinstance(data.get(k), list)), None)
    ek = next((k for k in _EDGE_KEYS if isinstance(data.get(k), list)), None)
    if not nk or not ek or not data[ek] or not all(isinstance(e, dict) for e in data[ek] + data[nk]):
        raise CompactError("pas un graphe")
    ends = next((p for p in _ENDS if all(p[0] in e and p[1] in e for e in data[ek])), None)
    if not ends or not all(isinstance(e[ends[0]], str) and isinstance(e[ends[1]], str) and ">" not in e[ends[0]]
                           and e[ends[0]] and e[ends[1]] for e in data[ek]):
        raise CompactError("graphe : aretes sans source/cible textuelles")
    nodes, edges = data[nk], data[ek]
    ncols = [k for k in (nodes[0] if nodes else {}) if all(k in x for x in nodes)]
    ecols = [k for k in edges[0] if k not in ends and all(k in x for x in edges)]
    lines = _root_fields(data, (nk, ek))
    lines.append(f"@noeuds {nk}[{len(nodes)}]:" + "".join(f" {_iname(c)}" for c in ncols))
    lines += [_cells(x, ncols) for x in nodes]
    lines.append(f"@aretes {ek}[{len(edges)}] {ends[0]}>{ends[1]}:" + "".join(f" {_iname(c)}" for c in ecols))
    for e in edges:
        # « a>b » : un seul mot ; une extremite qui le casserait (espace, « > »...) est refusee
        a, b = _icell(e[ends[0]], []), _icell(e[ends[1]], [])
        if a.startswith('"') or b.startswith('"'):
            raise CompactError("graphe : identifiant de noeud a guillemets")
        rest = _cells({k: v for k, v in e.items() if k not in ends}, ecols)
        lines.append(f"{a}>{b}" + ("" if rest == "." else " " + rest))
    return "\n".join(lines)


def decode_graphe(text):
    lines = text.split("\n")
    root, i = {}, 0
    if lines and lines[0].startswith("@@ "):
        root, i = _read_root_fields(lines[0], 1), 1
    m = re.match(r"^@noeuds (\S+)\[(\d+)\]:(.*)$", lines[i] if i < len(lines) else "")
    if not m:
        raise CompactError(f"ligne {i + 1} : « @noeuds » attendu")
    nk, nn, ncols = m.group(1), int(m.group(2)), [_icell_in(c)[0] for c in m.group(3).split()]
    nodes = [_uncells(lines[i + 1 + k], ncols, i + 2 + k) for k in range(nn)]
    j = i + 1 + nn
    m = re.match(r"^@aretes (\S+)\[(\d+)\] (\S+)>(\S+):(.*)$", lines[j] if j < len(lines) else "")
    if not m:
        raise CompactError(f"ligne {j + 1} : « @aretes » attendu")
    ek, ne, s_key, t_key = m.group(1), int(m.group(2)), m.group(3), m.group(4)
    ecols = [_icell_in(c)[0] for c in m.group(5).split()]
    if len(lines) != j + 1 + ne:
        raise CompactError(f"graphe : {ne} aretes annoncees, {len(lines) - j - 1} ecrites")
    edges = []
    for k, line in enumerate(lines[j + 1:]):
        head, _, rest = line.partition(" ")
        a, sep, b = head.partition(">")
        if not sep or not a or not b:
            raise CompactError(f"ligne {j + 2 + k} : « source>cible » attendu")
        e = {s_key: _icell_in(a)[0], t_key: _icell_in(b)[0]}
        e.update(_uncells(rest or ".", ecols, j + 2 + k))
        edges.append(e)
    out = dict(root)
    out[nk], out[ek] = nodes, edges
    return out


# -------------------------------------------------------------- conversation

def _messages(data):
    if isinstance(data, list):
        return None, data, {}
    if isinstance(data, dict) and isinstance(data.get("messages"), list):
        return "messages", data["messages"], {k: v for k, v in data.items() if k != "messages"}
    raise CompactError("pas une conversation")


def encode_conversation(data):
    key, msgs, root = _messages(data)
    if not msgs or not all(isinstance(m, dict) and isinstance(m.get("role"), str) and _NAME.match(m["role"])
                           for m in msgs):
        raise CompactError("conversation : messages sans role")
    lines = _root_fields(root, ()) + (["@messages"] if key else ["@conversation"])
    for msg in msgs:
        head = "#" + msg["role"]
        content = msg.get("content")
        extra = {k: v for k, v in msg.items() if k not in ("role", "content")}
        if "content" not in msg:
            head += " !"                      # message sans champ content
        elif not isinstance(content, str):
            extra = {"content": content, **extra}
        blocks = []
        head += "".join(f" {_iname(k)}={_icell(v, blocks)}" for k, v in extra.items())
        if blocks:
            raise CompactError("conversation : champ multiligne hors du texte")
        lines.append(head)
        if "content" in msg and isinstance(content, str):
            # Le texte est ecrit BRUT, une ligne par ligne ; une ligne qui commence
            # par « # » ou « \\ » est protegee par « \\ ».
            lines += [("\\" + ln if ln.startswith(("#", "\\")) else ln) for ln in content.split("\n")]
    return "\n".join(lines)


def decode_conversation(text):
    lines = text.split("\n")
    root, i = {}, 0
    if lines and lines[0].startswith("@@ "):
        root, i = _read_root_fields(lines[0], 1), 1
    if i >= len(lines) or lines[i] not in ("@conversation", "@messages"):
        raise CompactError(f"ligne {i + 1} : « @conversation » attendu")
    wrapped = lines[i] == "@messages"
    msgs, cur, body = [], None, None

    def close():
        if cur is not None:
            if body is not None:
                cur["content"] = "\n".join(body)
            msgs.append(cur)

    for n, ln in enumerate(lines[i + 1:], i + 2):
        if ln.startswith("#"):
            close()
            toks = _raw_tokens(ln[1:])
            if not toks or not _NAME.match(toks[0]):
                raise CompactError(f"ligne {n} : « #role » attendu")
            cur = {"role": toks[0]}
            rest = toks[1:]
            no_content = bool(rest) and rest[0] == "!"
            if no_content:
                rest = rest[1:]
            for t in rest:
                name, raw = _inamed(t)
                cur[name] = _icell_in(raw)[0] if raw else ""
            body = None if (no_content or "content" in cur) else []
        else:
            if cur is None or body is None:
                raise CompactError(f"ligne {n} : texte hors d'un message")
            body.append(ln[1:] if ln.startswith("\\") else ln)
    close()
    if not wrapped:
        if root:
            raise CompactError("conversation : champs de racine sans « @messages »")
        return msgs
    return {**root, "messages": msgs}


# -------------------------------------------------------------- appels d'outils

def _calls(data):
    if isinstance(data, dict):
        return False, [data]
    if isinstance(data, list) and data:
        return True, data
    raise CompactError("pas un appel")


def encode_appels(data):
    many, calls = _calls(data)
    lines = ["@appels" if many else "@appel"]
    for c in calls:
        if not (isinstance(c, dict) and isinstance(c.get("name"), str) and _NAME.match(c["name"])
                and isinstance(c.get("arguments"), dict) and list(c)[:2] == ["name", "arguments"]):
            raise CompactError("appel : {name, arguments} attendu, dans cet ordre")
        args = ", ".join(f"{k if _NAME.match(k) else _J(k)}={_J(v)}" for k, v in c["arguments"].items())
        extra = " ".join(f"{k if _NAME.match(k) else _J(k)}={_J(v)}" for k, v in list(c.items())[2:])
        lines.append(f"{c['name']}({args})" + (" " + extra if extra else ""))
    return "\n".join(lines)


def _kv_list(s, i, stop, n):
    """Paires cle=valeur JSON separees par « , » (ou espaces) jusqu'a `stop`."""
    dec, out = json.JSONDecoder(), {}
    while True:
        while i < len(s) and s[i] in " ,":
            i += 1
        if i >= len(s):
            if stop:
                raise CompactError(f"ligne {n} : « {stop} » manquant")
            return out, i
        if stop and s[i] == stop:
            return out, i + 1
        if s[i] == '"':
            key, i = dec.raw_decode(s, i)
        else:
            m = re.compile(r"[A-Za-z_][A-Za-z0-9_.]*").match(s, i)
            if not m:
                raise CompactError(f"ligne {n} : nom d'argument attendu")
            key, i = m.group(0), m.end()
        if i >= len(s) or s[i] != "=":
            raise CompactError(f"ligne {n} : « = » attendu apres {key!r}")
        try:
            val, i = dec.raw_decode(s, i + 1)
        except ValueError:
            raise CompactError(f"ligne {n} : valeur JSON attendue pour {key!r}") from None
        if key in out:
            raise CompactError(f"ligne {n} : {key!r} en double")
        out[key] = val


def decode_appels(text):
    lines = [ln for ln in text.split("\n")]
    if not lines or lines[0] not in ("@appel", "@appels"):
        raise CompactError("ligne 1 : « @appel » ou « @appels » attendu")
    calls = []
    for n, ln in enumerate(lines[1:], 2):
        m = re.match(r"([A-Za-z_][A-Za-z0-9_.]*)\(", ln)
        if not m:
            raise CompactError(f"ligne {n} : « nom(...) » attendu")
        args, i = _kv_list(ln, m.end(), ")", n)
        extra, i = _kv_list(ln, i, None, n)
        calls.append({"name": m.group(1), "arguments": args, **extra})
    if lines[0] == "@appel":
        if len(calls) != 1:
            raise CompactError("« @appel » : un seul appel attendu")
        return calls[0]
    return calls


# -------------------------------------------------------------- matrice

def _num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool) and v == v and v not in (float("inf"), float("-inf"))


def encode_matrice(data):
    if isinstance(data, list) and data and all(_num(x) for x in data):
        return f"@vecteur {len(data)}\n" + " ".join(_J(x) for x in data)
    if (isinstance(data, list) and data and all(isinstance(r, list) and r and all(_num(x) for x in r) for r in data)
            and len({len(r) for r in data}) == 1):
        return f"@matrice {len(data)}x{len(data[0])}\n" + "\n".join(" ".join(_J(x) for x in r) for r in data)
    raise CompactError("pas un vecteur ni une matrice de nombres")


def decode_matrice(text):
    lines = text.split("\n")
    m = re.match(r"^@vecteur (\d+)$", lines[0])
    if m:
        vals = [json.loads(x) for x in " ".join(lines[1:]).split()]
        if len(vals) != int(m.group(1)) or not all(_num(v) for v in vals):
            raise CompactError("vecteur : taille ou nombre illisible")
        return vals
    m = re.match(r"^@matrice (\d+)x(\d+)$", lines[0])
    if not m:
        raise CompactError("ligne 1 : « @vecteur N » ou « @matrice RxC » attendu")
    rows = [[json.loads(x) for x in ln.split()] for ln in lines[1:]]
    if len(rows) != int(m.group(1)) or any(len(r) != int(m.group(2)) for r in rows):
        raise CompactError("matrice : dimensions differentes de l'en-tete")
    return rows


# ------------------------------------------------------------------ mixte
# Les vraies reponses d'API melangent des objets irreguliers (ou la forme
# objets_json est la seule fiable en ecriture : 48/48) et des listes d'objets
# semblables (ou une table est 2 fois plus courte). La forme mixte fait les
# deux : la structure d'objets_json, et chaque liste d'objets devient une
# table dont les cellules sont des valeurs JSON.
#   crate:
#     name: "serde"
#   versions[2]: num downloads yanked
#     "1.0.229" 291102851 false
#     "1.0.228" 589727930 false license="MIT"
# Une colonne = un champ present dans TOUS les elements ; un autre champ
# s'ecrit nom=valeur en fin de ligne. Toutes les valeurs sont du JSON.

_MKEY = re.compile(r"[^\s\"?=#\-\[\]][^\s\"]*")
_MNAME = re.compile(r"[A-Za-z_][A-Za-z0-9_.\-]*")


def _mkey(k):
    """Cle sans guillemets si elle est sans espace, ne commence ni par « - »,
    « " », « ? », « = », « # », « [ », ne finit ni par « : » ni par « [N] »."""
    if k and _MKEY.fullmatch(k) and not k.endswith(":") and ": " not in k and not re.search(r"\[\d+\]$", k):
        return k
    return _J(k)


def _mcol(k):
    return k if _MNAME.fullmatch(k) else _J(k)


def _table_ok(v):
    return (isinstance(v, list) and len(v) >= 2 and all(isinstance(x, dict) and x and not
                                                         (len(x) == 1 and "$unknown" in x) for x in v))


def encode_mixte(data):
    lines = []

    def table(key, rows, ind):
        cols = [k for k in rows[0] if all(k in r for r in rows)]
        lines.append(f"{ind}{key}[{len(rows)}]:" + "".join(f" {_mcol(c)}" for c in cols))
        for r in rows:
            cells = [_J(r[c]) for c in cols] + [f"{_mcol(k)}={_J(v)}" for k, v in r.items() if k not in cols]
            lines.append(ind + "  " + (" ".join(cells) if cells else "."))

    def obj(d, ind):
        for k, v in d.items():
            key = _mkey(k)
            if _table_ok(v):
                table(key, v, ind)
            elif isinstance(v, dict) and v and not (len(v) == 1 and "$unknown" in v):
                lines.append(f"{ind}{key}:")
                obj(v, ind + "  ")
            elif isinstance(v, list) and v and any(isinstance(x, (dict, list)) for x in v):
                lines.append(f"{ind}{key}:")
                lines.extend(f"{ind}  - {_J(x)}" for x in v)
            else:
                lines.append(f"{ind}{key}: {_J(v)}")

    if isinstance(data, dict) and data and not (len(data) == 1 and "$unknown" in data):
        obj(data, "")
    elif _table_ok(data):
        table("", data, "")
    else:
        return "= " + _J(data)
    return "\n".join(lines)


def _json_seq(s, i, n):
    """Valeurs JSON separees par des espaces, puis des « nom=valeur »."""
    dec, vals, named = json.JSONDecoder(), [], {}
    while True:
        while i < len(s) and s[i] == " ":
            i += 1
        if i >= len(s):
            return vals, named
        m = re.compile(r'(?:"(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z0-9_.\-]*)=').match(s, i)
        if m:
            key = m.group(0)[:-1]
            key = json.loads(key) if key.startswith('"') else key
            try:
                named[key], i = dec.raw_decode(s, m.end())
            except ValueError:
                raise CompactError(f"ligne {n} : valeur JSON attendue apres {key}=") from None
            continue
        if named:
            raise CompactError(f"ligne {n} : valeur apres un champ nom=valeur")
        try:
            v, i = dec.raw_decode(s, i)
        except ValueError:
            raise CompactError(f"ligne {n} : valeur JSON attendue : {s[i:i + 40]!r}") from None
        vals.append(v)


def decode_mixte(text):
    lines = text.split("\n")
    pos = 0
    head_re = re.compile(r'^(?P<key>"(?:[^"\\]|\\.)*"|[^\s"]*?)\[(?P<n>\d+)\]:(?P<cols>.*)$')

    def indent(ln):
        return len(ln) - len(ln.lstrip(" "))

    def read_table(body, ind, n):
        nonlocal pos
        m = head_re.match(body)
        cols = []
        for t in _raw_tokens(m.group("cols")):
            cols.append(json.loads(t) if t.startswith('"') else t)
        rows = []
        for _ in range(int(m.group("n"))):
            if pos >= len(lines) or indent(lines[pos]) != ind + 2:
                raise CompactError(f"ligne {pos + 1} : ligne de table attendue (indentation {ind + 2})")
            row = lines[pos][ind + 2:]
            pos += 1
            vals, named = _json_seq(row, 0, pos) if row.strip() != "." else ([], {})
            if len(vals) != len(cols):
                raise CompactError(f"ligne {pos} : {len(cols)} valeurs attendues, {len(vals)} lues")
            r = dict(zip(cols, vals))
            r.update(named)
            rows.append(r)
        return rows

    def key_of(body, n):
        if body.startswith('"'):
            dec = json.JSONDecoder()
            key, j = dec.raw_decode(body)
            return key, body[j:]
        m = re.match(r"(.*?)(:(?: |$)|\[\d+\]:)", body)
        if not m or not m.group(1):
            raise CompactError(f"ligne {n} : « cle: valeur » attendu")
        return m.group(1), body[len(m.group(1)):]

    def obj(ind):
        nonlocal pos
        out = {}
        while pos < len(lines):
            ln = lines[pos]
            if not ln.strip():
                pos += 1
                continue
            if indent(ln) < ind:
                break
            if indent(ln) > ind:
                raise CompactError(f"ligne {pos + 1} : indentation inattendue")
            body = ln[ind:]
            pos += 1
            n = pos
            key, rest = key_of(body, n)
            if key in out:
                raise CompactError(f"ligne {n} : cle en double {key!r}")
            if re.match(r"^\[\d+\]:", rest):
                out[key] = read_table('""' + rest if body.startswith('"') else body, ind, n) \
                    if not body.startswith('"') else read_table(_J(key) + rest, ind, n)
            elif rest == ":":
                if pos < len(lines) and lines[pos][ind + 2:ind + 4] == "- " and indent(lines[pos]) == ind + 2:
                    items = []
                    while pos < len(lines) and indent(lines[pos]) == ind + 2 and lines[pos][ind + 2:].startswith("- "):
                        try:
                            items.append(json.loads(lines[pos][ind + 4:]))
                        except ValueError:
                            raise CompactError(f"ligne {pos + 1} : valeur JSON attendue apres « - »") from None
                        pos += 1
                    out[key] = items
                else:
                    out[key] = obj(ind + 2)
            elif rest.startswith(": "):
                try:
                    out[key] = json.loads(rest[2:])
                except ValueError:
                    raise CompactError(f"ligne {n} : valeur JSON attendue : {rest[2:42]!r}") from None
            else:
                raise CompactError(f"ligne {n} : « : » attendu apres la cle")
        return out

    if text.startswith("= "):
        return json.loads(text[2:])
    if head_re.match(lines[0]) and lines[0].startswith("["):
        pos = 1
        rows = read_table(lines[0], 0, 1)
        if any(ln.strip() for ln in lines[pos:]):
            raise CompactError(f"ligne {pos + 1} : texte en trop")
        return rows
    result = obj(0)
    if pos < len(lines) and any(ln.strip() for ln in lines[pos:]):
        raise CompactError(f"ligne {pos + 1} : texte en trop")
    return result


# -------------------------------------------------------------- changements
# Une liste d'operations JSON Patch (RFC 6902) : ce qui a change, pas le
# document entier. Une ligne par operation :
#   ~ /commandes/0/statut "expediee"        replace
#   + /commandes/0/lignes/- {"article":"x"}  add
#   - /commandes/1                           remove
#   > /a /b                                  move (de /a vers /b)
#   & /a /b                                  copy
#   = /x 5                                   test
# Le chemin est un pointeur JSON ; entre guillemets JSON s'il contient un espace
# ou commence par « " ». diff(avant, apres) calcule ces operations, apply les
# applique.

_OPS = {"replace": "~", "add": "+", "remove": "-", "move": ">", "copy": "&", "test": "="}
_SYM = {v: k for k, v in _OPS.items()}


def _path(p):
    return p if p and " " not in p and not p.startswith('"') and "\n" not in p and "\t" not in p else _J(p)


def encode_changements(data):
    if not isinstance(data, list) or not data:
        raise CompactError("pas une liste d'operations")
    lines = ["@changements"]
    for op in data:
        if not isinstance(op, dict) or op.get("op") not in _OPS or not isinstance(op.get("path"), str):
            raise CompactError("operation JSON Patch attendue")
        kind = op["op"]
        want = {"replace": ["op", "path", "value"], "add": ["op", "path", "value"], "test": ["op", "path", "value"],
                "remove": ["op", "path"], "move": ["op", "from", "path"], "copy": ["op", "from", "path"]}[kind]
        if sorted(op) != sorted(want):
            raise CompactError(f"operation {kind} : champs {want} attendus")
        if kind in ("move", "copy"):
            if not isinstance(op["from"], str):
                raise CompactError("from : texte attendu")
            lines.append(f"{_OPS[kind]} {_path(op['from'])} {_path(op['path'])}")
        elif kind == "remove":
            lines.append(f"- {_path(op['path'])}")
        else:
            lines.append(f"{_OPS[kind]} {_path(op['path'])} {_J(op['value'])}")
    return "\n".join(lines)


def decode_changements(text):
    lines = text.split("\n")
    if lines[0] != "@changements":
        raise CompactError("ligne 1 : « @changements » attendu")
    dec, out = json.JSONDecoder(), []
    for n, ln in enumerate(lines[1:], 2):
        if len(ln) < 3 or ln[0] not in _SYM or ln[1] != " ":
            raise CompactError(f"ligne {n} : « ~ + - > & = » suivi d'un espace attendu")
        kind, rest = _SYM[ln[0]], ln[2:]

        def path_at(s, i):
            if s[i:i + 1] == '"':
                return dec.raw_decode(s, i)
            j = s.find(" ", i)
            j = len(s) if j < 0 else j
            return s[i:j], j

        p1, i = path_at(rest, 0)
        if kind == "remove":
            if i != len(rest):
                raise CompactError(f"ligne {n} : texte en trop apres le chemin")
            out.append({"op": kind, "path": p1})
            continue
        if i >= len(rest) or rest[i] != " ":
            raise CompactError(f"ligne {n} : deuxieme element attendu")
        if kind in ("move", "copy"):
            p2, j = path_at(rest, i + 1)
            if j != len(rest):
                raise CompactError(f"ligne {n} : texte en trop")
            out.append({"op": kind, "from": p1, "path": p2})
        else:
            try:
                val, j = dec.raw_decode(rest, i + 1)
            except ValueError:
                raise CompactError(f"ligne {n} : valeur JSON attendue") from None
            if j != len(rest):
                raise CompactError(f"ligne {n} : texte en trop apres la valeur")
            out.append({"op": kind, "path": p1, "value": val})
    return out


def _esc(k):
    return str(k).replace("~", "~0").replace("/", "~1")


def diff(old, new, path=""):
    """Operations JSON Patch qui transforment `old` en `new` (apply(old, diff)
    == new). Objets : champ par champ ; listes : element par element, puis
    ajouts ou retraits en fin ; sinon remplacement."""
    if type(old) is type(new) and isinstance(old, dict):
        ops = []
        for k in old:
            if k not in new:
                ops.append({"op": "remove", "path": f"{path}/{_esc(k)}"})
            else:
                ops += diff(old[k], new[k], f"{path}/{_esc(k)}")
        for k in new:
            if k not in old:
                ops.append({"op": "add", "path": f"{path}/{_esc(k)}", "value": new[k]})
        return ops
    if type(old) is type(new) and isinstance(old, list):
        ops = []
        for i in range(min(len(old), len(new))):
            ops += diff(old[i], new[i], f"{path}/{i}")
        for i in range(len(old) - 1, len(new) - 1, -1):
            ops.append({"op": "remove", "path": f"{path}/{i}"})
        for i in range(len(old), len(new)):
            ops.append({"op": "add", "path": f"{path}/-", "value": new[i]})
        return ops
    if old == new and type(old) is type(new):
        return []
    return [{"op": "replace", "path": path, "value": new}]


def apply(doc, ops):
    """Applique des operations JSON Patch (add, remove, replace, move, copy,
    test) a une copie de `doc`. Leve CompactError si une operation echoue."""
    import copy
    doc = copy.deepcopy(doc)

    def parts(p):
        if p == "":
            return []
        if not p.startswith("/"):
            raise CompactError(f"chemin invalide : {p!r}")
        return [x.replace("~1", "/").replace("~0", "~") for x in p[1:].split("/")]

    def parent(d, ps):
        for k in ps[:-1]:
            d = d[int(k)] if isinstance(d, list) else d[k]
        return d

    def get(d, ps):
        for k in ps:
            d = d[int(k)] if isinstance(d, list) else d[k]
        return d

    try:
        for op in ops:
            ps = parts(op["path"])
            kind = op["op"]
            if kind in ("move", "copy"):
                value = get(doc, parts(op["from"]))
                if kind == "move":
                    fp = parts(op["from"])
                    par = parent(doc, fp)
                    del par[int(fp[-1]) if isinstance(par, list) else fp[-1]]
                kind, op = "add", {"op": "add", "path": op["path"], "value": copy.deepcopy(value)}
            if kind == "test":
                if get(doc, ps) != op["value"]:
                    raise CompactError(f"test echoue : {op['path']}")
                continue
            if not ps:
                if kind == "remove":
                    raise CompactError("retrait de la racine")
                doc = copy.deepcopy(op["value"])
                continue
            par = parent(doc, ps)
            last = ps[-1]
            if isinstance(par, list):
                if kind == "add":
                    if last == "-":
                        par.append(copy.deepcopy(op["value"]))
                    else:
                        par.insert(int(last), copy.deepcopy(op["value"]))
                elif kind == "remove":
                    del par[int(last)]
                else:
                    par[int(last)] = copy.deepcopy(op["value"])
            else:
                if kind == "remove":
                    del par[last]
                elif kind == "replace" and last not in par:
                    raise CompactError(f"replace sur un champ absent : {op['path']}")
                else:
                    par[last] = copy.deepcopy(op["value"])
    except (KeyError, IndexError, ValueError, TypeError) as e:
        raise CompactError(f"operation impossible : {e}") from None
    return doc


# --------------------------------------------------------------- relationnel
# Forme CSTL pour les donnees IRREGULIERES (2026-11-24). Base fiable mesuree :
# chaque valeur en JSON, structure indentee « cle: valeur » (objets_json :
# 48/48 en ecriture sur 12 reponses d'API). Trois apports propres a CSTL :
#   1. chemins condenses : une chaine d'objets a UN seul champ s'ecrit en un
#      chemin, sur une ligne : repository>url: "git+https://..." ; plusieurs
#      chemins vers le meme objet se fusionnent (scripts>lint, scripts>test)
#      (« les relations comptent plus que l'information ») ;
#   2. tables seulement si TOUS les elements ont exactement les memes champs,
#      dans le meme ordre (la cause des echecs de la forme mixte : des
#      colonnes partielles) : versions[3]: num downloads ; sinon, elements
#      « - » ;
#   3. le vide qui parle : ? ou ?{"sigma":0.3} ; le nombre d'elements de
#      chaque liste en tete : maintainers[2]:
# Le separateur de chemin est « > » : les vraies cles contiennent des points
# (versions 2.34.0, lodash.get), presque jamais « > ». Une cle qui contient
# « > », un espace, « " », ou commence par - ? = # [ s'ecrit en JSON.

# Une cle nue peut contenir des espaces (« Release notes ») : elle va jusqu'au
# premier « : », comme en YAML. Entre guillemets si elle contient « : ».
_RKEY = re.compile(r"[^\s\"?=#\-\[\]{>](?:(?:[^\s\">\[\]]| )*[^\s\">\[\]])?")


def _rkey_bare(k):
    return (bool(k) and bool(_RKEY.fullmatch(k)) and not k.endswith(":") and ": " not in k
            and "  " not in k and not re.search(r"\[\d+\]$", k))


def _rkey(k):
    return k if _rkey_bare(k) else _J(k)


def _rcol(k):
    """Nom de colonne d'une table : separe par des espaces, donc sans espace."""
    return k if _rkey_bare(k) and not re.search(r"\s", k) else _J(k)


def _unknown(v):
    return isinstance(v, dict) and len(v) == 1 and isinstance(v.get("$unknown"), dict)


def _rval(v):
    if _unknown(v):
        return "?" + (_J(v["$unknown"]) if v["$unknown"] else "")
    return _J(v)


def _regular_table(v):
    if not (isinstance(v, list) and len(v) >= 2 and all(isinstance(x, dict) and x and not _unknown(x) for x in v)):
        return None
    keys = list(v[0])
    if any(list(x) != keys for x in v) or not all(_rkey_bare(k) or True for k in keys):
        return None
    return keys


def encode_relationnel(data):
    lines = []

    def value_lines(key, v, ind):
        path = [key]
        # 1. chemins condenses : objets a un seul champ, cles sans guillemets
        while (isinstance(v, dict) and len(v) == 1 and not _unknown(v) and _rkey_bare(path[-1])
               and _rkey_bare(next(iter(v)))):
            k = next(iter(v))
            path.append(k)
            v = v[k]
        head = ind + ">".join(path if len(path) > 1 else [_rkey(key)])
        cols = _regular_table(v)
        if cols is not None:
            lines.append(f"{head}[{len(v)}]:" + "".join(f" {_rcol(c)}" for c in cols))
            lines.extend(ind + "  " + " ".join(_rval(x[c]) for c in cols) for x in v)
        elif isinstance(v, list) and v and any(isinstance(x, (dict, list)) and x and not _unknown(x) for x in v):
            lines.append(f"{head}[{len(v)}]:")
            items(v, ind + "  ")
        elif isinstance(v, dict) and v and not _unknown(v):
            lines.append(f"{head}:")
            obj(v, ind + "  ")
        else:
            lines.append(f"{head}: {_rval(v)}")

    def obj(d, ind):
        for k, v in d.items():
            value_lines(k, v, ind)

    def items(seq, ind):
        for x in seq:
            if isinstance(x, dict) and x and not _unknown(x):
                start = len(lines)
                obj(x, ind + "  ")
                lines[start] = ind + "- " + lines[start][len(ind) + 2:]
            else:
                lines.append(f"{ind}- {_rval(x)}")

    if isinstance(data, dict) and data and not _unknown(data):
        obj(data, "")
        return "\n".join(lines)
    cols = _regular_table(data)
    if cols is not None:
        lines.append(f"[{len(data)}]:" + "".join(f" {_rcol(c)}" for c in cols))
        lines.extend("  " + " ".join(_rval(x[c]) for c in cols) for x in data)
        return "\n".join(lines)
    if isinstance(data, list) and data and any(isinstance(x, (dict, list)) and x and not _unknown(x) for x in data):
        lines.append(f"[{len(data)}]:")
        items(data, "  ")
        return "\n".join(lines)
    return "= " + _rval(data)


def _rparse_value(raw, n):
    raw = raw.strip()
    if raw.startswith("?"):
        rest = raw[1:]
        try:
            details = json.loads(rest) if rest else {}
        except ValueError:
            raise CompactError(f"ligne {n} : « ?{{...}} » attendu : {raw[:40]!r}") from None
        if not isinstance(details, dict):
            raise CompactError(f"ligne {n} : details de l'inconnu : objet JSON attendu")
        return {"$unknown": details}
    try:
        return json.loads(raw)
    except ValueError:
        raise CompactError(f"ligne {n} : valeur JSON attendue : {raw[:60]!r}") from None


_RHEAD = re.compile(r'^(?P<key>"(?:[^"\\]|\\.)*"|[^\s"\[](?:[^"\[]*?[^\s"\[])??)(?:\[(?P<n>\d+)\])?:(?P<rest>(?: .*)?)$')


def _rsplit_key(body, n):
    m = _RHEAD.match(body)
    if not m:
        raise CompactError(f"ligne {n} : « cle: valeur » attendu : {body[:50]!r}")
    raw = m.group("key")
    path = [json.loads(raw)] if raw.startswith('"') else raw.split(">")
    if not raw.startswith('"') and any(not p for p in path):
        raise CompactError(f"ligne {n} : chemin invalide : {raw!r}")
    return path, (int(m.group("n")) if m.group("n") is not None else None), m.group("rest")


def decode_relationnel(text):
    lines = text.split("\n")
    pos = 0

    def indent(ln):
        return len(ln) - len(ln.lstrip(" "))

    def nest(path, value):
        for k in reversed(path[1:]):
            value = {k: value}
        return path[0], value

    def entry(body, ind, n):
        """Une ligne « chemin[...]: ... » et ses lignes dessous -> (cle, valeur)."""
        nonlocal pos
        path, count, rest = _rsplit_key(body, n)
        if count is not None:
            cols_text = rest.strip()
            if cols_text:
                cols = [json.loads(t) if t.startswith('"') else t for t in _raw_tokens(cols_text)]
                rows = []
                for _ in range(count):
                    if pos >= len(lines) or indent(lines[pos]) != ind + 2:
                        raise CompactError(f"ligne {pos + 1} : ligne de table attendue")
                    row, pos = lines[pos][ind + 2:], pos + 1
                    vals = _row_values(row, pos)
                    if len(vals) != len(cols):
                        raise CompactError(f"ligne {pos} : {len(cols)} valeurs attendues, {len(vals)} lues")
                    rows.append(dict(zip(cols, vals)))
                return nest(path, rows)
            got = items(ind + 2)
            if len(got) != count:
                raise CompactError(f"ligne {n} : [{count}] annonce, {len(got)} element(s) ecrit(s)")
            return nest(path, got)
        if rest.strip():
            return nest(path, _rparse_value(rest, n))
        return nest(path, obj(ind + 2))

    def obj(ind, first=None):
        nonlocal pos
        out = {}
        while True:
            if first is not None:
                body, n, first = first, pos, None
            else:
                while pos < len(lines) and not lines[pos].strip():
                    pos += 1
                if pos >= len(lines) or indent(lines[pos]) < ind:
                    break
                if indent(lines[pos]) > ind:
                    raise CompactError(f"ligne {pos + 1} : indentation inattendue")
                body = lines[pos][ind:]
                if body.startswith("- ") or body == "-":
                    break
                pos += 1
                n = pos
            key, value = entry(body, ind, n)
            if key in out:
                # Plusieurs chemins vers le meme objet (scripts>lint, puis
                # scripts>test) : ils se FUSIONNENT ; une vraie contradiction
                # (meme champ, deux valeurs) reste une erreur.
                out[key] = _merge(out[key], value, key, n)
            else:
                out[key] = value
        if not out:
            raise CompactError(f"ligne {pos + 1} : objet vide (ecrire {{}})")
        return out

    def items(ind):
        nonlocal pos
        out = []
        while pos < len(lines) and lines[pos].strip() and indent(lines[pos]) == ind and lines[pos][ind:].startswith("- "):
            body = lines[pos][ind + 2:]
            pos += 1
            n = pos
            if _RHEAD.match(body) and not body.startswith(("{", "[")) and not _is_json_scalar(body):
                out.append(obj(ind + 2, first=body))
            else:
                out.append(_rparse_value(body, n))
        return out

    body = text
    if body.startswith("= "):
        return _rparse_value(body[2:], 1)
    m = re.match(r"^\[(\d+)\]:(.*)$", lines[0])
    if m:
        pos = 1
        key, value = entry("_root" + lines[0], 0, 1)
        result = value
    else:
        result = obj(0)
    if any(ln.strip() for ln in lines[pos:]):
        raise CompactError(f"ligne {pos + 1} : texte en trop")
    return result


def _merge(a, b, key, n):
    if isinstance(a, dict) and isinstance(b, dict) and not _unknown(a) and not _unknown(b):
        out = dict(a)
        for k, v in b.items():
            out[k] = _merge(out[k], v, k, n) if k in out else v
        return out
    raise CompactError(f"ligne {n} : cle en double {key!r}")


def _is_json_scalar(body):
    try:
        json.loads(body)
        return True
    except ValueError:
        return body.startswith("?")


def _row_values(row, n):
    dec, vals, i = json.JSONDecoder(), [], 0
    while i < len(row):
        while i < len(row) and row[i] == " ":
            i += 1
        if i >= len(row):
            break
        if row[i] == "?":
            j = i + 1
            if j < len(row) and row[j] == "{":
                details, j = dec.raw_decode(row, j)
            else:
                details = {}
            vals.append({"$unknown": details})
            i = j
            continue
        try:
            v, i = dec.raw_decode(row, i)
        except ValueError:
            raise CompactError(f"ligne {n} : valeur JSON attendue : {row[i:i + 40]!r}") from None
        vals.append(v)
    return vals


ENCODERS = {"serie": encode_serie, "graphe": encode_graphe, "conversation": encode_conversation,
            "appels": encode_appels, "matrice": encode_matrice, "mixte": encode_mixte,
            "changements": encode_changements, "relationnel": encode_relationnel}
DECODERS = {"serie": decode_serie, "graphe": decode_graphe, "conversation": decode_conversation,
            "appels": decode_appels, "matrice": decode_matrice, "mixte": decode_mixte,
            "changements": decode_changements, "relationnel": decode_relationnel}


# Ces formes rejoignent le CSTL polymorphe de cstl_compact.
import cstl_compact as _cc  # noqa: E402

if "serie" not in _cc._ENCODERS:
    _cc._ENCODERS.update(ENCODERS)
    _cc._DECODERS.update(DECODERS)
    _cc.SPECIALISED = tuple(ENCODERS)
    _cc.FORMS_DOCUMENT = _cc.FORMS_DOCUMENT + _cc.SPECIALISED
