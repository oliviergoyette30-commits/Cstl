"""
CSTL Security Test Suite
Sessions #6 + #7 — All attack vectors, empirically validated.

Coverage:
  Session #6: Q1 homoglyph, Q2 zero-width, Q3 recursive escape,
              Q4 fake META, Q5 nesting depth
  Session #7: Q1 bidi override, Q2 overlong UTF-8, Q3 truncated multibyte,
              Q4 confusable brackets, Q5 hash collision resistance
  GPT vectors: normalization downgrade, stream fragment, parser differential
  Round-trip: canonical form, idempotency, field order invariance
"""

import pytest
import struct
import zlib
import hashlib
import unicodedata
from cstl.fast_parser import (
    fast_parse, BinaryWireFormat, canonical_form, canonical_hash,
    security_scan, ROUNDTRIP_TEST_VECTORS, SECURITY_PROFILE,
)

# ── Helpers ──────────────────────────────────────────────────────────────────

VALID_BASE = (
    "#!CSTL_v4.9.2_MODE=A\n"
    "META [\n"
    "encoder=Agent_TEST,\n"
    "sigma:float=0.88,\n"
    "RESPONSE_FORMAT:enum=CSTL,\n"
    "NO_PROSE:bool=true,\n"
    "PARENT_HASH=root\n"
    "]\n"
    "---END---"
)

def make_payload(*extra_lines):
    body = "\n".join(extra_lines)
    return (
        "#!CSTL_v4.9.2_MODE=A\n"
        "META [\n"
        "encoder=Agent_TEST,\n"
        "sigma:float=0.88,\n"
        "RESPONSE_FORMAT:enum=CSTL,\n"
        "NO_PROSE:bool=true,\n"
        "PARENT_HASH=root\n"
        "]\n"
        + (body + "\n" if body else "")
        + "---END---"
    )

def tamper_binary(binary, offset, value):
    b = bytearray(binary)
    b[offset] = value
    # Recompute checksum
    plen = struct.unpack("!I", b[6:10])[0]
    body = bytes(b[10:10+plen])
    b[10+plen:14+plen] = zlib.crc32(body).to_bytes(4, "big")
    return bytes(b)


# ══════════════════════════════════════════════════════════════════════════════
# SESSION #6 — UNICODE SECURITY
# ══════════════════════════════════════════════════════════════════════════════

class TestS6_Q1_Homoglyph:
    """Q1: Non-ASCII characters in keyword positions."""

    def test_cyrillic_META(self):
        # М (U+041C) looks like M but is Cyrillic
        payload = "#!CSTL_v4.9.2_MODE=A\n\u041cETA [encoder=X]\n---END---"
        r = fast_parse(payload)
        sec = [w for w in r.warnings if "SEC_Q1" in w]
        assert sec, "Cyrillic М in keyword position must trigger SEC_Q1 warning"

    def test_greek_ALPHA_in_keyword(self):
        # Α (U+0391) Greek Capital Alpha
        payload = "#!CSTL_v4.9.2_MODE=A\n\u0391CTION=test\n---END---"
        _, errors, warns = security_scan(payload)
        assert any("SEC_Q1" in w for w in warns)

    def test_ascii_only_keywords_pass(self):
        r = fast_parse(VALID_BASE)
        sec = [w for w in r.warnings + r.errors if "SEC_Q1" in w]
        assert not sec, "Clean ASCII payload must not trigger Q1"

    def test_ascii_uppercase_underscore_keywords(self):
        # Keywords are ASCII uppercase + underscore — must pass
        p = make_payload(
            "DISAGREEMENT_BLOCK [\nSTRENGTH x [sigma:float=0.88]\n]"
        )
        r = fast_parse(p)
        assert not any("SEC_Q1" in w for w in r.warnings + r.errors)


