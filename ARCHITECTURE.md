# CSTL OS Kernel Architecture Complète

**Date:** 29 Août 2026  
**Version:** 5.0.0  
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

CSTL Wire Format avec hashbang #!CSTL v5.0.0 MODE=A, SHA-256 immutable, zéro hallucination prouvée.

### Couche 2: Gouvernance / Résilience
**État:** ✅ TESTÉ - 4/4 modes  

Circuit Breaker avec quorum 2/3, dynamic whitelist, 3 modes défaillance, operator drift prevention.

### Couche 3a: Vérification Faits Publics
**État:** ✅ IMPLÉMENTÉE  

Fact Verification avec Wikidata + SPARQL, entity resolution.

### Couche 3b: Lab Logiciel + Arbitration
**État:** 🟡 DESIGNÉ, PAS WIRED  

RestrictedCouncil Framework, ExecutionLab subprocess-isolated, human arbitration channel.

### Couche 4: Calibration / Fiabilité
**État:** ✅ TESTÉ  

Laplace Smoothed Scoring per-agent, per-domain accuracy.

### Couche 5: Mémoire Persistante / Provenance
**État:** 🟡 FRAGMENTÉE  

SQLite store + hash entanglement + FastAPI server.

### Couche 6: Interface Humaine
**État:** 🔶 SQUELETTE  

Graphify (589 nodes) + Obsidian vault.

### Couche 7: Agent Discovery & Routing (CSTL Natif)
**État:** ❌ À CONSTRUIRE  

Zero external dependencies. Agent Registry, Agent Cards, Service Discovery - tout CSTL natif.

### Couche 8: Provenance Audit / Cryptographic Guarantee
**État:** ✅ DESIGNÉ  

Hash-Chained Audit Trail, Deontic Modality

cd ~/cstl && mkdir -p docs && cat > docs/ARCHITECTURE.md << 'EOF'
# CSTL OS Kernel Architecture Complète

**Date:** 29 Août 2026  
**Version:** 5.0.0  
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

CSTL Wire Format avec hashbang #!CSTL v5.0.0 MODE=A, SHA-256 immutable, zéro hallucination prouvée.

### Couche 2: Gouvernance / Résilience
**État:** ✅ TESTÉ - 4/4 modes  

Circuit Breaker avec quorum 2/3, dynamic whitelist, 3 modes défaillance, operator drift prevention.

### Couche 3a: Vérification Faits Publics
**État:** ✅ IMPLÉMENTÉE  

Fact Verification avec Wikidata + SPARQL, entity resolution.

### Couche 3b: Lab Logiciel + Arbitration
**État:** 🟡 DESIGNÉ, PAS WIRED  

RestrictedCouncil Framework, ExecutionLab subprocess-isolated, human arbitration channel.

### Couche 4: Calibration / Fiabilité
**État:** ✅ TESTÉ  

Laplace Smoothed Scoring per-agent, per-domain accuracy.

### Couche 5: Mémoire Persistante / Provenance
**État:** 🟡 FRAGMENTÉE  

SQLite store + hash entanglement + FastAPI server.

### Couche 6: Interface Humaine
**État:** 🔶 SQUELETTE  

Graphify (589 nodes) + Obsidian vault.

### Couche 7: Agent Discovery & Routing (CSTL Natif)
**État:** ❌ À CONSTRUIRE  

Zero external dependencies. Agent Registry, Agent Cards, Service Discovery - tout CSTL natif.

### Couche 8: Provenance Audit / Cryptographic Guarantee
**État:** ✅ DESIGNÉ  

Hash-Chained Audit Trail, Deontic Modality Audit.

### Couche 9: Orchestration Gouvernance Deontic
**État:** ❌ À CONSTRUIRE

Event-Driven Governance, Deontic Execution Model, Multi-Agent Orchestration.

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

## Différenciation Unique

vs LangGraph: state machine vs relation management, zero deontic natif, pas semantic fidelity
vs Institutional AI: governance graphs only, pas deontic modality, pas arbitration structuré
vs Constitutional Governance: règles simplistes vs deontic + arbitration
vs MCP: agent-to-tool vs agent-to-agent sémantique natif

## État Actuel & Timeline

### Fait & Prouvé:
- ✅ Couche 1: Transport 99.3%
- ✅ Couche 2: Gouvernance 4/4 modes
- ✅ Couche 3a: Fact verification Wikidata
- ✅ Couche 4: Calibration Laplace
- ✅ Couche 5a,5b: SQLite store + entanglement

### À Compléter (4-5 semaines):
- 🔴 Couche 3b: Arbitration channel wire (8h)
- 🔴 Couche 5c: FastAPI server integration
- 🔴 Couche 6: Graphify/Obsidian connection
- 🔴 Couche 7: CSTL agent discovery + routing natif
- 🔴 Couche 8: Hash-chained audit trail completion
- 🔴 Couche 9: Event-driven orchestration

### R8 Bugs (Rust Parser):
- Bug 1: indexing on "name" not "id" (30 min)
- Bug 2: extract_entity_type regex fix (30 min)

### ArXiv Timeline:
- Week 1: R8 bugs + arbitration wiring
- Week 2-3: Couches 7-9 integration
- Week 4-5: Testing + documentation
- Target: September 15-20, 2026 submission

## Pourquoi C'est Unique

1. Deontic modality première classe
2. Semantic fidelity prouvée par tests
3. Arbitration protocol structuré
4. Hash-chained immutable provenance
5. Relations au centre
6. Zero external dependencies (no MCP)

---

C'est ta fondation. Le reste est détail d'implémentation.
