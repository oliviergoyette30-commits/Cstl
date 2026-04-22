# Cross-Model Replication — CSTL v3.0.6

## Purpose

This document consolidates 9 zero-shot decoding runs of the CSTL-ASCII v3.0.6 payload across three frontier LLM providers (Google, OpenAI, Anthropic). The goal is to test whether the v3.0.6 result observed initially on Gemini (3 runs × 20/20, see `run_v306_literal.csv`) generalizes across model families and providers.

## Method

Same payload, same prompt, same scoring grid as the three Gemini runs. Each run executed in a fresh empty session with no prior CSTL context. All sessions used the strict literal prompt (numbered output, no synthesis).

The full payload and prompt are reproduced in `v3_0_6_patch_notes.md` and the scoring grid in `fix_validation_summary.md`.

## Results

### Run-by-run summary

| Run | Date | Provider | Model | Interface | Strict | Headers | Total |
|-----|------|----------|-------|-----------|--------|---------|-------|
| 6 | 2026-04-21 | Google | Gemini 2.5 | gemini.google.com | 16/16 | 4/4 | 20/20 |
| 7 | 2026-04-21 | Google | Gemini 2.5 | gemini.google.com | 16/16 | 4/4 | 20/20 |
| 8 | 2026-04-21 | Google | Gemini 2.5 | gemini.google.com | 16/16 | 4/4 | 20/20 |
| 9 | 2026-04-21 | OpenAI | GPT-5.3 | chatgpt.com | 16/16 | 4/4 | 20/20 |
| 10 | 2026-04-21 | OpenAI | GPT-5.3 | chatgpt.com | 16/16 | 4/4 | 20/20 |
| 11 | 2026-04-21 | OpenAI | GPT-5.3 | chatgpt.com | 16/16 | 4/4 | 20/20 |
| 12 | 2026-04-21 | OpenAI | GPT-5.3 | chatgpt.com | 16/16 | 4/4 | 20/20 |
| 13 | 2026-04-21 | OpenAI | GPT-5.3 | chatgpt.com | 16/16 | 4/4 | 20/20 |
| 14 | 2026-04-21 | Anthropic | Claude Opus 4.1 | use.ai (proxy, web search active) | 16/16 | 4/4 | 20/20 |

**Aggregate: 9 runs, all 20/20. Variance σ = 0.**

### By provider

| Provider | Model | Runs | Median | Range | σ |
|----------|-------|------|--------|-------|---|
| Google | Gemini 2.5 | 3 | 20/20 | 20–20 | 0 |
| OpenAI | GPT-5.3 | 5 | 20/20 | 20–20 | 0 |
| Anthropic | Claude Opus 4.1 | 1 | 20/20 | n/a | n/a |
| **All** | **3 frontier providers** | **9** | **20/20** | **20–20** | **0** |

## Stylistic variance across providers

While all three providers achieve identical scores, their decoding styles differ in observable ways:

### Numerical force rendering

- **Gemini 2.5**: qualitative ("relation forte et profonde") with occasional numeric ("0,92")
- **GPT-5.3**: systematically qualitative ("intensité élevée", "intensité modérée", "intensité notable")
- **Claude Opus 4.1**: numerically explicit ("lien de 0,92", "force de 0,95"), the most literal of the three

### TONE pragmatic markers

- **Gemini 2.5**: direct grammatical realization ("Signez l'accord !")
- **GPT-5.3**: meta-description ("Le ton impératif indique que la signature...")
- **Claude Opus 4.1**: direct interrogative form ("Quelles pourraient être les conséquences... ?")

### Layer attribute rendering

- **Gemini 2.5**: "profond / superficiel"
- **GPT-5.3**: "à un niveau profond / superficiel / fondamental"
- **Claude Opus 4.1**: "couche fondamentale / profondément ancrée / ancrage superficiel"

These stylistic differences preserve the full semantic content of all 16 criteria. They suggest that CSTL v3.0.6 is sufficiently underdetermined at the surface level to allow each model to apply its own register preferences, while the protocol's structural conventions ensure semantic equivalence.

## Training-data recognition declarations

In separate sessions (not influencing the decoding runs), each provider was directly asked whether it recognized CSTL from its training data.