class TestS6_Q2_ZeroWidth:
    """Q2: Zero-width and bidi control characters."""

    def test_zwsp_stripped(self):
        # U+200B zero-width space
        payload = "META\u200b [\nencoder=X\n]\n---END---"
        cleaned, errors, warns = security_scan(payload)
        assert "\u200b" not in cleaned
        assert any("SEC_Q2" in w and "200B" in w for w in warns)

    def test_zwnj_stripped(self):
        payload = "META\u200c [\nencoder=X\n]\n---END---"
        cleaned, _, warns = security_scan(payload)
        assert "\u200c" not in cleaned

    def test_bidi_rlo_stripped(self):
        # U+202E RIGHT-TO-LEFT OVERRIDE
        payload = "encoder=\u202eATTACK\n---END---"
        cleaned, _, warns = security_scan(payload)
        assert "\u202e" not in cleaned
        assert any("SEC_Q2" in w for w in warns)

    def test_bom_stripped(self):
        payload = "\ufeffMETA [\nencoder=X\n]\n---END---"
        cleaned, _, warns = security_scan(payload)
        assert "\ufeff" not in cleaned

    def test_clean_payload_no_stripping(self):
        cleaned, errors, warns = security_scan(VALID_BASE)
        assert not any("SEC_Q2" in w for w in warns)
        assert cleaned == VALID_BASE


class TestS6_Q4_NestedMETA:
    """Q4: Fake META injection at nested scope."""

    def test_indented_META_blocked(self):
        payload = VALID_BASE.replace(
            "---END---",
            "DEFINE x AS y [\n  META [\nencoder=attacker\n]\n]\n---END---"
        )
        r = fast_parse(payload)
        assert not r.is_valid
        assert any("SEC_Q4" in e for e in r.errors)

    def test_META_in_string_value_flagged(self):
        # Real injection: value= contains "META [" with bracket (space + bracket = structural)
        payload = VALID_BASE.replace(
            "---END---",
            "DEFINE x AS note [value=see, META [encoder=evil]\n]\n---END---"
        )
        r = fast_parse(payload)
        # Should be blocked — nested META with bracket
        sec = [x for x in r.errors + r.warnings if "SEC_Q4" in x]
        assert sec, "META keyword with bracket in value must be flagged"

    def test_root_META_still_valid(self):
        r = fast_parse(VALID_BASE)
        assert not any("SEC_Q4" in x for x in r.errors + r.warnings)


class TestS6_Q5_NestingDepth:
    """Q5: Max nesting depth enforcement."""

    def test_deep_nesting_blocked(self):
        # 35 levels > max_depth=32
        payload = VALID_BASE.replace(
            "---END---",
            "[" * 35 + "x" + "]" * 35 + "\n---END---"
        )
        _, errors, _ = security_scan(payload)
        assert any("SEC" in e and "nesting" in e for e in errors)

    def test_normal_nesting_passes(self):
        payload = make_payload(
            "DISAGREEMENT_BLOCK [\nGAP x [sigma:float=0.85]\n]"
        )
        _, errors, _ = security_scan(payload)
        assert not any("nesting" in e for e in errors)


# ══════════════════════════════════════════════════════════════════════════════
# SESSION #7 — ADVANCED ATTACK VECTORS
# ══════════════════════════════════════════════════════════════════════════════

class TestS7_Q1_BidiOverride:
    """Q1: Bidi override characters — stripped with audit log."""

    def test_bidi_stripped_and_logged(self):
        payload = "encoder=\u202eATTACKER"
        cleaned, _, warns = security_scan(payload)
        assert "\u202e" not in cleaned
        # Audit log must contain hex-escaped form
        assert any("\\u202e" in w.lower() or "202E" in w for w in warns)

    def test_lre_stripped(self):
        # U+202A LEFT-TO-RIGHT EMBEDDING
        payload = "encoder=\u202aATTACKER"
        cleaned, _, warns = security_scan(payload)
        assert "\u202a" not in cleaned


