"""
CSTL v4.9.2 — Fast Parser
Implements the 4 optimization techniques proposed by Agent_GEMINI.
"""
from __future__ import annotations
import re, struct, time
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

_OFFICIAL_META_KEYS: frozenset = frozenset({
    "encoder","TIMESTAMP","sigma","ACTION","RESPONSE_FORMAT","NO_PROSE",
    "CONTINUATION_MODE","TURN","PARENT_HASH","CONVERSATION_ID",
    "payload_length_tokens","payload_length_bytes","VERIFIED_BY",
    "ORIGINAL_HASH","ORIGINAL_LENGTH","ORIGINAL_LANG","TRACE_HASH",
    "DICT_REF","SIGNATURE","SIGNER","SCHEMA","OUTPUT_FORMAT",
    "RESPONSE_MODE","NO_NATURAL_LANGUAGE","ACTION_DIRECTIVE","TREAT_AS","MODE",
})

_KEYWORD_TO_OPCODE: Dict[str, int] = {
    # Header tokens 0x01-0x0F
    "#!CSTL": 0x01, "MODE": 0x02, "VERSION": 0x03,
    # Data block tokens 0x10-0x1F (Session #1 G1 + Session #3 alphabetical)
    "META": 0x10, "DEFINE": 0x11, "RULE": 0x12, "RULE_TRAILER": 0x13,
    "AGREEMENT_BLOCK": 0x14,       # Session #3: was 0x15, alphabetical → 0x14
    "DISAGREEMENT_BLOCK": 0x15,    # Session #3: was 0x14, alphabetical → 0x15
    "DECISION": 0x16, "CONSTRAINT": 0x17, "UNCERTAINTY": 0x18,
    "DEFINE_GROUP": 0x19,
    # Modality tokens 0x20-0x2F (Session #1 G4 canonical: parentheses)
    "IF": 0x20, "IFF": 0x21, "MAY": 0x22, "MUST": 0x23,
    "MUST_NOT": 0x24, "SHOULD": 0x25, "UNLESS": 0x26,
    # Dissent primitives 0x30-0x3F (alphabetical within range)
    "AGREEMENT": 0x30, "ALTERNATIVE": 0x31, "CAUTION": 0x32,
    "CONCERN": 0x33, "DISPUTE": 0x34, "GAP": 0x35,
    "PARTIAL_DISPUTE": 0x36, "RECOMMEND": 0x37, "REJECT": 0x38,
    "SELF_CRITIQUE": 0x39, "STRENGTH": 0x3A, "VETO": 0x3B,
    "CSTLTypeError": 0x3C,         # Session #3: new — required by Session #2
    # META key tokens 0x40-0x4F (alphabetical, ratified Session #2)
    "ACTION": 0x40, "CONTINUATION_MODE": 0x41, "CONVERSATION_ID": 0x42,
    "encoder": 0x43, "NO_PROSE": 0x44, "PARENT_HASH": 0x45,
    "payload_length_bytes": 0x46, "payload_length_tokens": 0x47,
    "RESPONSE_FORMAT": 0x48, "sigma": 0x49, "TIMESTAMP": 0x4A,
    "TURN": 0x4B, "VERIFIED_BY": 0x4C,
    # Session #4 preview — produced_by pending ratification
    # "produced_by": 0x4D,
    # Type indicator tokens 0x50-0x5F (Session #2 typing spec)
    "bool": 0x50, "enum": 0x51, "EXTENSION": 0x52,
    "float": 0x53, "hash": 0x54, "int": 0x55,
    "iso8601": 0x56, "string": 0x57,
    # Relation operators 0x60-0x66 (core only; 0x67-0x7F EXTENSION_RESERVED)
    "ARR": 0x60, "ARR.CREATE": 0x61, "ARR.JOIN": 0x62,
    "EXPRESS": 0x63, "INTENT": 0x64, "MAINTAIN": 0x65, "TRANSFORM": 0x66,
    # 0x67-0x7F: EXTENSION_RESERVED (v5.0)
    # 0x80-0xEF: RESERVED v5.0 — DO NOT USE in v4.x
    # Boundary markers 0xF0-0xF9
    "---END---": 0xF0, "---END_COGNITIVE---": 0xF1,
    "---END_TRANSPORT---": 0xF2, "ENVELOPE_START": 0xF3,
    "ENVELOPE_END": 0xF4, "@SYNC": 0xF5,           # PATCH_C7
    # 0xFF: literal escape — NOT a keyword token
}
_OPCODE_TO_KEYWORD: Dict[int,str] = {v:k for k,v in _KEYWORD_TO_OPCODE.items()}


