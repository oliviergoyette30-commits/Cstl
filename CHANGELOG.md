# Changelog

All notable changes to CSTL will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to semantic versioning principles.

---

## [v4.0] — 2026-04-27

### Added
- `INTENT_PAYLOAD` block: global meta-intent of the message (sender, receiver, purpose, priority)
- `META` block: encoder metadata (PAYLOAD_CONFIDENCE, ENCODED_BY, ENCODING_TIMESTAMP)
- `CONSTRAINTS` block: deontic obligations placed before relations to avoid premature inference bias
- `UNCERTAINTY` block: explicit epistemic markers (UNKNOWN, ESTIMATED, INFERRED)
- Compact symbol notation: `σ` (strength), `δ` (layer), `τ` (time), `ω` (weight), `ι` (id)
- 18 domain ontologies (`cstl_domains.py`): diplomatic, legal, medical, corporate, archaeological, astronomical, financial, cybersecurity, regulatory, supply_chain, HR, research, marketing, real_estate, insurance, education, journalism, energy
- Reference parser in Python (`cstl_parser.py`) with full v4.0 support
- Cross-LLM validation matrix: Claude / Gemini / ChatGPT (encoding and decoding)
- 9 distinct empirical test protocols (212/214 = 99.1% success rate)
- Comparative analysis: CSTL vs JSON Schema, CSTL vs G²CP, literature review

### Changed
- Symbol `λ` (originally for layer) replaced with `δ` to avoid ambiguity with "Language/Logic" interpretation
- `δ=f` (originally for surface) replaced with `δ=su` to avoid collision with `τ=f` (future)
- Block ordering: header → SYMBOLS → INTENT_PAYLOAD → META → CONSTRAINTS → UNCERTAINTY → DEFINE → RELATIONS → END

### Fixed
- Modality `[MOD]` mid-line detection in parser
- Unicode minus `−` recognition in weight attribute
- Duplicate attributes in DEFINE
- Domain operators incorrectly generating warnings

---

## [v3.0.9] — 2026-04-22

### Added
- Trailer instructions to eliminate decoder summary bias
- Phase 2c architecture (DEFINE in shared context, relations only in messages)

### Changed
- Improved bidirectional fidelity (ChatGPT↔Gemini)

### Fixed
- Decoder reproducibility on first attempt for both directions

### Empirical results
- Test 7 (Toledo archaeological): 100% on first try
- Test 8 (Paranal astronomical): 90% on first try
- Test 10 (Toledo with shared ontology): 100% with +47% compression vs natural language

---

## [v3.0.8] — 2026-04-15

### Added
- Operators: TRANSMIT_FAITHFUL, TRANSMIT_INFER for differentiating faithful vs inferential transmission
- Improved layer semantics (bedrock, deep, shallow, surface)

### Empirical results
- Test 1 (FamCorp v3.0.7): 95%
- Test 2 (FamCorp v3.0.8): 100%
- Test 3 (Marc medical): 91%
- Test 4 (Sophie legal): 100%
- Test 5 (Sommet diplomatic): 96%

---

## [v3.0.7] — 2026-04-08

### Added
- Initial DEFINE/RELATIONS architecture
- Modalities: [MUST], [NOT], [MAY], [IF...THEN]
- Numerical strength attributes (0.0—1.0)
- Three temporality markers (past, present, future)

---

## [v3.0.4] — 2026-04-01

### Added
- Initial CSTL specification
- Phase 1 (self-contained) and Phase 2 (shared signature) architectures

---

## Future versions

### [v4.x] — planned
- Refinements to v4.0 grammar based on empirical feedback
- Extended domain ontologies (medical sub-specialties, financial instruments)
- Performance optimizations to parser

### [v5.0 / CASTLE] — long-term
- CASTLE mode (network-shared dictionary)
- Dictionary establishment protocol
- Indexed reference syntax
- ~53% per-message compression target

---

## Versioning policy

- **Major version** (v4 → v5): breaking grammar changes
- **Minor version** (v4.0 → v4.1): backward-compatible additions
- **Patch version** (v4.0.0 → v4.0.1): bug fixes in reference parser

The grammar specification (SPEC_vN.md) is versioned independently from the reference parser implementation.
