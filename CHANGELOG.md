# Changelog

All notable changes to the CSTL specification and supporting artifacts are documented here.

## [3.0.4] — 2026-04-19

### Changed

- **Rule 12** (Part 5 — Grammar) — Reformulated with **narrow-scope** applicability.

  The v3.0.3 formulation ("selective parsimony — prefer the more parsimonious
  encoding when two are valid") caused **unintended over-generalization** by
  encoders at the prompt level: average encoding length dropped by 42%
  (`⟳` −40%, `⊃` −57%), including on structural glyphs not in scope of Rule 12.
  Ten regressions were documented in the v3.0.2/v3.0.3 comparative benchmark.

  v3.0.4 reformulates Rule 12 with:
  - **Explicit scope restriction** ("This rule applies ONLY to the five Tier B
    glyphs listed. It does NOT restrict any other glyph.").
  - **Encoding principle preamble** ("Produce RICH, STRUCTURAL encodings.
    Do NOT artificially minimize glyph count. Typical length: 60–150 chars.").
  - **Explicit structural-glyph protection** — new subsection listing `⟳`,
    `⊃`, `→`, `↔`, `⊗`, `⟲`, `↑`, `↓`, plus all Time, Modality, Tone, and
    Network glyphs as **not subject to Rule 12**.

  The Tier A / Tier B classification itself is preserved:
  - Tier A (free inference): `~̃`, `⊕`, `⟶`, `κ`, `ℙ`
  - Tier B (explicit marker required): `⊃[]`, `≡`, `≠`, `∿`, `ℝ`

### Unchanged

- 65-symbol semantic alphabet.
- 121-token syntactic grammar.
- Theorem 1 (k=9 uniqueness).
- Rules 1–11.
- Rule 13 (`⟳` vs `⟲` disambiguation).
- All v3.0.3 valid encodings remain valid in v3.0.4. The modification concerns
  the *operational interpretation* of Rule 12 by the encoder only.

### Validation

Benchmark `e3_plus_plus_comparative_v303_v304.csv` — 100 EN/FR parallel pairs,
Claude Opus 4.5 encoder and judge, strict JSON enum baseline, 2026-04-19.

- **CSTL v3.0.4 EN: 0.975** (vs v3.0.3: 0.955)
- **CSTL v3.0.4 FR: 0.940** (vs v3.0.3: 0.835)
- JSON-strict EN: 0.855
- JSON-strict FR: 0.780
- **CSTL advantage: +0.120 EN / +0.160 FR**
- **Cross-lingual invariance: 91%** (91/100 pairs have same verdict in EN and FR)
- CSTL v3.0.4 wins 35 pairs; JSON-strict wins 5; 60 ties. **Ratio 7:1**.

All ten v3.0.3 regressions corrected. The four v3.0.3 target cases (I03, E05,
K11, C03) remain resolved in v3.0.4.

### Verdict distribution (100 pairs)

| Encoding        | Preserved | Partial | Lost |
|-----------------|----------:|--------:|-----:|
| CSTL v3.0.4 EN  | 96        | 3       | 1    |
| CSTL v3.0.4 FR  | 91        | 6       | 3    |
| JSON-strict EN  | 74        | 23      | 3    |
| JSON-strict FR  | 66        | 24      | 10   |

### Per-family analysis

CSTL v3.0.4 outperforms JSON-strict on 14 of 15 semantic families.

Strong dominance (Δ ≥ 0.20 on at least one language):

| Family                       | Δ EN   | Δ FR   |
|------------------------------|-------:|-------:|
| B — Temporal ordering        | +0.417 | +0.250 |
| K — Complex combinations     | +0.308 | +0.385 |
| M — Cross-lingual invariance | +0.250 | +0.375 |
| A — Global headers           | +0.167 | +0.083 |
| O — Transitive propagation   | +0.167 | +0.167 |
| F — ψ layer (consciousness)  | +0.150 | +0.100 |

Single documented weakness:

| Family                   | Δ EN   | Δ FR   |
|--------------------------|-------:|-------:|
| H — Pragmatic tones      | −0.125 | 0.000  |

Cause identified: encoding of intensity markers ("entirely", "complètement")
not currently supported. Candidate for Rule 14 (intensity markers) in v3.0.5.

### Raw data

`experiments/20260419/e3_plus_plus_comparative_v303_v304.csv` — final benchmark.
`experiments/20260419/e3_plus_plus_comparative_v302_v303.csv` — intermediate (v3.0.3 regressions).

---

## [3.0.3] — 2026-04-19 (superseded by v3.0.4)

### Added

- **Rule 12** (Part 5 — Grammar) — Selective parsimony with Tier A / Tier B distinction.

  Glyphs are split into two tiers based on the inference latitude permitted to
  encoders:

  - **Tier A — Semantic inference authorized**: `~̃`, `⊕`, `⟶`, `κ`, `ℙ` may be
    used without explicit linguistic markers when the overall meaning supports
    their usage.
  - **Tier B — Explicit marker required**: `⊃[]`, `≡`, `≠`, `∿`, `ℝ` must only
    be used when a direct linguistic indicator supports the glyph.

  **Note (2026-04-19, superseded by v3.0.4)**: The original v3.0.3 formulation
  of Rule 12 included the phrase "When two encodings are valid, prefer the more
  parsimonious one." Empirical testing revealed this phrase caused
  over-generalization by encoders (42% global length reduction, affecting
  structural glyphs). v3.0.4 reformulates the rule with strict narrow scope.

- **Rule 13** (Part 5 — Grammar) — Operational disambiguation `⟳` vs `⟲`.

  - `⟳` when the outcome is stable and non-reversible at utterance time.
  - `⟲` when the transformation is active and potentially reversible.
  - `⟲ → ⟳` composition when the sentence explicitly describes a transition.

### Measured impact (v3.0.3 alone)

Measured in benchmark `e3_plus_plus_comparative_v302_v303.csv`:

- CSTL v3.0.3 EN: 0.955 (v3.0.2 was 0.960, −0.005)
- CSTL v3.0.3 FR: 0.835 (v3.0.2 was 0.825, +0.010)
- 28 improvements / 10 regressions

Regressions traced to over-generalization of the parsimony principle beyond
Tier B glyphs. Addressed in v3.0.4.

### Raw data

`experiments/20260419/e3_plus_plus_comparative_v302_v303.csv`

---

## [3.0.2] — 2026-04-19

### Added

- **Rule 8** (Part 5 — Grammar) — Performative tone `(!)` vs Modal obligation `[MUST]`.
- **Rule 9** (Part 5 — Grammar) — Mutual relation `↔` vs Resonance `ℝ`.
- **Rule 10** (Part 5 — Grammar) — Direct grip `⊃` vs Meta-control `⊃[]`.
- **Rule 11** (Part 5 — Grammar) — Transmission modes `≡` / `≠` / `∿`.

### Changed

- **Part 2.4 — Dynamics**: glyph for "transformation in progress" changes from
  `⟳` to `⟲`. Disambiguates from `⟳` in Part 2.2 (transformation achieved).
- **Part 2.8 — Transmission Modes**: glyph for "archeological" mode changes
  from `«` to `⇐`. Disambiguates from `«` in Part 2.5 (temporal past).

### Motivation

Internal audit of v3.0.1 identified six disambiguation zones. All resolved
through glyph separation (Fixes #1, #2) or added grammar rules (Fixes #3–#6).

### Validation

- **E3 phase 1** (20 baseline): CSTL 0.950, JSON 0.650 — zero regression.
- **E3 phase 2** (10 targeted): CSTL 0.900, JSON 0.450.
- **Global E3+ excluding NEW10**: CSTL 0.966, JSON 0.583. Advantage: +0.350.

### Cross-lingual follow-up (E3++ v2)

`e3_plus_plus_v2_strict_results.csv` (100 pairs EN/FR):

- CSTL v3.0.2 EN: 0.980 / FR: 0.840
- JSON-strict EN: 0.835 / FR: 0.765
- Advantage: +0.145 EN / +0.075 FR
- Cross-lingual invariance: 78%

A prior run with a verbose (free-text) JSON baseline artificially favored
JSON; free-text fields embedded source text verbatim. The strict baseline
corrects this methodological artifact.

### Unaffected

- Theorem 1 (k=9 uniqueness) holds unchanged.
- 65-symbol semantic alphabet — unchanged.
- 121-token syntactic grammar — unchanged.
- Grammar rule count: 7 → 11 rules.

### Raw data

`experiments/20260419/e3_v302_benchmark.csv`
`experiments/20260419/e3_plus_plus_v2_strict_results.csv`

---

## [3.0.1] — 2026-04-18

### Added

- **`[TIME]` global header** for utterance-time anchoring at the payload level.
  Values: `past | present | future | entangled`.

### Changed

- **Rule 4 of the grammar** (Part 5) refined: utterance-time anchoring uses the
  `[TIME]` header; relation-level operators `« = » «=»` are reserved for
  relative ordering between events within a payload.

### Motivation

Empirical evaluation on the E3 benchmark revealed case T01 as a loss: the
encoder used relation-level operators for utterance-time and dropped the
global tense of the scene. The `[TIME]` header resolves the ambiguity.

### Validation

Re-run of the E3 benchmark with v3.0.1:
- Score: 0.950 → **1.000** (+0.050)
- Improvements: 1 (T01) / Regressions: 0 / Stable: 19

Raw data: `experiments/20260418/e3_expressivity_v301.csv`.

### Unaffected

- Theorem 1 (k=9 uniqueness) holds unchanged.
- 65-symbol alphabet and 121-token grammar unchanged.
- Compression benchmarks (E1) unaffected.