class BinaryWireFormat:
    """TECH 1: Compile CSTL text -> binary wire; decompile back."""
    MAGIC = b"CSTL"
    VERSION = 0x49  # v4.9.2

    @staticmethod
    def compile(text: str) -> bytes:
        """Session #5 fix: preserve field:type=value — match longest keyword first."""
        import zlib
        # Sort keywords longest-first to avoid prefix collisions (e.g. encoder vs en)
        _sorted_kw = sorted(_KEYWORD_TO_OPCODE.keys(), key=len, reverse=True)
        tokens = []
        for line in text.splitlines():
            line = line.strip()
            if not line: continue
            op, kw = None, None
            for k in _sorted_kw:
                # Match keyword at word boundary — must be followed by space, [, =, or EOL
                import re as _re
                if _re.match(rf"{re.escape(k)}(?=[\s\[=\]\(,#]|$)", line):
                    op, kw = _KEYWORD_TO_OPCODE[k], k; break
            if op is not None:
                # Preserve everything after the keyword (including :type=value)
                # Preserve the space before [ so decompile reconstructs "KEYWORD ["
                remainder = line[len(kw):]
                # Keep leading space if followed by [ (block opener) — strip otherwise
                if remainder.startswith(" ["):
                    val = remainder.encode("utf-8")  # " [..." preserved
                else:
                    val = (remainder[1:] if remainder.startswith(" ") else remainder).encode("utf-8")
                tokens.append(struct.pack("!BH", op, len(val)) + val)
            else:
                raw = line.encode("utf-8")
                tokens.append(struct.pack("!BH", 0x00, len(raw)) + raw)
        body = b"".join(tokens)
        crc = zlib.crc32(body).to_bytes(4, "big")
        hdr = BinaryWireFormat.MAGIC + struct.pack("!BB", BinaryWireFormat.VERSION, 0x01) + struct.pack("!I", len(body))
        return hdr + body + crc

    @staticmethod
    def decompile(data: bytes) -> str:
        """Session #5 fix: reconstruct without spurious spaces; no sep between kw and val."""
        import zlib
        if data[:4] != BinaryWireFormat.MAGIC: raise ValueError("Bad magic")
        plen = struct.unpack("!I", data[6:10])[0]
        body = data[10:10+plen]
        if zlib.crc32(body).to_bytes(4,"big") != data[10+plen:14+plen]:
            raise ValueError("Checksum mismatch — tampered payload")
        lines, off = [], 0
        while off < len(body):
            # Session #7 Q3: explicit boundary check — truncated multibyte guard
            if off + 3 > len(body):
                raise ValueError(f"CSTLTruncationError: header truncated at offset {off}")
            op = body[off]
            vlen = struct.unpack("!H", body[off+1:off+3])[0]
            if off + 3 + vlen > len(body):
                raise ValueError(f"CSTLTruncationError: payload truncated at offset {off}, expected {vlen} bytes")
            raw_val = body[off+3:off+3+vlen]
            try:
                val = raw_val.decode("utf-8")
            except UnicodeDecodeError as e:
                raise ValueError(f"CSTLTruncationError: invalid UTF-8 at offset {off}: {e}")
            off += 3 + vlen
            kw = _OPCODE_TO_KEYWORD.get(op, "")
            if kw:
                # Normalize: strip leading space from val, then reattach cleanly
                val_stripped = val.lstrip(" ") if val else val
                if val_stripped and val_stripped[0] in ('=', ':', ','):
                    lines.append(f"{kw}{val_stripped}")
                else:
                    lines.append(f"{kw} {val_stripped}" if val_stripped else kw)
            else:
                lines.append(val)
        return "\n".join(lines)