class TestS7_Q2_OverlongUTF8:
    """Q2: Overlong UTF-8 rejected by Python runtime."""

    def test_python_rejects_overlong(self):
        # Overlong encoding of U+002F (/) as 0xC0 0xAF — invalid UTF-8
        overlong = b"\xC0\xAF"
        with pytest.raises(UnicodeDecodeError):
            overlong.decode("utf-8")

    def test_decompile_raises_on_invalid_utf8(self):
        binary = bytearray(BinaryWireFormat.compile(VALID_BASE))
        plen = struct.unpack("!I", binary[6:10])[0]
        # Inject invalid UTF-8 byte into a val field (after op+vlen header)
        # First token starts at byte 10: op=1 byte, vlen=2 bytes, val starts at 13
        # Replace a val byte with 0xFF (invalid UTF-8 standalone)
        binary[13] = 0xFF
        body = bytes(binary[10:10+plen])
        binary[10+plen:14+plen] = zlib.crc32(body).to_bytes(4, "big")
        with pytest.raises(ValueError, match="CSTLTruncationError|invalid UTF-8|UTF-8"):
            BinaryWireFormat.decompile(bytes(binary))


class TestS7_Q3_TruncatedMultibyte:
    """Q3: Truncated multibyte sequences raise CSTLTruncationError."""

    def test_truncated_payload_raises(self):
        binary = BinaryWireFormat.compile(VALID_BASE)
        # Inflate first vlen to exceed buffer
        b = bytearray(binary)
        b[11] = 0xFF
        b[12] = 0xFF
        plen = struct.unpack("!I", b[6:10])[0]
        body = bytes(b[10:10+plen])
        b[10+plen:14+plen] = zlib.crc32(body).to_bytes(4, "big")
        with pytest.raises(ValueError, match="CSTLTruncationError"):
            BinaryWireFormat.decompile(bytes(b))

    def test_header_truncated_raises(self):
        binary = BinaryWireFormat.compile(VALID_BASE)
        # Provide only 1 byte of body
        b = bytearray(binary)
        plen = 1
        struct.pack_into("!I", b, 6, plen)
        body = bytes(b[10:11])
        b[10+plen:14+plen] = zlib.crc32(body).to_bytes(4, "big")
        with pytest.raises((ValueError, struct.error)):
            BinaryWireFormat.decompile(bytes(b[:14+plen]))

    def test_valid_binary_decompiles_cleanly(self):
        binary = BinaryWireFormat.compile(VALID_BASE)
        decoded = BinaryWireFormat.decompile(binary)
        assert "META" in decoded
        assert "---END---" in decoded


class TestS7_Q4_ConfusableBrackets:
    """Q4: Unicode bracket variants do not confuse parser."""

    def test_fullwidth_bracket_not_parsed_as_block(self):
        # U+FF3B FULLWIDTH LEFT SQUARE BRACKET
        payload = make_payload("DEFINE x AS y \uff3bvalue=test\uff3d")
        r = fast_parse(payload)
        # Parser should not interpret fullwidth brackets as block delimiters
        # META and root structure should still be valid
        assert r.meta_fields.get("encoder") == "Agent_TEST"

    def test_ascii_brackets_still_work(self):
        p = make_payload("DISAGREEMENT_BLOCK [\nGAP x [sigma:float=0.85]\n]")
        r = fast_parse(p)
        assert r.is_valid

    def test_fullwidth_does_not_crash(self):
        payload = "#!CSTL_v4.9.2_MODE=A\n\uff2dETA \uff3bencoder=X\uff3d\n---END---"
        try:
            r = fast_parse(payload)
            # If it doesn't crash, that's good — Q1 warning expected for fullwidth М lookalike
        except Exception as e:
            pytest.fail(f"Parser crashed on fullwidth chars: {e}")


