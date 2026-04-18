#!/usr/bin/env python3
"""
E4 — Empirical validation of Theorem 1 (k=9 uniqueness)
========================================================

Generates corpora of distinct CSTL relations at increasing scales and checks
that the k=9 encoding produces zero collisions — consistent with the
construction-based injectivity guarantee of Theorem 1.

"Semantic duplicates" (relations identical across all fields, produced by
random sampling in the same corpus) are counted separately and deduplicated
before computing collisions, since they are not a theorem violation.

No LLM calls. Fully deterministic.

Usage:
    python3 e4_k9_uniqueness.py [--sizes 1000,10000,100000,500000]

Author: Olivier Goyette
License: MIT
"""

from __future__ import annotations

import argparse
import hashlib
import random
import sys
from collections import Counter


def encode_k9(relation: dict, layer: str) -> tuple:
    """Canonical 9-position encoding per the CSTL v3.0.1 spec.

    Position 1 = archeological layer (surface/shallow/deep/bedrock)
    Positions 2-8 = relation content
    Position 9 = checksum of the layer (cross-layer disambiguation)
    """
    src, op, tgt, conf, depth = (
        relation["source"], relation["op"], relation["target"],
        relation["confidence"], relation["depth"])
    return (
        layer,                                         # pos1: layer
        "[",                                           # pos2: left context
        src,                                           # pos3: cause
        "|",                                           # pos4: link separator
        op,                                            # pos5: operator
        tgt,                                           # pos6: effect
        str(conf),                                     # pos7: parameter
        "]",                                           # pos8: closure
        hashlib.md5(layer.encode()).hexdigest()[:4],  # pos9: checksum
    )


def relation_key(r: dict) -> tuple:
    """Semantic identity of a relation (what makes it unique)."""
    return (r["source"], r["op"], r["target"], r["confidence"], r["depth"])


def generate_corpus(n: int, seed: int = 42) -> list[dict]:
    rng = random.Random(seed)
    ops = ["ARR", "AMP", "ATT", "INH", "CYC", "BID", "SYN", "ANT"]
    depths = ["surface", "shallow", "deep", "bedrock"]
    ents = [f"E{i}" for i in range(1, 500)]
    corpus = []
    for _ in range(n):
        a, b = rng.sample(ents, 2)
        corpus.append({
            "source": a, "op": rng.choice(ops), "target": b,
            "confidence": round(rng.uniform(0.5, 1.0), 2),
            "depth": rng.choice(depths),
        })
    return corpus


def main() -> int:
    ap = argparse.ArgumentParser(description="E4 k=9 uniqueness validation")
    ap.add_argument("--sizes", default="1000,10000,100000,500000",
                    help="comma-separated corpus sizes")
    args, _ = ap.parse_known_args()
    sizes = [int(s) for s in args.sizes.split(",")]

    print("=" * 72)
    print("E4 — Empirical validation of Theorem 1 (k=9 uniqueness)")
    print("=" * 72)

    any_collision = False

    for N in sizes:
        corpus = generate_corpus(N)

        # Semantic dedup (random sampling can produce duplicates)
        unique_relations = {relation_key(r): r for r in corpus}
        n_semantic_unique = len(unique_relations)
        n_duplicates = N - n_semantic_unique

        # Encode the distinct relations only
        encoded = [encode_k9(r, r["depth"]) for r in unique_relations.values()]
        n_encoded_unique = len(set(encoded))
        true_collisions = n_semantic_unique - n_encoded_unique

        print(f"\nCorpus of {N:>7,} samples")
        print(f"  Semantically distinct relations : {n_semantic_unique:>7,}")
        print(f"  Semantic duplicates (expected)  : {n_duplicates:>7,}")
        print(f"  Distinct encodings              : {n_encoded_unique:>7,}")
        print(f"  TRUE collisions (theorem viol.) : {true_collisions:>7,}")

        if true_collisions == 0:
            print(f"  ✓ Theorem 1 validated : "
                  f"{n_semantic_unique:,} distinct → {n_encoded_unique:,} encodings")
        else:
            any_collision = True
            counts = Counter(encoded)
            dups = [k for k, v in counts.items() if v > 1]
            print(f"  ⚠ {len(dups)} collision classes — sample inspection:")
            for col in dups[:3]:
                matching = [r for r in unique_relations.values()
                            if encode_k9(r, r["depth"]) == col]
                print(f"    collision key : {col}")
                for r in matching:
                    print(f"      <- {r}")

    print("\n" + "=" * 72)
    print("Capacity vs empirical occupancy")
    print("=" * 72)
    space = 121 ** 9
    print(f"Address space k=9 : 121^9 ≈ {space:.2e}")
    max_tested = max(sizes)
    print(f"Largest corpus    : {max_tested:,}")
    print(f"Occupancy ratio   : {max_tested / space:.2e}")

    return 1 if any_collision else 0


if __name__ == "__main__":
    sys.exit(main())