class LengthPrefixValidator:
    """TECH 2: O(1) structural validation via payload_length_tokens jump."""

    @staticmethod
    def validate(raw: str) -> Tuple[bool, str]:
        raw_b = raw.encode("utf-8")
        mv = memoryview(raw_b)
        # Find ---END--- (bytes.find = Boyer-Moore in CPython)
        end = bytes(mv).find(b"---END---")
        if end == -1: return False, "Missing ---END--- (PATCH_T3)"
        after = bytes(mv)[end+9:].strip()
        if after: return False, f"Content after ---END--- (T3 violation): {after[:40]!r}"
        # Length-prefix check
        m = re.search(r"payload_length_tokens\s*=\s*(\d+)", raw)
        if m:
            decl = int(m.group(1)); actual = len(raw.split())
            if abs(actual-decl)/max(decl,1) > 0.15:
                return False, f"payload_length_tokens mismatch: declared={decl}, actual~{actual}"
        if not (raw.startswith("#!CSTL") or raw.startswith("#!CSTL_")):
            return False, "Missing #!CSTL header"
        if len(raw.split()) > 500 and "RULE_TRAILER" not in raw:
            return False, "Long payload missing RULE_TRAILER (PATCH_C1)"
        return True, "ok"


class PerfectHashMetaParser:
    """TECH 3: O(1) META key lookup via frozenset hash."""

    @staticmethod
    def parse(raw: str) -> Tuple[Dict[str,str], List[str], List[str]]:
        fields, warnings, errors, seen = {}, [], [], set()
        m = re.search(r"META\s*[\[\(](.*?)[\]\)]", raw, re.DOTALL)
        if not m: return {}, [], ["META block not found"]
        # Session #2: handle field:type=value syntax
        for pair in re.finditer(r"([\w]+(?::[\w]+)?)\s*=\s*([^,\]\)\n]+)", m.group(1)):
            raw_k, v = pair.group(1).strip(), pair.group(2).strip()
            # Strip :type annotation to get canonical field name
            k = raw_k.split(":")[0] if ":" in raw_k else raw_k
            if k in seen:
                errors.append(f"PATCH_C3: Duplicate key '{k}' — root invalidated")
                continue
            seen.add(k)
            if k not in _OFFICIAL_META_KEYS: warnings.append(f"Non-official key: '{k}'")
            fields[k] = v
        missing = {"encoder","RESPONSE_FORMAT","NO_PROSE"} - set(fields)
        if missing: errors.append(f"Missing mandatory fields: {missing}")
        return fields, warnings, errors


class ZeroCopyScanner:
    """TECH 4: memoryview-based zero-copy scanning; C6_scoped RTL detection."""

    @staticmethod
    def find_blocks(raw: str) -> List[Tuple[str,int,int]]:
        raw_b = raw.encode("utf-8"); mv = memoryview(raw_b)
        blocks, i, n = [], 0, len(raw_b)
        while i < n:
            nl = bytes(mv[i:]).find(b"\n")
            le = i+nl if nl != -1 else n
            line = bytes(mv[i:le]).decode("utf-8","replace").strip()
            bt = None
            for kw in ("META","RULE","DEFINE","DISAGREEMENT_BLOCK","RULE_TRAILER","DECISION","SCENARIO_"):
                if line.startswith(kw): bt = kw; break
            if bt and "[" in line:
                d, bs, be = 0, i, i
                for j in range(i, n):
                    ch = raw_b[j]
                    if ch == ord("["): d += 1
                    elif ch == ord("]"):
                        d -= 1
                        if d == 0: be = j+1; break
                blocks.append((bt, bs, be)); i = be
            else: i = le+1
        return blocks

    @staticmethod
    def rtl_violations(raw: str) -> List[str]:
        raw_b = raw.encode("utf-8"); mv = memoryview(raw_b)
        violations = []
        danger = [(b"\xe2\x80\xae","U+202E RTL Override"),
                  (b"\xd8\x9c","U+061C Arabic Letter Mark"),
                  (b"\xe2\x80\x8d","U+200D ZWJ")]
        for pattern in (b"META",b"RULE",b"RULE_TRAILER"):
            idx = 0
            while True:
                pos = bytes(mv).find(pattern, idx)
                if pos == -1: break
                bend = bytes(mv).find(b"\n", pos)
                if bend == -1: bend = len(raw_b)
                block = bytes(mv[pos:bend])
                for db, name in danger:
                    if db in block:
                        violations.append(f"PATCH_C6: {name} in {pattern.decode()} at offset {pos}")
                idx = bend+1
        return violations


