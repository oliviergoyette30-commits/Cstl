//! EWMA (Exponentially Weighted Moving Average) adaptive calibration
//! for inter-LLM sigma standardization (Double Livre pattern v5.2)
//!
//! This module implements rapid adaptation of confidence scores across
//! different LLM backends without modifying signed payloads.

use serde::{Deserialize, Serialize};

/// EWMA calibration state for a single agent sender
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EwmaCalibration {
    /// Agent name / sender identifier
    pub agent_name: String,

    /// Current accuracy score (0.0 = always wrong, 1.0 = always correct)
    pub accuracy_score: f64,

    /// Number of samples used to compute this EWMA
    pub sample_count: usize,

    /// Smoothing factor for exponential decay (0.0 < alpha <= 1.0)
    /// Recommended: 0.2 for rapid adaptation (~50 message effective window)
    pub alpha: f64,
}

impl EwmaCalibration {
    /// Initialize a new EWMA calibration for an agent
    pub fn new(agent_name: String, initial_score: f64, alpha: f64) -> Self {
        assert!(alpha > 0.0 && alpha <= 1.0, "alpha must be in (0.0, 1.0]");
        assert!(initial_score >= 0.0 && initial_score <= 1.0, "score must be in [0.0, 1.0]");

        Self {
            agent_name,
            accuracy_score: initial_score,
            sample_count: 1,
            alpha,
        }
    }

    /// Update EWMA with a new verdict (correct/incorrect)
    ///
    /// Formula: score_new = (alpha * verdict) + ((1 - alpha) * score_old)
    /// where verdict = 1.0 if correct, 0.0 if incorrect
    pub fn update(&mut self, is_correct: bool) {
        let verdict_val = if is_correct { 1.0 } else { 0.0 };
        self.accuracy_score = (self.alpha * verdict_val) + ((1.0 - self.alpha) * self.accuracy_score);
        self.sample_count += 1;
    }

    /// Get the current calibrated score
    pub fn score(&self) -> f64 {
        self.accuracy_score.clamp(0.0, 1.0)
    }

    /// Check if the agent has converged (stabilized around a value)
    /// Convergence is assumed after a minimum number of samples
    pub fn is_converged(&self) -> bool {
        // Effective window = 1/alpha samples (logarithmic, not strict)
        // For alpha=0.2, effective_window ≈ 5 samples
        // For convergence, require at least 10× effective window
        let effective_window = 1.0 / self.alpha;
        self.sample_count as f64 > effective_window * 10.0
    }

    /// Estimate the confidence interval width based on sample count
    /// Uses square root of sample count as a rough confidence measure
    pub fn confidence_margin(&self) -> f64 {
        if self.sample_count < 2 {
            1.0 // Maximum uncertainty
        } else {
            (1.0 / (self.sample_count as f64).sqrt()).min(1.0)
        }
    }
}

/// Server-side sigma (σ) adjustment using EWMA calibration
///
/// The "Double Livre" pattern:
/// - Payload contains agent_sigma (what the agent believes)
/// - Server computes server_sigma_effective (calibrated confidence)
/// - These remain separate in audit trail (layer 2 metadata)
pub struct SigmaCalibrator {
    /// EWMA calibrations per agent
    calibrations: std::collections::HashMap<String, EwmaCalibration>,

    /// Default alpha for new agents (recommended: 0.2)
    default_alpha: f64,
}

impl SigmaCalibrator {
    /// Create a new sigma calibrator
    pub fn new(default_alpha: f64) -> Self {
        assert!(default_alpha > 0.0 && default_alpha <= 1.0);
        Self {
            calibrations: std::collections::HashMap::new(),
            default_alpha,
        }
    }

    /// Get or create EWMA calibration for an agent
    pub fn get_or_create_calibration(&mut self, agent_name: String, initial_score: f64) -> &mut EwmaCalibration {
        self.calibrations
            .entry(agent_name.clone())
            .or_insert_with(|| {
                EwmaCalibration::new(agent_name, initial_score, self.default_alpha)
            })
    }

    /// Update the calibration after receiving ground truth about a payload
    ///
    /// This simulates a correctness verdict arriving (e.g., from human validation,
    /// cross-model consensus, or later reconciliation).
    ///
    /// If no calibration exists for this agent, one is created at neutral (0.5)
    /// and then updated with the verdict.
    pub fn observe_verdict(&mut self, agent_name: &str, is_correct: bool) {
        let calibration = self.get_or_create_calibration(agent_name.to_string(), 0.5);
        calibration.update(is_correct);
    }

