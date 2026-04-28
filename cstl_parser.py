"""
CSTL v4.0 — Parser Python officiel
Auteur : Olivier Goyette + Claude Sonnet 4
Date   : 27 avril 2026
"""

import re
import json
from dataclasses import dataclass, field
from typing import Optional, List, Dict, Any


# ============================================================
# CONSTANTES FIXES
# ============================================================

CSTL_VERSION = "v4.0"

OPERATORS_FIXED = {
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST",
    "AMP", "INH", "PRESSURE", "CATALYZE",
    "MUTUAL", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND"
}

LAYER_MAP = {
    "b": "bedrock",  "bedrock": "bedrock",
    "d": "deep",     "deep":    "deep",
    "s": "shallow",  "shallow": "shallow",
    "su": "surface", "surface": "surface"
}

TIME_MAP = {
    "p": "past",    "past":    "past",
    "n": "present", "present": "present",
    "f": "future",  "future":  "future"
}

WEIGHT_MAP = {
    "+": "positive",  "positive": "positive",
    "-": "negative",  "negative": "negative",
    "−": "negative",  # tiret typographique unicode
    "°": "neutral",   "neutral":  "neutral"
}

MODALITY_MAP = {
    "[!]": "MUST",  "[MUST]": "MUST",
    "[¬]": "NOT",   "[NOT]":  "NOT",
    "[?]": "MAY",   "[MAY]":  "MAY"
}

ENTITY_TYPES = {
    "human", "agent", "document", "system", "concept",
    "place", "event", "infrastructure", "threat", "deliverable"
}


# ============================================================
# DATACLASSES
# ============================================================

@dataclass
class ParseMessage:
    severity: str   # "warning" | "error"
    line: int
    message: str

    def __str__(self):
        return f"[{self.severity.upper()}] ligne {self.line}: {self.message}"


@dataclass
class Attrs:
    strength: Optional[float] = None
    layer:    Optional[str]   = None
    time:     Optional[str]   = None
    weight:   Optional[str]   = None
    id:       Optional[str]   = None
    deadline: Optional[str]   = None
    date:     Optional[str]   = None
    trust:    Optional[float] = None
    extra:    Dict[str, Any]  = field(default_factory=dict)

    def to_dict(self):
        d = {}
        if self.strength  is not None: d["strength"]  = self.strength
        if self.layer     is not None: d["layer"]     = self.layer
        if self.time      is not None: d["time"]      = self.time
        if self.weight    is not None: d["weight"]    = self.weight
        if self.id        is not None: d["id"]        = self.id
        if self.deadline  is not None: d["deadline"]  = self.deadline
        if self.date      is not None: d["date"]      = self.date
        if self.trust     is not None: d["trust"]     = self.trust
        if self.extra:                 d.update(self.extra)
        return d


@dataclass
class Relation:
    source:   str
    operator: str
    target:   str
    modality: Optional[str] = None
    attrs:    Attrs = field(default_factory=Attrs)
    line:     int   = 0

    def to_dict(self):
        d = {
            "source":   self.source,
            "operator": self.operator,
            "target":   self.target,
        }
        if self.modality: d["modality"] = self.modality
        d.update(self.attrs.to_dict())
        return d


@dataclass
class Entity:
    name:  str
    type:  str
    attrs: Attrs = field(default_factory=Attrs)
    extra: Dict[str, Any] = field(default_factory=dict)
    line:  int = 0

    def to_dict(self):
        d = {"name": self.name, "type": self.type}
        d.update(self.attrs.to_dict())
        d.update(self.extra)
        return d


@dataclass
class UncertaintyItem:
    element:    str
    state:      str
    source:     Optional[str]   = None
    confidence: Optional[float] = None

    def to_dict(self):
        d = {"element": self.element, "state": self.state}
        if self.source:     d["source"]     = self.source
        if self.confidence is not None: d["confidence"] = self.confidence
        return d


@dataclass
class IntentPayload:
    reason:   str
    priority: Optional[str] = None
    sender:   Optional[str] = None
    receiver: Optional[str] = None
    purpose:  Optional[str] = None
    context:  Optional[str] = None

    def to_dict(self):
        d = {"reason": self.reason}
        for k in ("priority", "sender", "receiver", "purpose", "context"):
            v = getattr(self, k)
            if v: d[k] = v
        return d


