#!/usr/bin/env python3
"""test_demo_live.py -- la demonstration (cstl_demo.py) passe 7/7 contre le
vrai serveur (2026-11-25). Saute si target/debug/cstl_parser n'est pas compile."""
import os
import sys
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
BINARY = SDK.parent.parent / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))


def test_demo_passes_every_check(monkeypatch):
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    import cstl_demo
    monkeypatch.setenv("HOME", os.environ.get("HOME", "/tmp"))   # la demo change HOME ; restaure apres
    assert cstl_demo.run(BINARY) == 0
