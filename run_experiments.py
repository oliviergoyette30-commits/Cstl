#!/usr/bin/env python3
"""
CSTL v3 — Experimental Harness (Colab-friendly, v1.1.0)
========================================================

Reproducible evaluation harness for CSTL v3. Produces raw CSV logs and a
summary with mean ± std / 95% CI across seeds and models.

Four experiments:
  1. FIDELITY       — AI-to-AI round-trip fidelity (with JSON baseline)
  2. STSB           — Semantic textual similarity on STS-B dev subsample
  3. COMPRESSION    — CSTL + gzip vs raw NL + gzip
  4. FICTIONAL      — Korthax + Velundra closed-domain fidelity

Usage:
  # From a shell (recommended)
  python3 run_experiments.py --models mock --seeds 1,2,3 --n-payloads 20

  # From a Colab / Jupyter cell, option A (shell escape, cleanest)
  !python3 run_experiments.py --models mock --seeds 1,2,3 --n-payloads 20

  # From a Colab / Jupyter cell, option B (Python call, no argparse)
  from run_experiments import run
  run(models="mock", seeds="1,2,3", n_payloads=20)

Outputs (in ./results/<run_id>/):
  - raw.csv              : one row per (model, seed, payload, protocol) trial
  - summary.csv          : aggregated mean ± std per (model, experiment, protocol)
  - run_metadata.json    : model versions, dates, hyperparameters
  - errors.csv           : failing rows with model output for error analysis

Author: Olivier Goyette
License: see LICENSE
"""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import math
import os
import random
import statistics
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

# -----------------------------------------------------------------------------
# LLM CLIENT ADAPTERS
# -----------------------------------------------------------------------------
# Each adapter exposes .name, .version, and .complete(system, user, seed, temp)
# returning the completion text. Missing SDKs raise at construction time, NOT
# at import time, so a user with only one provider can still run a subset.

class BaseClient:
    name: str = "base"
    version: str = "unknown"

    def complete(self, system: str, user: str, seed: int, temperature: float) -> str:
        raise NotImplementedError


class AnthropicClient(BaseClient):
    name = "claude"

    def __init__(self, model: str = "claude-opus-4-5"):
        try:
            import anthropic  # type: ignore
        except ImportError as e:
            raise RuntimeError("Install `anthropic` to use Claude client") from e
        self._anthropic = anthropic
        self._client = anthropic.Anthropic()
        self.version = model

    def complete(self, system: str, user: str, seed: int, temperature: float) -> str:
        # Anthropic API does not accept a seed directly; we encode the seed
        # into the system prompt so runs with different seeds are non-identical.
        sys_seeded = f"{system}\n\n[trial_seed={seed}]"
        msg = self._client.messages.create(
            model=self.version,
            max_tokens=2048,
            temperature=temperature,
            system=sys_seeded,
            messages=[{"role": "user", "content": user}],
        )
        parts = [b.text for b in msg.content if getattr(b, "type", "") == "text"]
        return "".join(parts).strip()


class OpenAIClient(BaseClient):
    name = "gpt4"

    def __init__(self, model: str = "gpt-4-turbo-2024-04-09"):
        try:
            from openai import OpenAI  # type: ignore
        except ImportError as e:
            raise RuntimeError("Install `openai` to use OpenAI client") from e
        self._client = OpenAI()
        self.version = model

    def complete(self, system: str, user: str, seed: int, temperature: float) -> str:
        resp = self._client.chat.completions.create(
            model=self.version,
            temperature=temperature,
            seed=seed,
            max_tokens=2048,
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        )
        return (resp.choices[0].message.content or "").strip()


class GeminiClient(BaseClient):
    name = "gemini"

    def __init__(self, model: str = "gemini-2.5-pro"):
        try:
            import google.generativeai as genai  # type: ignore
        except ImportError as e:
            raise RuntimeError("Install `google-generativeai` to use Gemini") from e
        api_key = os.environ.get("GOOGLE_API_KEY")
        if not api_key:
            raise RuntimeError("GOOGLE_API_KEY not set")
        genai.configure(api_key=api_key)
        self._genai = genai
        self.version = model
        self._model = genai.GenerativeModel(model)

    def complete(self, system: str, user: str, seed: int, temperature: float) -> str:
        full_prompt = f"{system}\n\n[trial_seed={seed}]\n\n{user}"
        resp = self._model.generate_content(
            full_prompt,
            generation_config={"temperature": temperature, "max_output_tokens": 2048},
        )
        return (resp.text or "").strip()


