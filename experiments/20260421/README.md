# Experiments — 2026-04-21

## Zero-shot CSTL decoding on frontier LLM (Unicode vs ASCII vs v3.0.6 + ablation control)

This folder documents an 8-run experimental trajectory testing two questions:

1. Can CSTL payloads be decoded by a frontier LLM **without providing the specification**?
2. Is the observed decoding fidelity attributable to CSTL's specific design, or to a general LLM capability to decode any English-mnemonic notation?

## Motivation

The 2026-04-20 statistical validation (Q2) closed two methodological gaps (bootstrap CIs, permutation tests) but left open the core question of the paper: **what is the round-trip fidelity of CSTL in a realistic zero-shot inter-agent scenario?**

If decoding requires shipping the full spec with every payload, the protocol collapses to a bespoke DSL. If decoding works zero-shot, the protocol has genuine emergent interoperability — and its design properties become empirically measurable.

## Files

- `README.md` — this file
- `zxkl_ablation_control.md` — negative control experiment ruling out generic LLM decoding
- `v3_0_6_patch_notes.md` — the three syntactic fixes (to be added)
- `runs_v305_5_sessions.csv` — 5 baseline runs × 16 criteria (to be added)
- `run_v306_literal.csv` — 3 validation runs v3.0.6 (to be added)
- `run_v306_interpretive.md` — open-form decoding of v3.0.6 (to be added)
- `fix_validation_summary.md` — H1-H4 hypothesis validation (to be added)

## Experimental sequence

### Phase 1 — Unicode payload (established negative result)

A 19-relation payload using Unicode glyphs (⊕, κ, ≡, ∿, ⟲, ⟶, ⊃, ℙ) was decoded zero-shot. Score: 12/19 strict preservation, one critical semantic inversion (`κ` catalyze → "freezes"), hallucinations on rare glyphs.

### Phase 2 — ASCII payload, 5-run stability (CSTL v3.0.5)

The same relations re-encoded using ASCII mnemonics (PRESSURE, CATALYZE, TRANSMIT_FAITHFUL, TRANSMIT_INFER, TRANSFORM, INTENT, EMOTION, MUTUAL, RESIST, PAST/NOW/FUT, etc.). Five independent zero-shot sessions of the same model:

| Run | Strict score | Headers | Total |
|-----|-------------|---------|-------|
| 1 | 14/16 | 2/4 | 16/20 (80%) |
| 2 | 13/16 | 2/4 | 15/20 (75%) |
| 3 | 14/16 | 2/4 | 16/20 (80%) |
| 4 | 13/16 | 2/4 | 15/20 (75%) |
| 5 | 15/16 | 2/4 | 17/20 (85%) |

Median: 14/16 (87.5%). Range: 13–15/16. σ ≈ 0.84. **Zero semantic inversions across 80 decoded relations.**

### Phase 3 — Failure-mode analysis (5 runs pooled)

13 of 16 criteria stable at 5/5. Three systematic failures identified:

- **FUT absorbed by [MUST]** — future aspect dropped 5/5
- **Anonymous force values ignored** — strength rendered only 1/5, qualitatively
- **ARR ambiguous on animate subjects** — "arrived at" 2/5 vs "created" 3/5

### Phase 4 — Syntactic fixes (CSTL v3.0.6)

Three minimal changes, no alphabet extension, no spec dependency:

- Fix A: composite modality `[MUST,FUT]` in place of prefix `FUT:`
- Fix B: labeled attributes `strength=0.92 | layer=deep`
- Fix C: ARR subtypes `ARR.CREATE`, `ARR.CAUSE`, `ARR.JOIN`

Plus header renaming: `session_id =`, `X has_trust Y`, `global_time =`.

### Phase 5 — Validation (3 independent runs of v3.0.6)

| Run | Strict score | Headers | Total |
|-----|-------------|---------|-------|
| 6 | 16/16 | 4/4 | 20/20 (100%) |
| 7 | 16/16 | 4/4 | 20/20 (100%) |
| 8 | 16/16 | 4/4 | 20/20 (100%) |

Median: 20/20. σ = 0. Zero variance across replications.

### Phase 6 — Ablation control (ZXKL)

Pseudo-protocol isomorphic to CSTL v3.0.6 at the structural level, with every operator replaced by an unrelated English synonym (ARR.CREATE → BLURP.FOUND, PRESSURE → SQUEEZE, CATALYZE → BOOSTS, etc.). Same decoder, same session conditions, same 16-criterion grid.

Result: **12/20 (60%)** vs v3.0.6's **20/20 (100%)**. The 40-point gap rules out the hypothesis that frontier LLMs decode any English-mnemonic notation with equivalent fidelity. Four specific CSTL design properties survive the ablation — see `zxkl_ablation_control.md` for details.

## Headline results

| Configuration | Total fidelity | Runs | σ |
|---------------|---------------|------|---|
| Unicode (v3.0.4) | 63% | 1 | n/a |
| ASCII v3.0.5 | 80% (median) | 5 | 0.84 |
| **ASCII v3.0.6** | **100%** | 3 | **0** |
| ZXKL ablation | 60% | 1 | n/a |

## Honest limitations

1. **Single model.** All eight sessions used the same frontier LLM. Cross-model validation (Claude, GPT, Mistral) is pending.
2. **Single payload family.** 16 relations over 7 families is coverage-adequate but not exhaustive. Untested: ⟳ vs ⟲ distinction, multi-agent transitive chains, purge/merge/fork network primitives, full ψ-layer.
3. **Self-designed rubric.** The 16 criteria were selected by the author. External rubric validation is future work.
4. **Model self-declared absence of training exposure** to CSTL (separate session, documented). This is reassuring but not probative — LLMs lack reliable introspection on their training data.

## Reproducibility

All eight sessions ran through web interfaces with no prior CSTL context loaded, no API, no temperature control, no seed. Payloads and prompts are provided verbatim in the text files of this folder. Anyone with frontier LLM access can replicate within 15 minutes.
