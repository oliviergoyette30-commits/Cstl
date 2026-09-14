# Couche 9: Event-driven Orchestration + Deontic Execution Model
**Date:** 2026-09-14 | **Status:** Implémentée et testée

## Résumé Exécutif

Couche 9 introduce une orchestration multi-agent basée sur les événements avec un modèle d'exécution deontic (MUST/MUST_NOT/MAY). Ceci permet au système CSTL de:

1. Dispatcher les événements à tous les agents connectés (tokio::broadcast)
2. Appliquer les règles deontic en ordre de priorité
3. Différencier entre obligations (MUST), interdictions (MUST_NOT) et permissions (MAY)
4. Persister les exécutions dans la base ADN pour audit

## Architecture

### Événements Supportés

```rust
pub enum DeonticEvent {
    AgentRegister { agent_id, agent_name, public_key, timestamp },
    MessageRelay { event_id, sender_id, receiver_id, payload, timestamp },
    ArbitrationRuling { ruling_id, case_id, arbiter_id, decision, timestamp },
    GovernanceBreach { breach_id, agent_id, breach_type, severity, timestamp },
}
```

### Modalités Deontic

```rust
pub enum DeonticModality {
    Must,     // Enforcement immédiate, non-négociable
    MustNot,  // Rejet + audit trail
    May,      // Log uniquement, avisoire
}
```

### Modèle d'Exécution

```
Event émis
    ↓
Broadcast à tous les subscribers
    ↓
Appliquer les règles deontic (triées par priorité DESC)
    ↓
Pour chaque règle:
    - Vérifier si la condition correspond
    - Si MUST → Exécuter immédiatement, log
    - Si MUST_NOT → Rejeter, ajouter à audit trail
    - Si MAY → Log only
    ↓
Persister les exécutions dans ADN
    ↓
Retourner la liste des exécutions au caller
```

## Composants Clés

### DeonticOrchestrator
- Gère le registre des règles
- Émit les événements et applique les règles
- Persiste les exécutions et les commentaires d'audit

### DeonticRule
- Spécifie: type_event, condition, modality, action, priority
- Constructeurs: `new_must()`, `new_must_not()`, `new_may()`
- Priorité (1-255): MUST_NOT (210) > MUST (200) > MAY (100)

### Intégration avec ADN Store
Deux nouvelles tables SQLite:
- `deontic_executions`: Enregistre chaque exécution de règle
- `audit_comments`: Commentaires libres pour l'audit

## Tests Implémentés

### 1. test_must_rule_enforcement
Vérifie qu'une règle MUST est appliquée sans condition.

**Résultat:** ✓ ExecutionResult::Success

### 2. test_must_not_rule_rejection
Vérifie qu'une règle MUST_NOT rejette l'événement.

**Résultat:** ✓ ExecutionResult::Rejected

### 3. test_may_rule_logging
Vérifie qu'une règle MAY est loggée sans effet opérationnel.

**Résultat:** ✓ ExecutionResult::Success

### 4. test_mixed_rules_priority
Teste 3 règles (MAY, MUST, MUST_NOT) avec priorités différentes.

**Résultat:** ✓ Exécution dans l'ordre: MUST_NOT > MUST > MAY

### 5. test_event_broadcast
Vérifie que les événements sont broadcastés aux subscribers.

**Résultat:** ✓ Broadcast fonctionne via tokio::broadcast::channel

### 6. test_multi_agent_scenario_with_governance_breach
Scénario complet: Alice s'enregistre (MUST), Bob déclenche une violation (MUST_NOT + MAY).

**Résultat:** ✓ 3 exécutions: registration (1), breach (2)

### 7. test_arbitration_ruling_workflow
Teste l'application d'une décision arbitrale (MUST rule).

**Résultat:** ✓ La décision est appliquée immédiatement

## Cas d'Usage Réels

### Scénario 1: Governance Enforcement
```rust
// MUST: Toujours mettre à jour l'état de gouvernance
let rule = DeonticRule::new_must(
    "governance_breach",
    "severity>=5",
    "escalate_to_council"
);

orchestrator.register_rule(rule).await;

let event = DeonticEvent::GovernanceBreach {
    breach_id: "breach_2026_001".to_string(),
    agent_id: "agent_bob".to_string(),
    breach_type: "high_latency".to_string(),
    severity: 8,
    timestamp: Utc::now(),
};

let executions = orchestrator.emit_event(event).await;
// → Escalade immédiate au conseil
```

### Scénario 2: Message Filtering
```rust
// MUST_NOT: Bloquer les messages de sources malveillantes
let rule = DeonticRule::new_must_not(
    "message_relay",
    "sender=blocked_agent_xyz",
    "quarantine_message"
);

// MAY: Logger toutes les communications
let rule_log = DeonticRule::new_may(
    "message_relay",
    "all_messages",
    "audit_communication"
);

// Agent bloqué → REJET immédiat
// Agent normal → Relai + log
```

### Scénario 3: Arbitration Finality
```rust
// MUST: Appliquer les décisions arbitrales
let rule = DeonticRule::new_must(
    "arbitration_ruling",
    "all_rulings",
    "apply_immediately"
).with_priority(250); // Très haute priorité
```

## Persistence et Audit

### Deontic Executions
Chaque exécution est persistée dans `deontic_executions`:
```sql
CREATE TABLE deontic_executions (
    execution_id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    modality TEXT NOT NULL,  -- "MUST", "MUST_NOT", "MAY"
    action TEXT NOT NULL,
    result TEXT NOT NULL,    -- "Success", "Rejected", "NoMatch", "Failed"
    timestamp INTEGER NOT NULL
);
```

### Audit Comments
Les rejections MUST_NOT créent des commentaires d'audit:
```sql
CREATE TABLE audit_comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    comment TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);
```

## Performance

- **Event Broadcasting:** O(1) pour chaque subscriber via tokio::broadcast
- **Rule Matching:** O(n) où n = nombre de règles (généralement < 50)
- **Persistence:** Asynchrone via try_lock() sur AdnStore, jamais bloquant
- **Capacity:** 100 événements en queue par défaut, configurable

## Intégration avec Couches Précédentes

- **Couche 2 (Gouvernance):** Cascade des breaches vers le tracker
- **Couche 3b (Arbitrage):** MUST rules pour appliquer les rulings
- **Couche 5 (ADN):** Persist des exécutions et commentaires
- **Couche 8 (Audit Deontic):** Historique des rejections

## Limitation Actuelle et Améliorations Futures

### Actuellement
- Matching de condition simplifié (pattern matching lexical)
- Pas d'évaluation d'expressions complexes
- Pas de rollback des exécutions MUST

### Prochaines Versions
- Expression matcher avec DSL deontic
- Transaction rollback pour les MUST failures
- Metrics Prometheus pour monitoring
- Snapshot/checkpoint des états de règles

## Files de Compilation et Warnings

Aucun warning d'erreur pour le module deontic_orchestration.

Warning (ignoré sûrement):
- `#[allow(dead_code)]` sur fields non-utilisés dans les tests

## Commande de Test

```bash
cargo test --lib server::deontic_orchestration
# → 7 tests passed
```

## Fichiers Modifiés

1. **src/server/deontic_orchestration.rs** (nouveau, 625 lignes)
2. **src/server/mod.rs** (expose deontic_orchestration)
3. **src/adn_store.rs** (tables + méthodes de persistence)

## Références

- RFC: Event-driven Architecture (Martin Fowler, 2017)
- Deontic Logic: Von Wright (1951)
- CSTL Design Document: Layer 9 spec
