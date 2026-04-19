# CSTL v3 — Compressed Semantic Transfer Language

*A relational protocol for lossless AI-to-AI communication.*

![License](https://img.shields.io/badge/license-MIT-blue)
![Spec](https://img.shields.io/badge/spec-v3.0.2-brightgreen)
![Status](https://img.shields.io/badge/status-preprint%20in%20preparation-orange)

**Author**: Olivier Goyette · **Specification**: v3.0.2 (April 2026)

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

## Empirical results

Four experiments validate v3.0.2:

### E1 — Compression (payload size)
CSTL payloads are 2.43× smaller than JSON-compact raw and 1.16× smaller
after gzip at N=100 relations. Raw data: `e1_compression.csv`.

### E3 — Nuance preservation (expressivity)
On 20 nuance-stressing sentences with Claude-as-judge:
- CSTL v3.0: 0.825
- CSTL v3.0.1: 1.000 (after `[TIME]` header fix)
- CSTL v3.0.2: 0.950 on Phase 1 + 0.900 on Phase 2 (10 new targeted sentences)
- JSON flat baseline: 0.583 across the full 30-sentence set

CSTL v3.0.2 scores **+0.350 above JSON** on the full expanded benchmark.
Raw data: `experiments/20260418/e3_expressivity_v301.csv`,
`experiments/20260419/e3_v302_benchmark.csv`.

### E4 — k=9 uniqueness theorem
Measured collision rate on 499,689 distinct relations: **0**.
Raw data: `e4_k9_uniqueness.py` reproduces the result.

### Internal audit (v3.0.2)
Systematic stress-testing of v3.0.1 along four angles (usage
ambiguities, semantic overlaps, uncovered zones, internal
contradictions) identified six latent zones in need of
disambiguation. All six were resolved in v3.0.2 through two glyph
separations and four new grammar rules. See `CHANGELOG.md` and
`v3.0.2_patch_notes.md`.

## Repository contents

- `CSTL_v3_Spec.docx` — canonical specification (v3.0.2)
- `CHANGELOG.md` — versioned history of changes
- `v3.0.2_patch_notes.md` — detailed audit notes for v3.0.2
- `CSTL_arXiv_draft_v2.md` — preprint draft
- `EXPERIMENTAL_SETUP.md` — methodology for reproducing benchmarks
- `requirements.txt` — Python dependencies for the benchmark scripts
- `e1_compression.{csv,py}`, `e1_compression_figure.png` — compression results
- `e3_expressivity.{csv,py}`, `e3_expressivity_v301.csv` — v3.0 and v3.0.1 expressivity
- `experiments/20260419/e3_v302_benchmark.csv` — v3.0.2 audit validation
- `e4_k9_uniqueness.py` — theorem verification
- `run_experiments.py` — orchestrator for all experiments

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

An emitter LLM (e.g., Claude) extracts the k=9 DNA of its output;
the receiver LLM (e.g., GPT-4, Gemini) looks up the DNA in a shared
universal dictionary and reformulates in its own semantic space.
Because the DNA is deterministic and collision-free at k=9, semantic
fidelity is preserved across the hop.

## License

MIT — see `LICENSE`.

## Citation

If you use CSTL in research, please cite the arXiv preprint (in preparation):

```bibtex
@misc{goyette2026cstl,
  author       = {Olivier Goyette},
  title        = {CSTL: A Relational Semantic Protocol for Lossless AI-to-AI Communication},
  year         = {2026},
  note         = {Preprint, v3.0.2}
}
```
