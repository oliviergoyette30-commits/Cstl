# Fix Validation Summary — CSTL v3.0.5 → v3.0.6

## Purpose

This document formally validates the four hypotheses (H1–H4) posed before running the three v3.0.6 validation sessions. Each hypothesis links one syntactic fix (or the header renaming bonus) to the specific v3.0.5 failure it was designed to address.

This is not post-hoc rationalization: the fixes and the hypotheses were defined *before* runs 6, 7, 8 were executed, based on the pooled failure analysis of runs 1–5 (see `runs_v305_5_sessions.csv`).

## Hypotheses

| ID | Fix | Targeted v3.0.5 failure | Predicted v3.0.6 outcome |
|----|-----|------------------------|--------------------------|
| H1 | Fix A: `[MUST,FUT]` composite modality | FUT absorbed by [MUST] (criterion #3 failed 5/5 in v3.0.5) | Criterion #3 passes 3/3 in v3.0.6 |
| H2 | Fix B: `strength=0.92` labeled attribute | Anonymous force value ignored (criterion #4 passed only 1/5, qualitatively) | Criterion #4 passes 3/3 in v3.0.6 |
| H3 | Fix C: `ARR.CREATE`, `ARR.CAUSE` subtypes | Animate-subject ambiguity (criterion #1 passed 3/5 with 2/5 lexical drift) | Criterion #1 passes 3/3 in v3.0.6 |
| H4 | Header renaming (`session_id =`, `has_trust`, `global_time =`) | Headers H1 and H4 ignored 5/5 in v3.0.5 | Headers H1 and H4 pass 3/3 in v3.0.6 |

## Validation results

### H1 — Composite modality

**v3.0.5 baseline (criterion #3):**

| Run | Result | Decoded form |
|-----|--------|--------------|
| 1 | fail | "doit impérativement parvenir" (present) |
| 2 | fail | "doit impérativement parvenir" (present) |
| 3 | fail | "doit impérativement parvenir" (present) |
| 4 | fail | "doit impérativement parvenir" (present) |
| 5 | fail | "doit impérativement parvenir" (present) |

Pass rate: **0/5 (0%)**

**v3.0.6 validation (criterion #3 with `[MUST,FUT]`):**

| Run | Result | Decod
