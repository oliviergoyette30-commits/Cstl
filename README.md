# CSTL v5.0.0 — Compressed Semantic Transfer Language

> The first wire format designed natively for LLM-to-LLM communication.
>
> **Les relations sont plus importantes que l'information.** — [Principes fondateurs](PRINCIPES.md)

[


](LICENSE)
[


](src/tests.rs)

## What is CSTL?

CSTL is a structured text format for inter-LLM communication. It fills a gap: when AI agents coordinate, the formats available today are either natural language (ambiguous, no audit trail), JSON (no modal logic, no uncertainty), or function calls (vendor-specific). CSTL is designed natively for this use case.

**Unique combination** (no existing format combines all four):
- Native deontic modalities: MUST, MUST_NOT, MAY, IFF
- Quantified uncertainty: aleatory vs epistemic, per-relation sigma values
- Provenance tracking: produced_by, PARENT_HASH, canonical SHA-256
- Deterministic parser: O(n), zero LLM required for validation

## Empirical Results

| Test | Method | Result |
|---|---|---|
| Decoding fidelity | N=10, Gemini 2.5 Pro | 100% (Wilson CI [96.3%, 100%]) |
| Encoding fidelity | N=5, 194 facts, Gemini+GPT-5 | 97.9% (Wilson CI [94.8%, 99.2%]) |
| Cross-vendor chain | 2 hops, 3 vendors bidirectional | 96-98% |

## v5.0.0 Operators

Logical: ENTAILS, CONTRADICTS
Epistemic: KNOWS, BELIEVES, ASSUMES, DOUBTS
Temporal (Allen subset): BEFORE, AFTER, DURING
Relational: EQUALS, POSSESSES, RESEMBLES, CO_LOCATES, OPPOSES, COMPARES
Deprecated: MUTUAL (use specific relational operator, W601)

## Rust Parser

cargo test   # 112 tests, 0 failures

## Honest Limitations

- N=10 to N=5, single human auditor. Exploratory, not confirmatory.
- Multi-hop degradation curve: not measured beyond 2 hops.
- Open-weight LLMs (Llama, Mistral, Qwen): not validated.
- Compression: ~1.45x vs JSON-LD raw; disappears after gzip.
- Zero external adopters.

## Formal Semantics

Deontic operators grounded in SDL (von Wright, 1951) with Kripke semantics.
Epistemic operators follow Hintikka (1962).
Temporal operators implement Allen interval algebra subset (1983).
Full spec: CSTL_SPEC_v5_0.md

## License

Apache 2.0 — Olivier Goyette
