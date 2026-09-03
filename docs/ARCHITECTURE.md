# CSTL OS Kernel Architecture Complète

**Date de dernière vérification:** 3 Septembre 2026
**Version:** 5.0.0
**Auteur:** Olivier Goyette
**Concept fondateur:** Les relations sont plus importantes que l'information

> Ce document décrit l'architecture conceptuelle. Pour le statut d'implémentation
> détaillé et vérifié (quel fichier Rust fait quoi, ce qui est réellement câblé
> vs designé, les limitations honnêtes), voir [`README.md`](../README.md#architecture--9-layers)
> et sa section "Honest Limitations" — ce document-ci est resynchronisé avec ces
> deux sources au moment de la date ci-dessus, mais README.md reste la source de
> vérité en cas de divergence future.

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

Implémentation actuelle: Rust natif (`src/`), serveur TCP async (tokio). L'ancienne
implémentation Python (parser, ADN store, serveur FastAPI) a été abandonnée au
profit de ce portage Rust — toute mention de FastAPI, de fichiers `.py`, ou d'un
serveur séparé ci-dessous serait une régression de cette réécriture.

### Couche 1: Transport (FORME/TRANSPORT)
**État:** ✅ PROUVÉ
**Fidelité:** 99.3% sur 12+ hops multimodel

CSTL Wire Format avec hashbang `#!CSTL v5.0.0 MODE=A`, SHA-256 immutable, validation déterministe (`src/server/parser.rs`, `src/server/validator.rs`, `src/server/audit.rs`).

### Couche 2: Gouvernance / Résilience
**État:** 🔴 NON CONSTRUITE — audit multi-angle du 2026-09-03

Aucun circuit breaker, aucune logique de quorum, aucune détection de drift
d'opérateur n'existe nulle part dans ce dépôt (vérifié par grep exhaustif).
Le "✅ TESTÉ - 4/4 modes" affiché ici auparavant n'a jamais été vrai. Le
primitif le plus proche réellement construit est `RestrictedCouncil`
(Couche 3b ci-dessous) : quorum=1, pas 2/3, sans circuit breaker ni
détection de drift.

### Couche 3a: Vérification Faits Publics
**État:** ✅ IMPLÉMENTÉE, câblée live (`src/kb_verify.rs`)

Fact Verification avec Wikidata + SPARQL, entity resolution. Appelée pour chaque `RELATION` d'un payload reçu par le serveur en cours d'exécution.

### Couche 3b: Lab Logiciel + Arbitration
**État:** 🟡 PARTIEL, câblé live avec portée réduite

`ExecutionLab` (`src/execution_lab.rs`): détection de contradictions et de cycles, câblée live. `RestrictedCouncil` (`src/restricted_council.rs`): câblée live, avec pont Telegram (boutons, réponse en direct) — mais portée réduite à un seul membre autorisé (quorum=1), pas le quorum 2/3 multi-personnes décrit plus bas. Coherence check désormais croisé avec l'historique complet de l'ADN store (`check_consistency_with_history`), pas seulement les relations d'un seul payload reçu.

### Couche 4: Calibration / Fiabilité
**État:** ✅ TESTÉ

Laplace Smoothed Scoring per-agent, per-domain accuracy.

### Couche 5: Mémoire Persistante / Provenance
**État:** 🟡 Construite en Rust, câblée live

`src/adn_store.rs`: SQLite store + hash entanglement. Pas encore unifiée avec la hash chain d'audit dans un seul schéma — les deux systèmes restent liés seulement par un hash partagé.

### Couche 6: Interface Humaine
**État:** 🟡 PARTIEL

Escalade Obsidian (`src/obsidian_escalation.rs`): réelle, câblée live, vérifiée end-to-end contre un vrai vault (contradiction détectée par `ExecutionLab` → écrite dans `CSTL_Restricted_Council.md`). Graphify: réel désormais — corrige une contradiction avec README.md détectée par l'audit multi-angle du 2026-09-03 (ce document disait encore "inactif, pas installé" alors que README.md documentait déjà l'installation et la régénération). L'outil (`graphifyy`, PyPI, venv local) a été installé et le graphe régénéré le 2026-09-03 : 967 nœuds, 1800 arêtes, 63 communautés étiquetées sémantiquement, construit depuis le commit `3326f917`. Redevient stale après chaque nouveau commit tant que `graphify update .` n'est pas relancé.

### Couche 7: Agent Discovery & Routing (CSTL Natif)
**État:** ✅ CONSTRUITE ET CÂBLÉE LIVE

`src/agent_discovery.rs`: Agent Registry, zero external dependencies, utilisée par chaque requête reçue par le serveur.

### Couche 8: Provenance Audit / Cryptographic Guarantee
**État:** ✅ DESIGNÉ

Hash-Chained Audit Trail (`src/server/audit.rs`), Deontic Modality Audit.

### Couche 9: CASTLE (mode de compression)
**État:** 🟡 ARCHITECTURÉ, PAS DE CODE

Session-amortized shared dictionary, event-driven orchestration. Pas encore implémenté.

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

## Pourquoi C'est Unique

1. Deontic modality première classe
2. Semantic fidelity prouvée par tests
3. Arbitration protocol structuré (portée réduite: quorum=1, pas 2/3 — voir Couche 3b)
4. Hash-chained immutable provenance
5. Relations au centre
6. Zero external dependencies (no MCP)

---

C'est la fondation. Le reste est détail d'implémentation — voir [`README.md`](../README.md) pour le détail vérifié fichier par fichier.