class MockClient(BaseClient):
    """Deterministic offline client for testing the harness without API calls.

    Implements a degenerate round-trip: returns the input CSTL verbatim with a
    small controllable drift rate so the pipeline can be validated end-to-end.
    """

    name = "mock"

    def __init__(self, drift_rate: float = 0.001):
        self.version = f"mock-drift-{drift_rate}"
        self.drift_rate = drift_rate

    def complete(self, system: str, user: str, seed: int, temperature: float) -> str:
        rng = random.Random(seed)
        # Heuristic: the last contiguous block of non-empty lines is the payload
        lines = [ln for ln in user.splitlines() if ln.strip()]
        payload_lines = []
        for ln in reversed(lines):
            if ln.startswith("---") or ln.startswith("##"):
                break
            payload_lines.insert(0, ln)
        out = []
        for ln in payload_lines:
            if rng.random() < self.drift_rate:
                out.append(ln.replace(" | ", " || "))  # injected drift
            else:
                out.append(ln)
        return "\n".join(out)


# -----------------------------------------------------------------------------
# PAYLOAD GENERATION
# -----------------------------------------------------------------------------

@dataclass
class Payload:
    pid: str
    family: str            # CSTL Layer-1 family this payload exercises
    cstl: str              # CSTL-formatted payload
    json_eq: str           # Equivalent payload in plain JSON (for baseline)
    nl: str                # Natural language description (for compression)

    def checksum(self) -> str:
        return hashlib.sha256(self.cstl.encode("utf-8")).hexdigest()[:12]


def build_fidelity_payloads(n: int = 100, seed: int = 0) -> list[Payload]:
    """Synthetic CSTL payloads spanning the 10 Layer-1 families.

    Symbol choices are restricted to the 37+ empirically testable set to avoid
    the safety-classifier blind spot on negative-affect symbols (paper §6.3).
    """
    rng = random.Random(seed)
    families = {
        "relations":    ["ARR", "BID", "ATT", "CYC"],
        "weight":       ["+", "-", "°"],
        "dynamics":     ["↑", "↓"],
        "time":         ["«", "=", "»"],
        "forces":       ["⊕", "⊖", "ℝ", "κ"],
        "transmission": ["≡", "≠", "∿", "│"],
        "pragmatics":   ["(+)", "(?)", "(!)", "[IF]", "[MUST]", "[MAY]"],
        "network":      ["[NET]", "[TRUST]", "[STATE]", "[PURGE]", "[MERGE]", "[FORK]"],
        "operators":    ["AMP", "INH", "SYN", "ANT"],
        "entities":     ["simple", "meta"],
    }
    fam_list = list(families.keys())
    entities = [f"E{i}" for i in range(1, 21)]

    payloads: list[Payload] = []
    for i in range(n):
        fam = fam_list[i % len(fam_list)]
        op = rng.choice(families[fam])
        a, b = rng.sample(entities, 2)
        conf = round(rng.uniform(0.70, 1.00), 2)
        depth = rng.choice(["surface", "shallow", "deep", "bedrock"])

        cstl_lines = [
            f"# payload_{i:03d}",
            f"[NET]: trial_{i:03d}",
            f"[TRUST][agent_A] = {round(rng.uniform(0.8, 1.0), 2)}",
            f"{a} | {op} | {b} | {conf} | {depth}",
        ]
        json_eq = json.dumps({
            "net": f"trial_{i:03d}",
            "trust": {"agent_A": round(rng.uniform(0.8, 1.0), 2)},
            "relation": {"source": a, "op": op, "target": b,
                         "confidence": conf, "depth": depth},
        }, ensure_ascii=False)
        nl = (f"In trial {i}, we establish a {depth}-level {op} relation from "
              f"{a} to {b} with confidence {conf}.")
        payloads.append(Payload(pid=f"p{i:03d}", family=fam,
                                cstl="\n".join(cstl_lines),
                                json_eq=json_eq, nl=nl))
    return payloads


