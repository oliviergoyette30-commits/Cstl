#!/usr/bin/env python3
"""
E1 — Compression benchmark for CSTL v3
=======================================

Compares the byte size of CSTL payloads against JSON (compact + pretty) and
YAML equivalents on multi-relation payloads of increasing size. Measures both
raw and gzip-compressed sizes.

No LLM calls. Fully deterministic. Output: CSV + matplotlib figure.

Usage:
    python3 e1_compression.py [--out-dir ./results/e1]

Author: Olivier Goyette
License: MIT (see LICENSE in repo root)
"""

from __future__ import annotations

import argparse
import gzip
import json as _json
import random
from pathlib import Path

import pandas as pd


# -----------------------------------------------------------------------------
# Payload generation
# -----------------------------------------------------------------------------

def gen_relations(n_rels: int, seed: int) -> list[dict]:
    """Generate `n_rels` random CSTL-style relations."""
    rng = random.Random(seed)
    ops = ["ARR", "AMP", "ATT", "INH", "CYC", "BID", "SYN", "ANT"]
    depths = ["surface", "shallow", "deep", "bedrock"]
    ents = [f"E{i}" for i in range(1, 100)]
    return [{
        "source": (r := rng.sample(ents, 2))[0],
        "op": rng.choice(ops),
        "target": r[1],
        "confidence": round(rng.uniform(0.6, 1.0), 2),
        "depth": rng.choice(depths),
    } for _ in range(n_rels)]


def to_cstl(rels: list[dict], session: str) -> str:
    lines = [f"[NET]: {session}", "[TRUST][A] = 0.95"]
    for r in rels:
        lines.append(f'{r["source"]} | {r["op"]} | {r["target"]} | '
                     f'{r["confidence"]} | {r["depth"]}')
    return "\n".join(lines)


def to_json_compact(rels: list[dict], session: str) -> str:
    return _json.dumps({"net": session, "trust": {"A": 0.95}, "relations": rels},
                       separators=(',', ':'))


def to_json_pretty(rels: list[dict], session: str) -> str:
    return _json.dumps({"net": session, "trust": {"A": 0.95}, "relations": rels},
                       indent=2)


def to_yaml(rels: list[dict], session: str) -> str:
    """Minimal YAML emission — no external dependency."""
    lines = [f"net: {session}", "trust:", "  A: 0.95", "relations:"]
    for r in rels:
        lines.append(f"  - source: {r['source']}")
        lines.append(f"    op: {r['op']}")
        lines.append(f"    target: {r['target']}")
        lines.append(f"    confidence: {r['confidence']}")
        lines.append(f"    depth: {r['depth']}")
    return "\n".join(lines)


# -----------------------------------------------------------------------------
# Benchmark
# -----------------------------------------------------------------------------

def run(sizes: list[int], n_trials: int, seed_offset: int = 0) -> pd.DataFrame:
    rows = []
    for n_rels in sizes:
        for trial in range(n_trials):
            rels = gen_relations(n_rels, seed=seed_offset + trial * 100 + n_rels)
            session = f"sess_{n_rels}_{trial}"

            cstl = to_cstl(rels, session)
            jcompact = to_json_compact(rels, session)
            jpretty = to_json_pretty(rels, session)
            yml = to_yaml(rels, session)

            rows.append({
                "n_rels": n_rels, "trial": trial,
                "cstl_bytes": len(cstl.encode('utf-8')),
                "json_compact_bytes": len(jcompact.encode('utf-8')),
                "json_pretty_bytes": len(jpretty.encode('utf-8')),
                "yaml_bytes": len(yml.encode('utf-8')),
                "cstl_gz": len(gzip.compress(cstl.encode('utf-8'))),
                "json_compact_gz": len(gzip.compress(jcompact.encode('utf-8'))),
                "json_pretty_gz": len(gzip.compress(jpretty.encode('utf-8'))),
                "yaml_gz": len(gzip.compress(yml.encode('utf-8'))),
            })
    return pd.DataFrame(rows)


