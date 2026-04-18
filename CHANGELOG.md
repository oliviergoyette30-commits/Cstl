# Changelog

All notable changes to the CSTL specification and tooling are documented here.

## [3.0.1] — 2026-04-18

### Added
- **`[TIME]` global header** for utterance-time anchoring at the payload level.
  Values: `past | present | future | entangled`.
  Introduced to disambiguate utterance-time from inter-event ordering.

### Changed
- **Rule 4 of the grammar** (Partie 5 of the spec) is refined:
  - *Before*: "Le flux d'information doit être orienté : passé «, présent =, futur »"
  - *After*: Utterance-time anchoring uses the `[TIME]` header; the
    relation-level operators `« = » «=»` are reserved for relative ordering
    between events within a payload.

### Motivation
Empirical evaluation of CSTL v3.0 on a nuance-preservation benchmark (E3,
20 sentences, Claude-as-judge) revealed case T01 as a loss: the encoder used
the relation-level operators for utterance-time and dropped the global tense
of the scene. The `[TIME]` header resolves the ambiguity.

### Validation
Re-run of the E3 benchmark with v3.0.1:
- Score: 0.950 → **1.000** (+0.050)
- Improvements: 1 (T01)
- Regressions: 0
- Stable: 19

Raw data: `experiments/20260418/e3_expressivity_v301.csv`.

### Unaffected
- Theorem 1 (k=9 uniqueness) holds unchanged: the archeological layer still
  occupies position 9.
- The 65-symbol semantic alphabet and 121-token syntactic grammar are
  unchanged.
- Compression benchmarks (E1) are unaffected by this revision.

---

## [3.0.0] — 2026

Initial unified specification.
- Layer 1: 65 semantic symbols organized in 10 families.
- Layer 2: 121 syntactic tokens organized in 8 symmetry groups.
- Layer 3: k=9 DNA transport with address space 121⁹ ≈ 5.56 × 10¹⁸.
- 8 foundational axioms, 7 grammar rules, 6 semantic levels, 5 transmission modes.