def build_fictional_domain(name: str, seed: int = 0) -> list[Payload]:
    """Generate Korthax-style or Velundra-style relations over fictional entities."""
    rng = random.Random(seed)
    if name == "korthax":
        ents = ["Thalirion", "Vekmar", "Orothane", "Selindra", "Kharzul",
                "Oborim", "Tessarak", "Valrune", "Mephora", "Zindrel"]
        ops = ["ARR", "BID", "ANT", "AMP"]
    else:  # velundra
        ents = ["Luvenar", "Phyrexil", "Menthaak", "Quorlith", "Xylanor",
                "Beranthys", "Cirthanol", "Undarim", "Therovex", "Perimanth"]
        ops = ["ARR", "CYC", "SYN", "INH"]

    payloads = []
    for i in range(40):
        a, b = rng.sample(ents, 2)
        op = rng.choice(ops)
        conf = round(rng.uniform(0.85, 1.00), 2)
        depth = rng.choice(["deep", "bedrock"])
        cstl = (f"# {name}_rel_{i:02d}\n"
                f"[NET]: {name}\n"
                f"{a} | {op} | {b} | {conf} | {depth}")
        json_eq = json.dumps({"domain": name, "source": a, "op": op,
                              "target": b, "confidence": conf, "depth": depth})
        nl = f"In the {name} domain, {a} has a {op} relationship with {b}."
        payloads.append(Payload(pid=f"{name[:3]}_{i:02d}", family="fictional",
                                cstl=cstl, json_eq=json_eq, nl=nl))
    return payloads


# -----------------------------------------------------------------------------
# SCORING
# -----------------------------------------------------------------------------

def canonicalize(text: str) -> list[str]:
    """Canonical form: non-empty, stripped, non-comment lines, sorted."""
    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
    lines = [ln for ln in lines if not ln.startswith("#")]
    return sorted(lines)


def fidelity_score(original: str, reconstructed: str) -> float:
    """Line-level F1 after canonical ordering."""
    orig = set(canonicalize(original))
    rec = set(canonicalize(reconstructed))
    if not orig and not rec:
        return 1.0
    if not orig or not rec:
        return 0.0
    tp = len(orig & rec)
    precision = tp / len(rec) if rec else 0.0
    recall = tp / len(orig) if orig else 0.0
    if precision + recall == 0:
        return 0.0
    return 2 * precision * recall / (precision + recall)


def extract_code_block(text: str) -> str:
    """If the model wrapped its answer in ```...``` fences, extract the content."""
    if "```" in text:
        segs = text.split("```")
        if len(segs) >= 3:
            inner = segs[1]
            if inner.startswith(("cstl", "json", "txt")):
                inner = inner.split("\n", 1)[1] if "\n" in inner else inner
            return inner.strip()
    return text


# -----------------------------------------------------------------------------
# PROTOCOL DRIVERS
# -----------------------------------------------------------------------------

SYSTEM_CSTL = (
    "You are a CSTL v3 relay agent. CSTL lines have the form:\n"
    "  Source | Operator | Target | confidence | depth\n"
    "where operator ∈ {ARR, BID, AMP, ATT, INH, CYC, SYN, ANT}, confidence ∈ [0,1],\n"
    "depth ∈ {surface, shallow, deep, bedrock}. Section headers [NET], [TRUST], [STATE]\n"
    "scope the payload. You will receive a CSTL payload. Your job: reformulate it into\n"
    "natural language in one sentence per relation, then re-encode it into the SAME CSTL\n"
    "payload. Output ONLY the final CSTL, no prose, no fences."
)

SYSTEM_JSON = (
    "You are a JSON relay agent. You will receive a JSON object representing a relation.\n"
    "Your job: reformulate it into natural language in one sentence, then re-encode it\n"
    "into the SAME JSON structure. Output ONLY the final JSON, no prose, no fences."
)


def run_roundtrip(client: BaseClient, payload: Payload, protocol: str,
                  seed: int, temperature: float) -> tuple[str, str, float]:
    """Execute a round-trip. Returns (model_output, original, fidelity)."""
    if protocol == "cstl":
        system, original = SYSTEM_CSTL, payload.cstl
    elif protocol == "json":
        system, original = SYSTEM_JSON, payload.json_eq
    else:
        raise ValueError(f"unknown protocol: {protocol}")

    try:
        output = client.complete(system, original, seed, temperature)
        output_clean = extract_code_block(output)
        score = fidelity_score(original, output_clean)
    except Exception as e:
        output_clean = f"[ERROR: {type(e).__name__}: {e}]"
        score = 0.0
    return output_clean, original, score


# -----------------------------------------------------------------------------
# EXPERIMENT RUNNERS
# -----------------------------------------------------------------------------

@dataclass
class Trial:
    run_id: str
    experiment: str
    model: str
    model_version: str
    protocol: str
    seed: int
    payload_id: str
    family: str
    original_len: int
    output_len: int
    fidelity: float
    wallclock_s: float
    error: str = ""


