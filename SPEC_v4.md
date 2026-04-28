# CSTL v4.0 — Formal Specification

**Author**: Olivier Goyette
**Version**: v4.0
**Date**: April 2026
**Status**: Stable

---

## 1. Overview

CSTL (Compressed Semantic Transfer Language) is a textual semantic protocol for lossless LLM-to-LLM communication. This document specifies the v4.0 syntax, semantics, and parsing rules.

CSTL v4.0 introduces 4 structural additions over v3.0.9:

1. `INTENT_PAYLOAD` — global meta-intent of the message
2. `META` — encoder metadata and confidence
3. `CONSTRAINTS` block — deontic obligations placed before relations
4. `UNCERTAINTY` block — explicit epistemic markers

---

## 2. Document structure

A CSTL v4.0 document follows this canonical block order:

```
#!CSTL v4.0
LANG:[language code]
DOMAIN:[domain identifier]
SESSION:[session identifier]

[SYMBOLS_DECLARATION]   (optional, informational)

INTENT_PAYLOAD: [reason] [
  priority=[critical|high|normal|low],
  sender=[encoder identity],
  receiver=[decoder identity],
  purpose=[message objective],
  context=[global context]
]

META:
  PAYLOAD_CONFIDENCE: [0.0-1.0]
  ENCODED_BY: [model identifier]
  ENCODING_TIMESTAMP: [ISO 8601]
  VERSION: v4.0

CONSTRAINTS:
  [modality] [entity] [operator] [target] [attributes]
  ...

UNCERTAINTY:
  [element] UNKNOWN [source=reason]
  [element] ESTIMATED [σ=value]
  [element] INFERRED [σ=value]

DEFINE [Entity] AS [type] [
  id=[unique_id],
  layer=[bedrock|deep|shallow|surface],
  [attributes]
]

RELATIONS:
  [entity] [operator] [target] [attributes]
  ...

---END---
```

---

## 3. Compact symbol notation

CSTL v4.0 supports two notation modes:

### Verbose mode
```
[strength=0.92, layer=bedrock, time=present, weight=positive, id=r001]
```

### Compact mode (preferred for LLM-to-LLM)
```
[σ=0.92, δ=b, τ=n, ω=+, ι=r001]
```

| Symbol | Meaning | Values |
|--------|---------|--------|
| `σ` | strength | 0.0 — 1.0 |
| `δ` | layer | b=bedrock, d=deep, s=shallow, su=surface |
| `τ` | time | p=past, n=present, f=future |
| `ω` | weight | +=positive, −=negative, °=neutral |
| `ι` | id | unique identifier (eXXX, rXXX, cXXX) |

**Symbol stability**: `σδτωι` are FIXED. `δ=su` (not `f`) for surface to avoid collision with `τ=f` (future).

---

## 4. Modalities

CSTL v4.0 supports five deontic modalities as first-class syntax:

| Modality | Compact | Semantics | Example |
|----------|---------|-----------|---------|
| `[MUST]` | `[!]` | Absolute obligation | `[!] Sofia OBTAIN article12` |
| `[NOT]` | `[¬]` | Absolute prohibition | `[¬] James DIVULGE secret` |
| `[MAY]` | `[?]` | Permission | `[?] Yaw PROPOSE compromise` |
| `[IF] X [MUST] Y` | — | Conditional obligation | `[IF] delay [MUST] escalate` |
| `[IF] X [NOT] Y` | — | Conditional prohibition | `[IF] absent [NOT] sign` |

---

## 5. Operators (21 official)

### Causality
- `ARR` — general causal
- `ARR.CREATE` — creation
- `ARR.JOIN` — junction
- `ARR.PRODUCE` — emission/production
- `ARR.ACCESS` — reception/access

### Intentionality
- `INTENT` — intention, will
- `MAINTAIN` — state preservation
- `TRANSFORM` — state transformation
- `RESIST` — opposition

### Dynamic
- `AMP` — amplification
- `INH` — inhibition
- `PRESSURE` — pressure
- `CATALYZE` — catalysis

### Relational
- `MUTUAL` — reciprocity
- `TRANSMIT_FAITHFUL` — faithful transmission
- `TRANSMIT_INFER` — inferential transmission

### Speech acts
- `COMMAND` — order
- `ASK` — question
- `STATE` — assertion
- `PERFORM` — performative act
- `RECOMMEND` — recommendation

### Domain operators
Each of the 18 supported domains adds its own operators (see `cstl_domains.py`). These are validated against domain ontology and produce warnings if used outside their domain.

---

## 6. Attributes

Maximum 9 attributes per relation (k=9 rule). Available attributes:

| Attribute | Compact | Values | Use |
|-----------|---------|--------|-----|
| `strength` | `σ=` | 0.0—1.0 | Force of relation |
| `layer` | `δ=` | b/d/s/su | Semantic depth |
| `time` | `τ=` | p/n/f | Temporality |
| `date` | — | YYYY-MM-DD | Exact date |
| `deadline` | — | ISO 8601 | Due date |
| `weight` | `ω=` | +/−/° | Polarity |
| `id` | `ι=` | eXXX/rXXX/cXXX | Unique identifier |
| `trust` | — | 0.0—1.0 | Trust level |
| `status` | — | free | State of element |