@dataclass
class Meta:
    payload_confidence:  Optional[float] = None
    encoded_by:          Optional[str]   = None
    encoding_timestamp:  Optional[str]   = None
    version:             Optional[str]   = None

    def to_dict(self):
        d = {}
        if self.payload_confidence is not None:
            d["payload_confidence"] = self.payload_confidence
        if self.encoded_by:         d["encoded_by"]         = self.encoded_by
        if self.encoding_timestamp: d["encoding_timestamp"] = self.encoding_timestamp
        if self.version:            d["version"]            = self.version
        return d


@dataclass
class CSTLDocument:
    version:  str = ""
    lang:     str = ""
    domain:   str = ""
    session:  Optional[str] = None

    intent_payload: Optional[IntentPayload]  = None
    meta:           Optional[Meta]           = None
    constraints:    List[Relation]           = field(default_factory=list)
    uncertainty:    List[UncertaintyItem]    = field(default_factory=list)
    entities:       Dict[str, Entity]        = field(default_factory=dict)
    relations:      List[Relation]           = field(default_factory=list)

    messages:  List[ParseMessage] = field(default_factory=list)
    is_valid:  bool               = True

    def to_dict(self):
        return {
            "version":  self.version,
            "lang":     self.lang,
            "domain":   self.domain,
            "session":  self.session,
            "intent_payload": self.intent_payload.to_dict() if self.intent_payload else None,
            "meta":           self.meta.to_dict() if self.meta else None,
            "constraints":    [r.to_dict() for r in self.constraints],
            "uncertainty":    [u.to_dict() for u in self.uncertainty],
            "entities":       {k: v.to_dict() for k, v in self.entities.items()},
            "relations":      [r.to_dict() for r in self.relations],
            "is_valid":       self.is_valid,
            "messages":       [str(m) for m in self.messages],
        }

    def to_json(self, indent=2):
        return json.dumps(self.to_dict(), ensure_ascii=False, indent=indent)

    def warnings(self):
        return [m for m in self.messages if m.severity == "warning"]

    def errors(self):
        return [m for m in self.messages if m.severity == "error"]


# ============================================================
# PARSER
# ============================================================

