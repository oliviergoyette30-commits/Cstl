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
**État:** 🟡 PARTIEL, observation seule — câblée live le 2026-09-03 (`src/governance.rs`)

Un circuit breaker par expéditeur (fenêtre glissante sur les événements
d'incohérence `ExecutionLab`) et un ratio de drift d'opérateur (fenêtre
glissante sur les avertissements `SEMANTIC_WARNING`) sont désormais
calculés pour chaque payload et exposés dans un nouveau bloc de réponse
`GOVERNANCE [...]`, avec escalade Telegram renforcée quand un seuil est
franchi — mais **aucun des deux ne rejette jamais un payload**, décision
explicite cohérente avec le seul mécanisme de blocage réel du pipeline
(sécurité/parse/validation). `RestrictedCouncil::quorum_size()` implémente
maintenant l'arithmétique réelle du quorum 2/3 (ceil(2/3·n)) et
`AdnStore::cast_commit_vote` compte les votants distincts par hash —
vérifié en direct avec un council à 2 membres
(`examples/governance_smoke_test.rs`). Limites assumées: la config de
production (`main.rs`) n'enregistre encore qu'un seul membre ("Olivier"),
donc quorum=1 en pratique aujourd'hui ; l'état du breaker/drift est en
mémoire uniquement, perdu au redémarrage. Le "✅ TESTÉ - 4/4 modes" affiché
ici avant le 2026-09-03 n'a jamais été vrai.

**Identité/authentification (2026-09-04, `src/signing.rs`):** signature
Ed25519 par message, câblée live et vérifiée en direct
(`examples/signing_registration_smoke_test.rs`, 6 scénarios sur une vraie
connexion TCP + dev/release). Ferme partiellement OWASP ASI03 (Identity &
Privilege Abuse) et ASI07 (Insecure Inter-Agent Communication) — avant ce
travail, `sender`/`receiver` étaient de simples chaînes de texte sans
AUCUNE vérification cryptographique. Portée v1 assumée, pas une découverte
après coup: la signature est **optionnelle globalement, obligatoire
seulement pour un expéditeur déjà enregistré avec une `public_key`**
(`META.public_key`, `INTENT_PAYLOAD.signature`, STEP 2a de
`server/handler.rs`) — sinon les 148 tests et smoke-tests legacy non
signés casseraient tous pour fermer un risque qui ne concerne, dans les
faits, que les identités déjà établies. Pas de PKI/CA: une auto-signature
prouve seulement la possession de la clé privée, pas une identité
pré-existante (modèle de confiance mono-opérateur, cohérent avec
`RestrictedCouncil`). Limite documentée: aucune vérification
d'autorisation de rotation contre l'ANCIENNE clé lors d'un
réenregistrement.

### Couche 3a: Vérification Faits Publics
**État:** ✅ IMPLÉMENTÉE, câblée live (`src/kb_verify.rs`)

Fact Verification avec Wikidata + SPARQL, entity resolution. Appelée pour chaque `RELATION` d'un payload reçu par le serveur en cours d'exécution.

### Couche 3b: Lab Logiciel + Arbitration
**État:** 🟡 PARTIEL, câblé live avec portée réduite

`ExecutionLab` (`src/execution_lab.rs`): détection de contradictions et de cycles, câblée live. `RestrictedCouncil` (`src/restricted_council.rs`): câblée live, avec pont Telegram (boutons, réponse en direct) — l'arithmétique du quorum 2/3 (`quorum_size()`) et le comptage des votants distincts (`AdnStore::cast_commit_vote`) sont maintenant implémentés et testés (voir Couche 2 ci-dessus), mais la config de production n'enregistre encore qu'un seul membre autorisé, donc quorum=1 en pratique aujourd'hui. Coherence check désormais croisé avec l'historique complet de l'ADN store (`check_consistency_with_history`), pas seulement les relations d'un seul payload reçu.

### Couche 4: Calibration / Fiabilité
**État:** ✅ TESTÉ

Laplace Smoothed Scoring per-agent, per-domain accuracy.

### Couche 5: Mémoire Persistante / Provenance
**État:** 🟡 Construite en Rust, câblée live — persistance de la chaîne corrigée le 2026-09-04

`src/adn_store.rs`: SQLite store + hash entanglement.

**Trouvaille corrigée (2026-09-04):** avant cette passe, la chaîne de hachage
(`seq`/`parent_hash`, `src/server/audit.rs::HashChain`) était **purement en
mémoire** — remise à zéro à CHAQUE redémarrage du serveur, alors même que
`adn_store.rs` persistait déjà l'historique des payloads (avec leur propre
`parent_hash`) sur disque. `AuditStore` (`src/server/audit_store.rs`)
implémentait déjà exactement la persistance nécessaire, mais n'était appelé
NULLE PART en dehors de son propre test unitaire — code mort depuis sa
création. Un redémarrage réel cassait donc silencieusement la continuité
seq/parent_hash de la chaîne, une régression invisible de la garantie de
"provenance immuable" (Couche 8) que rien ne signalait ni ne testait.

