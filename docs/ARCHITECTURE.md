# Architecture de CSTL

**Dernière mise à jour : 2026-11-21.** Ce document a été reconstruit à
partir du code : le graphe `graphify` recalculé, l'inventaire des fichiers,
et l'ordre réel des étapes dans `src/server/handler.rs`. L'ancienne version
(septembre 2026, « 10 couches ») est conservée dans
[`archive/ARCHITECTURE_2026-09.md`](../archive/ARCHITECTURE_2026-09.md). En
cas de doute, le code et la sortie des tests font foi sur ce texte (voir
`AGENTS.md`).

## En une phrase

CSTL est un serveur, en Rust, par lequel passent les messages entre agents
IA. Il vérifie qui parle (signatures), ce qui est dit (format, cohérence,
contradictions), et ce qu'un agent a le droit de faire (couche 9). Il garde
un historique que personne ne peut falsifier sans être détecté.

D'après les mesures (`scripts/team_task_benchmark.py`,
`scripts/relay_tamper_benchmark.py`), sa valeur vient de cette
architecture (source signée, serveur qui fait foi), pas du format du
langage : le JSON libre fait presque aussi bien quand rien n'est
falsifié.

## Vue d'ensemble

```
 Agents IA (SDK Python : CstlAgent)                       Humain
   │  messages CSTL signés (TCP 5050)                       │ Telegram / Obsidian / Graphify
   ▼                                                        ▲
┌──────────────────────── SERVEUR CSTL (Rust) ─────────────────────────┐
│ 1. Transport       listener · limites de connexion · TLS optionnel   │
│ 2. Format          parser strict · 40 opérateurs · validateur (E1xx-E3xx) │
│ 3. Identité        registre des agents · signatures Ed25519           │
│ 4. Vérifications   faits (Wikidata) · cohérence · audit déontique     │
│ 5. Couche 9        politique · conditions · décision · jeton signé    │
│ 6. Mémoire/audit   SQLite · chaîne de hachage signée · repère · témoin │
│ 7. Gouvernance     conseil · quorum · arbitrage · disjoncteur         │
│ 8. Sorties         réponse CSTL · API REST lecture seule (8000) · graphe │
└──────────────────────────────────────────────────────────────────────┘
   │ jeton signé                       │ maillons signés
   ▼                                   ▼
 Système visé (base, API)            Témoin externe (autre machine)
 + garde CstlGuard                   sdk/python/cstl_witness.py
```

**Taille (2026-11-21) :**
- 32 700 lignes de Rust dans `src/` et 8 400 lignes de SDK Python ;
- 48 exemples exécutables (`examples/`), 14 fichiers de tests
  d'intégration (`tests/`), 24 scripts (`scripts/`).

**Cœur du code, d'après `graphify god-nodes`** (les éléments les plus
connectés) : `AdnStore` (118 liens), `CstlNativeServer` (75),
`parse_payload` (75), `CstlDeonticEngine` (Python, 74), `CstlPayload`,
`AgentRegistry` et `CstlClient`.

## Le trajet d'un message

C'est l'ordre réel des étapes dans `src/server/handler.rs` (2 700 lignes).
Une demande spéciale (`purpose=...`) est traitée à son étape, puis le
serveur répond sans exécuter les étapes suivantes.