class CSTLParser:

    def parse(self, text: str) -> CSTLDocument:
        self.doc   = CSTLDocument()
        self.lines = text.strip().splitlines()
        self._run()
        self._validate()
        return self.doc

    # --------------------------------------------------------
    # DISPATCH PAR BLOCS
    # --------------------------------------------------------

    def _run(self):
        i = 0
        n = len(self.lines)

        while i < n:
            raw  = self.lines[i]
            line = raw.strip()

            if not line or line.startswith("//"):
                i += 1; continue

            if line == "---END---":
                break

            # Header
            if line.startswith("#!CSTL"):
                m = re.match(r"#!CSTL\s+(v[\d.]+)", line)
                self.doc.version = m.group(1) if m else "unknown"

            elif line.startswith("LANG:"):
                self.doc.lang = line[5:].strip()

            elif line.startswith("DOMAIN:"):
                self.doc.domain = line[7:].strip()

            elif line.startswith("SESSION:"):
                self.doc.session = line[8:].strip()

            elif line.startswith("SYMBOLS:"):
                pass  # Informatif — symboles fixés par constantes

            elif line.startswith("INTENT_PAYLOAD:"):
                i, block = self._collect_block(i)
                self._parse_intent(block)
                continue

            elif line.startswith("META:"):
                i, block = self._collect_block(i)
                self._parse_meta(block)
                continue

            elif line.startswith("CONSTRAINTS:"):
                i, block = self._collect_block(i)
                self._parse_constraints(block)
                continue

            elif line.startswith("UNCERTAINTY:"):
                i, block = self._collect_block(i)
                self._parse_uncertainty(block)
                continue

            elif line.startswith("DEFINE "):
                self._parse_define(line, i)

            elif line.startswith("RELATIONS:"):
                i, block = self._collect_block(i)
                self._parse_relations(block)
                continue

            i += 1

    def _collect_block(self, start: int):
        """Collecte toutes les lignes indentées après une ligne de bloc."""
        block_keywords = {
            "INTENT_PAYLOAD:", "META:", "CONSTRAINTS:", "UNCERTAINTY:",
            "DEFINE ", "RELATIONS:", "---END---", "#!CSTL", "LANG:",
            "DOMAIN:", "SESSION:", "SYMBOLS:"
        }
        lines = [(start, self.lines[start])]
        i = start + 1
        while i < len(self.lines):
            raw = self.lines[i]
            s   = raw.strip()
            if not s:
                i += 1; continue
            # Nouveau bloc de haut niveau ?
            is_new = any(s.startswith(kw) for kw in block_keywords)
            if is_new:
                break
            lines.append((i, raw))
            i += 1
        return i, lines

    # --------------------------------------------------------
    # INTENT_PAYLOAD
    # --------------------------------------------------------

    def _parse_intent(self, lines):
        intent = IntentPayload(reason="")
        full = " ".join(l.strip() for _, l in lines)

        m = re.search(r"INTENT_PAYLOAD:\s+(\S+)", full)
        if m: intent.reason = m.group(1)

        m = re.search(r"\[(.+?)\]", full, re.DOTALL)
        if m:
            for kv in re.split(r",\s*", m.group(1)):
                kv = kv.strip()
                if "=" in kv:
                    k, v = kv.split("=", 1)
                    k, v = k.strip(), v.strip()
                    setattr(intent, k, v) if hasattr(intent, k) else None

        self.doc.intent_payload = intent

    # --------------------------------------------------------
    # META
    # --------------------------------------------------------

    def _parse_meta(self, lines):
        meta = Meta()
        for lineno, line in lines[1:]:
            s = line.strip()
            if ":" not in s: continue
            k, v = s.split(":", 1)
            k, v = k.strip(), v.strip()
            if k == "PAYLOAD_CONFIDENCE":
                try: meta.payload_confidence = float(v)
                except: self._warn(lineno, f"Valeur invalide: {v}")
            elif k == "ENCODED_BY":          meta.encoded_by = v
            elif k == "ENCODING_TIMESTAMP":  meta.encoding_timestamp = v
            elif k == "VERSION":             meta.version = v
        self.doc.meta = meta

    # --------------------------------------------------------
    # CONSTRAINTS
    # --------------------------------------------------------

    def _parse_constraints(self, lines):
        for lineno, line in lines[1:]:
            s = line.strip()
            if not s or s.startswith("//"): continue
            rel = self._parse_relation_line(s, lineno)
            if rel: self.doc.constraints.append(rel)

    # --------------------------------------------------------
    # UNCERTAINTY
    # --------------------------------------------------------

    def _parse_uncertainty(self, lines):
        for lineno, line in lines[1:]:
            s = line.strip()
            if not s or s.startswith("//"): continue

            m = re.match(r"(\S+)\s+UNKNOWN(?:\s+\[source=([^\]]+)\])?", s)
            if m:
                self.doc.uncertainty.append(UncertaintyItem(
                    element=m.group(1), state="UNKNOWN", source=m.group(2)
                )); continue

            m = re.match(r"(\S+)\s+ESTIMATED\s+\[(?:σ|strength)=([\d.]+)\]", s)
            if m:
                self.doc.uncertainty.append(UncertaintyItem(
                    element=m.group(1), state="ESTIMATED",
                    confidence=float(m.group(2))
                )); continue

            m = re.match(r"(\S+)\s+INFERRED\s+\[(?:σ|strength)=([\d.]+)\]", s)
            if m:
                self.doc.uncertainty.append(UncertaintyItem(
                    element=m.group(1), state="INFERRED",
                    confidence=float(m.group(2))
                )); continue

            self._warn(lineno, f"Ligne UNCERTAINTY non reconnue: {s}")

    # --------------------------------------------------------
    # DEFINE
    # --------------------------------------------------------

    def _parse_define(self, s: str, lineno: int):
        m = re.match(r"DEFINE\s+(\S+)\s+AS\s+(\S+)\s+\[([^\]]*)\]", s)
        if not m:
            self._warn(lineno, f"DEFINE mal formé: {s}"); return

        name, etype, attrs_str = m.group(1), m.group(2), m.group(3)

        if etype not in ENTITY_TYPES:
            self._warn(lineno, f"Type inconnu '{etype}' — accepté")

        attrs, extra = self._parse_attrs(attrs_str, lineno)

        if name in self.doc.entities:
            self._warn(lineno, f"Entité dupliquée: {name}")

        self.doc.entities[name] = Entity(
            name=name, type=etype, attrs=attrs, extra=extra, line=lineno
        )

    # --------------------------------------------------------
    # RELATIONS
    # --------------------------------------------------------

    def _parse_relations(self, lines):
        for lineno, line in lines[1:]:
            s = line.strip()
            if not s or s.startswith("//"): continue
            rel = self._parse_relation_line(s, lineno)
            if rel: self.doc.relations.append(rel)

    # --------------------------------------------------------
    # PARSE UNE RELATION
    # --------------------------------------------------------

    def _parse_relation_line(self, s: str, lineno: int) -> Optional[Relation]:
        # Extraire attrs à la fin [ ... ]
        attrs = Attrs()
        m = re.search(r"\[([^\]]+)\]$", s)
        if m:
            attrs, _ = self._parse_attrs(m.group(1), lineno)
            s = s[:m.start()].strip()

        # [IF] condition — supprimer
        if s.startswith("[IF]"):
            s = re.sub(r"^\[IF\]\s+\S+\s+", "", s)

        modality = None

        # Format A : [MOD] source OPERATOR target  (CONSTRAINTS)
        for sym, name in MODALITY_MAP.items():
            if s.startswith(sym):
                modality = name
                s = s[len(sym):].strip()
                break

        parts = s.split()
        if len(parts) < 2:
            self._warn(lineno, f"Relation incomplète: {s}")
            return None

        # Format B : source [MOD] OPERATOR target  (RELATIONS)
        if modality is None and len(parts) >= 3 and parts[1] in MODALITY_MAP:
            modality = MODALITY_MAP[parts[1]]
            parts = [parts[0]] + parts[2:]

        if len(parts) < 3:
            self._warn(lineno, f"Relation incomplète: {s}")
            return None

        source   = parts[0]
        operator = parts[1]
        target   = " ".join(parts[2:])

        if operator not in OPERATORS_FIXED:
            self._warn(lineno, f"Opérateur non officiel: '{operator}'")

        # Clamp strength
        if attrs.strength is not None and not (0.0 <= attrs.strength <= 1.0):
            self._warn(lineno, f"strength={attrs.strength} hors [0,1] — clampé")
            attrs.strength = max(0.0, min(1.0, attrs.strength))

        return Relation(source=source, operator=operator, target=target,
                        modality=modality, attrs=attrs, line=lineno)

    # --------------------------------------------------------
    # PARSE ATTRIBUTS
    # --------------------------------------------------------

    def _parse_attrs(self, text: str, lineno: int):
        attrs = Attrs()
        extra = {}
        count = 0

        for attr in re.split(r",\s*", text):
            attr = attr.strip()
            if not attr: continue
            count += 1
            if count > 9:
                self._warn(lineno, "k=9 dépassé — attributs supplémentaires ignorés")
                break

            # strength: σ= ou strength=
            m = re.match(r"(?:σ|strength)=([\d.]+)", attr)
            if m:
                try: attrs.strength = float(m.group(1))
                except: self._warn(lineno, f"strength invalide: {attr}")
                continue

            # layer: δ= ou layer=
            m = re.match(r"(?:δ|layer)=(\S+)", attr)
            if m:
                v = m.group(1)
                attrs.layer = LAYER_MAP.get(v, v)
                if v not in LAYER_MAP: self._warn(lineno, f"layer inconnu: {v}")
                continue

            # time: τ= ou time=
            m = re.match(r"(?:τ|time)=(\S+)", attr)
            if m:
                v = m.group(1)
                attrs.time = TIME_MAP.get(v, v)
                if v not in TIME_MAP: self._warn(lineno, f"time inconnu: {v}")
                continue

            # weight: ω= ou weight=
            m = re.match(r"(?:ω|weight)=([+\-\−°\w]+)", attr)
            if m:
                v = m.group(1)
                attrs.weight = WEIGHT_MAP.get(v, v)
                if v not in WEIGHT_MAP: self._warn(lineno, f"weight inconnu: {v}")
                continue

            # id: ι= ou id=
            m = re.match(r"(?:ι|id)=(\w+)", attr)
            if m:
                attrs.id = m.group(1)
                continue

            # deadline=
            m = re.match(r"deadline=(\S+)", attr)
            if m: attrs.deadline = m.group(1); continue

            # date=
            m = re.match(r"date=(\S+)", attr)
            if m: attrs.date = m.group(1); continue

            # trust=
            m = re.match(r"trust=([\d.]+)", attr)
            if m:
                try: attrs.trust = float(m.group(1))
                except: pass
                continue

            # Extra
            if "=" in attr:
                k, v = attr.split("=", 1)
                extra[k.strip()] = v.strip()

        attrs.extra = extra
        return attrs, extra

    # --------------------------------------------------------
    # VALIDATION
    # --------------------------------------------------------

    def _validate(self):
        # R1 — IDs uniques
        seen = {}
        for rel in self.doc.relations + self.doc.constraints:
            if rel.attrs.id:
                if rel.attrs.id in seen:
                    self._error(rel.line, f"ID dupliqué: {rel.attrs.id}")
                    self.doc.is_valid = False
                else:
                    seen[rel.attrs.id] = rel.line

        # R2 — Version présente
        if not self.doc.version:
            self._warn(0, "Version CSTL manquante")

        # R3 — Domaine présent
        if not self.doc.domain:
            self._warn(0, "Domaine CSTL manquant")

    def _warn(self, line, msg):
        self.doc.messages.append(ParseMessage("warning", line + 1, msg))

    def _error(self, line, msg):
        self.doc.messages.append(ParseMessage("error", line + 1, msg))


