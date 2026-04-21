# CSTL v3.0.6 — Open-form decoding observation

## Purpose

Beyond the strict literal decoding validated in the three runs of `run_v306_literal.csv` (20/20 each), we tested how the same payload behaves under an open-ended prompt that does not constrain the output format. This tests whether CSTL v3.0.6 supports not only faithful transmission but also higher-order semantic synthesis.

## Method

Same payload as the three literal runs. Same frontier LLM. New empty session. Prompt changed from structured ("one phrase per line, no synthesis") to open ("decode this in French, structure as you see fit").

## Model response (verbatim)

The model produced a thematic analysis organized into seven semantic domains (emojis preserved as received):
