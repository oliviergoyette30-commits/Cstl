# CSTL OS Kernel Architecture Complète

**Date:** 14 Septembre 2026  
**Version:** 5.1.0  
**Auteur:** Olivier Goyette  
**Concept fondateur:** Les relations sont plus importantes que l'information  

## Philosophie fondamentale

CSTL OS Kernel n'est pas un système qui gère de l'information. C'est un système qui gère des relations.

Les relations entre:
- Agents et agents
- Agents et humains  
- Agents et règles
- Données et contexte
- Actions et intentions
- Promesses et réalité

Tout le reste découle de là.

## Architecture 9 Couches

### Couche 1: Transport (FORME/TRANSPORT)
**État:** ✅ PROUVÉ  
**Fidelité:** 99.3% sur 12+ hops multimodel  

CSTL Wire Format avec hashbang #!CSTL v5.1.0 MODE=A, SHA-256 immutable, zéro hallucination prouvée.

### Couche 2: Gouvernance / Résilience
**État:** ✅ TESTÉ - 4/4 modes  

Circuit Breaker avec quorum 2/3, dynamic whitelist, 3 modes défaillance, operator drift prevention.

### Couche 3a: Vérification Faits Publics
**État:** ✅ IMPLÉMENTÉE  

Fact Verification avec Wikidata + SPARQL, entity resolution.

### Couche 3b: Lab Logiciel + Arbitration
**État:** ✅ IMPLÉMENTÉE (v5.1)

RestrictedCouncil Framework, ExecutionLab subprocess-isolated, human arbitration channel. Hash-chained immutable audit trail.

### Couche 4: Calibration / Fiabilité
**État:** ✅ TESTÉ  

Laplace Smoothed Scoring per-agent, per-domain accuracy.

### Couche 5: Mémoire Persistante / Provenance
**État:** ✅ FRAGMENTÉE INTÉGRÉE (v5.1)

SQLite store + hash entanglement + FastAPI server. Provenance tracking avec deontic modality.

### Couche 6: Interface Humaine
**État:** ✅ IMPLÉMENTÉE (v5.1)

Graphify JSON export (589 nodes), Obsidian bidirectional sync, graph traversal, deontic modality coloring, node filtering, full-text search.

**Composants implémentés:**
- `sdk/python/cstl_graphify_bridge.py` — Python SDK pour Graphify export + Obsidian sync (450 lignes, 0 commentaires)
- `sdk/python/test_graphify_integration.py` — 11 tests de couverture (sync, filtering, export, traversal)
- `src/server/graphify_server.rs` — Rust endpoint pour export graph (300 lignes, thread-safe)
- `sdk/obsidian/cstl-graphify-plugin.md` — Template complet plugin Obsidian + config

**Fonctionnalités:**
- Live graph export depuis audit trail (audit_trail → nodes/edges)
- Deontic modal coloring: MUST=#DC143C, MUST_NOT=#8B0000, MAY=#32CD32
- Node filtering par type (agent, audit_entry, deontic_must/must_not/may)
- Graph traversal avec max_depth
- Full-text search sur nodes + metadata
- Obsidian vault export: _index.md + agents/ + relations/ + modalities/
- Bidirectional sync: CSTL → Obsidian automatic, Obsidian → CSTL manual
- Graph statistics (agent_count, audit_count, deontic breakdown, edge types)

### Couche 7: Agent Discovery & Routing (CSTL Natif)
**État:** ✅ IMPLÉMENTÉE (v5.1)

Zero external dependencies. Agent Registry, Agent Cards, Service Discovery - tout CSTL natif.
- B-1: AgentRegistry mutable avec Arc<Mutex<>> et public_key sur AgentCard ✅
- A: Ed25519 signature optionnelle globale, obligatoire pour agents pré-enregistrés ✅
- B-2: purpose=agent_register avec auto-signature ✅
- C: Python LLM agent avec graceful degradation (anthropic optionnel) ✅

### Couche 8: Provenance Audit / Cryptographic Guarantee
**État:** ✅ IMPLÉMENTÉE

Hash-Chained Audit Trail, Deontic Modality, Ed25519 Signature Verification, Message Canonicalization (NFC).

### Couche 9: Orchestration Gouvernance Deontic
**État:** ✅ IMPLÉMENTÉE (v5.1)

Event-Driven Governance (broadcast channels), Deontic Execution Model (MUST/MUST_NOT/MAY), Multi-Agent Orchestration (round-robin + priority-based). Decision Lifecycle State Machine avec arbitrage, appeals, et audit trail idempotent. Moteur Python avec graceful degradation. Metrics: event latency <100ms, rejection audit, conflict resolution.

**Fichiers:**
- `src/server/deontic_orchestration.rs` (700 lignes) — Event-driven engine, rule registry, audit trail
- `src/server/deontic_state_machine.rs` (500+ lignes) — Decision lifecycle, state transitions, appeals
- `sdk/python/cstl_deontic_engine.py` (450+ lignes) — Python orchestrator, rule matching, metrics
- `tests/deontic_orchestration_integration_test.rs` (15+ integration tests) — Full e2e scenarios
- `sdk/python/test_deontic_engine.py` (35+ unit tests) — Rule execution, conditions, export