# ============================================================
# ENCODEUR — CSTLDocument → texte CSTL
# ============================================================

class CSTLEncoder:

    def encode(self, doc: CSTLDocument, compact: bool = True) -> str:
        out = []
        sym = self._sym if compact else self._verbose

        # Header
        out.append(f"#!CSTL {doc.version or CSTL_VERSION}")
        out.append(f"LANG:{doc.lang or 'fr'}")
        out.append(f"DOMAIN:{doc.domain or 'general'}")
        if doc.session: out.append(f"SESSION:{doc.session}")
        out.append("")

        # Symbols
        if compact:
            out.append("SYMBOLS: [FIXED] σ=strength | δ=layer(b=bedrock,d=deep,s=shallow,su=surface) | τ=time(p=past,n=present,f=future) | ω=weight(+=positive,−=negative,°=neutral) | ι=id")
            out.append("")

        # Intent
        if doc.intent_payload:
            ip = doc.intent_payload
            out.append(f"INTENT_PAYLOAD: {ip.reason} [")
            for k in ("priority", "sender", "receiver", "purpose", "context"):
                v = getattr(ip, k)
                if v: out.append(f"  {k}={v},")
            out.append("]")
            out.append("")

        # Meta
        if doc.meta:
            m = doc.meta
            out.append("META:")
            if m.payload_confidence is not None:
                out.append(f"  PAYLOAD_CONFIDENCE: {m.payload_confidence}")
            if m.encoded_by:         out.append(f"  ENCODED_BY: {m.encoded_by}")
            if m.encoding_timestamp: out.append(f"  ENCODING_TIMESTAMP: {m.encoding_timestamp}")
            if m.version:            out.append(f"  VERSION: {m.version}")
            out.append("")

        # Constraints
        if doc.constraints:
            out.append("CONSTRAINTS:")
            for rel in doc.constraints:
                out.append("  " + self._encode_relation(rel, compact))
            out.append("")

        # Uncertainty
        if doc.uncertainty:
            out.append("UNCERTAINTY:")
            for u in doc.uncertainty:
                if u.state == "UNKNOWN":
                    src = f" [source={u.source}]" if u.source else ""
                    out.append(f"  {u.element} UNKNOWN{src}")
                elif u.state in ("ESTIMATED", "INFERRED"):
                    s = sym("σ", "strength")
                    out.append(f"  {u.element} {u.state} [{s}={u.confidence}]")
            out.append("")

        # Entities
        for name, entity in doc.entities.items():
            attrs_str = self._encode_attrs(entity.attrs, compact)
            all_attrs = attrs_str




        if doc.entities: out.append("")

        # Relations
        if doc.relations:
            out.append("RELATIONS:")
            for rel in doc.relations:
                out.append("  " + self._encode_relation(rel, compact))

        out.append("---END---")
        return "\n".join(out)

    def _encode_relation(self, rel: Relation, compact: bool) -> str:
        mod = f"[{'!' if rel.modality=='MUST' else '¬' if rel.modality=='NOT' else '?'}] " if rel.modality else ""
        attrs_str = self._encode_attrs(rel.attrs, compact)
        brackets = f" [{attrs_str}]" if attrs_str else ""
        return f"{mod}{rel.source} {rel.operator} {rel.target}{brackets}"

    def _encode_attrs(self, attrs: Attrs, compact: bool) -> str:
        parts = []
        sym = self._sym if compact else self._verbose
        if attrs.strength  is not None: parts.append(f"{sym('σ','strength')}={attrs.strength}")
        if attrs.layer:                  parts.append(f"{sym('δ','layer')}={self._layer_short(attrs.layer) if compact else attrs.layer}")
        if attrs.time:                   parts.append(f"{sym('τ','time')}={self._time_short(attrs.time) if compact else attrs.time}")
        if attrs.weight:                 parts.append(f"{sym('ω','weight')}={self._weight_short(attrs.weight) if compact else attrs.weight}")
        if attrs.id:                     parts.append(f"{sym('ι','id')}={attrs.id}")
        if attrs.deadline:               parts.append(f"deadline={attrs.deadline}")
        if attrs.date:                   parts.append(f"date={attrs.date}")
        if attrs.trust is not None:      parts.append(f"trust={attrs.trust}")
        for k, v in attrs.extra.items(): parts.append(f"{k}={v}")
        return ", ".join(parts)

    def _sym(self, s, v): return s
    def _verbose(self, s, v): return v

    def _layer_short(self, v):
        return {"bedrock": "b", "deep": "d", "shallow": "s", "surface": "su"}.get(v, v)

    def _time_short(self, v):
        return {"past": "p", "present": "n", "future": "f"}.get(v, v)

    def _weight_short(self, v):
        return {"positive": "+", "negative": "−", "neutral": "°"}.get(v, v)


