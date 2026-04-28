# CSTL — Compressed Semantic Transfer Language

**A semantic transfer protocol for lossless LLM-to-LLM communication.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Spec](https://img.shields.io/badge/spec-v4.0-green)]()
[![Status](https://img.shields.io/badge/status-preprint%20in%20preparation-orange)]()
[![arXiv](https://img.shields.io/badge/arXiv-coming%20soon-red)]()

**Author**: Olivier Goyette · **Specification**: v4.0 (April 2026)

---

## What is CSTL?

CSTL is a semantic transfer protocol designed for the problem of **lossless inter-LLM communication** — preserving propositional content, deontic modalities, numerical force, temporality, uncertainty, and multi-agent trust state across agent hops.

Unlike RDF (static facts), AMR (single sentences), JSON-LD (web metadata), or G²CP (graph-grounded operations), CSTL is a **textual semantic format** designed natively for LLM-to-LLM exchange:

- **Self-sufficient payloads** readable by any frontier LLM in zero-shot
- **Native deontic modalities** (`[MUST]`, `[NOT]`, `[IF]`) as first-class syntax
- **Explicit uncertainty markers** (`UNKNOWN`, `ESTIMATED`, `INFERRED`)
- **No infrastructure required** — plain UTF-8 text, archivable as legal documents
- **AI Act compliance native** — aligned with Articles 12, 13, 14 (record-keeping, transparency, human oversight)

---

## Two modes, one grammar

CSTL operates in two complementary modes that share a unified underlying grammar:

### 🚶 CSTL — Standalone mode (default)

Network-independent. Each payload is **self-sufficient and contains its full semantic content**. Readable in isolation by any frontier LLM. Archivable for AI Act compliance.

**Status**: v4.0 stable. Empirically validated on 9 distinct test protocols (212/214 tests passed). This repository implements the standalone mode.

### 🏰 CASTLE — Networked mode (planned)

Network-shared dictionary mode. Achieves **~53% per-message compression** after dictionary establishment. Optimized for sustained multi-agent sessions where infrastructure coordination is acceptable.

**Status**: Conceptualized. Reserved for future work.

> **The naming is structural**: CSTL is contained within CASTLE. The two added letters represent the two additional components of the networked mode — **A** for **A**gent network coordination, **E** for dictionary **E**ncoding. The core CSTL grammar (C-S-T-L) is preserved unchanged at the center.
>
> ```
> CSTL  →  C [A] S T L [E]  →  CASTLE
>            ↑           ↑
>          Agent     Encoding
> ```

---

## Quick example

```cstl
#!CSTL v4.0
LANG:en
DOMAIN:financial_credit_scoring
SESSION:credit_eval_audit_2026

INTENT_PAYLOAD: regulatory_audit_context [
  priority=high,
  sender=NovaTech,
  receiver=ACPR_auditor
]

META:
  PAYLOAD_CONFIDENCE: 0.94
  ENCODED_BY: claude-sonnet-4
  ENCODING_TIMESTAMP: 2026-04-28T14:00:00

CONSTRAINTS:
  [MUST] NovaTech DELIVER technical_documentation [σ=0.92, deadline=2026-07-15, id=c1]
  [MUST] FidBank MAINTAIN audit_trail [σ=0.95, ref=Article_12, id=c2]
  [NOT] CreditEval-1.7 PERFORM auto_decision_above_50000_EUR [σ=1.0, id=c3]
  [IF] borrower_contests [MUST] FidBank PROVIDE explanation [σ=0.93, response=P7D, id=c4]

UNCERTAINTY:
  false_positive_rate ESTIMATED [σ=0.75, value=0.062]
  sociodemographic_bias UNKNOWN
  rgpd_compliance INFERRED [σ=0.70]

DEFINE NovaTech AS agent [id=e1, role=editor]
DEFINE FidBank AS agent [id=e2, role=deployer]
DEFINE CreditEval-1.7 AS system [id=s1, classification=high_risk]
DEFINE ACPR AS agent [id=e3, role=regulator]

RELATIONS:
  NovaTech ARR.CREATE CreditEval-1.7 [σ=1.0, τ=p, id=r1]
  FidBank DEPLOY CreditEval-1.7 [σ=0.98, τ=p, occurred_on=2026-03-14, id=r2]
  ACPR MONITOR FidBank [σ=0.93, τ=n, id=r3]

---END---
```

A juriste, an LLM, or a parser can read this directly. No infrastructure required.

The full payload is available as [`payload_novatech.cstl`](./payload_novatech.cstl) in this repository.

---

## Why CSTL?

### The problem

Multi-agent LLM systems suffer from **semantic drift**: when agent A communicates with agent B in free-text, ~30-40% of structural information (modalities, force, temporality) is lost or distorted. After 3-4 hops, the original content is unrecognizable.

JSON Schema preserves data types but loses deontic semantics. Function Calling encodes parameters but not modalities. Graph protocols (G²CP) require shared infrastructure. Free-text loses everything.

### The CSTL approach

CSTL provides **structural priming** of LLM payloads:

| Information type | Free-text | JSON | Function Calling | CSTL |
|---|---|---|---|---|
| Numerical values | Lossy | ✅ | ✅ | ✅ |
| Deontic modalities | Lossy | Convention | Convention | **Native syntax** |
| Temporality | Lossy | Convention | Convention | **Native attribute** |
| Force/strength | Lossy ("strongly"...) | ✅ | ✅ | **Native attribute** |
| Uncertainty | Lossy | Custom | Custom | **Native block** |
| Human auditability | Variable | Nested | Deeply nested | **Native** |

---

## Validation

CSTL v4.0 has been empirically validated on **9 distinct test protocols**:

| Protocol | Score |
|---|---|
| QA Accuracy zero-shot (Claude) | 24/24 |
| Cross-LLM convergence (Claude + Gemini + ChatGPT decoding) | 72/72 |
| Pipeline 3-hops with enrichment | 12/12 |
| Stress tests (adversarial + length + ambiguity) | 9/9 |
| Chaos testing (corruption + multi-format + conflicts) | 4/4 |
| Integrated test 6 dimensions | 13/13 |
| Inverse encoding Gemini→Claude | 20/20 |
| Inverse encoding ChatGPT→Gemini (engineered prompt) | 20/20 |
| JSON Schema baseline comparison | 5/5 dimensions |
| **Total** | **212/214 = 99.1%** |

Statistical validation (bootstrap CI 95%, permutation tests, sign tests) is documented in [`experiments/`](./experiments/) with random seed 42 for reproducibility.

---

## Related work

CSTL is positioned within a broader landscape of LLM communication protocols:

| Protocol | Category | Layer | Relationship to CSTL |
|---|---|---|---|
| **MCP** (Anthropic, 2024) | LLM ↔ Tools | Transport | Complementary (different layer) |
| **A2A** (Google, 2025) | Agent ↔ Agent | Transport | Complementary (different layer) |
| **G²CP** (Ben Khaled & Monticolo, AAMAS 2026) | Multi-agent | Graph operations | Different category (graph-grounded vs textual) |
| **Function Calling** (OpenAI, 2023) | LLM tool invocation | JSON Schema subset | Subset (no native modalities) |
| **AMR** (Banarescu, 2013) | Semantic representation | Sentence-level graph | Different scope (sentence vs document) |
| **TypeChat** (Microsoft, 2023) | Structured output | TypeScript types | Different scope (no semantic layer) |
| **DSPy** (Stanford, 2024) | Programming framework | Module orchestration | Different category (framework vs format) |

CSTL occupies a specific niche: **textual, self-sufficient, AI Act-compliant, zero-infrastructure** semantic protocol for LLM-to-LLM communication.

Detailed comparative analyses available in this repository:
- [`CSTL_vs_JSON.md`](./CSTL_vs_JSON.md) — rigorous 5-dimension comparison
- [`CSTL_vs_G2CP.md`](./CSTL_vs_G2CP.md) — deep analysis of direct competitor
- [`CSTL_literature_review_S1.md`](./CSTL_literature_review_S1.md) — review of 20 related works

---

## Repository contents

### Core specification and code
- [`README.md`](./README.md) — This file
- [`SPEC_v4.md`](./SPEC_v4.md) — Formal specification v4.0
- [`CHANGELOG.md`](./CHANGELOG.md) — Version history
- [`CITATION.cff`](./CITATION.cff) — Academic citation metadata
- [`LICENSE`](./LICENSE) — MIT License
- [`requirements.txt`](./requirements.txt) — Python dependencies

### Reference implementation
- [`cstl_parser.py`](./cstl_parser.py) — Reference parser v4.0
- [`cstl_domains.py`](./cstl_domains.py) — 18 domain ontologies
- [`cstl_codec.py`](./cstl_codec.py) — Codec utilities
- [`cstl_colab.py`](./cstl_colab.py) — Colab notebook helpers
- [`cstl_stsb_test.py`](./cstl_stsb_test.py) — STSB benchmark utilities

### Canonical examples
- [`payload_novatech.cstl`](./payload_novatech.cstl) — CSTL example (financial AI Act scenario)
- [`payload_novatech.json`](./payload_novatech.json) — JSON Schema equivalent
- [`payload_novatech.fc.json`](./payload_novatech.fc.json) — Function Calling equivalent

### Comparative analyses
- [`CSTL_vs_JSON.md`](./CSTL_vs_JSON.md) — 5-dimension rigorous comparison
- [`CSTL_vs_G2CP.md`](./CSTL_vs_G2CP.md) — Analysis of direct competitor (AAMAS 2026)
- [`CSTL_literature_review_S1.md`](./CSTL_literature_review_S1.md) — Literature review (20 protocols)

### Benchmarks and tests
- [`test_5aspects.py`](./test_5aspects.py) — 5-dimension benchmark framework
- [`run_experiments.py`](./run_experiments.py) — Reproducibility entry point
- [`e1_compression.py`](./e1_compression.py) + [`e1_compression.csv`](./e1_compression.csv) — Compression measurements
- [`e3_expressivity.py`](./e3_expressivity.py) + [`e3_expressivity.csv`](./e3_expressivity.csv) — Expressivity measurements
- [`e4_k9_uniqueness.py`](./e4_k9_uniqueness.py) — k=9 attribute uniqueness study

### Empirical experiments
- [`experiments/`](./experiments/) — Full empirical trajectory v3.0.2 → v3.0.4 with statistical validation (bootstrap CI, permutation tests, sign tests)

### Version history
- [`v3.0.1_patch_notes.md`](./v3.0.1_patch_notes.md)
- [`v3.0.2_patch_notes.md`](./v3.0.2_patch_notes.md)
- [`v3.0.4_prompt_system.md`](./v3.0.4_prompt_system.md)
- [`COMMIT_GUIDE_v304.md`](./COMMIT_GUIDE_v304.md)
- [`EXPERIMENTAL_SETUP.md`](./EXPERIMENTAL_SETUP.md)

### Paper draft
- [`CSTL_arXiv_draft_v2.md`](./CSTL_arXiv_draft_v2.md) — Preprint in preparation

---

## Installation

```bash
git clone https://github.com/oliviergoyette/cstl.git
cd cstl
pip install -r requirements.txt
python cstl_parser.py  # Run reference parser tests
```

PyPI release planned: `pip install cstl` (Q2 2026).

---

## Quick start

```python
from cstl_parser import parse, encode, validate

# Parse a CSTL payload
with open('payload_novatech.cstl') as f:
    doc = parse(f.read())

print(f"Domain: {doc.domain}")
print(f"Constraints: {len(doc.constraints)}")
print(f"Relations: {len(doc.relations)}")

# Validate syntax and report
report = validate(payload_text)
print(report)

# Re-encode (compact mode with σδτωι symbols)
text = encode(doc, compact=True)
print(text)
```

---

## Citation

If you use CSTL in research, please cite:

```bibtex
@misc{goyette2026cstl,
  author       = {Goyette, Olivier},
  title        = {CSTL: A Compressed Semantic Transfer Language for Lossless LLM-to-LLM Communication},
  year         = {2026},
  howpublished = {\url{https://github.com/oliviergoyette/cstl}},
  note         = {Preprint in preparation}
}
```

A formal arXiv preprint is in preparation.

---

## License

MIT License — see [LICENSE](./LICENSE) for details.

---

## Contact

Author: **Olivier Goyette**

For questions, collaborations, or commercial inquiries, please open an issue on this repository.
