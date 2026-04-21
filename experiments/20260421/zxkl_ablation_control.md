# ZXKL Ablation Control — 2026-04-21

## Purpose

This file documents a systematic control experiment designed to answer a
reviewer-anticipated question: **is the observed zero-shot decoding
fidelity of CSTL-ASCII v3.0.6 (20/20 across 3 runs) attributable to the
protocol's specific design, or to a general capability of frontier LLMs
to decode any English-mnemonic notation?**

## Hypothesis

If CSTL's decoding success is a generic LLM property, then an equivalent
pseudo-protocol that replaces every CSTL operator with an arbitrary
English synonym — while preserving the exact syntactic structure —
should decode with comparable fidelity.

If CSTL's decoding success is attributable to specific design choices,
the pseudo-protocol should decode measurably worse.

## Method

We constructed ZXKL (pronounced "zekkel"), a pseudo-protocol isomorphic
to CSTL-ASCII v3.0.6 at the structural level but with every operator
replaced by an unrelated English synonym. All 16 test criteria and the
4 global headers were preserved in semantic content.

### Correspondence table

| CSTL v3.0.6 | ZXKL | Domain of mnemonic |
|-------------|------|---------------------|
| ARR.CREATE | BLURP.FOUND | neutral → opaque prefix |
| ARR (neutral causal) | FIXATE | neutral → fixation semantic |
| INTENT | DESIRE | neutral → affect-loaded |
| ARR (bond) | HUGS | neutral → affect-positive |
| weight=neutral | COOLNESS | systematic → idiomatic |
| EMOTION(x) | feel=(x) | labeled → informal |
| MUTUAL | SHARES | symmetric → unidirectional-leaning |
| PRESSURE | SQUEEZE | physical → physical-different |
| RESIST | PUSHBACK | physical → social |
| CATALYZE | BOOSTS | chemical → informal |
| TRANSFORM | METAMORPH | technical → technical-different |
| [NOT] | [FORBID] | formal → formal |
| [MAY] | [ALLOW] | formal → formal |
| TRANSMIT_FAITHFUL | MIRRORS | explicit → metaphorical |
| TRANSMIT_INFER | GUESSES | explicit → colloquial |
| ARR.CAUSE | BLURP.TRIGGER | neutral → opaque prefix |
| [MUST,FUT] | [MUST,FWD] | standard abbreviation → non-standard |
| TONE(imperative) | TONE(command) | preserved |
| TONE(query) | TONE(ask) | preserved |

The structural syntax (pipes, attributes, composite modalities, numeric
strengths, layer tags) was kept byte-identical to isolate the operator
vocabulary as the only independent variable.

### ZXKL payload (exact text submitted to model)