# ============================================================
# FONCTIONS UTILITAIRES
# ============================================================

def parse(text: str) -> CSTLDocument:
    """Parse un texte CSTL et retourne un CSTLDocument."""
    return CSTLParser().parse(text)


def encode(doc: CSTLDocument, compact: bool = True) -> str:
    """Encode un CSTLDocument en texte CSTL."""
    return CSTLEncoder().encode(doc, compact)


def validate(text: str) -> dict:
    """Valide un texte CSTL et retourne un rapport."""
    doc = parse(text)
    return {
        "valid":    doc.is_valid,
        "version":  doc.version,
        "domain":   doc.domain,
        "entities": len(doc.entities),
        "relations": len(doc.relations),
        "constraints": len(doc.constraints),
        "uncertainty": len(doc.uncertainty),
        "warnings": [str(m) for m in doc.warnings()],
        "errors":   [str(m) for m in doc.errors()],
    }


# ============================================================
# TESTS
# ============================================================

TEST_PAYLOAD = """
#!CSTL v4.0
LANG:fr
DOMAIN:diplomatique
SESSION:test_symboles_004

SYMBOLS: [FIXED] σ=strength | δ=layer(b=bedrock,d=deep,s=shallow,su=surface) | τ=time(p=past,n=present,f=future) | ω=weight(+=positive,−=negative,°=neutral) | ι=id

INTENT_PAYLOAD: valider_symboles [
  priority=critical,
  sender=Claude,
  receiver=Gemini,
  purpose=test_reconnaissance_symboles_v4,
  context=validation_trilaterale
]

META:
  PAYLOAD_CONFIDENCE: 0.99
  ENCODED_BY: claude-sonnet-4
  ENCODING_TIMESTAMP: 2026-04-27T18:30:00
  VERSION: v4.0

CONSTRAINTS:
  [¬] James DIVULGUE instructions_washington [σ=1.0, δ=b]
  [!] Sofia OBTAIN article12 [σ=0.95, δ=b]
  [IF] accord_absent [¬] delegation SIGNER traite [σ=0.90]

UNCERTAINTY:
  mandat_james UNKNOWN [source=confidentiel]
  position_finale ESTIMATED [σ=0.55]
  duree_negociation INFERRED [σ=0.68]

DEFINE James AS agent [ι=e001, δ=b, pays=USA]
DEFINE Sofia AS agent [ι=e002, δ=b, pays=EU]
DEFINE accord AS document [ι=e003, δ=b, deadline=2026-04-27]

RELATIONS:
  James [¬] DIVULGUE instructions_washington [σ=1.0, δ=b, τ=n, ω=−, ι=e010]
  Sofia [!] OBTAIN article12 [σ=0.95, δ=b, τ=f, ω=+, ι=e011]
  James RESIST accord [σ=0.78, δ=d, τ=n, ω=−, ι=e012]
  Sofia PRESSURE James [σ=0.82, δ=d, τ=n, ω=−, ι=e013]
  negociation STATE critique [σ=0.88, δ=su, τ=n, ω=−, ι=e014]

---END---
"""

