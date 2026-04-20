# Experiments — 2026-04-19

This folder contains the full empirical trajectory for CSTL v3.0.2 → v3.0.4
produced on 2026-04-19.

## Files

### 30-sentence targeted benchmark (E3+)

- **`e3_v302_benchmark.csv`** — Phase 1 (20 baseline sentences re-run with
  v3.0.2 prompt) + Phase 2 (10 new targeted sentences for the 4 v3.0.2 fix
  zones). Score: CSTL 0.933 vs JSON 0.583 (+0.350).

### E3++ benchmark design

- **`e3_plus_plus_design.md`** — Full methodology documentation: 15 families,
  level annotations (L1/L2/L3), difficulty distribution, 100 EN/FR parallel
  pairs design.
- **`e3_plus_plus_benchmark.csv`** — Design sheet of the 100 EN/FR pairs.

### E3++ benchmark results (chronological)

- **`e3_plus_plus_results.csv`** — E3++ v1, 100 EN/FR pairs, verbose JSON
  baseline with free-text fields.
  ⚠️ **Methodological artifact detected**: verbose JSON embedded source text
  verbatim (e.g., `time: "yesterday, before colleagues arrived"`), artificially
  inflating JSON preservation scores. Run kept for methodological transparency.
  Superseded by v2.

- **`e3_plus_plus_v2_strict_results.csv`** — E3++ v2, 100 pairs, **strict JSON
  enum baseline** (enum-only values, no free text).
  - CSTL v3.0.2 EN: 0.980 / FR: 0.840
  - JSON-strict EN: 0.835 / FR: 0.765
  - Advantage: +0.145 EN / +0.075 FR
  - Cross-lingual invariance: 78%

- **`e3_plus_plus_comparative_v302_v303.csv`** — v3.0.2 vs v3.0.3 comparative
  benchmark on the same 100 pairs.
  - CSTL v3.0.3 EN: 0.955 / FR: 0.835
  - 28 improvements / 10 regressions
  - Revealed over-generalization of initial Rule 12 formulation
    (42% length reduction, structural glyph retrieval: `⟳` −40%, `⊃` −57%)
  - Led to v3.0.4 narrow-scope reformulation

- **`e3_plus_plus_comparative_v303_v304.csv`** — **FINAL v3.0.4 validation.**
  - **CSTL v3.0.4 EN: 0.975** (vs JSON-strict: 0.855)
  - **CSTL v3.0.4 FR: 0.940** (vs JSON-strict: 0.780)
  - **Advantage: +0.120 EN / +0.160 FR**
  - **Cross-lingual invariance: 91%**
  - Pair-level wins: CSTL 35 / JSON 5 / ties 60 (ratio 7:1)
  - 14/15 semantic families dominated by CSTL
  - Single documented weakness: Family H (pragmatic tone intensifiers)

## Methodology

- **Encoder**: Claude Opus 4.5 (`claude-opus-4-5`, temperature default)
- **Judge**: Claude Opus 4.5 (same model, same session)
- **Baseline**: strict JSON with enum-only field values (v2 onwards)
- **Scoring**: preserved=1.0, partial=0.5, lost=0.0 (trinary verdict)
- **Protocol**: each pair encoded in EN and FR, judged independently.
  Comparative benchmarks (v302_v303, v303_v304) encode each pair with both
  versions in the same session to eliminate judge variance between runs.

## Trajectory summary

| Version | CSTL EN | CSTL FR | Invariance | Notes |
|---------|--------:|--------:|-----------:|-------|
| v3.0.2  | 0.980   | 0.840   | 78%        | Baseline (6-fix audit) |
| v3.0.3  | 0.955   | 0.835   | —          | 10 regressions documented |
| **v3.0.4** | **0.975** | **0.940** | **91%** | **Final, narrow-scope Rule 12** |

## Cost and duration

| Benchmark | API calls | Duration | Cost |
|-----------|----------:|---------:|-----:|
| E3+ (30 sentences) | 120 | ~12 min | ~$1 |
| E3++ v1 | 600 | ~29 min | ~$3 |
| E3++ v2 strict | 600 | ~30 min | ~$3 |
| E3++ comparative v302/v303 | 1200 | ~38 min | ~$6 |
| E3++ comparative v303/v304 | 1200 | ~40 min | ~$6 |
| **Total** | **3720** | **~2h30** | **~$19** |

## Reproducibility

Scripts to re-run the benchmarks are in the repo root:

- `e3_plus_plus_comparative_v303_v304.py` — final benchmark cell (Colab)
- Other scripts documented in `../../EXPERIMENTAL_SETUP.md`

All benchmarks use `ANTHROPIC_API_KEY` environment variable or paste-in
(as in the uploaded Colab cells).