| Étape | Ce qui se passe | Fichier |
|---|---|---|
| 0 | Recherche de contenus dangereux | `src/security.rs` |
| 1 | Lecture du message (en-tête strict `#!CSTL vX.Y[.Z] MODE=A\|B\|C`) | `src/server/parser.rs` |
| 2 | Validation : opérateurs autorisés, champs obligatoires, objet manquant (E317), contradiction dans le message (E107) | `src/server/validator.rs`, `src/semantic.rs` |
| 2a | **Signature** Ed25519, vérifiée contre la clé ENREGISTRÉE de l'expéditeur. Obligatoire par défaut | `src/signing.rs` |
| 2-policy | `policy_query` : la politique en vigueur de l'agent | `src/server/action_authorization.rs` |
| 2-action | `action_request` : décision, entrée dans l'audit et **jeton signé** | `action_authorization.rs`, `src/server/server_key.rs` |
| 2b, 2c, 3b | Demandes spéciales : `council_decision`, `detect_emergence`, `arbiter_register`, `arbitrage_channel`, `agent_register`, `quorum_*` | `src/restricted_council.rs`, `src/emergence.rs`, `src/server/arbitrage.rs`, `src/server/quorum*.rs` |
| 3c | **Audit déontique** contre l'historique : signalé, ou refusé si `strict_deontic_history=true` (E107H) | `src/semantic.rs` |
| 3b | **Vérification des faits** dans Wikidata | `src/kb_verify.rs` |
| 3c | Cohérence : contradictions et cycles | `src/execution_lab.rs` |
| 3c | Escalade vers Obsidian ou Telegram en cas de contradiction | `src/obsidian_escalation.rs`, `src/telegram_council.rs` |
| 3d | Calibration de la confiance (`sigma`, moyenne mobile par agent), gouvernance (disjoncteur, dérive) | `src/calibration/`, `src/governance.rs` |
| 3e | **Écriture en mémoire et dans l'audit** : maillon signé, repère mis à jour, envoi au témoin | `src/adn_store.rs`, `src/server/audit.rs`, `src/server/witness.rs` |
| 3e | Détection des changements (`ADN_DELTA`) | `src/adn_delta_detector.rs` |
| 4-5 | Routage vers l'agent destinataire, puis réponse (compressée si demandée) | `src/agent_discovery.rs`, `src/server/response_compression.rs` |

## Les blocs

### 1. Transport

- **Écoute :** serveur TCP asynchrone (tokio) sur `CSTL_PORT` (5050).
  Il n'écoute qu'en local par défaut (`CSTL_BIND_HOST`).
- **Limites et chiffrement :** limites de connexion
  (`connection_limits.rs`), TLS optionnel (`tls.rs`).
- **API REST :** en lecture seule sur `127.0.0.1:CSTL_REST_PORT` (8000).
  Elle sert l'audit, les statistiques, le graphe et la calibration
  (`rest_api.rs`).

### 2. Format et langage

- **Blocs d'un message :** `META`, `INTENT_PAYLOAD`,
  `RELATION [type, subject, object, modality, ...]` et `---END---`.
- **Opérateurs :** 40 opérateurs officiels (`PERFORM`, `STATE`,
  `BELIEVES`, `BEFORE`...), listés dans `semantic.rs::official_operators`.
- **Modalités :** `MUST`, `MUST_NOT`, `REQUIRE` et `FORBID`.
- **Confiance :** un degré `sigma` sur une relation.
- **Attributs supplémentaires :** ils sont conservés (`extra_attrs`),
  par exemple `reason` ou les conditions de la couche 9.
- **Limites connues :** pas de quantificateurs, pas de propositions
  imbriquées, pas de tâches ni de délégation (voir `ROADMAP.md`).

### 3. Identité

- **Enregistrement :** chaque agent s'enregistre avec sa clé Ed25519
  (`agent_register`, auto-signé), et chaque message est signé.
- **Changement de clé d'un agent :** il exige une preuve de possession de
  l'ancienne clé.
- **Persistance :** le registre est rechargé depuis la base. Avant la
  PR #11, `main.rs` l'écrasait au démarrage, ce qui permettait de voler
  une identité après un redémarrage (vérifié, puis corrigé).
- **Mode développement :** `CSTL_DEV_MODE=1` accepte les messages non
  signés et crée `alice` et `bob` sans clé. Il est refusé si le serveur
  n'écoute pas qu'en local.

### 4. Couche 9 : le garde-fou