    /// Compute effective server-side sigma given:
    /// - agent_sigma: what the agent reported
    /// - agent_name: identity for lookup
    /// Returns adjusted sigma accounting for agent's historical accuracy
    pub fn compute_effective_sigma(&mut self, agent_name: &str, agent_sigma: f64) -> f64 {
        // Get or create calibration (starts at neutral 0.5)
        let calibration = self.get_or_create_calibration(agent_name.to_string(), 0.5);

        // Blend agent's sigma with server's accuracy assessment
        // If accuracy_score = 1.0, use agent_sigma as-is
        // If accuracy_score = 0.5 (neutral), use 0.5 as conservative default
        // If accuracy_score = 0.0, invert to (1.0 - agent_sigma)

        let accuracy = calibration.score();

        // Linear interpolation:
        // effective = agent_sigma * accuracy + (1 - agent_sigma) * (1 - accuracy)
        // This gives:
        //   - When agent_sigma=0.9 and accuracy=1.0 → 0.9 (trust the agent)
        //   - When agent_sigma=0.9 and accuracy=0.0 → 0.1 (invert)
        //   - When agent_sigma=0.9 and accuracy=0.5 → 0.5 (maximum uncertainty)

        let effective = (agent_sigma * accuracy) + ((1.0 - agent_sigma) * (1.0 - accuracy));
        effective.clamp(0.0, 1.0)
    }

    /// Get current calibration state for monitoring/debugging
    pub fn get_calibration(&self, agent_name: &str) -> Option<&EwmaCalibration> {
        self.calibrations.get(agent_name)
    }

    /// Get all calibrations as a snapshot
    pub fn get_all_calibrations(&self) -> Vec<EwmaCalibration> {
        self.calibrations.values().cloned().collect()
    }

    /// Reset calibration for an agent (when re-registering or after known corruption)
    pub fn reset_calibration(&mut self, agent_name: &str) {
        self.calibrations.remove(agent_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ewma_basic_convergence() {
        let mut ewma = EwmaCalibration::new("test_agent".to_string(), 0.5, 0.2);

        // Feed correct verdicts
        for _ in 0..10 {
            ewma.update(true);
        }

        // Should converge towards 1.0
        assert!(ewma.score() > 0.9, "After 10 correct verdicts, score should be > 0.9, got {}", ewma.score());
    }

    #[test]
    fn test_ewma_handles_wrong_verdicts() {
        let mut ewma = EwmaCalibration::new("test_agent".to_string(), 1.0, 0.2);

        // Feed wrong verdicts
        for _ in 0..5 {
            ewma.update(false);
        }

        // Should decay from 1.0
        assert!(ewma.score() < 0.5, "After 5 wrong verdicts, score should drop, got {}", ewma.score());
    }

    #[test]
    fn test_sigma_calibrator_blending() {
        let mut calibrator = SigmaCalibrator::new(0.2);

        // Initial state: unknown agent, neutral accuracy
        let sigma1 = calibrator.compute_effective_sigma("agent_a", 0.9);
        assert_eq!(sigma1, 0.5, "Unknown agent with neutral accuracy should give 0.5");

        // Simulate the agent being correct 10 times
        for _ in 0..10 {
            calibrator.observe_verdict("agent_a", true);
        }

        // Now high sigma should be trusted
        let sigma2 = calibrator.compute_effective_sigma("agent_a", 0.9);
        assert!(sigma2 > 0.8, "After proving accuracy, 0.9 sigma should be trusted, got {}", sigma2);
    }

    #[test]
    fn test_convergence_detection() {
        let mut ewma = EwmaCalibration::new("test".to_string(), 0.5, 0.2);

        assert!(!ewma.is_converged(), "Should not converge with 1 sample");

        // 0.2 alpha → effective window = 5 samples
        // Need 50 samples for convergence
        for _ in 0..50 {
            ewma.update(true);
        }

        assert!(ewma.is_converged(), "Should converge after 50+ samples with alpha=0.2");
    }

    #[test]
    fn test_confidence_margin_decreases_with_samples() {
        let mut ewma = EwmaCalibration::new("test".to_string(), 0.5, 0.2);

        let margin_1 = ewma.confidence_margin();

        for _ in 0..100 {
            ewma.update(true);
        }

        let margin_100 = ewma.confidence_margin();
        assert!(margin_100 < margin_1, "Confidence margin should decrease with more samples");
    }

    #[test]
    fn test_clamp_bounds() {
        // Test that score() always returns clamped values even after extreme updates
        let mut ewma = EwmaCalibration::new("test".to_string(), 0.5, 1.0); // alpha=1.0 for extreme movements

        // Feed many corrects to push towards 1.0
        for _ in 0..20 {
            ewma.update(true);
        }
        // Even with extreme alpha, score should be clamped
        assert!(ewma.score() >= 0.0 && ewma.score() <= 1.0, "Score must be in [0.0, 1.0]");

        // Feed many wrongs to push towards 0.0
        for _ in 0..20 {
            ewma.update(false);
        }
        assert!(ewma.score() >= 0.0 && ewma.score() <= 1.0, "Score must be in [0.0, 1.0]");
    }
}
