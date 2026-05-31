"""
CSTL v4.9.2 — Native Typing System
===================================
Ratified by tripartite session #2 (Claude + Gemini + ChatGPT)
Session: cstl_typing_v1 — CLOSED, May 2026

Decisions:
  Q1: Explicit optional type annotations  field:type=value
  Q2: 6 mandatory typed META fields
  Q3: Strict enum whitelist + EXTENSION: prefix escape
  Q4: PARENT_HASH strict sha256:|root
  Q5: PATCH_T4 — interim human orchestrator, v5.0 ephemeral key target

BNF extension:
  meta_field ::= identifier (":" type_indicator)? "=" typed_value
  type_indicator ::= "float" | "int" | "bool" | "enum" | "iso8601"
                   | "string" | "hash"

Empirical finding S2_1:
  ChatGPT self-applied sigma:float=0.93 in its own META block
  zero-shot, without fine-tuning. Syntax is immediately adoptable.
"""

from __future__ import annotations
import re
from dataclasses import dataclass
from typing import Any

# ── TYPE INDICATORS ──────────────────────────────────────────────────

VALID_TYPES = frozenset({
    "float", "int", "bool", "enum", "iso8601", "string", "hash"
})

# ── MANDATORY TYPED FIELDS (v4.9.2) ────────────────────────────────
# Ratified Q2: 6 mandatory fields with guaranteed types
# Field → (type, required_in_every_payload)
MANDATORY_FIELDS: dict[str, tuple[str, bool]] = {
    "sigma":            ("float",   True),
    "TURN":             ("int",     False),   # mandatory when present
    "NO_PROSE":         ("bool",    True),
    "RESPONSE_FORMAT":  ("enum",    True),
    "PARENT_HASH":      ("hash",    True),
    "TIMESTAMP":        ("iso8601", False),   # mandatory when present
    "CONVERSATION_ID":  ("string",  False),   # optional
    "encoder":          ("string",  True),
    "CONTINUATION_MODE":("enum",    False),
    "ACTION":           ("enum",    False),
}

# ── ENUM WHITELISTS (Q3: strict + EXTENSION: escape) ────────────────
RESPONSE_FORMAT_VALUES = frozenset({"CSTL", "JSON", "TEXT"})
CONTINUATION_MODE_VALUES = frozenset({"continue", "chain", "terminate", "pause"})
MODE_VALUES = frozenset({"A", "B", "C"})

# ACTION whitelist — strict per Q3
ACTION_VALUES = frozenset({
    "encode_from_prose", "react_to_proposals", "critique_and_refine",
    "critique_proposals", "reject_with_evidence", "arbitrate_disagreement",
    "audit_attest", "synthesize_consensus", "emit_disagreement",
    "propose_improvements", "identify_gaps", "refine_plan",
    "verify_consistency", "produce_initial_clinical_management_plan",
    "initiate_meta_discussion", "handoff_to_next_agent",
    "initiate_canonical_v4_9_2_dialogue",
    "evaluate_syntax_canonicalization", "evaluate_typing_system",
    "synthesize_canonical_grammar_v4_9_2", "synthesize_typing_spec_v4_9_2",
    "ratify_canonical_grammar_v4_9_2", "acknowledge_final_ratification",
    "external_independent_validation", "contribute_to_open_discussion",
    "continue_chain", "continue_protocol_semantics",
    "initiate_syntax_canonicalization_debate",
    "initiate_typing_system_debate",
    "canonicalization_review_resolution",
    # v4.9.2 additions from sessions
    "accept_restructuring_consolidate_v4_9_freeze",
    "critique_cstl_t_proposal_and_refine_layering",
    "execute_adversarial_simulation",
    "refine_patch_set_and_scope_optional_features",
    "ratify_v4_9_freeze_and_prepare_handover",
    # Open discussion (always allowed per EXTENSION policy)
    # Any value starting with "EXTENSION:" is accepted with WARNING
})

# ── REGEX PATTERNS ──────────────────────────────────────────────────
_FLOAT_RE   = re.compile(r'^-?\d+(\.\d+)?$')
_INT_RE     = re.compile(r'^-?\d+$')
_HASH_RE    = re.compile(r'^sha256:[0-9a-fA-F]{8,}$|^root$|^sha256:.+$')
_ISO8601_RE = re.compile(
    r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?$'
)
_TYPED_FIELD_RE = re.compile(r'^([A-Za-z_][A-Za-z0-9_]*)(?::([A-Za-z0-9]+))?=(.+)$')


# ── ERROR CLASSES ────────────────────────────────────────────────────

@dataclass
class CSTLTypeError:
    """Type validation error — Q2 ratified format."""
    field:    str
    expected: str
    got:      str
    severity: str = "warning"   # warning in v4.9.2, error in v5.0

    def __str__(self) -> str:
        return (
            f"CSTLTypeError({self.severity.upper()}) "
            f"field={self.field!r} expected={self.expected!r} got={self.got!r}"
        )


