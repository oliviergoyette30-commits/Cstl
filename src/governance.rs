//! src/governance.rs — Couche 2 (Gouvernance / Résilience), v1.
//!
//! Avant ce module, la Couche 2 était entièrement vide: aucun circuit
//! breaker, aucune détection de drift d'opérateur n'existait nulle part
//! dans ce dépôt (README.md ligne 49, docs/ARCHITECTURE.md lignes 42-50),
//! malgré le badge "✅ Tested (4/4 modes)" qui a longtemps affirmé le
//! contraire à tort.
//!
//! Portée v1, décidée explicitement (2026-09-03): **observation seule**.
//! Ce module calcule un état par expéditeur (breaker ouvert/fermé, ratio
//! de drift) et l'expose dans la réponse serveur (bloc `GOVERNANCE`), et
//! déclenche une escalade Telegram plus urgente quand un seuil est
//! franchi — mais ne rejette JAMAIS un payload. C'est cohérent avec le
//! seul mécanisme de blocage réel du pipeline (sécurité/parse/validation,
//! `handler.rs` STEP 0/1/2): tout le reste (vérification KB, cohérence,
//! avertissements d'opérateurs sémantiques) est déjà purement consultatif.
//! Ajouter un chemin bloquant ici aurait introduit un précédent que rien
//! n'a demandé.
//!
//! Limite v1 assumée: l'état est en mémoire uniquement
//! (`Arc<Mutex<GovernanceTracker>>` sur le serveur, même schéma que
//! `chain`/`adn_store`) — il repart à zéro à chaque redémarrage du
//! serveur. Acceptable parce que le mécanisme est une fenêtre glissante
//! auto-cicatrisante par construction, contrairement à l'audit/la
//! provenance (`adn_store.rs`) qui doivent, elles, survivre à un
//! redémarrage.
//!
//! Un seul mécanisme générique ("compteur d'événements par expéditeur, à
//! fenêtre glissante, avec une étiquette de raison") sert à la fois pour
//! le circuit breaker (événements d'incohérence, `execution_lab.rs`) et
//! pour le drift d'opérateur (avertissements `SEMANTIC_WARNING`,
//! `validator::check_sdl_operator_whitelist`) plutôt que deux trackers
//! séparés.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Fenêtre du circuit breaker: nombre d'incohérences tolérées sur cette
/// durée avant que le circuit ne s'ouvre pour cet expéditeur.
pub const BREAKER_WINDOW: Duration = Duration::from_secs(600); // 10 min
/// Seuil de déclenchement du breaker: 3 événements d'incohérence dans la fenêtre.
pub const BREAKER_THRESHOLD: u32 = 3;
/// Fenêtre d'observation du drift d'opérateur.
pub const DRIFT_WINDOW: Duration = Duration::from_secs(3600); // 1h
/// Pas de ratio de drift calculé avant d'avoir observé au moins ce nombre
/// de payloads pour un expéditeur donné (évite un ratio 100% trompeur sur 1 seul payload).
pub const DRIFT_MIN_SAMPLES: u32 = 5;
/// Ratio (payloads avec SEMANTIC_WARNING / total) au-delà duquel le drift est signalé.
pub const DRIFT_RATIO_THRESHOLD: f64 = 0.5;
/// Anti-spam: pas plus d'une alerte Telegram par expéditeur sur cette durée.
pub const ALERT_COOLDOWN: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventReason {
    Inconsistency,
    SemanticWarning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GovernanceState {
    pub circuit_open: bool,
    pub breaker_trips: u32,
    pub drift_ratio: f64,
    pub drift_flagged: bool,
    /// true si (circuit_open || drift_flagged) ET le cooldown d'alerte est écoulé.
    pub should_alert: bool,
}

struct SenderWindow {
    /// Horodatages des événements d'incohérence dans la fenêtre du breaker.
    inconsistency_events: Vec<Instant>,
    /// (horodatage, avait_un_semantic_warning) pour chaque payload observé,
    /// dans la fenêtre de drift.
    drift_samples: Vec<(Instant, bool)>,
    last_alert: Option<Instant>,
}

impl SenderWindow {
    fn new() -> Self {
        Self { inconsistency_events: Vec::new(), drift_samples: Vec::new(), last_alert: None }
    }
}

pub struct GovernanceTracker {
    senders: HashMap<String, SenderWindow>,
    breaker_window: Duration,
    breaker_threshold: u32,
    drift_window: Duration,
    drift_min_samples: u32,
    drift_ratio_threshold: f64,
    alert_cooldown: Duration,
}

impl GovernanceTracker {
    /// Constructeur réel, utilisé par le serveur — fenêtres/seuils par défaut.
    pub fn with_defaults() -> Self {
        Self::new(
            BREAKER_WINDOW,
            BREAKER_THRESHOLD,
            DRIFT_WINDOW,
            DRIFT_MIN_SAMPLES,
            DRIFT_RATIO_THRESHOLD,
            ALERT_COOLDOWN,
        )
    }

    /// Constructeur explicite — permet aux tests d'injecter des fenêtres
    /// de quelques millisecondes plutôt que de dormir 10 minutes pour
    /// vérifier un reset.
    pub fn new(
        breaker_window: Duration,
        breaker_threshold: u32,
        drift_window: Duration,
        drift_min_samples: u32,
        drift_ratio_threshold: f64,
        alert_cooldown: Duration,
    ) -> Self {
        Self {
            senders: HashMap::new(),
            breaker_window,
            breaker_threshold,
            drift_window,
            drift_min_samples,
            drift_ratio_threshold,
            alert_cooldown,
        }
    }

    /// Enregistre un événement pour `sender` (0..n raisons portées par ce
    /// payload) et retourne l'état de gouvernance à jour pour cet
    /// expéditeur. Ne bloque jamais rien — c'est à l'appelant (handler.rs)
    /// de décider quoi faire de `should_alert`.
    pub fn record(&mut self, sender: &str, reasons: &[EventReason]) -> GovernanceState {
        let now = Instant::now();
        let breaker_window = self.breaker_window;
        let breaker_threshold = self.breaker_threshold;
        let drift_window = self.drift_window;
        let drift_min_samples = self.drift_min_samples;
        let drift_ratio_threshold = self.drift_ratio_threshold;
        let alert_cooldown = self.alert_cooldown;

        let window = self.senders.entry(sender.to_string()).or_insert_with(SenderWindow::new);

        if reasons.contains(&EventReason::Inconsistency) {
            window.inconsistency_events.push(now);
        }
        window.inconsistency_events.retain(|t| now.duration_since(*t) <= breaker_window);

        let had_semantic_warning = reasons.contains(&EventReason::SemanticWarning);
        window.drift_samples.push((now, had_semantic_warning));
        window.drift_samples.retain(|(t, _)| now.duration_since(*t) <= drift_window);

        let breaker_trips = window.inconsistency_events.len() as u32;
        let circuit_open = breaker_trips >= breaker_threshold;

        let total_samples = window.drift_samples.len() as u32;
        let warned_samples = window.drift_samples.iter().filter(|(_, w)| *w).count() as u32;
        let drift_ratio = if total_samples > 0 {
            warned_samples as f64 / total_samples as f64
        } else {
            0.0
        };
        let drift_flagged = total_samples >= drift_min_samples && drift_ratio >= drift_ratio_threshold;

        let wants_alert = circuit_open || drift_flagged;
        let cooldown_elapsed = match window.last_alert {
            None => true,
            Some(t) => now.duration_since(t) >= alert_cooldown,
        };
        let should_alert = wants_alert && cooldown_elapsed;
        if should_alert {
            window.last_alert = Some(now);
        }

        GovernanceState { circuit_open, breaker_trips, drift_ratio, drift_flagged, should_alert }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn tracker_for_tests() -> GovernanceTracker {
        // Fenêtres en millisecondes: pas de sleep de 10 minutes dans les tests.
        GovernanceTracker::new(ms(200), 3, ms(500), 3, 0.5, ms(100))
    }

    #[test]
    fn test_normal_payload_no_reasons_leaves_circuit_closed() {
        let mut t = tracker_for_tests();
        let state = t.record("alice", &[]);
        assert!(!state.circuit_open);
        assert_eq!(state.breaker_trips, 0);
        assert!(!state.should_alert);
    }

    #[test]
    fn test_breaker_trips_below_threshold_stays_closed() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::Inconsistency]);
        let state = t.record("alice", &[EventReason::Inconsistency]);
        assert_eq!(state.breaker_trips, 2);
        assert!(!state.circuit_open);
    }

    #[test]
    fn test_breaker_opens_at_threshold_but_never_blocks() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        let state = t.record("alice", &[EventReason::Inconsistency]);
        assert_eq!(state.breaker_trips, 3);
        assert!(state.circuit_open);
        // "should_alert" est un signal d'escalade, pas un rejet — le
        // module n'a ni le pouvoir ni la fonction de bloquer un payload.
        assert!(state.should_alert);
    }

    #[test]
    fn test_breaker_resets_after_window_elapses() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        std::thread::sleep(ms(250)); // > breaker_window (200ms)
        let state = t.record("alice", &[]);
        assert_eq!(state.breaker_trips, 0);
        assert!(!state.circuit_open);
    }

    #[test]
    fn test_breaker_is_isolated_per_sender() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        let bob_state = t.record("bob", &[]);
        assert_eq!(bob_state.breaker_trips, 0);
        assert!(!bob_state.circuit_open);
    }

    #[test]
    fn test_drift_ratio_ignored_below_min_samples() {
        let mut t = tracker_for_tests();
        // drift_min_samples=3 dans tracker_for_tests()
        t.record("alice", &[EventReason::SemanticWarning]);
        let state = t.record("alice", &[EventReason::SemanticWarning]);
        assert!(!state.drift_flagged, "seulement 2 échantillons, min_samples=3");
    }

    #[test]
    fn test_drift_flagged_once_ratio_and_min_samples_met() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::SemanticWarning]);
        t.record("alice", &[EventReason::SemanticWarning]);
        let state = t.record("alice", &[EventReason::SemanticWarning]);
        assert_eq!(state.drift_ratio, 1.0);
        assert!(state.drift_flagged);
    }

    #[test]
    fn test_drift_ratio_below_threshold_not_flagged() {
        let mut t = tracker_for_tests();
        t.record("alice", &[]);
        t.record("alice", &[]);
        let state = t.record("alice", &[EventReason::SemanticWarning]);
        assert!(state.drift_ratio < 0.5);
        assert!(!state.drift_flagged);
    }

    #[test]
    fn test_alert_cooldown_debounces_repeated_alerts() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        let first = t.record("alice", &[EventReason::Inconsistency]);
        assert!(first.should_alert);
        // Immédiatement après: circuit toujours ouvert mais cooldown actif.
        let second = t.record("alice", &[EventReason::Inconsistency]);
        assert!(second.circuit_open);
        assert!(!second.should_alert, "cooldown pas encore ecoule");
    }

    #[test]
    fn test_alert_fires_again_after_cooldown_elapses() {
        let mut t = tracker_for_tests();
        t.record("alice", &[EventReason::Inconsistency]);
        t.record("alice", &[EventReason::Inconsistency]);
        let first = t.record("alice", &[EventReason::Inconsistency]);
        assert!(first.should_alert);
        std::thread::sleep(ms(150)); // > alert_cooldown (100ms)
        let later = t.record("alice", &[EventReason::Inconsistency]);
        assert!(later.circuit_open);
        assert!(later.should_alert, "cooldown ecoule, une nouvelle alerte doit pouvoir partir");
    }
}