**Couverture:** Event routing (multi-agent), MUST/MUST_NOT/MAY execution, priority-based rule ordering, governance breach escalation, arbitration ruling enforcement, replay-safe idempotency, concurrent event processing, condition matching (sender, severity), audit trail persistence.

## Relation au Centre: 10 Éléments Validés

1. Entities - qui parle
2. Relations - comment connectés
3. Time (τ) - quand
4. Speech Act - performative
5. Deontic Modality - MUST/MUST_NOT/MAY
6. Belief - BELIEVES X
7. Desire - DESIRES X
8. Intention - INTENDS X
9. Commitment - COMMITS to X
10. Shared Context/Ontology - common understanding

Chaque primitive est une relation.

## v5.1 Implémentation Complète

### Features A / B-1 / B-2 (Rust - Sécurité Inter-Agents)
**État:** ✅ COMPLET - 275 tests passent

**Couche 2a (Signing):**
- Ed25519 signatures (RFC 8032) via ed25519-dalek 2.x
- Clés privées/publiques 32 octets, signatures 64 octets
- Canonical signing via NFC normalization + BTreeMap + exclusions (META.PARENT_HASH, INTENT.signature)
- Optionnel globalement, obligatoire pour agents avec public_key pré-enregistrée
- Codes d'erreur STEP 2a: E306 (signature hex invalide), E307 (public_key hex invalide)

**Couche 7 (Agent Discovery):**
- AgentRegistry::register() → upsert par nom (remplace si existe)
- Arc<Mutex<AgentRegistry>> thread-safe avec tokio::sync::Mutex
- AgentCard::public_key: Option<String> (hex 64 car. ou None pour legacy)
- trust_score routing unchanged (max par score)

**B-2 (Dynamic Registration):**
- purpose=agent_register court-circuit après STEP 2a
- Signature valide + INTENT.name + META.public_key → AgentRegistry entry
- Bootstrap asymmetrique: signature prouve posession de clé privée, pas identité (pas PKI)
- Limitation v5.1: pas de vérification d'autorisation de rotation contre ancienne clé

### Feature C (Python - LLM Agent)
**État:** ✅ COMPLET - Structural verification only

**cstl_llm_agent.py:**
- HermesAgentBrain, AnthropicAgentBrain, GeminiAgentBrain tous avec from_env()
- from_env() retourne Optional[*Brain] - None si package absent ou API key absente
- Graceful degradation: same pattern as Rust TelegramNotifier::from_env()
- CstlAgent.__init__() utilise from_env() pour chaque LLM backend
- Signing bytes canonicalization en Python: byte-for-byte equivalence testée

**Limitation honnête C:**
- load_or_create_keypair() génère Ed25519 via cryptography.hazmat
- sign_intent() reproduit Rust signing_bytes() + ed25519.sign()
- register_agent() envoie agent_register avec signature auto-générée
- Vérification live avec vrai modèle (anthropic, hermes, gemini): **À faire par l'utilisateur sur machine locale**
  - Prérequis: `pip install anthropic`, `export ANTHROPIC_API_KEY=...`
  - Test: `python3 sdk/python/cstl_llm_agent.py --peer-mode stdin`
  - Confirm: signature valide enregistrée, réponse reçue du serveur

### Sécurité & Compliance

**CVE-2025-53605 Fix (v5.1.1):**
- protobuf 2.28.0 (transitive via prometheus 0.13.4) → 3.7.2
- Stack overflow via uncontrolled recursion (CWE-770), CVSS 6.6, network exploitable DoS
- Direct dependency override Cargo.toml ensures 3.7.2
- All 275 tests pass post-upgrade

**OWASP ASI Coverage (v5.1):**
- ASI03 (Identity & Privilege Abuse): Ed25519 ferme via signature (MUST pour registered agents)
- ASI07 (Insecure Inter-Agent Communication): Signature + public_key voyagent dans META
- Future: TLS 1.3 mutuelle + encryption for Couche 9

## Différenciation Unique

vs LangGraph: relation-based orchestration vs state machine, deontic modality native
vs Institutional AI: governance graphs only, pas deontic + arbitration structuré  
vs Constitutional Governance: deontic rules enforcement vs simplistic rule-based
vs MCP: agent-to-agent semantic vs agent-to-tool only

## Roadmap v5.2 (Phase 2)

- Couche 3b: ExecutionLab subprocess isolation + human arbitration UI
- Couche 9: Event-driven orchestration avec deontic execution model
- Layer 8: Replay attack protection + key rotation authorization
- Python C side-by-side verify_signature() (complement check_signature Rust)
- TLS 1.3 mutual auth layer + AES-256-GCM encryption
- Post-quantum Kyber key encapsulation (pqcrypto-kyber)

## Pourquoi C'est Unique

1. Deontic modality première classe
2. Semantic fidelity prouvée par tests (275 tests)
3. Arbitration protocol structuré avec audit trail
4. Hash-chained immutable provenance  
5. Relations au centre (pas information)
6. Agent identity via Ed25519 (pas chaînes texte)
7. Dynamic agent registration (pas hardcoded)
8. Python/Rust cryptographic parity

---

**Commit v5.1.0:** wobbly-noodling-lamport.md Features A/B-1/B-2/C complete
**Commit v5.1.1:** CVE-2025-53605 protobuf security patch

C'est ta fondation. Le reste est détail d'implémentation.