def run_fidelity(clients: list[BaseClient], payloads: list[Payload],
                 seeds: list[int], temperature: float, run_id: str,
                 write_row: Callable[[Trial], None]) -> None:
    protocols = ["cstl", "json"]
    total = len(clients) * len(payloads) * len(seeds) * len(protocols)
    done = 0
    for client in clients:
        for protocol in protocols:
            for seed in seeds:
                for p in payloads:
                    t0 = time.time()
                    output, original, score = run_roundtrip(
                        client, p, protocol, seed, temperature)
                    err = ""
                    if output.startswith("[ERROR:"):
                        err = output
                    write_row(Trial(
                        run_id=run_id, experiment="fidelity",
                        model=client.name, model_version=client.version,
                        protocol=protocol, seed=seed, payload_id=p.pid,
                        family=p.family, original_len=len(original),
                        output_len=len(output), fidelity=score,
                        wallclock_s=time.time() - t0, error=err))
                    done += 1
                    if done % 10 == 0:
                        print(f"  fidelity {done}/{total}", file=sys.stderr)


def run_fictional(clients: list[BaseClient], seeds: list[int],
                  temperature: float, run_id: str,
                  write_row: Callable[[Trial], None]) -> None:
    for domain in ["korthax", "velundra"]:
        payloads = build_fictional_domain(domain, seed=42)
        for client in clients:
            for seed in seeds:
                for p in payloads:
                    t0 = time.time()
                    output, original, score = run_roundtrip(
                        client, p, "cstl", seed, temperature)
                    write_row(Trial(
                        run_id=run_id, experiment=f"fictional_{domain}",
                        model=client.name, model_version=client.version,
                        protocol="cstl", seed=seed, payload_id=p.pid,
                        family=domain, original_len=len(original),
                        output_len=len(output), fidelity=score,
                        wallclock_s=time.time() - t0))


def run_compression(payloads: list[Payload], run_id: str,
                    write_row: Callable[[Trial], None]) -> None:
    """Compression benchmark is deterministic: no LLM calls needed."""
    for p in payloads:
        cstl_raw = p.cstl.encode("utf-8")
        nl_raw = p.nl.encode("utf-8")
        cstl_gz = gzip.compress(cstl_raw, compresslevel=9)
        nl_gz = gzip.compress(nl_raw, compresslevel=9)
        # Reduction of CSTL+gzip vs NL+gzip for the same semantic content
        if len(nl_gz) > 0:
            ratio = 1.0 - (len(cstl_gz) / len(nl_gz))
        else:
            ratio = 0.0
        write_row(Trial(
            run_id=run_id, experiment="compression", model="offline",
            model_version="gzip-9", protocol="cstl", seed=0,
            payload_id=p.pid, family=p.family,
            original_len=len(nl_raw), output_len=len(cstl_gz),
            fidelity=ratio, wallclock_s=0.0))


# -----------------------------------------------------------------------------
# AGGREGATION
# -----------------------------------------------------------------------------

def aggregate(raw_path: Path, summary_path: Path) -> None:
    """Compute mean, std, 95% CI grouped by (experiment, model, protocol)."""
    from collections import defaultdict
    groups: dict[tuple, list[float]] = defaultdict(list)
    with raw_path.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            key = (row["experiment"], row["model"], row["protocol"])
            try:
                groups[key].append(float(row["fidelity"]))
            except ValueError:
                pass
    with summary_path.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["experiment", "model", "protocol", "n",
                    "mean", "std", "ci95_lo", "ci95_hi"])
        for (exp, model, proto), scores in sorted(groups.items()):
            n = len(scores)
            if n == 0:
                continue
            m = statistics.fmean(scores)
            s = statistics.stdev(scores) if n > 1 else 0.0
            half = 1.96 * s / math.sqrt(n) if n > 1 else 0.0
            w.writerow([exp, model, proto, n,
                        f"{m:.4f}", f"{s:.4f}",
                        f"{m - half:.4f}", f"{m + half:.4f}"])


# -----------------------------------------------------------------------------
# ENTRY POINTS
# -----------------------------------------------------------------------------

def parse_models(spec: str) -> list[BaseClient]:
    clients: list[BaseClient] = []
    for name in [s.strip() for s in spec.split(",") if s.strip()]:
        if name == "mock":
            clients.append(MockClient())
        elif name == "claude":
            clients.append(AnthropicClient())
        elif name == "gpt4":
            clients.append(OpenAIClient())
        elif name == "gemini":
            clients.append(GeminiClient())
        else:
            raise SystemExit(f"Unknown model: {name}")
    return clients