def print_summary(df: pd.DataFrame, sizes: list[int]) -> pd.DataFrame:
    agg = df.groupby('n_rels').agg({
        'cstl_bytes': 'mean', 'json_compact_bytes': 'mean',
        'json_pretty_bytes': 'mean', 'yaml_bytes': 'mean',
        'cstl_gz': 'mean', 'json_compact_gz': 'mean',
        'json_pretty_gz': 'mean', 'yaml_gz': 'mean',
    }).round(0).astype(int)

    print("=" * 72)
    print("E1 — Compression : mean bytes by number of relations")
    print("=" * 72)

    print("\n--- Raw bytes ---")
    print(agg[['cstl_bytes', 'json_compact_bytes', 'json_pretty_bytes', 'yaml_bytes']]
          .rename(columns=lambda c: c.replace('_bytes', '')))

    print("\n--- After gzip ---")
    print(agg[['cstl_gz', 'json_compact_gz', 'json_pretty_gz', 'yaml_gz']]
          .rename(columns=lambda c: c.replace('_gz', '')))

    print("\n--- Ratio vs CSTL (1.0 = same size) ---\n")
    print("    Raw:")
    for n in sizes:
        row = agg.loc[n]
        print(f"  {n:3d} rels  "
              f"JSON-compact: {row['json_compact_bytes']/row['cstl_bytes']:.2f}x   "
              f"JSON-pretty: {row['json_pretty_bytes']/row['cstl_bytes']:.2f}x   "
              f"YAML: {row['yaml_bytes']/row['cstl_bytes']:.2f}x")
    print("\n    After gzip:")
    for n in sizes:
        row = agg.loc[n]
        print(f"  {n:3d} rels  "
              f"JSON-compact: {row['json_compact_gz']/row['cstl_gz']:.2f}x   "
              f"JSON-pretty: {row['json_pretty_gz']/row['cstl_gz']:.2f}x   "
              f"YAML: {row['yaml_gz']/row['cstl_gz']:.2f}x")
    return agg


def save_figure(agg: pd.DataFrame, sizes: list[int], out_path: Path) -> None:
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("[warn] matplotlib not installed — skipping figure")
        return

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 4))

    for col, label, marker in [
        ('cstl_bytes', 'CSTL', 'o'),
        ('json_compact_bytes', 'JSON compact', 's'),
        ('json_pretty_bytes', 'JSON pretty', '^'),
        ('yaml_bytes', 'YAML', 'd'),
    ]:
        ax1.plot(agg.index, agg[col], marker=marker, label=label, linewidth=2)
    ax1.set_xlabel('Number of relations')
    ax1.set_ylabel('Size (bytes)')
    ax1.set_title('Raw size')
    ax1.set_xscale('log')
    ax1.set_yscale('log')
    ax1.legend()
    ax1.grid(True, alpha=0.3)

    ratios_raw = [agg.loc[n, 'json_compact_bytes'] / agg.loc[n, 'cstl_bytes']
                  for n in sizes]
    ratios_gz = [agg.loc[n, 'json_compact_gz'] / agg.loc[n, 'cstl_gz']
                 for n in sizes]
    ax2.plot(sizes, ratios_raw, 'o-', label='JSON/CSTL raw', linewidth=2)
    ax2.plot(sizes, ratios_gz, 's-', label='JSON/CSTL gzip', linewidth=2)
    ax2.axhline(1.0, color='gray', linestyle='--', alpha=0.5)
    ax2.set_xlabel('Number of relations')
    ax2.set_ylabel('Ratio (JSON compact / CSTL)')
    ax2.set_title('CSTL advantage: raw vs gzip')
    ax2.set_xscale('log')
    ax2.legend()
    ax2.grid(True, alpha=0.3)

    plt.tight_layout()
    plt.savefig(out_path, dpi=150, bbox_inches='tight')
    print(f"✓ Figure saved: {out_path}")


def main() -> int:
    ap = argparse.ArgumentParser(description="E1 compression benchmark")
    ap.add_argument("--out-dir", default="./results/e1")
    ap.add_argument("--n-trials", type=int, default=20)
    ap.add_argument("--sizes", default="1,5,10,25,50,100",
                    help="comma-separated relation counts per payload")
    args, _ = ap.parse_known_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    sizes = [int(s) for s in args.sizes.split(",")]
    df = run(sizes, args.n_trials)

    csv_path = out_dir / "e1_compression.csv"
    df.to_csv(csv_path, index=False)
    print(f"✓ Raw data: {csv_path}")

    agg = print_summary(df, sizes)
    save_figure(agg, sizes, out_dir / "e1_compression_figure.png")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
