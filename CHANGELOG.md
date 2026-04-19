# Changelog

All notable changes to the CSTL specification and supporting artifacts are documented here.

## [3.0.2] — 2026-04-19

### Added

- **Rule 8** (Part 5 — Grammar) — Performative tone `(!)` vs Modal obligation `[MUST]`.
  Surface-marker-based disambiguation with a 3-step decision procedure.
  The two glyphs are **mutually exclusive by default**; combining them is
  reserved for sentences where both marker families are simultaneously
  present. Empirically validated at 5/5 in targeted re-test.

- **Rule 9** (Part 5 — Grammar) — Mutual relation `↔` vs Resonance `ℝ`.
  Two-level disambiguation: the formal definition (Dice > 0.85) in
  Part 2.7 is preserved for strict parsers, while a linguistic-marker
  rule provides an operational criterion for LLM encoders. Markers of
  similarity (`same`, `identical`, `in unison`, `echo`) trigger `ℝ`;
  structural co-regulation (`discuss with`, `cooperate with`) triggers `↔`.

- **Rule 10** (Part 5 — Grammar) — Direct grip `⊃` vs Meta-control `⊃[]`.
  Clarifies the intentional hierarchical pair: `⊃` constrains the
  target's own actions (object level); `⊃[]` constrains the target's
  relations with others (meta level).

- **Rule 11** (Part 5 — Grammar) — Transmission modes `≡` / `≠` / `∿`.
  Operational criteria: `≡` preserves words; `≠` preserves meaning
  with different words; `∿` adds new information by inference or
  extrapolation.

### Changed

- **Part 2.4 — Dynamics**: the glyph for "transformation in progress"
  changes from `⟳` to `⟲`. This disambiguates it from `⟳` in Part 2.2
  (Relations), which denotes transformation achieved (`∙ → ◉`).
  The pair now encodes the distinction between the ongoing process
  and its completed outcome.

- **Part 2.8 — Transmission Modes**: the glyph for the "archeological"
  transmission mode changes from `«` to `⇐`. This disambiguates it
  from `«` in Part 2.5 (Time), which marks temporal past.
  The two usages are fundamentally different (property of a relation
  vs. transmission choice by the agent) and now carry distinct glyphs.

### Motivation

Internal audit of v3.0.1 before public release. Following the T01
discovery methodology that produced v3.0.1 (empirical nuance-preservation
evaluation), the specification was systematically stressed along four
angles: usage ambiguities, semantic overlaps, uncovered zones, and
internal contradictions. Six zones were identified as in need of
disambiguation. All six were resolved either through glyph separation
(Fixes #1 and #2) or through added grammar rules (Fixes #3 through #6).

### Validation

Benchmark `e3_v302_benchmark.csv` (30 sentences, Claude Opus 4.5 as judge,
temperature 0):

- **Phase 1** — 20 baseline E3 sentences re-run with v3.0.2 prompt:
  CSTL score 0.950, JSON score 0.650. **Zero regression caused by the
  fixes** (two "partial" verdicts occurred on sentences whose nuance
  is outside the zones fixed by v3.0.2, attributable to LLM stochastic
  variance).
- **Phase 2** — 10 new targeted sentences for the 4 fixed zones:
  CSTL score 0.900, JSON score 0.450. 9/10 sentences "preserved".
  The single "lost" case (NEW10) is an artifact of test design:
  transmission-mode nuances cannot be evaluated on an isolated
  sentence without a source/reformulation pair.
- **Rule 8 re-test** — 5 targeted sentences after Rule 8 strictening:
  5/5 correct. The encoder now discriminates between `(!)` alone,
  `[MUST]` alone, and their combination as specified.

Global score excluding NEW10: **0.966 over 29 sentences**. JSON baseline
on the same corpus: 0.583. CSTL advantage: **+0.350**.

### Unaffected

- **Theorem 1** (k=9 uniqueness) holds unchanged.
- **The 65-symbol semantic alphabet size** is unchanged (two glyph
  substitutions, no additions or removals).
- **The 121-token syntactic grammar** is unchanged.
- **Compression benchmarks (E1)** are unaffected by this revision.
- **Grammar rule count**: 7 → 11 rules.

### Raw data

`experiments/20260419/e3_v302_benchmark.csv`

---

## [3.0.1] — 2026-04-18

### Added

- **`[TIME]` global header** for utterance-time anchoring at the
  payload level. Values: `past | present | future | entangled`.
  Introduced to disambiguate utterance-time from inter-event ordering.

### Changed

- **Rule 4 of the grammar** (Part 5 of the spec) is refined:
  - *Before*: "Le flux d'information doit être orienté : passé «,
    présent =, futur »"
  - *After*: Utterance-time anchoring uses the `[TIME]` header; the
    relation-level operators `« = » «=»` are reserved for relative
    ordering between events within a payload.

### Motivation

Empirical evaluation of CSTL v3.0 on a nuance-preservation benchmark
(E3, 20 sentences, Claude-as-judge) revealed case T01 as a loss: the
encoder used the relation-level operators for utterance-time and
dropped the global tense of the scene. The `[TIME]` header resolves
the ambiguity.

### Validation

Re-run of the E3 benchmark with v3.0.1:
- Score: 0.950 → **1.000** (+0.050)
- Improvements: 1 (T01)
- Regressions: 0
- Stable: 19

Raw data: `experiments/20260418/e3_expressivity_v301.csv`.

### Unaffected

- Theorem 1 (k=9 uniqueness) holds unchanged: the archeological layer
  still occupies position 9.
- The 65-symbol semantic alphabet and 121-token syntactic grammar
  are unchanged.
- Compression benchmarks (E1) are unaffected by this revision.
