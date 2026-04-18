#!/usr/bin/env python3
"""
E3 — Expressivity: CSTL v3.0.1 vs flat JSON on pragmatic-nuance benchmark
=========================================================================

Tests whether CSTL's dedicated primitives (temporality, modality, forces,
psi-family, trust) preserve semantic nuances that flat JSON drops without
a custom schema.

Protocol:
  1. 20 English sentences, each containing one target nuance.
  2. Claude encodes each sentence in CSTL, then in flat JSON.
  3. Claude judges each encoding: preserved | partial | lost.
  4. Aggregate score and produce a comparison table.

Requires: ANTHROPIC_API_KEY environment variable.
Cost: ~$0.40.
Duration: ~5 minutes.

Limitations documented in the paper:
  - Single-judge: Claude-as-judge introduces self-consistency bias.
  - Benchmark is curated to exercise CSTL primitives; not an adversarial test.
  - n=20 sentences — confidence intervals are wide, treat as indicative.

Author: Olivier Goyette
License: MIT
"""

from __future__ import annotations

import argparse
import json as _json
import os
import sys
import time
from pathlib import Path

import pandas as pd


NUANCE_BANK = [
    # Temporal
    {"id": "T01", "nuance": "temporal_past",
     "sentence": "Marie hesitated about the marriage when her mother fell ill."},
    {"id": "T02", "nuance": "temporal_future_uncertain",
     "sentence": "If the funding comes through, the project will launch next year."},
    {"id": "T03", "nuance": "temporal_entanglement",
     "sentence": "His past and his future both weigh on the decision he makes now."},
    {"id": "T04", "nuance": "temporal_ongoing",
     "sentence": "The negotiation has been dragging on for three years already."},
    # Graded confidence
    {"id": "C01", "nuance": "graded_confidence",
     "sentence": "He is probably but not certainly the author of the anonymous letter."},
    {"id": "C02", "nuance": "low_confidence_assertion",
     "sentence": "Some rumors suggest, though nothing is confirmed, that he resigned."},
    {"id": "C03", "nuance": "certainty_stack",
     "sentence": "We are virtually certain about A, less sure about B, and skeptical of C."},
    # Deontic modality
    {"id": "M01", "nuance": "obligation_vs_permission",
     "sentence": "Doctors must disclose side effects; patients may decline treatment."},
    {"id": "M02", "nuance": "prohibition",
     "sentence": "Employees are not permitted to discuss ongoing investigations externally."},
    {"id": "M03", "nuance": "conditional_obligation",
     "sentence": "If the motion passes, the committee is required to act within 30 days."},
    # Dynamic forces
    {"id": "F01", "nuance": "pressure_toward_rupture",
     "sentence": "Tensions between the two factions keep building toward a breaking point."},
    {"id": "F02", "nuance": "resistance",
     "sentence": "The old guard resists every reform proposal the new director puts forward."},
    {"id": "F03", "nuance": "catalysis",
     "sentence": "The scandal accelerated changes that were going to happen anyway."},
    # Psi-family pragmatics
    {"id": "P01", "nuance": "performative",
     "sentence": "I hereby resign from my position effective immediately."},
    {"id": "P02", "nuance": "intention",
     "sentence": "She intends to finish the paper before the conference deadline."},
    {"id": "P03", "nuance": "emotional_modulation",
     "sentence": "He said yes, but with visible reluctance in his voice."},
    # Multi-agent trust
    {"id": "N01", "nuance": "asymmetric_trust",
     "sentence": "Alice trusts Bob completely but remains wary of Carol."},
    {"id": "N02", "nuance": "trust_degradation",
     "sentence": "The partnership started on solid trust, but recent events have eroded it."},
    # Combined
    {"id": "X01", "nuance": "modal+temporal+trust",
     "sentence": "If the board agrees, Alice must report to Bob by Friday; she trusts him to keep it confidential."},
    {"id": "X02", "nuance": "force+confidence+modal",
     "sentence": "The evidence strongly suggests fraud, though not conclusively, so the auditor may but is not required to investigate further."},
]


# ----- Prompts -----
SYS_ENCODE_CSTL_V301 = """You are an encoder from English to CSTL v3.0.1.

CSTL v3.0.1 primitives:

GLOBAL HEADERS (scope the whole payload):
- [NET]: <session_id>
- [TRUST][<agent>] = <0..1>
- [STATE]: <state_name>
- [TIME]: past | present | future | entangled
  New in v3.0.1. Marks the UTTERANCE-TIME anchor of the entire scene.
  Use this when the sentence has a clear tense/temporal perspective.

RELATION-LEVEL OPERATORS (between two entities within a line):
- Temporal (RELATIVE order between events): « (precedes), = (simultaneous),
  » (follows), «=» (entangled across time)
  These mark ordering BETWEEN events, NOT the global tense.
- Modality: [IF], [MUST], [MAY], [NOT]
- Forces: ⊕ ⊖ ℝ ℜ κ (pressure, resistance, resonance, rupture, catalysis)
- Pragmatics ψ: ⟶ ~̃ ⊃ Δ ℙ (intention, emotion, ascendance, deixis, performative)
- Operators: ARR AMP ATT INH CYC BID SYN ANT
- Weight: + - °
- Confidence in [0,1], depth in {surface, shallow, deep, bedrock}

Line format: Source | Operator | Target | confidence | depth

Rules:
1. Always emit a [TIME] header if the sentence has a clear tense.
2. Use « = » » AT THE RELATION LEVEL only for relative ordering between events.
3. One line per relation.

Output ONLY the CSTL, no prose, no fences."""

