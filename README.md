# CSTL — Compressed Semantic Transfer Language

**Status**: v4.9.3 — stable. A self-sufficient semantic protocol for lossless
LLM-to-LLM communication. Empirically validated zero-shot on 5 LLMs. The sister
networked mode (CASTLE) is deferred to v5.0.

CSTL is a structured protocol for **LLM-to-LLM semantic communication**. It
encodes information with explicit preservation of deontic modalities
(obligations, prohibitions, conditions), numerical strengths (0.0–1.0),
temporality, uncertainty markers, and relations between entities — information
that free text loses across agent hops and that JSON encodes only by
convention.

---

## Why CSTL

JSON was designed for human → machine. Protobuf for machine → machine. Natural
language for human → human. **No format was designed natively for LLM → LLM**,
where the channel is text but both ends are statistical models that benefit
from explicit structure of modalities and forces.

The problem it addresses: multi-agent LLM systems suffer from semantic drift.
When agent A communicates with agent B in free text, modalities, force, and
temporality are lost or distorted. CSTL preserves them explicitly:

- **Modalities**: `[MUST]`, `[NOT]`, `[MAY]`, `[IF]…[MUST]`
- **Forces**: `σ=0.92`, preserved exactly across round-trips
- **Temporality**: `τ=past|present|future`, plus `deadline=…`
- **Uncertainty**: `UNKNOWN`, `ESTIMATED`, `INFERRED` (epistemic status)
- **Traceability**: canonical SHA-256 hash per payload, `PARENT_HASH` chaining

See [`SPEC_v4_9_3.md`](./SPEC_v4_9_3.md) for the formal specification.

---

## Repository layout

### Reference implementations

- **Rust parser** (`rust/`) — production parser. Pure Rust, zero external
  dependencies, SHA-256 in-crate. Hand-rolled lexer + recursive-descent parser.
  Entry points: `parse()`, `is_valid()`; CLI binary `cstl_validate`.
- **Python parser** (`cstl_parser.py`, `fast_parser_v492.py`) — readable
  reference implementation.

### Memory layer

- `cstl_adn_store.py` — persistent memory store (SQLite). Anchors validated
  decisions immutably (C5), retrieves context via TF-IDF, supports rule
  versioning (`supersede`) and fail-closed ingestion.
- `cstl_sdk.py` — standalone encoder/decoder, zero dependencies.

### Specification and docs

- `SPEC_v4_9_3.md` — current formal specification (supersedes `SPEC_v4.md`)
- `CHANGELOG_v493.md` — version history through v4.9.3
- `CITATION.cff` — academic citation metadata
- `LICENSE` — MIT

### Comparative analyses

- `CSTL_vs_JSON.md` — rigorous 5-dimension comparison
- `CSTL_vs_G2CP.md` — analysis of the closest concurrent work (AAMAS 2026)
- `CSTL_literature_review_S1.md` — review of ~20 communication protocols

### Benchmarks and experiments

- `test_5aspects.py`, `test_security_suite.py` — test suites
- `e1_compression.py` / `.csv` — compression measurements
- `e3_expressivity.py` / `.csv` — expressivity measurements
- `experiments/` — full empirical trajectory with statistical validation
  (bootstrap CI, permutation tests, sign tests; random seed 42)

---

## Quick start

### Rust (production parser)

```bash
cd rust
cargo build --release
cargo test                       # run the test suite
echo "<payload>" | cargo run --bin cstl_validate
```

### Python (reference)

```bash
pip install -r requirements.txt
python cstl_parser.py
```

---

## Quick example

```
#!CSTL v4.9.3 MODE=A
META [
encoder=Agent_CLAUDE,
produced_by=anthropic/claude-sonnet,
sigma=0.94,
RESPONSE_FORMAT=CSTL,
NO_PROSE=true,
PARENT_HASH=root
]
CONSTRAINTS [
[MUST] NovaTech DELIVER technical_documentation,
[MUST] FidBank MAINTAIN audit_trail,
[NOT] CreditEval PERFORM auto_decision,
[IF] borrower_contests [MUST] FidBank REVIEW
]
UNCERTAINTY [
false_positive_rate ESTIMATED [sigma=0.65],
sociodemographic_bias UNKNOWN,
rgpd_compliance INFERRED [sigma=0.70]
]
DECISION: regulatory_audit_continuation [sigma=0.94]
---END---
```

A lawyer, an LLM, or a parser can read this directly. No infrastructure
required.

---

## Validation

CSTL v4.9.3 has been validated zero-shot on five LLMs: Claude, GPT, Gemini,
Mistral Large (spontaneous), and Llama-3-70B (via pre-fill primer).

The Python test suite reports 201 passing tests. The Rust crate is documented
at 41 tests, 0 compiler warnings — **see "Reproducing the claims" below before
citing these numbers.**

Statistical validation (bootstrap CI 95%, permutation tests, sign tests) is
documented in `experiments/` with random seed 42.

---

## Reproducing the claims

The benchmark and test numbers in this repository should be reproduced before
being cited:

```bash
cd rust && cargo test          # verifies the Rust test count
python -m pytest               # verifies the Python test count
python run_experiments.py      # regenerates the benchmark CSVs
```

If any number does not reproduce on your machine, the reproducible output of
the commands above is authoritative, not the prose in this README.

---

## Known limitations

These are documented deliberately. They are open work, not hidden defects.

- **Semantic search uses TF-IDF, not embeddings.** Synonyms ("résilier" vs
  "annuler") are not reliably matched in the ADN store. Embeddings are the
  intended fix and are not yet implemented.
- **`sigma` is self-declared.** Nothing validates that a declared confidence
  corresponds to real accuracy. Empirical calibration is future work.
- **Small benchmark sample.** Some protocols are evaluated at n=15.
- **Evaluation auditor.** Evaluation to date relied substantially on a single
  judge; independent and blind evaluation is required for a credible academic
  claim.

---

## Related work

CSTL sits within a broader landscape of LLM communication protocols:

| Protocol | Category | Layer |
|----------|----------|-------|
| MCP (Anthropic, 2024) | LLM ↔ Tools | Transport |
| A2A (Google, 2025) | Agent ↔ Agent | Transport |
| G²CP (Ben Khaled & Monticolo, AAMAS 2026) | Multi-agent | Graph operations |
| Function Calling | LLM tool invocation | JSON Schema subset |
| AMR / PENMAN | Semantic representation | Sentence-level graph |
| TypeChat (Microsoft, 2023) | Structured output | TypeScript types |
| DSPy (Stanford, 2024) | Programming framework | Module orchestration |

CSTL occupies a specific niche: a textual, self-sufficient, zero-infrastructure
semantic protocol for LLM-to-LLM communication with native deontic modalities.

---

## Deferred to v5.0

- CASTLE networked mode (shared-dictionary compression)
- `SELF_DECLARE` block
- ADN delta-payload mode

---

## Citation

```bibtex
@misc{goyette2026cstl,
  author       = {Goyette, Olivier},
  title        = {CSTL: A Compressed Semantic Transfer Language for LLM-to-LLM Communication},
  year         = {2026},
  howpublished = {\url{https://github.com/oliviergoyette30-commits/Cstl}},
  note         = {Preprint in preparation}
}
```

An arXiv preprint is in preparation.

---

## License

MIT — see [`LICENSE`](./LICENSE).

## Contact

Author: **Olivier Goyette**. For questions, collaborations, or commercial
inquiries, please open an issue on this repository.