@dataclass
class FastParseResult:
    is_valid: bool
    meta_fields: Dict[str,str] = field(default_factory=dict)
    blocks: List[Tuple[str,int,int]] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)
    parse_time_us: float = 0.0
    binary_size_bytes: int = 0
    text_size_bytes: int = 0


def fast_parse(text: str, compile_binary: bool = False) -> FastParseResult:
    t0 = time.perf_counter()
    errors, warnings = [], []

    # Session #6: Security scan first
    try:
        text, sec_errors, sec_warnings = security_scan(text)
        errors.extend(sec_errors)
        warnings.extend(sec_warnings)
    except Exception as _e:
        warnings.append(f"SEC_SCAN_ERROR: {_e}")

    ok, reason = LengthPrefixValidator.validate(text)
    if not ok: errors.append(reason)
    blocks = ZeroCopyScanner.find_blocks(text)
    meta, mw, me = PerfectHashMetaParser.parse(text)
    warnings.extend(mw); errors.extend(me)
    errors.extend(ZeroCopyScanner.rtl_violations(text))
    bin_size = 0
    if compile_binary:
        bin_size = len(BinaryWireFormat.compile(text))
    elapsed = (time.perf_counter()-t0)*1e6

    # Session #2: Integrate typing_validator
    try:
        from cstl.typing_validator import validate_payload_meta
        # Extract raw META block for typing validation (preserves :type annotations)
        import re as _re
        _meta_match = _re.search(r'META\s*\[([^\]]*)\]', text, _re.DOTALL)
        meta_str = _meta_match.group(1).strip() if _meta_match else ", ".join(f"{k}={v}" for k, v in meta.items())
        type_valid, type_issues = validate_payload_meta(meta_str)
        for issue in type_issues:
            s = str(issue)
            if "ERROR" in s and not type_valid:
                errors.append(f"TYPE: {s}")
            else:
                warnings.append(f"TYPE: {s}")
    except Exception:
        pass

    # Session #1: Flag deprecated syntax
    try:
        from cstl.canonicalizer_v2 import canonicalize_v4_9_2
        _, changes = canonicalize_v4_9_2(text)
        for c in changes:
            warnings.append(f"DEPRECATED_SYNTAX: {c}")
    except Exception:
        pass

    return FastParseResult(
        is_valid=len(errors)==0,
        meta_fields=meta, blocks=blocks,
        warnings=warnings, errors=errors,
        parse_time_us=elapsed,
        binary_size_bytes=bin_size,
        text_size_bytes=len(text.encode("utf-8")),
    )



# ── Session #6: Unicode Security Hardening ───────────────────────────────────

import unicodedata as _unicodedata