SYS_ENCODE_JSON = """You are an encoder from English to flat JSON.

Encode the sentence as a JSON object with fields that capture its content.
You may invent field names — there is no pre-defined schema.
Keep it a single JSON object (flat, no deep nesting).
Output ONLY the JSON, no prose, no fences."""

SYS_JUDGE = """You are an expert evaluator of semantic encodings.

You will receive an original English sentence with an identified nuance, plus two
encodings: one in CSTL (a relational protocol) and one in flat JSON.

For each encoding, decide whether the specified nuance is PRESERVED, PARTIAL,
or LOST in the encoding. Be strict: "preserved" means a downstream agent reading
ONLY the encoding (without the original sentence) could recover the nuance with
reasonable confidence.

Output format (strict JSON, no prose):
{"cstl": "preserved"|"partial"|"lost",
 "json": "preserved"|"partial"|"lost",
 "justification": "one short sentence"}"""


def strip_fences(text: str) -> str:
    if "```" in text:
        parts = text.split("```")
        if len(parts) >= 3:
            inner = parts[1]
            for prefix in ("json", "cstl", "txt"):
                if inner.startswith(prefix):
                    inner = inner.split("\n", 1)[1] if "\n" in inner else ""
            return inner.strip()
    return text


def make_client(model: str):
    try:
        import anthropic
    except ImportError:
        sys.exit("Install the Anthropic SDK: pip install anthropic")
    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("Set ANTHROPIC_API_KEY before running.")
    client = anthropic.Anthropic()

    def call(system: str, user: str, temperature: float = 0.0) -> str:
        msg = client.messages.create(
            model=model, max_tokens=2048, temperature=temperature,
            system=system, messages=[{"role": "user", "content": user}])
        return "".join(b.text for b in msg.content
                       if getattr(b, "type", "") == "text").strip()
    return call


def run(model: str, out_dir: Path) -> pd.DataFrame:
    call = make_client(model)
    results = []
    n = len(NUANCE_BANK)

    for i, item in enumerate(NUANCE_BANK, 1):
        sid, nuance, sentence = item["id"], item["nuance"], item["sentence"]
        print(f"[{i:2d}/{n}] {sid} ({nuance})")

        cstl_enc = strip_fences(call(SYS_ENCODE_CSTL_V301, sentence))
        json_enc = strip_fences(call(SYS_ENCODE_JSON, sentence))

        judge_prompt = (f'Original sentence: "{sentence}"\n'
                        f'Target nuance: {nuance}\n\n'
                        f'--- CSTL encoding ---\n{cstl_enc}\n\n'
                        f'--- JSON encoding ---\n{json_enc}')
        judge_raw = call(SYS_JUDGE, judge_prompt)
        judge_clean = strip_fences(judge_raw)

        try:
            judgment = _json.loads(judge_clean)
        except Exception as e:
            judgment = {"cstl": "error", "json": "error",
                        "justification": f"parse_error: {e}"}

        results.append({
            "id": sid, "nuance": nuance, "sentence": sentence,
            "cstl": cstl_enc, "json": json_enc,
            "cstl_verdict": judgment.get("cstl"),
            "json_verdict": judgment.get("json"),
            "justification": judgment.get("justification", ""),
        })
        time.sleep(0.2)

    df = pd.DataFrame(results)
    csv_path = out_dir / "e3_expressivity_v301.csv"
    df.to_csv(csv_path, index=False)
    print(f"\n✓ Results: {csv_path}")
    return df


def summarize(df: pd.DataFrame) -> None:
    print("\n" + "=" * 72)
    print("E3 — Expressivity (single-judge Claude)")
    print("=" * 72)
    print("\nCSTL :", df["cstl_verdict"].value_counts().to_dict())
    print("JSON :", df["json_verdict"].value_counts().to_dict())

    score_map = {"preserved": 1.0, "partial": 0.5, "lost": 0.0}
    df["cstl_score"] = df["cstl_verdict"].map(score_map)
    df["json_score"] = df["json_verdict"].map(score_map)

    print(f"\nCSTL mean : {df['cstl_score'].mean():.3f}  "
          f"(n valid: {df['cstl_score'].notna().sum()}/{len(df)})")
    print(f"JSON mean : {df['json_score'].mean():.3f}  "
          f"(n valid: {df['json_score'].notna().sum()}/{len(df)})")

    cstl_wins = (df["cstl_score"] > df["json_score"]).sum()
    json_wins = (df["json_score"] > df["cstl_score"]).sum()
    ties = (df["cstl_score"] == df["json_score"]).sum()
    print(f"\nCSTL > JSON : {cstl_wins}")
    print(f"JSON > CSTL : {json_wins}")
    print(f"Ties        : {ties}")


def main() -> int:
    ap = argparse.ArgumentParser(description="E3 expressivity benchmark")
    ap.add_argument("--model", default="claude-opus-4-5")
    ap.add_argument("--out-dir", default="./results/e3")
    args, _ = ap.parse_known_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    df = run(args.model, out_dir)
    summarize(df)
    return 0


if __name__ == "__main__":
    sys.exit(main())