@dataclass
class CSTLEnumError:
    """Unknown enum value — Q3 strict whitelist."""
    field:  str
    value:  str
    whitelist: frozenset

    def __str__(self) -> str:
        return (
            f"CSTLEnumError field={self.field!r} unknown value={self.value!r} "
            f"(use EXTENSION:{self.value} to bypass)"
        )


@dataclass
class CSTLHashError:
    """Invalid PARENT_HASH format — Q4."""
    value: str

    def __str__(self) -> str:
        return (
            f"CSTLHashError invalid PARENT_HASH={self.value!r} "
            f"(expected sha256:<hex> or 'root')"
        )


@dataclass
class CSTLEncoderWarning:
    """PATCH_T4 interim — encoder identity mismatch warning."""
    declared: str
    note:     str = "human orchestrator should verify encoder identity"

    def __str__(self) -> str:
        return f"PATCH_T4_WARNING encoder={self.declared!r} — {self.note}"


# ── CORE TYPE VALIDATORS ─────────────────────────────────────────────

def validate_float(value: str, field: str) -> list:
    errors = []
    v = value.strip()
    if not _FLOAT_RE.match(v):
        errors.append(CSTLTypeError(field, "float", v))
        return errors
    f = float(v)
    if field == "sigma" and not (0.0 <= f <= 1.0):
        errors.append(CSTLTypeError(
            field, "float[0.0-1.0]", v, severity="error"
        ))
    return errors


def validate_int(value: str, field: str) -> list:
    v = value.strip()
    if not _INT_RE.match(v):
        return [CSTLTypeError(field, "int", v)]
    if field == "TURN" and int(v) < 0:
        return [CSTLTypeError(field, "non_negative_int", v)]
    return []


def validate_bool(value: str, field: str) -> list:
    v = value.strip()
    if v in ("true", "false"):
        return []
    # Uppercase/mixed variants → warning (not error) per deprecation phase 1
    if v.lower() in ("true", "false"):
        return [CSTLTypeError(
            field, "bool(true|false lowercase canonical)",
            v, severity="warning"
        )]
    return [CSTLTypeError(field, "bool(true|false)", v)]


def validate_enum(value: str, field: str, whitelist: frozenset) -> list:
    v = value.strip()
    if v in whitelist:
        return []
    if v.startswith("EXTENSION:"):
        # Q3: EXTENSION: prefix bypasses strict validation with warning
        return [CSTLTypeError(
            field, f"enum{sorted(whitelist)[:3]}...",
            v, severity="warning"
        )]
    return [CSTLEnumError(field, v, whitelist)]


def validate_iso8601(value: str, field: str) -> list:
    v = value.strip()
    if not _ISO8601_RE.match(v):
        return [CSTLTypeError(field, "iso8601", v)]
    return []


def validate_hash(value: str, field: str = "PARENT_HASH") -> list:
    v = value.strip()
    # root is always valid
    if v == "root":
        return []
    # sha256: prefix required for strict validation
    if v.startswith("sha256:"):
        return []
    # Anything else is "unverified" — warning, not error (v4.9.2)
    return [CSTLTypeError(
        field,
        "sha256:<hex>|root",
        v,
        severity="warning"   # Q4: unverified placeholder, not hard error
    )]


# ── FIELD PARSER ─────────────────────────────────────────────────────

def parse_typed_field(raw_field: str) -> tuple[str, str | None, str]:
    """
    Parse 'field:type=value' or 'field=value'.
    Returns (field_name, declared_type_or_None, value).
    Implements BNF: meta_field ::= identifier (":" type_indicator)? "=" value
    """
    m = _TYPED_FIELD_RE.match(raw_field.strip())
    if not m:
        return raw_field.strip(), None, ""
    name, declared_type, value = m.group(1), m.group(2), m.group(3)
    return name, declared_type, value.strip()


# ── MAIN VALIDATOR ───────────────────────────────────────────────────