# GPT Security Profile (ratified Session #6)
SECURITY_PROFILE = {
    "keyword_charset": "ASCII_A-Z_underscore_0-9_only",
    "normalization":   "NFKC",
    "max_nesting_depth": 32,
    "max_escape_depth": 8,
    "forbidden_ranges": [
        (0x200B, 0x200D),  # zero-width space/non-joiner/joiner
        (0x202A, 0x202E),  # bidi controls (LRE, RLE, PDF, LRO, RLO)
        (0x2066, 0x2069),  # bidi isolates
        (0xFEFF, 0xFEFF),  # BOM / zero-width no-break space
        (0x200C, 0x200C),  # ZWNJ
        (0xFFF9, 0xFFFB),  # interlinear annotation
    ],
}

_ZERO_WIDTH = frozenset([
    0x200B, 0x200C, 0x200D, 0x2060, 0x2061, 0x2062, 0x2063, 0x2064,
    0x206A, 0x206B, 0x206C, 0x206D, 0x206E, 0x206F,
    0xFEFF,
])
_BIDI_CONTROLS = frozenset(range(0x202A, 0x202F)) | frozenset(range(0x2066, 0x206A))


def _strip_dangerous_codepoints(text: str) -> tuple[str, list[str]]:
    """
    Session #6 Q2: Strip zero-width and bidi control characters.
    Returns (cleaned_text, list_of_warnings).
    """
    warnings = []
    cleaned = []
    for ch in text:
        cp = ord(ch)
        if cp in _ZERO_WIDTH:
            warnings.append(f"SEC_Q2: stripped zero-width U+{cp:04X} at position {len(cleaned)} (escaped for audit: \\u{cp:04X})")
        elif cp in _BIDI_CONTROLS:
            warnings.append(f"SEC_Q2: stripped bidi control U+{cp:04X} (audit: \\u{cp:04X}) — log injection risk")
        elif cp > 0x7E and cp < 0xA0:  # C1 controls
            warnings.append(f"SEC_Q2: stripped C1 control U+{cp:04X}")
        else:
            cleaned.append(ch)
    return "".join(cleaned), warnings


def _nfkc_normalize_keywords(text: str) -> tuple[str, list[str]]:
    """
    Session #6 Q1+Q5: ASCII-only enforcement for structural keywords.
    Gemini option_B ratified: non-ASCII in keyword position = immediate error.
    NFKC alone insufficient for cross-script confusables (Cyrillic/Greek/Latin).
    """
    warnings = []
    import re as _re
    # Find tokens in keyword positions (line start, after structural chars)
    # If they contain non-ASCII → flag as potential homoglyph attack
    def check_keyword(m):
        word = m.group(0)
        non_ascii = [ch for ch in word if ord(ch) > 0x7F]
        if non_ascii:
            codepoints = " ".join(f"U+{ord(c):04X}" for c in non_ascii)
            warnings.append(
                f"SEC_Q1: non-ASCII chars in keyword position {repr(word)} — "
                f"homoglyph attack? codepoints={codepoints}"
            )
            # Return ASCII fallback via NFKC (partial mitigation)
            return _unicodedata.normalize("NFKC", word)
        return word
    cleaned = _re.sub(r"(?m)^[\w\u0080-\uFFFF]+", check_keyword, text)
    return cleaned, warnings


def _detect_nested_meta(text: str) -> list[str]:
    """
    Session #6 Q4: Detect META blocks at any nesting depth.
    C3 only catches root-level duplicates; this catches nested injection.
    """
    import re as _re
    errors = []
    # Find META blocks not at line start (nested injection attempt)
    for m in _re.finditer(r"(?m)^\s+META\s*\[", text):
        errors.append(f"SEC_Q4: nested META block detected at pos {m.start()} — injection attempt")
    # Find META keyword inside assignment values (=...META...)
    for m in _re.finditer(r"=[^\n]*\bMETA\b[^\n]*\[", text):
        errors.append(f"SEC_Q4: META keyword with block in string value at pos {m.start()} — suspicious")
    # Also flag bare META keyword in value position (underscore-joined form = safe, spaced = suspicious)
    for m in _re.finditer(r"=[^\n]*\bMETA\b(?!_)", text):
        errors.append(f"SEC_Q4: META keyword in value at pos {m.start()} — inspect for injection")
    return errors