def run(models: str = "mock",
        seeds: str = "1,2,3,4,5",
        n_payloads: int = 100,
        temperature: float = 0.0,
        experiments: str = "fidelity,fictional,compression",
        out_dir: str = "./results",
        run_id: str | None = None) -> Path:
    """Python entry point — callable from a Colab cell, notebook, or script.

    Returns the Path of the directory where results were written.

    Example (from a Colab cell):
        from run_experiments import run
        out = run(models="mock", seeds="1,2,3", n_payloads=20)
        print("Results in:", out)
    """
    run_id = run_id or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_path = Path(out_dir) / run_id
    out_path.mkdir(parents=True, exist_ok=True)

    clients = parse_models(models)
    seed_list = [int(s) for s in seeds.split(",")]
    exp_list = [e.strip() for e in experiments.split(",")]
    payloads = build_fidelity_payloads(n_payloads, seed=0)

    # Metadata
    meta = {
        "run_id": run_id,
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "models": [{"name": c.name, "version": c.version} for c in clients],
        "seeds": seed_list,
        "n_payloads": n_payloads,
        "temperature": temperature,
        "experiments": exp_list,
        "harness_version": "1.1.0",
        "python": sys.version.split()[0],
    }
    (out_path / "run_metadata.json").write_text(json.dumps(meta, indent=2))

    raw_path = out_path / "raw.csv"
    err_path = out_path / "errors.csv"
    fields = list(Trial.__dataclass_fields__.keys())

    with raw_path.open("w", newline="") as raw_f, \
         err_path.open("w", newline="") as err_f:
        raw_w = csv.DictWriter(raw_f, fieldnames=fields)
        err_w = csv.DictWriter(err_f, fieldnames=fields)
        raw_w.writeheader()
        err_w.writeheader()

        def write_row(t: Trial) -> None:
            raw_w.writerow(asdict(t))
            if t.error:
                err_w.writerow(asdict(t))

        if "fidelity" in exp_list:
            print(f"[{run_id}] fidelity experiment", file=sys.stderr)
            run_fidelity(clients, payloads, seed_list, temperature,
                         run_id, write_row)
        if "fictional" in exp_list:
            print(f"[{run_id}] fictional-domain experiment", file=sys.stderr)
            run_fictional(clients, seed_list, temperature, run_id, write_row)
        if "compression" in exp_list:
            print(f"[{run_id}] compression experiment (offline)", file=sys.stderr)
            run_compression(payloads, run_id, write_row)

    aggregate(raw_path, out_path / "summary.csv")

    meta["ended_utc"] = datetime.now(timezone.utc).isoformat()
    (out_path / "run_metadata.json").write_text(json.dumps(meta, indent=2))
    print(f"\nResults written to {out_path}", file=sys.stderr)
    print("  raw.csv        — one row per trial", file=sys.stderr)
    print("  summary.csv    — aggregated stats", file=sys.stderr)
    print("  errors.csv     — failing trials for analysis", file=sys.stderr)
    print("  run_metadata.json", file=sys.stderr)
    return out_path


def main() -> int:
    ap = argparse.ArgumentParser(description="CSTL experimental harness")
    ap.add_argument("--models", default="mock",
                    help="comma-separated: mock,claude,gpt4,gemini")
    ap.add_argument("--seeds", default="1,2,3,4,5",
                    help="comma-separated seed integers")
    ap.add_argument("--n-payloads", type=int, default=100)
    ap.add_argument("--temperature", type=float, default=0.0)
    ap.add_argument("--experiments", default="fidelity,fictional,compression",
                    help="comma-separated subset")
    ap.add_argument("--out-dir", default="./results")
    ap.add_argument("--run-id", default=None)
    # parse_known_args() ignores extra arguments injected by Jupyter/Colab
    # (e.g. `-f /root/.local/share/jupyter/runtime/kernel-xxx.json`)
    args, unknown = ap.parse_known_args()
    if unknown:
        print(f"[info] ignoring unknown args: {unknown}", file=sys.stderr)

    run(models=args.models,
        seeds=args.seeds,
        n_payloads=args.n_payloads,
        temperature=args.temperature,
        experiments=args.experiments,
        out_dir=args.out_dir,
        run_id=args.run_id)
    return 0


if __name__ == "__main__":
    sys.exit(main())