Corrigé: `CstlNativeServer::with_data_path` (nouveau constructeur, `new()`
l'appelle avec `"cstl_adn.db"`) ouvre `AuditStore` sur le MÊME fichier que
`adn_store` et charge la chaîne persistée au démarrage
(`AuditStore::load_chain`); `server/handler.rs` appelle `audit_store.save()`
juste après chaque `chain.append()`. `AuditStore::save` est passé de
`INSERT` strict à `INSERT OR IGNORE` — nécessaire car `HashChain::append` en
mémoire n'a jamais dédupliqué les hash (un payload de contenu identique
soumis deux fois produit deux `AuditEntry` avec le même hash mais des `seq`
différents), un `INSERT` strict aurait donc fait planter la persistance dès
le premier renvoi d'un payload identique.

Vérifié en direct avec un **vrai arrêt puis redémarrage du binaire de
production** (pas seulement une simulation in-process): le second processus
affiche `[AuditStore] Loaded 2 entries from disk` et le payload suivant
continue correctement à `seq=2` (au lieu de repartir à `seq=0`/`parent=root`)
— confirmé aussi par un smoke-test dédié
(`examples/audit_persistence_smoke_test.rs`, 2 instances `CstlNativeServer`
séquentielles pointées sur le même fichier réel).

Limite assumée, pas encore faite: ceci reste DEUX connexions SQLite
(`adn_store`, `audit_store`) vers le MÊME fichier, pas encore fusionnées en
un seul schéma/une seule `Connection`/un seul verrou — un rapprochement réel
("un seul fichier" au lieu de deux systèmes disjoints), mais pas la fusion
complète.

### Couche 6: Interface Humaine
**État:** 🟡 PARTIEL

Escalade Obsidian (`src/obsidian_escalation.rs`): réelle, câblée live, vérifiée end-to-end contre un vrai vault (contradiction détectée par `ExecutionLab` → écrite dans `CSTL_Restricted_Council.md`). Graphify: réel désormais — corrige une contradiction avec README.md détectée par l'audit multi-angle du 2026-09-03 (ce document disait encore "inactif, pas installé" alors que README.md documentait déjà l'installation et la régénération). L'outil (`graphifyy`, PyPI, venv local) a été installé et le graphe régénéré le 2026-09-03 : 967 nœuds, 1800 arêtes, 63 communautés étiquetées sémantiquement, construit depuis le commit `3326f917`. Redevient stale après chaque nouveau commit tant que `graphify update .` n'est pas relancé.

### Couche 7: Agent Discovery & Routing (CSTL Natif)
**État:** ✅ CONSTRUITE ET CÂBLÉE LIVE — enregistrement désormais dynamique (2026-09-04)

`src/agent_discovery.rs`: Agent Registry, zero external dependencies, utilisée par chaque requête reçue par le serveur. Jusqu'au 2026-09-04, le registre était figé à la compilation (alice/bob codés en dur dans `main.rs`, `Arc` immuable) — aucune inscription dynamique possible, contrairement aux Agent Cards d'A2A. Corrigé: `AgentRegistry` est maintenant `Arc<Mutex<_>>`, et `purpose=agent_register` (nouveau message wire, `server/handler.rs`) permet à un agent de s'enregistrer/se réenregistrer (upsert par nom) via une auto-signature Ed25519 (réutilise la vérification de la Couche 2 ci-dessus) — vérifié en direct (mêmes 6 scénarios que la signature). Un agent LLM réel (`sdk/python/cstl_llm_agent.py`, Ed25519 via `cryptography`) s'enregistre et signe ses messages avec cette voie — vérifié en direct contre le serveur Rust réel (enregistrement, rejet du non-signé, acceptation du signé) ; la génération de contenu par un vrai modèle Claude reste à vérifier par l'utilisateur (aucun paquet `anthropic` ni clé API dans ce sandbox).

### Couche 8: Provenance Audit / Cryptographic Guarantee
**État:** 🟡 Hash-chained audit trail câblée live ET persistée (2026-09-04) ; "Deontic Modality Audit" reste un intitulé sans code correspondant

Hash-Chained Audit Trail (`src/server/audit.rs::HashChain`, canonicalisation NFC+BTreeMap, SHA-256): calculée et vérifiée en direct sur chaque payload depuis plusieurs passes de cette session, et depuis le 2026-09-04 sa continuité (`seq`/`parent_hash`) survit aussi à un redémarrage réel du serveur (voir Couche 5 ci-dessus pour le détail de ce correctif — `AuditStore` était du code mort jusque-là). Le badge "✅ DESIGNÉ" d'avant cette passe sous-estimait déjà ce qui existait (le hachage/chaînage tournait en production depuis longtemps) tout en survolant une vraie régression (la non-persistance) que rien ne signalait — corrigé aux deux bouts. "Deontic Modality Audit" reste un intitulé sans implémentation trouvée dans ce dépôt (recherche exhaustive: aucune occurrence hors de cette ligne) — pas vérifié dans cette passe, à traiter séparément si le besoin est réel.

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
3. Arbitration protocol structuré (quorum 2/3 implémenté et testé — voir Couche 2/3b; production encore configurée à un seul membre, donc quorum=1 en pratique)
4. Hash-chained immutable provenance
5. Relations au centre
6. Zero external dependencies (no MCP)

---

C'est la fondation. Le reste est détail d'implémentation — voir [`README.md`](../README.md) pour le détail vérifié fichier par fichier.