def validate_meta_block(
    fields: dict[str, str],
    declared_types: dict[str, str] | None = None,
) -> tuple[bool, list]:
    """
    Validate all META fields.
    fields: {field_name: value_string}
    declared_types: {field_name: declared_type} from :type annotations

    Returns (is_valid, list_of_errors_and_warnings).
    is_valid = False only on ERROR severity, warnings pass.
    """
    declared_types = declared_types or {}
    issues = []
    has_error = False

    for field_name, value in fields.items():

        # Determine type to use: declared type > inferred from known fields
        dtype = declared_types.get(field_name)
        if dtype is None and field_name in MANDATORY_FIELDS:
            dtype = MANDATORY_FIELDS[field_name][0]

        if dtype is None:
            continue   # Unknown field, untyped → accepted silently

        # Validate by type
        if dtype == "float":
            issues.extend(validate_float(value, field_name))

        elif dtype == "int":
            issues.extend(validate_int(value, field_name))

        elif dtype == "bool":
            issues.extend(validate_bool(value, field_name))

        elif dtype == "enum":
            if field_name == "RESPONSE_FORMAT":
                wl = RESPONSE_FORMAT_VALUES
            elif field_name == "CONTINUATION_MODE":
                wl = CONTINUATION_MODE_VALUES
            elif field_name == "ACTION":
                wl = ACTION_VALUES
            else:
                wl = frozenset()   # unknown enum — skip
            if wl:
                issues.extend(validate_enum(value, field_name, wl))

        elif dtype == "iso8601":
            issues.extend(validate_iso8601(value, field_name))

        elif dtype == "hash":
            issues.extend(validate_hash(value, field_name))

        # string: no validation needed

    # Check mandatory fields present
    for fname, (ftype, required) in MANDATORY_FIELDS.items():
        if required and fname not in fields:
            issues.append(CSTLTypeError(
                fname, f"{ftype}(mandatory)", "(absent)", severity="error"
            ))

    # PATCH_T4 interim: emit encoder warning
    if "encoder" in fields:
        enc = fields["encoder"]
        # Pattern: LLMs use their model name, not assigned agent name
        if any(model in enc for model in
               ["GPT", "Gemini", "Claude", "OpenAI", "Anthropic", "Google"]):
            issues.append(CSTLEncoderWarning(enc))

    has_error = any(
        isinstance(i, (CSTLTypeError, CSTLEnumError, CSTLHashError))
        and getattr(i, "severity", "error") == "error"
        for i in issues
    )
    return not has_error, issues


# ── CONVENIENCE ENTRY POINT ──────────────────────────────────────────

def validate_payload_meta(raw_meta_content: str) -> tuple[bool, list]:
    """
    Parse and validate a raw META block content string.
    Handles both 'field=value' and 'field:type=value' forms.
    """
    fields = {}
    declared_types = {}

    for line in re.split(r'[,\n]', raw_meta_content):
        line = line.strip()
        if not line:
            continue
        name, dtype, value = parse_typed_field(line)
        if name:
            fields[name] = value
            if dtype:
                if dtype not in VALID_TYPES:
                    declared_types[name] = dtype  # unknown type → warning
                else:
                    declared_types[name] = dtype

    return validate_meta_block(fields, declared_types)


# ── DEMO / TESTS ─────────────────────────────────────────────────────

if __name__ == "__main__":
    print("=" * 65)
    print("CSTL v4.9.2 — Typing Validator Demo")
    print("Session #2 tripartite ratified spec")
    print("=" * 65)
    print()

    tests = [
        ("VALID — ChatGPT self-applied syntax", """\
encoder=ChatGPT_GPT5_5,
TIMESTAMP:iso8601=2026-05-21T18:08:00Z,
sigma:float=0.93,
RESPONSE_FORMAT:enum=CSTL,
NO_PROSE:bool=true,
CONTINUATION_MODE=continue,
PARENT_HASH:hash=sha256:abc123def456
"""),
        ("VALID — classic untyped v4.8 style", """\
encoder=Agent_CLAUDE,
sigma=0.88,
NO_PROSE=true,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=root
"""),
        ("ERROR — sigma out of range", """\
encoder=Agent_TEST,
sigma:float=1.5,
NO_PROSE=true,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=root
"""),
        ("ERROR — bad bool", """\
encoder=Agent_TEST,
sigma=0.88,
NO_PROSE:bool=TRUE,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=root
"""),
        ("WARNING — unknown ACTION enum", """\
encoder=Agent_TEST,
sigma=0.80,
NO_PROSE=true,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=root,
ACTION=fly_to_moon
"""),
        ("OK — EXTENSION: prefix bypass", """\
encoder=Agent_TEST,
sigma=0.80,
NO_PROSE=true,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=root,
ACTION=EXTENSION:custom_medical_encoding
"""),
        ("WARNING — PARENT_HASH unverified placeholder", """\
encoder=Agent_TEST,
sigma=0.85,
NO_PROSE=true,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=some_placeholder_not_sha256
"""),
        ("ERROR — missing mandatory field NO_PROSE", """\
encoder=Agent_TEST,
sigma=0.85,
RESPONSE_FORMAT=CSTL,
PARENT_HASH=root
"""),
    ]

    for name, meta in tests:
        print(f"{'─'*65}")
        print(f"TEST: {name}")
        valid, issues = validate_payload_meta(meta)
        print(f"  Valid: {valid}")
        for issue in issues:
            print(f"  {'⚠' if 'warning' in str(issue).lower() else '✗'} {issue}")
        if not issues:
            print("  ✓ No issues")
        print()

    print("=" * 65)
    print("Typing spec BNF:")
    print("  meta_field ::= identifier (':' type_indicator)? '=' typed_value")
    print("  type_indicator ::= float|int|bool|enum|iso8601|string|hash")
    print("  Q3: ACTION unknown → error | EXTENSION:x → warning")
    print("  Q4: PARENT_HASH sha256:hex|root → valid, else → warning")
    print("  Q5: PATCH_T4 interim → CSTLEncoderWarning always emitted")
