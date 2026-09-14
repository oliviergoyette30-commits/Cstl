# CSTL v5.1/v5.2 Push Status — 2026-09-14

## Commits Ready to Push (9 total)

### v5.1 (4 commits)
1. `c296061` - Feature C: Add from_env() graceful degradation to Python LLM providers
2. `796c94a` - Fix CVE-2025-53605: Upgrade protobuf to 3.7.2
3. `687f7da` - Update ARCHITECTURE.md: v5.1 completion + CVE-2025-53605 fix
4. `c97d422` - Add v5.1 completion summary

### v5.2 Layers (5 commits)
5. `37082df` - Couche 5c: FastAPI server integration
6. `52c78b6` - Couche 3b: Arbitration channel wire + ExecutionLab
7. `ab1056d` - Couche 6: Graphify/Obsidian UI bridge
8. `47fb255` - Couche 7 v5.2: Cross-validation Rust/Python signing
9. `f9d1c15` - Couche 9: Event-driven orchestration + Deontic execution model

## Test Results
- **290 tests passing** (up from 275 in v5.1)
- Build: Release optimized, no errors
- All critical path tests verified

## Files to Sync
Core modified/new files included in downloads:
- Cargo.toml (protobuf 3.7.2 CVE fix)
- Cargo.lock
- ARCHITECTURE.md (v5.1.0 → v5.2 roadmap)
- COMPLETION_v5_1_2026-09-14.md

Rust layers:
- src/server/arbitrage.rs (Couche 3b)
- src/server/rest_api.rs (Couche 5c)
- src/server/deontic_orchestration.rs (Couche 9)
- src/server/mod.rs (updated)
- src/adn_store.rs (updated)

Python layers:
- sdk/python/cstl_obsidian_bridge.py (Couche 6)
- sdk/python/cstl_llm_agent.py (Feature C)
- sdk/python/cstl_signing.py (cross-validation)
- sdk/python/test_adn_server.py (Couche 5c tests)
- sdk/python/test_obsidian_bridge.py (Couche 6 tests)

Tests:
- tests/cross_validation_signing_test.rs (Couche 7 v5.2)

## Ready to Push
All commits are in local repo, ready for push to main.