if __name__ == "__main__":
    print("=" * 60)
    print("CSTL v4.0 — Test du parser")
    print("=" * 60)

    doc = parse(TEST_PAYLOAD)

    print(f"\nVersion  : {doc.version}")
    print(f"Domaine  : {doc.domain}")
    print(f"Session  : {doc.session}")
    print(f"Valide   : {doc.is_valid}")

    if doc.meta:
        print(f"\nMETA:")
        print(f"  Confiance    : {doc.meta.payload_confidence}")
        print(f"  Encodeur     : {doc.meta.encoded_by}")

    if doc.intent_payload:
        print(f"\nINTENT: {doc.intent_payload.reason}")
        print(f"  Priorité : {doc.intent_payload.priority}")
        print(f"  Émetteur : {doc.intent_payload.sender} → {doc.intent_payload.receiver}")

    print(f"\nCONTRAINTES ({len(doc.constraints)}):")
    for c in doc.constraints:
        mod = f"[{c.modality}] " if c.modality else ""
        print(f"  {mod}{c.source} {c.operator} {c.target} | σ={c.attrs.strength}")

    print(f"\nINCERTITUDE ({len(doc.uncertainty)}):")
    for u in doc.uncertainty:
        conf = f" σ={u.confidence}" if u.confidence else ""
        src  = f" source={u.source}" if u.source else ""
        print(f"  {u.element} → {u.state}{conf}{src}")

    print(f"\nENTITÉS ({len(doc.entities)}):")
    for name, e in doc.entities.items():
        print(f"  {name} AS {e.type} | layer={e.attrs.layer} id={e.attrs.id}")

    print(f"\nRELATIONS ({len(doc.relations)}):")
    for r in doc.relations:
        mod = f"[{r.modality}] " if r.modality else ""
        print(f"  {mod}{r.source} {r.operator} {r.target} | σ={r.attrs.strength} δ={r.attrs.layer} τ={r.attrs.time}")

    if doc.messages:
        print(f"\nMESSAGES ({len(doc.messages)}):")
        for m in doc.messages:
            print(f"  {m}")

    print("\n" + "=" * 60)
    print("RE-ENCODAGE compact:")
    print("=" * 60)
    print(encode(doc, compact=True))

    print("\n" + "=" * 60)
    print("RAPPORT DE VALIDATION:")
    print("=" * 60)
    report = validate(TEST_PAYLOAD)
    print(json.dumps(report, ensure_ascii=False, indent=2))


