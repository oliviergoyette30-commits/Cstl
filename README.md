# CSTL v3 — Compressed Semantic Transfer Language

A relational protocol for lossless AI-to-AI communication.

**Author:** Olivier Goyette
**Version:** 3.0.1 (April 2026)
**Status:** Preprint in preparation

---

## What is CSTL?

CSTL is a three-layer relational protocol engineered for the problem of *lossless inter-LLM communication* — preserving propositional content, numerical force, temporality, transmission intent, and multi-agent trust state across agent hops.

Unlike RDF (static facts), AMR (single sentences), or JSON-LD (web metadata), CSTL targets discourse-level, weighted, time-stamped, multi-agent payloads with a uniqueness guarantee by construction.

## Architecture (v3.0.1)

| Layer | Name | Role | Size |
|---|---|---|---|
| 1 | Semantic | What the relation *means* | 65 symbols |
| 2 | Syntactic | How the relation is *encoded* | 121 tokens |
| 3 | Transport (DNA) | How the relation is *transmitted* | k=9 nucleotides |

- **Theorem 1** (uniqueness): the k=9 encoding is injective by construction for corpora up to 10⁸ relations. Address space: 121⁹ ≈ 5.56 × 10¹⁸.
- **8 foundational axioms** (existence, curvature, transformation, gravity, time, coherence closure, conservation, purge).
- **4-stage pipeline**: `LLM reads → CSTL structures → CSTL verifies → LLM reformulates`.

See [`spec/`](./spec/) for the formal specification.

## Repository structure

```
.
├── spec/                    # CSTL specification (v3.0.1)
├── harness/                 # Reproducible evaluation harness
├── experiments/             # Standalone experiment scripts + raw results
│   └── 20260418/            # Results from the April 2026 run
├── notebooks/               # Colab notebook reproducing the full plan
├── paper/drafts/            # arXiv preprint drafts
├── CHANGELOG.md             # Version history
└── LICENSE
```

## Empirical results (preprint preview)

Four experiments, all reproducible from this repo:

| Experiment | Result | Cost |
|---|---|---|
| **E1 — Compression** | CSTL 2.43× smaller than JSON-compact (raw), 1.16× after gzip at N=100 | $0 |
| **E3 — Expressivity** (v3.0) | 0.950 vs JSON 0.825 on 20-sentence nuance benchmark | ~$0.40 |
| **E3 — Expressivity** (v3.0.1 after `[TIME]` fix) | **1.000** vs JSON 0.825, zero regressions | ~$0.40 |
| **E4 — Theorem 1 validation** | 0 collisions on 499,689 distinct relations | $0 |

Raw data: [`experiments/20260418/`](./experiments/20260418/).

## Reproducing the results

```bash
# Install
pip install -r harness/requirements.txt

# Sanity check (no API calls)
python3 harness/run_experiments.py --models mock --n-payloads 20

# E1 — compression (no API calls)
python3 experiments/e1_compression.py

# E4 — Theorem 1 empirical validation (no API calls)
python3 experiments/e4_k9_uniqueness.py

# E3 — expressivity (requires ANTHROPIC_API_KEY)
python3 experiments/e3_expressivity.py
```

See [`harness/EXPERIMENTAL_SETUP.md`](./harness/EXPERIMENTAL_SETUP.md) for exact model versions, hyperparameters, and dates of the published runs.

## Citation

If you use CSTL, please cite:

```
@misc{goyette2026cstl,
  author = {Goyette, Olivier},
  title = {CSTL: A Three-Layer Relational Protocol for Lossless AI-to-AI Communication},
  year = {2026},
  note = {arXiv preprint (forthcoming)}
}
```

## License

See [LICENSE](./LICENSE).
