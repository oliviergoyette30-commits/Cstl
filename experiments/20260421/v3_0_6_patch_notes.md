# CSTL v3.0.6 Patch Notes — 2026-04-21

## Summary

Three minimal syntactic fixes to the CSTL-ASCII profile, motivated by a 5-run stability analysis on v3.0.5 (see `runs_v305_5_sessions.csv`). No alphabet extension, no semantic change, no specification dependency.

Impact: total zero-shot decoding fidelity rises from median 16/20 (v3.0.5, σ≈0.84) to 20/20 (v3.0.6, σ=0, n=3), with a 40-point gap to the ZXKL ablation control (see `zxkl_ablation_control.md`).

## Context

CSTL v3.0.5 introduced the ASCII profile as a zero-shot-decodable variant of the Unicode specification. Five independent sessions of the same frontier LLM decoded a 16-relation payload covering 7 semantic families at median 14/16 strict + 2/4 headers = 16/20 (80%), with zero semantic inversions but three systematic (non-random) failures.

## Observed failures in v3.0.5

### Failure A — FUT absorbed by following [MUST]

**Example:** `FUT: conseil [MUST] ARR decision_vote`
**Observed decoding (5/5 runs):** "doit impérativement parvenir à un vote" (present tense of obligation)
**Expected decoding:** "devra impérativement voter" (future tense of obligation)

**Diagnosis:** the `FUT:` prefix is a line-level marker that the decoder parses as a label on the line rather than as a tense modifier on the verb. When `[MUST]` follows, the present tense of modal obligation dominates the future aspect.

### Failure B — Anonymous numerical force values ignored

**Example:** `Alice ARR Bob | 0.92 | deep`
**Observed decoding (4/5 runs):** "Alice est liée à Bob" (force omitted entirely)
**Occasional qualitative rendering (1/5):** "relation forte" (qualitative, not numerical)

**Diagnosis:** the decoder treats columnar values without named labels as formatting metadata (padding, delimiters, parsing hints) rather than as semantic content. The positional convention `| value | value |` does not signal "these are distinct properties of the relation" to a model that has not seen the CSTL specification.

### Failure C — ARR ambiguous on animate subjects

**Example:** `fondateur ARR entreprise_2015`
**Observed decoding:** "a créé/fondé/établi" (3/5 runs), "est arrivé dans" (2/5 runs)

**Diagnosis:** the `ARR` mnemonic has two plausible English roots: **AR**Row (causal pointer) and **ARR**ive (joining/reaching). With inanimate subjects (reactors, pressure, temperature) the causal reading dominates. With animate subjects (humans, agents, founders) the "arrive/join" reading becomes competitive, producing lexical drift.

## Fixes

### Fix A — Composite modalities

Replace line-level tense prefix with in-bracket composition.
