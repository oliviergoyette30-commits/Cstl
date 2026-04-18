# CSTL v3 — Experimental Setup

This document fixes the experimental conditions for all results reported in the CSTL v3 paper. Any future run must match these settings to be comparable.

## Models

| Label | Provider | Exact model string | Notes |
|---|---|---|---|
| `claude` | Anthropic | `claude-opus-4-5` | Frontier model, April 2026 |
| `gpt4` | OpenAI | `gpt-4-turbo-2024-04-09` | Stable checkpoint, not the rolling `gpt-4-turbo` alias |
| `gemini` | Google | `gemini-2.5-pro` | Verify exact version at run time; Google's naming changes frequently |

> **Olivier**: replace the exact model strings above with the ones you actually used for the runs you plan to report. The harness records whatever it connects to in `run_metadata.json`, so there is no guessing after the fact.

## Hyperparameters

| Parameter | Value | Rationale |
|---|---|---|
| `temperature` | `0.0` | Minimize sampling noise; stochastic variation is measured separately by re-running with `temperature=0.7` |
| `max_tokens` | `2048` | Large enough for the longest observed round-trip output |
| `top_p` | provider default | Not overridden; we rely on temperature for stochasticity |
| `seed` | `{1, 2, 3, 4, 5}` | Five seeds per condition for variance estimation |

## Payload design

- **Fidelity experiment**: 100 synthetic payloads sampling the 10 Layer-1 families uniformly, with symbols restricted to the 37-symbol empirically-testable subset.
- **Fictional-domain experiments**: 40 relations each in Korthax (geopolitical, operators ARR/BID/ANT/AMP) and Velundra (biological, operators ARR/CYC/SYN/INH). Entity names are generated to have zero web presence.
- **Baseline**: every fidelity payload has a matching JSON representation (same relational content, different syntax) so that CSTL and JSON round-trips are measured on identical semantic content.

## Fidelity metric

We report **line-level F1 after canonical ordering**. Canonical form: strip whitespace, drop comment lines (`#...`) and empty lines, sort. F1 = 2·P·R/(P+R) over the symmetric difference of line sets. An exact match scores 1.0; a completely different output scores 0.0.

Alternative metrics we considered and rejected:
- *Exact string match*: too strict, penalizes trivial reordering.
- *BLEU/ROUGE*: designed for natural language, reward partial token overlap in a way that rewards hallucinated-but-similar output.
- *Edit distance*: continuous, but hard to interpret on multi-line structured payloads.

## Compression metric

Size reduction ratio = `1 - len(gzip(cstl)) / len(gzip(nl))` where `nl` is the natural-language description of the same semantic content. Positive = CSTL is smaller. Ratios are sensitive to payload length; we report the distribution, not just the mean.

## Run schedule

| Phase | Models | Seeds | Experiments | Estimated trials |
|---|---|---|---|---|
| Pilot | `mock` | 1-3 | all | ~400 (offline) |
| Production | `claude, gpt4, gemini` | 1-5 | fidelity, fictional | ~3 900 LLM calls |
| Stochastic re-run | `claude, gpt4, gemini` | 1-5 at temp=0.7 | fidelity only | ~3 000 LLM calls |

## Access dates

Experiments to be conducted: **[fill in when run]**.
First run: `[YYYY-MM-DD]`.
Last run: `[YYYY-MM-DD]`.

This matters because LLM behavior drifts over time without public notice. The paper must cite the exact dates.

## Environment

- Python `3.10+`
- Harness version: see `run_metadata.json` for each run
- OS: any POSIX-like (the harness has no OS-specific code)
- Hardware: inference is remote, local compute negligible

## What is NOT controlled

Honest disclosure of uncontrolled variables:

1. **Provider-side routing**: Anthropic/OpenAI/Google may route the same model name to different inference clusters. We cannot detect or control this.
2. **Silent model updates**: `gpt-4-turbo-2024-04-09` is a dated checkpoint and should be stable; `gemini-2.5-pro` may change. `run_metadata.json` records what the API reported at call time.
3. **Safety-classifier updates**: the ψ-family blind spot (paper §6.3) may change as providers tune their classifiers. Re-running on the same prompts months later may yield different refusal rates.
4. **Tokenization differences across providers**: the compression ratio comparison against gzip is independent of provider tokenization, but the per-call cost is not.

These are documented limitations, not bugs. A v1.1 of the harness may add proxied calls through a single inference gateway to reduce variability.