# ============================================================
# INTÉGRATION DES ONTOLOGIES DE DOMAINE
# ============================================================

def _load_domain_operators(domain: str) -> set:
    """Charge les opérateurs d'un domaine depuis cstl_domains."""
    try:
        from cstl_domains import get_domain_operators
        return get_domain_operators(domain)
    except ImportError:
        return set()


# Monkey-patch du parser pour utiliser les domaines
_original_run = CSTLParser._run

def _patched_run(self):
    _original_run(self)
    # Après parsing, enrichir avec les opérateurs du domaine
    self._domain_ops = _load_domain_operators(self.doc.domain)

CSTLParser._run = _patched_run

_original_warn_op = None

def _patched_parse_relation_line(self, s: str, lineno: int):
    rel = _original_parse_relation(self, s, lineno)
    if rel and rel.operator not in OPERATORS_FIXED:
        domain_ops = _load_domain_operators(self.doc.domain)
        if rel.operator in domain_ops:
            # Supprimer le warning précédent sur cet opérateur
            self.doc.messages = [
                m for m in self.doc.messages
                if not (m.severity == "warning" and rel.operator in m.message and "non officiel" in m.message)
            ]
    return rel

_original_parse_relation = CSTLParser._parse_relation_line
CSTLParser._parse_relation_line = _patched_parse_relation_line