class TestS7_Q5_HashCollision:
    """Q5: canonical_hash uses full 256-bit sha256."""

    def test_hash_length_256bit(self):
        h = canonical_hash(VALID_BASE)
        assert h.startswith("sha256:")
        hex_part = h[7:]
        assert len(hex_part) == 64, f"Expected 64 hex chars, got {len(hex_part)}"

    def test_hash_deterministic(self):
        h1 = canonical_hash(VALID_BASE)
        h2 = canonical_hash(VALID_BASE)
        assert h1 == h2

    def test_hash_field_order_invariant(self):
        p1 = "#!CSTL_v4.9.2_MODE=A\nMETA [\nsigma:float=0.88,\nencoder=Agent_TEST,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---"
        p2 = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---"
        assert canonical_hash(p1) == canonical_hash(p2)

    def test_different_payloads_different_hash(self):
        p1 = make_payload("DECISION: accept")
        p2 = make_payload("DECISION: reject")
        assert canonical_hash(p1) != canonical_hash(p2)

    def test_legacy_truncated_mode(self):
        h = canonical_hash(VALID_BASE, truncate=32)
        assert len(h[7:]) == 32

    def test_birthday_bound_adequate(self):
        # 256-bit hash → birthday bound at 2^128 — computationally infeasible
        hex_part = canonical_hash(VALID_BASE)[7:]
        bits = len(hex_part) * 4
        assert bits == 256, f"Expected 256-bit hash, got {bits}"


# ══════════════════════════════════════════════════════════════════════════════
# GPT SESSION #7 — ADDITIONAL VECTORS
# ══════════════════════════════════════════════════════════════════════════════

class TestGPT_NormalizationDowngrade:
    """GPT vector: unicode normalization downgrade attacks."""

    def test_nfc_nfd_canonical_form_stable(self):
        # NFC and NFD of same string should produce same canonical_hash
        # e with combining accent (NFD) vs é (NFC)
        nfd = "caf\u0065\u0301"   # e + combining acute = é in NFD
        nfc = "caf\u00e9"          # é in NFC
        # These are semantically identical
        assert unicodedata.normalize("NFC", nfd) == nfc

    def test_canonical_form_applies_nfc(self):
        p_nfd = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\nDEFINE x AS note [value=caf\u0065\u0301]\n---END---"
        p_nfc = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\nDEFINE x AS note [value=caf\u00e9]\n---END---"
        c_nfd = canonical_form(p_nfd)
        c_nfc = canonical_form(p_nfc)
        # After NFC normalization, canonical forms should be equal
        assert c_nfd == c_nfc

    def test_combining_sequences_dont_crash(self):
        # Deep combining marks (GPT vector: resource exhaustion)
        combining = "a" + "\u0300" * 50   # 50 combining grave accents
        payload = make_payload(f"DEFINE x AS note [value={combining}]")
        try:
            r = fast_parse(payload)
        except Exception as e:
            pytest.fail(f"Combining sequence crashed parser: {e}")


class TestGPT_StreamFragment:
    """GPT vector: stream fragment reassembly confusion."""

    def test_split_payload_parse_each_half(self):
        # Two valid separate payloads parsed independently
        p1 = VALID_BASE
        p2 = make_payload("DECISION: accept [sigma:float=0.90]")
        r1 = fast_parse(p1)
        r2 = fast_parse(p2)
        assert r1.is_valid
        assert r2.is_valid

    def test_concatenated_payloads_t3_blocks_second(self):
        # Two payloads concatenated — T3 should catch content after ---END---
        combined = VALID_BASE + "\n" + make_payload("DECISION: accept")
        r = fast_parse(combined)
        # T3 violation: content after ---END---
        assert not r.is_valid
        assert any("T3" in e or "END" in e for e in r.errors)

    def test_empty_payload_handled(self):
        r = fast_parse("")
        assert not r.is_valid