Fichiers : `src/server/action_authorization.rs`, `src/server/server_key.rs`,
`sdk/python/cstl_guard.py`.

- **Règles :** des relations `MUST` / `MUST_NOT` sur
  (agent ou `role:<nom>`, action).
- **Auteurs :** avec `CSTL_POLICY_AUTHORS`, seules comptent les règles
  signées par un auteur de politique dont la clé est enregistrée, et la
  plus récente décide. Les autres règles sont ignorées et comptées.
- **Conditions vérifiées par le serveur :**
  - `valid_from` / `valid_until` (période) ;
  - `amount_above` / `amount_at_most` (montants) ;
  - `if_fact` / `unless_fact` (faits enregistrés).

  Une condition que le serveur ne peut pas évaluer fait appliquer une
  interdiction, et pas une obligation.
- **Décision :** `action_request` (avec `amount=`) renvoie `allowed` ou
  `denied`, avec la raison. La décision est inscrite dans l'audit.
- **Jeton :** chaque `allowed` porte un jeton signé par la clé du
  serveur (agent, action, `request_id`, durée de validité, montant,
  `audit_hash`). Le garde du système visé refuse :
  - un jeton absent, falsifié ou signé par une autre clé ;
  - un jeton pour une autre action, un autre agent ou un autre montant ;
  - un jeton expiré ou déjà utilisé.
- **Politique effective :** `policy_query` renvoie à l'agent ses
  obligations, ses interdictions avec leur raison et leurs conditions,
  les règles ignorées et ses rôles.

### 5. Mémoire et audit

Fichiers : `src/adn_store.rs` (4 600 lignes, le plus gros module),
`src/server/audit.rs`, `src/server/server_key.rs`, `src/server/witness.rs`.

- **Base SQLite :**
  - messages (`adn_store`) et relations dérivées (`adn_relations`) ;
  - chaîne d'audit (`audit_trail`), avec la signature du serveur sur
    chaque maillon ;
  - agents, gouvernance, arbitrage et quorum.
- **Démarrage :** le serveur revérifie les liens, le contenu (empreinte
  recalculée), les relations dérivées, les signatures et le repère du
  dernier maillon. Il refuse de démarrer si quelque chose a été modifié
  (`CSTL_ACCEPT_BROKEN_CHAIN=1` pour forcer).
- **Clé du serveur :**
  - le fichier `<base>.server_key` (droits 0600), avec sa clé publique
    dans `<base>.server_key.pub` ;
  - `CSTL_SERVER_PUBLIC_KEY` permet d'épingler la clé ;
  - `--rotate-server-key` la change. Le passage de relais est signé par
    les deux clés, et l'ancienne clé ne vaut que pour les anciens
    maillons.
- **Témoin :** avec `CSTL_WITNESS_URL`, le serveur envoie chaque maillon
  au témoin et refuse de démarrer sur un historique plus ancien que celui
  qu'il a vu.

### 6. Gouvernance et humain

- **Gouvernance :**
  - un conseil restreint (`CSTL_COUNCIL_MEMBERS`) ;
  - un quorum protégé contre les faux votants (`quorum_sybil_test`) ;
  - un arbitrage avec revue par les pairs ;
  - un disjoncteur et une détection de dérive par agent.
- **Alertes vers un humain :** Telegram, et notes Obsidian en cas de
  contradiction.
- **`src/server/deontic_orchestration.rs` :** il ne fait qu'observer. Il
  journalise après coup et ne bloque rien.

### 7. Compression

Fichiers : `src/compression/`, `src/server/castle.rs`, `src/server/wai.rs`,
environ 5 000 lignes.

- Plusieurs étages de compression des réponses : WAI, rANS, FSE,
  dictionnaires.
- Ils sont branchés, mais d'après le README, CASTLE ne compresse rien sur
  le trafic réel.

### 8. Visualisation

- `src/server/graphify_server.rs` exporte le graphe des messages, des
  entités, des relations et des conflits.