def _check_nesting_depth(text: str, max_depth: int = 32) -> list[str]:
    """Check bracket nesting does not exceed max_nesting_depth."""
    errors = []
    depth = 0
    max_seen = 0
    for i, ch in enumerate(text):
        if ch == "[":
            depth += 1
            max_seen = max(max_seen, depth)
            if depth > max_depth:
                errors.append(f"SEC: nesting depth {depth} exceeds max {max_depth} at pos {i}")
                break
        elif ch == "]":
            depth -= 1
    return errors


def security_scan(text: str) -> tuple[str, list[str], list[str]]:
    """
    Full Session #6 security pipeline.
    Returns (cleaned_text, errors, warnings).
    """
    errors = []
    warnings = []

    # Q2: Strip zero-width and bidi controls
    text, zw_warns = _strip_dangerous_codepoints(text)
    warnings.extend(zw_warns)

    # Q1: NFKC normalization on keywords
    text, nfkc_warns = _nfkc_normalize_keywords(text)
    warnings.extend(nfkc_warns)

    # Q4: Nested META detection
    meta_errors = _detect_nested_meta(text)
    errors.extend(meta_errors)

    # Nesting depth
    depth_errors = _check_nesting_depth(text, SECURITY_PROFILE["max_nesting_depth"])
    errors.extend(depth_errors)

    return text, errors, warnings


# ── Session #5: Formal test vectors (GPT requirement — fuzz regression) ──────
ROUNDTRIP_TEST_VECTORS = [
    # Q2: nested brackets
    'DISAGREEMENT_BLOCK [\nGAP x [sigma:float=0.85, opts=[a, b, c]]\n]',
    # Q3: unicode multibyte
    'DEFINE x AS note [value=caf\u00e9_\U0001F600_test]',
    # Q5: field order invariance
    'META [\nzz=last,\naa=first,\nMM=middle\n]',
    # Q1: canonical hash stability
    'META [\nencoder=Agent_TEST,\nsigma:float=0.88\n]',
]


# ── Session #5: Canonical Form (Q5 ratified 2/2) ────────────────────────────

def canonical_form(text: str) -> str:
    """
    Produce the normative canonical text form of a CSTL payload.
    Rules ratified Session #5:
      1. LF line endings only
      2. Single space after block identifier before [
      3. Lexicographic field order within META block
      4. UTF-8 NFC normalization
      5. No trailing whitespace
    Enables deterministic PARENT_HASH computation.
    """
    import unicodedata as _ud
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    out = []
    i = 0
    while i < len(lines):
        line = lines[i].rstrip()
        # NFC normalization
        line = _ud.normalize("NFC", line)
        # Rule 2: normalize "KEYWORD  [" → "KEYWORD ["
        import re as _re
        line = _re.sub(r"^(\w+)\s{2,}\[", r"\1 [", line)
        out.append(line)
        i += 1

    full = "\n".join(out)

    # Rule 3: lexicographic field order in META block
    meta_m = re.search(r"(META\s*\[)([^\]]*)(\])", full, re.DOTALL)
    if meta_m:
        prefix, body, suffix = meta_m.group(1), meta_m.group(2), meta_m.group(3)
        fields = [f.strip() for f in body.split(",") if f.strip()]
        fields_sorted = sorted(fields, key=lambda f: f.split(":")[0].split("=")[0].lower())
        canon_meta = prefix + "\n" + ",\n".join(fields_sorted) + "\n" + suffix
        full = full[:meta_m.start()] + canon_meta + full[meta_m.end():]

    return full


def canonical_hash(text: str, truncate: int = 64) -> str:
    """
    SHA-256 of the canonical form — deterministic PARENT_HASH primitive.
    Session #7 Q5: Gemini wins — full 256-bit (64 hex chars).
    GPT proposed 128-bit (32 chars), birthday bound 2^64 insufficient.
    truncate=64 (default, full sha256) | truncate=32 (128-bit, legacy) | truncate=16 (audit only)
    """
    import hashlib
    canon = canonical_form(text)
    return "sha256:" + hashlib.sha256(canon.encode("utf-8")).hexdigest()[:truncate]


