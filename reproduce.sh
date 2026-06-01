#!/usr/bin/env bash
#
# reproduce.sh — one-command reproduction of CSTL v4.9.3 claims
#
# Runs the Rust test suite and the Python test/benchmark suite, then prints
# a summary. The OUTPUT of this script is authoritative — not the numbers
# quoted in the README. If a claim does not reproduce here, the README is
# wrong, not this script.
#
# Usage:
#   ./reproduce.sh
#
# Requirements:
#   - Rust toolchain (cargo)         https://rustup.rs
#   - Python 3.10+ and pip
#
set -u  # treat unset variables as errors; do NOT set -e so we run all stages

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FAILURES=0

section() { printf '\n========================================\n%s\n========================================\n' "$1"; }

# ----------------------------------------------------------------------
section "1/3  Rust parser — build + tests"
# ----------------------------------------------------------------------
if command -v cargo >/dev/null 2>&1; then
  if [ -f "$ROOT/Rust/Cargo.toml" ]; then
    ( cd "$ROOT/Rust" && cargo build --release ) || { echo "RUST BUILD FAILED"; FAILURES=$((FAILURES+1)); }
    ( cd "$ROOT/Rust" && cargo test ) || { echo "RUST TESTS FAILED"; FAILURES=$((FAILURES+1)); }
  else
    echo "SKIP: Rust/Cargo.toml not found"
  fi
else
  echo "SKIP: cargo not installed — see https://rustup.rs"
fi

# ----------------------------------------------------------------------
section "2/3  Python — dependencies + test suite"
# ----------------------------------------------------------------------
if command -v python3 >/dev/null 2>&1; then
  if [ -f "$ROOT/requirements.txt" ]; then
    python3 -m pip install -r "$ROOT/requirements.txt" --quiet || echo "WARN: pip install reported issues"
  fi
  # Run the test files that exist in the repo. Add/remove as the suite evolves.
  for t in test_cstl_parser.py test_parser_robustness.py test_security_suite.py test_fuzzing.py; do
    if [ -f "$ROOT/$t" ]; then
      echo "--- running $t ---"
      python3 -m pytest "$ROOT/$t" -q || { echo "PYTHON TEST FAILED: $t"; FAILURES=$((FAILURES+1)); }
    fi
  done
else
  echo "SKIP: python3 not installed"
fi

# ----------------------------------------------------------------------
section "3/3  Benchmarks (regenerate measurement CSVs)"
# ----------------------------------------------------------------------
if command -v python3 >/dev/null 2>&1; then
  for b in e1_compression.py e3_expressivity.py run_experiments.py; do
    if [ -f "$ROOT/$b" ]; then
      echo "--- running $b ---"
      python3 "$ROOT/$b" || echo "WARN: $b reported issues (non-fatal)"
    fi
  done
fi

# ----------------------------------------------------------------------
section "Summary"
# ----------------------------------------------------------------------
if [ "$FAILURES" -eq 0 ]; then
  echo "All verifiable stages passed (or were skipped for missing tooling)."
  echo "If cargo/python were skipped, install them and re-run for full verification."
  exit 0
else
  echo "$FAILURES stage(s) FAILED. The README claims are NOT reproduced on this machine."
  echo "Treat this output as authoritative and fix the discrepancy before citing numbers."
  exit 1
fi