### Gemini 2.5 (verbatim, translated from French)
> *"No, I have no trace of a language or protocol officially named 'CSTL' in my training data. It is not a public standard nor a documented programming language. The notation (`fondateur ARR.CREATE...`) — I have never encountered it before. It does not belong to any technical documentation, open-source codebase, or known academic corpus. My understanding is purely deductive and semantic, based on the English mnemonic terms."*

### GPT-5.3 (verbatim, translated from French)
> *"Frankly: no, I do not recognize this exact format as a known standard or widely-used formal language. The elements resemble a mix of conventions inspired by symbolic logic, multi-agent systems, or modeling — but the exact ensemble (with `ARR.CREATE`, `INTENT`, `TRANSMIT_FAITHFUL`, the tags `[MUST,FUT]`, and the attributes `strength=` / `layer=`) does not correspond to any canonical or documented format I would have seen as such. It looks more like a custom or experimental schema."*

### Claude Opus 4.1 via use.ai with active web search (verbatim, translated from French)
> *"No, I have no trace of a protocol called 'COMPRESSED SEMANTIC TRANSFER LANGUAGE' (CSTL) in my training. The search results I just consulted do not show this name either. We find: CSTL (OliverHxh/CSTL on GitHub) — a computer vision research project ('contrastive spatio-temporal learning'); CSTML — Concrete Syntax Tree Markup Language; CSTLM — Compressed Suffix Tree Language Model; C-STS — Conditional Semantic Textual Similarity. None of these projects corresponds to a 'Compressed Semantic Transfer Language'. If this term exists, it is either very recent or used in a restricted circle that I have not covered."*

**All three providers, asked independently, declared no recognition of the protocol.** Claude additionally consulted real-time web search and found four unrelated CSTL/CSTML/CSTLM/C-STS projects, but not the one in this repository.

## Methodological caveats

1. **LLM self-reports on training data are not probative.** Models lack reliable introspection on their training corpus. The three concordant negative declarations are reassuring but do not constitute proof of non-exposure. The ZXKL ablation control (`zxkl_ablation_control.md`) provides the harder evidence — if the v3.0.6 success were due to training exposure rather than to design properties, ZXKL would have decoded comparably; instead it scored 12/20 versus CSTL's 20/20.

2. **Claude tested via third-party proxy with web search.** The Claude Opus 4.1 run was conducted on use.ai (third-party proxy) with web search apparently active. This is not equivalent to a clean claude.ai API test. The non-recognition declaration is even stronger in this configuration (model + real-time search), but the decoding test should ideally be replicated on direct claude.ai access for full methodological cleanliness.

3. **Imbalanced sample sizes.** 5 GPT-5.3 runs vs 3 Gemini runs vs 1 Claude run. The Claude single run does not establish intra-model variance for that provider. A 3-run replication on direct claude.ai is desirable.

4. **Single payload.** All 9 runs used the same 16-relation payload. Whether CSTL v3.0.6 generalizes to different content domains (medical, legal, scientific) at the same fidelity is untested.

5. **Frontier models only.** Llama, Mistral, DeepSeek, Grok, Qwen are not represented. Cross-model claims should be qualified as applying to closed-source frontier models from the three major US providers.

## Interpretation

**What is established:**
- CSTL-ASCII v3.0.6 decodes at 100% strict fidelity zero-shot on three frontier LLM providers.
- Inter-model variance is 0; inter-provider variance is 0.
- Stylistic variance exists but does not affect semantic preservation.
- All three providers self-declare non-recognition of the protocol.
- The ZXKL ablation rules out the alternative hypothesis that the result is a generic LLM mnemonic-decoding capability.

**What is not established:**
- Generalization to other model families (open-source, smaller models).
- Generalization to other content domains.
- Reproducibility across longer or more complex payloads.
- Long-horizon stability (these are single-turn tests).

## Reproducibility

All 9 runs can be replicated by anyone with access to the three providers (Gemini, ChatGPT, Claude) by submitting the verbatim payload from `v3_0_6_patch_notes.md` in an empty session, then scoring against the 16-criterion grid documented in `fix_validation_summary.md`. Total replication time: approximately 15 minutes for the full cross-model dataset.