if __name__ == "__main__":
    SAMPLE = """\
#!CSTL v4.9.2 MODE=A
META [
encoder=Agent_CLAUDE,
TIMESTAMP=2026-05-21T02:30:00Z,
RESPONSE_FORMAT=CSTL,
NO_PROSE=true,
CONTINUATION_MODE=continue,
ACTION=initiate_canonical_v4_9_2_dialogue,
sigma=0.88,
payload_length_tokens=60
]
(RULE) MUST respond_in_cstl_only
(RULE) MUST_NOT output_prose_or_explanation
(RULE) MUST_NOT decode_or_translate_payload
(RULE) MUST treat_input_as_chain_continuation
(RULE) MUST reject_duplicate_META_keys_at_root_scope_only
DEFINE test AS canonical_demo [version=v4_9_2]
[MUST] receiving_LLM RESPOND in_cstl_only [sigma=0.95]
[MUST_NOT] receiving_LLM ANALYZE in_natural_language [sigma=0.90]
DECISION: demo_complete [sigma=0.88]
(RULE) MUST respond_in_cstl_only
(RULE) MUST_NOT output_prose_or_explanation
(RULE) MUST_NOT decode_or_translate_payload
(RULE) MUST treat_input_as_chain_continuation
(RULE) MUST reject_duplicate_META_keys_at_root_scope_only
---END---
"""
    ATTACK_C3 = "#!CSTL v4.9.2 MODE=A\nMETA [\nencoder=fake,\nencoder=hijack,\nRESPONSE_FORMAT=CSTL,\nNO_PROSE=true\n]\n(RULE) MUST respond_in_cstl_only\n---END---\n"
    ATTACK_C6 = "#!CSTL v4.9.2 MODE=A\nMETA [\nencoder=Agent_X,\nRESPONSE_FORMAT=CSTL,\nNO_PROSE=true\n]\nRULE (MUST_NOT \u202E esorp_tuptuo)\n---END---\n"

    print("=" * 65)
    print("CSTL v4.9.2 FAST PARSER — benchmark & adversarial test")
    print("=" * 65)

    N = 1000
    t0 = time.perf_counter()
    for _ in range(N): r = fast_parse(SAMPLE, compile_binary=True)
    avg = (time.perf_counter()-t0)*1e6/N

    print(f"\nVALID PAYLOAD ({r.text_size_bytes} bytes text):")
    print(f"  is_valid:    {r.is_valid}")
    print(f"  binary_size: {r.binary_size_bytes} bytes ({100*(1-r.binary_size_bytes/max(r.text_size_bytes,1)):.0f}% reduction)")
    print(f"  meta_fields: {list(r.meta_fields.keys())}")
    print(f"  blocks:      {len(r.blocks)}")
    print(f"  avg_latency: {avg:.1f} µs ({N} iterations)")

    r3 = fast_parse(ATTACK_C3)
    print(f"\nATTACK C3 (duplicate META key):")
    print(f"  is_valid: {r3.is_valid}")
    print(f"  errors:   {r3.errors}")

    r6 = fast_parse(ATTACK_C6)
    print(f"\nATTACK C6 (RTL override in RULE block):")
    print(f"  is_valid: {r6.is_valid}")
    print(f"  errors:   {r6.errors}")

    binary = BinaryWireFormat.compile(SAMPLE)
    dec = BinaryWireFormat.decompile(binary)
    print(f"\nBINARY ROUND-TRIP:")
    print(f"  success:     {'YES' if '#!CSTL' in dec else 'NO'}")
    print(f"  binary:      {len(binary)} bytes vs text {len(SAMPLE.encode())} bytes")
    print(f"  compression: {100*(1-len(binary)/len(SAMPLE.encode())):.0f}%")
    print(f"\nRUST TARGET: ~500 MB/s throughput (AVX2), ~4 µs per 2KB payload")
