# Experiments — 2026-04-20

## Statistical validation (Q2)

This folder adds bootstrap confidence intervals and permutation tests to
the E1 (compression) and E3++ (expressivity) results produced in
`../20260419/`. No new data was collected — only statistical analysis
of existing CSVs.

Motivation : Gemini methodological audit (April 19) flagged the absence
of significance testing as a likely reviewer objection. This addresses
that gap.

## Files

- `Q2_resultats_stats.txt` — All confidence intervals, p-values, and
  paper-ready phrasings for §5.3, §5.5, and §6.

## Methods

- **Bootstrap** : 10 000 paired resamples with replacement, 95% CI on
  the ratio (E1) or mean difference (E3++).
- **Permutation test** : 10 000 paired sign flips, two-sided p-value
  on H₀: CSTL = baseline.
- **Sign test** : exact binomial on non-tied pairs (E3++ only).

## Headline results

**E1 — Compression (n=20 trials × 6 sizes, source: `../20260419/e1_compression.csv`)**

- CSTL/JSON-compact at N=100 : 0.412 (95% CI [0.411, 0.412])
- CSTL.gz/JSON-c.gz at N=100 : 0.864 (95% CI [0.862, 0.866])
- Permutation p < 10⁻⁴ at every N

**E3++ — Expressivity (200 paired obs, source: `../20260419/e3_plus_plus_comparative_v303_v304.csv`)**

- CSTL v3.0.4 mean : 0.958 | JSON mean : 0.818
- Δ = +0.140 (95% bootstrap CI [+0.098, +0.185])
- Permutation p < 10⁻⁴
- Sign test : 52/58 non-tied pairs favor CSTL (p < 10⁻⁶)
- Family H underperforms by 12.5% — documented as known limitation

## Caveats (still open)

1. **Single judge** (Claude). Multi-judge re-evaluation with Gemini and
   GPT-4 on ≥20% of the corpus deferred to v3.1.
2. **JSON-strict baseline**, not AMR. AMR comparison deferred to v3.1.

## Reproduction

The full bootstrap and permutation procedure is described in
`Q2_resultats_stats.txt`. Random seed: 42.
