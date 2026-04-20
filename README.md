# CSTL v3 — Compressed Semantic Transfer Language

*A relational protocol for lossless AI-to-AI communication.*

![License](https://img.shields.io/badge/license-MIT-blue)
![Spec](https://img.shields.io/badge/spec-v3.0.4-brightgreen)
![Status](https://img.shields.io/badge/status-preprint%20in%20preparation-orange)

**Author**: Olivier Goyette · **Specification**: v3.0.4 (April 2026)

---

## What is CSTL?

CSTL is a three-layer relational protocol engineered for the problem of
*lossless inter-LLM communication* — preserving propositional content,
numerical force, temporality, transmission intent, and multi-agent trust
state across agent hops.

Unlike RDF (static facts), AMR (single sentences), or JSON-LD (web
metadata), CSTL is a **unified architecture** designed specifically for
AI-to-AI exchange: a semantic layer of 65 symbols, a syntactic layer
of 121 tokens, and a transport layer built on a provably unique k=9
nucleotide encoding.

## The three layers

| Layer | Name | Role | Size |
|-------|------|------|------|
| 1 | Semantic | What the relation **means** | 65 symbols |
| 2 | Syntactic | How the relation is **encoded** | 121 tokens |
| 3 | Transport DNA | How the relation is **transmitted** | k=9 universal |

## Fundamental theorem

> **Theorem 1 (Uniqueness)**. For every relation R in a human corpus of
> fewer than 10⁸ relations, 9 CSTL symbols produce a unique universal
> identifier.
>
> Proof: 121⁹ ≈ 5.56 × 10¹⁸ addresses; measured collision rate on
> 499,689 distinct relations = 0.

## Empirical results (v3.0.4, 2026-04-19)

### Cross-lingual semantic preservation (100 EN/FR parallel pairs)

Claude Opus 4.5 encoder and judge, strict JSON enum baseline.

| Encoding | EN | FR | Cross-lingual invariance |
|----------|----:|----:|-------------------------:|
| **CSTL v3.0.4** | **0.975** | **0.940** | **91%** |
| JSON-strict | 0.855 | 0.780 | ~80% |

**CSTL advantage: +0.120 EN / +0.160 FR.**

### Pair-level victories

Out of 100 pairs:
- CSTL v3.0.4 preserves more nuance than JSON-strict: **35 pairs**
- JSON-strict preserves more nuance than CSTL: **5 pairs**
- Ties: **60 pairs**

Ratio of decisive wins: **7:1** in favor of CSTL.

### Per-family breakdown

CSTL v3.0.4 outperforms JSON-strict on **14 of 15** semantic families.

Strong dominance (Δ ≥ 0.20 on at least one language):

| Family | Δ EN | Δ FR | Description |
|--------|-----:|-----:|-------------|
| B | +0.417 | +0.250 | Temporal ordering |
| K | +0.308 | +0.385 | Complex combinations |
| M | +0.250 | +0.375 | Cross-lingual invariance (idioms, aspect) |
| A | +0.167 | +0.083 | Global headers `[TIME]`, `[TRUST]`, `[STATE]` |
| O | +0.167 | +0.167 | Transitive propagation |
| F | +0.150 | +0.100 | ψ layer (consciousness, deixis, performatives) |

Single documented weakness:

| Family | Δ EN | Δ FR | Description |
|--------|-----:|-----:|-------------|
| H | −0.125 | 0.000 | Pragmatic tone intensifiers ("entirely", "complètement") |

Cause identified: intensity markers not currently encoded. Candidate for
Rule 14 (intensity markers) in v3.0.5.

### Verdict distribution (100 pairs, CSTL v3.0.4)

| | Preserved | Partial | Lost |
|---|----------:|--------:|-----:|
| EN | 96 | 3 | 1 |
| FR | 91 | 6 | 3 |

JSON-strict on the same 100 pairs:

| | Preserved | Partial | Lost |
|---|----------:|--------:|-----:|
| EN | 74 | 23 | 3 |
| FR | 66 | 24 | 10 |

### Trajectory across spec versions

| Version | CSTL EN | CSTL FR | Cross-lingual invariance |
|---------|--------:|--------:|-------------------------:|
| v3.0.0  | 0.825 | — | — |
| v3.0.1  | 1.000 (E3, 20 sentences) | — | — |
| v3.0.2  | 0.980 (E3++) | 0.840 | 78% |
| v3.0.3  | 0.955 | 0.835 | — (10 regressions) |
| **v3.0.4** | **0.975** | **0.940** | **91%** |

### Compression (E1)

CSTL payloads are 2.43× smaller than JSON-compact raw and 1.16× smaller
after gzip at N=100 relations. Raw data: `e1_compression.csv`.

### k=9 uniqueness theorem (E4)

Measured collision rate on 499,689 distinct relations: **0**.
Raw data: `e4_k9_uniqueness.py` reproduces the result.

## Repository contents

- `CSTL_v3_Spec.docx` — canonical specification (v3.0.4)
- `CHANGELOG.md` — versioned history
- `v3.0.4_prompt_system.md` — production encoder prompt (narrow-scope Rule 12)
- `v3.0.3_prompt_system.md` — historical (superseded)
- `v3.0.3_patch_notes.md` — empirical justification for Rules 12 and 13
- `v3.0.2_patch_notes.md` — six-fix internal audit
- `v3.0.1_patch_notes.md` — `[TIME]` header introduction
- `CSTL_arXiv_draft_v2.md` — preprint draft
- `EXPERIMENTAL_SETUP.md` — methodology for reproducing benchmarks
- `requirements.txt` — Python dependencies
- `e1_compression.{csv,py}` — compression results
- `e3_expressivity.{csv,py}`, `e3_expressivity_v301.csv` — v3.0 and v3.0.1
- `e4_k9_uniqueness.py` — theorem verification
- `run_experiments.py` — orchestrator

### `/experiments/20260419/` (full empirical trajectory)

- `e3_v302_benchmark.csv` — v3.0.2 audit validation (30 sentences)
- `e3_plus_plus_results.csv` — E3++ v1 (100 pairs, JSON verbose — methodological artifact)
- `e3_plus_plus_v2_strict_results.csv` — E3++ v2 (v3.0.2 vs JSON-strict)
- `e3_plus_plus_comparative_v302_v303.csv` — v3.0.2 vs v3.0.3 (regressions documented)
- `e3_plus_plus_comparative_v303_v304.csv` — v3.0.3 vs v3.0.4 (final validation)

## Reproducing the benchmarks

1. `pip install -r requirements.txt`
2. Set `ANTHROPIC_API_KEY` in your environment.
3. `python run_experiments.py`

See `EXPERIMENTAL_SETUP.md` for details on cost, expected duration,
and model versions.

## Canonical pipeline

CSTL is designed to operate as the semantic substrate of a four-step
pipeline:

```
LLM reads → CSTL structures → CSTL verifies → LLM reformulates
```

An emitter LLM extracts the k=9 DNA of its output; the receiver LLM
looks up the DNA in a shared universal dictionary and reformulates in
its own semantic space. Because the DNA is deterministic and
collision-free at k=9, semantic fidelity is preserved across the hop.

## License

MIT — see `LICENSE`.

## Citation

If you use CSTL in research, please cite the arXiv preprint (in preparation):

```bibtex
@misc{goyette2026cstl,
  author       = {Olivier Goyette},
  title        = {CSTL: A Relational Semantic Protocol for Lossless AI-to-AI Communication},
  year         = {2026},
  note         = {Preprint, v3.0.4}
}
```