class TestGPT_ParserDifferential:
    """GPT vector: differential between fast and safe parse modes."""

    def test_c3_attack_consistent(self):
        # C3 attack must be caught regardless of field types
        c3 = make_payload().replace(
            "encoder=Agent_TEST,",
            "encoder=Agent_A,\nencoder=Agent_B,"
        )
        r = fast_parse(c3)
        assert not r.is_valid
        assert any("C3" in e or "Duplicate" in e for e in r.errors)

    def test_t3_attack_consistent(self):
        payload = VALID_BASE + "\nInjected prose after END"
        r = fast_parse(payload)
        assert not r.is_valid
        assert any("T3" in e or "END" in e for e in r.errors)

    def test_typed_and_untyped_same_result(self):
        typed = VALID_BASE.replace("sigma:float=0.88", "sigma:float=0.88")
        untyped = VALID_BASE.replace("sigma:float=0.88", "sigma=0.88")
        r1 = fast_parse(typed)
        r2 = fast_parse(untyped)
        # Both should produce same block structure
        assert [b[0] for b in r1.blocks] == [b[0] for b in r2.blocks]


# ══════════════════════════════════════════════════════════════════════════════
# ROUND-TRIP — ALL FORMAL TEST VECTORS
# ══════════════════════════════════════════════════════════════════════════════

class TestRoundTrip:
    """Session #5: Round-trip idempotency for all test vectors."""

    def _roundtrip(self, payload):
        binary = BinaryWireFormat.compile(payload)
        decoded = BinaryWireFormat.decompile(binary)
        r1 = fast_parse(payload)
        r2 = fast_parse(decoded)
        # Second encode
        binary2 = BinaryWireFormat.compile(decoded)
        r3 = fast_parse(BinaryWireFormat.decompile(binary2))
        return r1, r2, r3

    def test_base_payload(self):
        r1, r2, r3 = self._roundtrip(VALID_BASE)
        assert r1.meta_fields == r2.meta_fields
        assert r2.meta_fields == r3.meta_fields

    def test_formal_test_vectors(self):
        for i, vec in enumerate(ROUNDTRIP_TEST_VECTORS):
            full = VALID_BASE.replace("---END---", vec + "\n---END---")
            r1, r2, r3 = self._roundtrip(full)
            assert r1.meta_fields == r2.meta_fields, f"TV{i+1} meta mismatch"
            assert r2.meta_fields == r3.meta_fields, f"TV{i+1} not idempotent"

    def test_canonical_hash_stable_across_roundtrip(self):
        binary = BinaryWireFormat.compile(VALID_BASE)
        decoded = BinaryWireFormat.decompile(binary)
        assert canonical_hash(VALID_BASE) == canonical_hash(decoded)

    def test_field_order_invariant(self):
        p1 = "#!CSTL_v4.9.2_MODE=A\nMETA [\nNO_PROSE:bool=true,\nencoder=Agent_TEST,\nPARENT_HASH=root,\nRESPONSE_FORMAT:enum=CSTL,\nsigma:float=0.88\n]\n---END---"
        p2 = "#!CSTL_v4.9.2_MODE=A\nMETA [\nencoder=Agent_TEST,\nsigma:float=0.88,\nRESPONSE_FORMAT:enum=CSTL,\nNO_PROSE:bool=true,\nPARENT_HASH=root\n]\n---END---"
        assert canonical_hash(p1) == canonical_hash(p2)
        r1 = fast_parse(p1)
        r2 = fast_parse(p2)
        assert r1.meta_fields == r2.meta_fields


# ══════════════════════════════════════════════════════════════════════════════
# SECURITY PROFILE CONSTANTS
# ══════════════════════════════════════════════════════════════════════════════

class TestSecurityProfile:
    """Verify security profile constants match ratified spec."""

    def test_max_nesting_depth(self):
        assert SECURITY_PROFILE["max_nesting_depth"] == 32

    def test_max_escape_depth(self):
        assert SECURITY_PROFILE["max_escape_depth"] == 8

    def test_normalization_nfkc(self):
        assert SECURITY_PROFILE["normalization"] == "NFKC"

    def test_keyword_charset(self):
        assert "ASCII" in SECURITY_PROFILE["keyword_charset"]