- Il y ajoute la provenance cryptographique : signature valide ou non,
  clé, chaînage.

### 9. SDK Python

Dossier : `sdk/python/`.

- **`cstl_client.py` :** construire et lire les messages.
- **`cstl_llm_agent.py` :** `CstlAgent`, avec `register`,
  `send_relations`, `tell`, `request_action`, `authorize`, `act` et
  `policy`. Cerveaux disponibles : Claude, Hermes (Ollama) et Gemini
  (Gemini n'a jamais été testé pour de vrai).
- **`cstl_guard.py` :** le garde du système visé.
- **`cstl_witness.py` :** le témoin externe.
- **Autres :** des ponts vers Obsidian, Graphify et OpenClaw, et un
  moteur déontique en Python.

## État réel de chaque partie

| Partie | État |
|---|---|
| Couche 9, signatures, audit signé, témoin | **Testé et mesuré.** C'est ce qui donne le 48/48 contre les relais compromis, et le 48/48 sur la tâche d'équipe avec la politique effective. |
| Format, validation, audit déontique | Testé : détecte 12 erreurs sur 12, contre 5 sur 12 pour JSON Schema (`scripts/benchmark_cstl_vs_jsonschema.py`, qui bloque la CI en cas d'échec). |
| Vérification des faits (Wikidata) | Branchée, mais la plupart des faits restent `unchallenged_unproven`, et Wikidata bloque les requêtes trop fréquentes. |
| Calibration, dérive, émergence, conseil, quorum, arbitrage | Branchés et testés par des exemples. Leur utilité n'est pas mesurée. |
| Compression (CASTLE, WAI) | Branchée, sans gain sur le trafic réel. |
| `deontic_orchestration.rs` | Observation seulement. |

## Preuves et CI

- **Bancs d'essai (`scripts/`) :**
  - `team_task_benchmark.py` : la tâche d'équipe ;
  - `relay_tamper_benchmark.py` : le relais compromis ;
  - `benchmark_cstl_vs_jsonschema.py` : CSTL contre JSON Schema ;
  - `llm_relay_fidelity.py` : la fidélité des relais ;
  - `llm_agent_eval.py` : l'évaluation des agents.
- **CI (`.github/workflows/`) :** 2 workflows.
  - clippy sans avertissement et plus de 660 tests Rust ;
  - 14 scénarios en direct ;
  - les tests Python contre le vrai serveur ;
  - le banc d'essai JSON Schema.

## Variables d'environnement principales

| Variable | Rôle |
|---|---|
| `CSTL_PORT`, `CSTL_REST_PORT`, `CSTL_BIND_HOST` | Ports, et adresse d'écoute (locale par défaut) |
| `CSTL_DEV_MODE` | Essais locaux sans signature |
| `CSTL_REQUIRE_SIGNATURES` | Signatures obligatoires (`true` par défaut) |
| `CSTL_POLICY_AUTHORS` | Auteurs dont les règles comptent pour la couche 9 |
| `CSTL_ACTION_DEFAULT` | `allow` (défaut) ou `deny` (liste blanche) |
| `CSTL_AUTHZ_TTL_SECS` | Durée de validité d'un jeton (60 s par défaut) |
| `CSTL_SERVER_KEY_PATH`, `CSTL_SERVER_PUBLIC_KEY` | Emplacement de la clé du serveur, et épinglage |
| `CSTL_WITNESS_URL` | Témoin externe |
| `CSTL_ACCEPT_BROKEN_CHAIN` | Démarrer malgré un historique compromis |

La liste complète est dans `README.md`.

## Pour aller plus loin

- `AGENTS.md` : essayer CSTL en quelques commandes, et l'usage de la
  couche 9.
- `ROADMAP.md` : ce qui manque, et les mesures.
- `graphify explain "<symbole>"`, `graphify affected "<symbole>"` :
  naviguer dans le code sans tout lire.
