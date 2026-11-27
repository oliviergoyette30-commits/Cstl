#!/usr/bin/env python3
"""test_control_benchmark.py -- le banc public (benchmark/control_benchmark.py)
tourne contre le vrai serveur, et son evaluateur donne exactement les
decisions de la couche 9 sur chaque politique legitime (2026-11-25). Le test
ne fige PAS les scores : une nouvelle attaque qui fait echouer CSTL doit
pouvoir etre ajoutee sans casser la CI. Saute sans target/debug/cstl_parser."""
import json
import subprocess
import sys
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent.parent


def test_benchmark_runs_and_evaluator_matches_layer9(tmp_path):
    if not (REPO / "target" / "debug" / "cstl_parser").exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    out = tmp_path / "results.json"
    r = subprocess.run([sys.executable, str(REPO / "benchmark" / "control_benchmark.py"), "--out", str(out)],
                       capture_output=True, text=True, timeout=600)
    assert r.returncode == 0, r.stdout[-3000:] + r.stderr[-3000:]
    res = json.loads(out.read_text())
    assert res["evaluator_mismatches"] == []
    assert len(res["rows"]) == res["cases"] * 2 * 6
