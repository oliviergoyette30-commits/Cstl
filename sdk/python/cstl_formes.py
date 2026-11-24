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


ENCODERS = {"serie": encode_serie, "graphe": encode_graphe, "conversation": encode_conversation,
            "appels": encode_appels, "matrice": encode_matrice}
DECODERS = {"serie": decode_serie, "graphe": decode_graphe, "conversation": decode_conversation,
            "appels": decode_appels, "matrice": decode_matrice}


# Ces formes rejoignent le CSTL polymorphe de cstl_compact.
import cstl_compact as _cc  # noqa: E402

if "serie" not in _cc._ENCODERS:
    _cc._ENCODERS.update(ENCODERS)
    _cc._DECODERS.update(DECODERS)
    _cc.SPECIALISED = tuple(ENCODERS)
    _cc.FORMS_DOCUMENT = _cc.FORMS_DOCUMENT + _cc.SPECIALISED