---

## 7. UNCERTAINTY block

Three states distinguish epistemic markers:

```
UNCERTAINTY:
  totally_unknown_element UNKNOWN
  source_known_but_unknown UNKNOWN [source=confidential]
  approximate_value ESTIMATED [σ=0.X, value=...]
  encoder_inferred INFERRED [σ=0.X]
```

- `UNKNOWN` — information absent
- `ESTIMATED` — approximate value with confidence
- `INFERRED` — deduced by encoder, not explicitly stated

---

## 8. Architecture phases

CSTL supports three deployment phases:

| Phase | Description | Compression | Use case |
|-------|-------------|-------------|----------|
| Phase 1 | Self-contained (full header+trailer) | Reference | One-shot, no prerequisites |
| Phase 2a | Signature (#!CSTL) + DEFINE + relations | +20% vs natural language | Regular sessions |
| Phase 2c | Signature + relations (DEFINE in context) | +47% vs natural language | Long sessions, shared ontology |

The current repository implements all three phases. CASTLE mode (planned) introduces Phase 3 with shared dictionary indexing.

---

## 9. Validation rules

| Rule | Description | Severity |
|------|-------------|----------|
| R1 | IDs unique within a SESSION | Fatal error if duplicate |
| R2 | Version must be declared | Warning |
| R3 | strength clamped to [0.0, 1.0] | Warning + auto-clamp |
| R4 | Canonical block order | Recommended |
| R5 | Operators must be in 21 official + domain set | Warning |
| R6 | Symbols σδτωι are immutable | FIXED |
| R7 | Unknown operator = warning, duplicate ID = fatal | — |

---

## 10. EBNF grammar (simplified)

```bnf
document     ::= header symbols_decl? intent_payload? meta?
                 constraints? uncertainty? define* relations trailer

header       ::= "#!CSTL v4.0" NEWLINE lang domain session?

relation     ::= modality? entity operator entity "[" attr_list "]"
               | entity modality operator entity "[" attr_list "]"

modality     ::= "[!]"|"[MUST]" | "[¬]"|"[NOT]" | "[?]"|"[MAY]"

operator     ::= ARR | ARR.CREATE | ARR.JOIN | ARR.PRODUCE | ARR.ACCESS
               | INTENT | MAINTAIN | TRANSFORM | RESIST
               | AMP | INH | PRESSURE | CATALYZE
               | MUTUAL | TRANSMIT_FAITHFUL | TRANSMIT_INFER
               | COMMAND | ASK | STATE | PERFORM | RECOMMEND
               | <domain_operator>

attr_list    ::= attribute ("," attribute)*   (* max 9 *)

strength_attr ::= ("σ="|"strength=") float    (* 0.0—1.0 *)
layer_attr    ::= ("δ="|"layer=") layer_val
time_attr     ::= ("τ="|"time=") time_val
weight_attr   ::= ("ω="|"weight=") weight_val
id_attr       ::= ("ι="|"id=") id_val

layer_val    ::= "b"|"bedrock" | "d"|"deep" | "s"|"shallow" | "su"|"surface"
time_val     ::= "p"|"past" | "n"|"present" | "f"|"future"
weight_val   ::= "+"|"positive" | "−"|"negative" | "°"|"neutral"
```

---

## 11. Reference implementation

The Python reference parser (`cstl_parser.py`) implements:

- Full v4.0 syntax parsing
- Compact and verbose attribute notation
- Validation per rules R1-R7
- Re-encoding with format choice (compact/verbose)
- Domain operator validation via `cstl_domains.py`
- Public API: `parse(text)`, `encode(doc)`, `validate(text)`

```python
from cstl_parser import parse, encode, validate

doc = parse(payload_text)
report = validate(payload_text)
text = encode(doc, compact=True)
```

---

## 12. Backward and forward compatibility

- **Backward compatible**: A v3.0.9 decoder will gracefully ignore v4.0-only blocks (`INTENT_PAYLOAD`, `META`, `CONSTRAINTS`, `UNCERTAINTY`). The `DEFINE` and `RELATIONS` blocks remain identical.

- **Forward compatible**: A v4.0 decoder can read v3.0.9 payloads. Missing v4.0 blocks are treated as absent.

---

## 13. CASTLE mode (future work)

CSTL v4.0 documents the standalone mode. CASTLE — the network-shared dictionary mode — is reserved for future work. CASTLE will add:

- A shared dictionary block sent once per session
- Indexed references in subsequent messages (e.g., `e1 op1 d1` instead of full names)
- Estimated ~53% per-message compression after dictionary establishment

The CASTLE specification is not yet finalized and is not part of this v4.0 document.

---

## 14. Citation

```bibtex
@misc{goyette2026cstl,
  author       = {Goyette, Olivier},
  title        = {CSTL v4.0: A Compressed Semantic Transfer Language for Lossless LLM-to-LLM Communication},
  year         = {2026},
  howpublished = {\url{https://github.com/oliviergoyette/cstl}},
  note         = {Specification v4.0}
}
```

---

*End of specification.*
