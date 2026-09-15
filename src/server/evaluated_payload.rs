//! EvaluatedPayload — Double Livre layer 2 (metadata separation)
//!
//! The "Double Livre" pattern separates two concerns:
//! - Layer 1: raw_payload (immutable, signed, cryptographic guarantee)
//! - Layer 2: server_evaluation (mutable, server-side only, never signed)
//!
//! This module implements layer 2: the metadata wrapper that holds
//! server calibration results without ever modifying the signed payload.

use serde::{Deserialize, Serialize};
use crate::server::parser::CstlPayload;
use crate::calibration::EwmaCalibration;

/// Layer 2: Server-side evaluation metadata
///
/// This struct NEVER replaces the original CstlPayload. Instead, it wraps it
/// and adds computed confidence metrics that remain separate in the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatedPayload {
    /// Layer 1: Original immutable payload (signed, hashed)
    pub raw_payload: CstlPayload,

    /// SHA-256 hash of raw_payload (immutable reference)
    pub payload_hash: String,

    /// Layer 2 Metadata: Server-computed effective sigma
    /// Result of blending agent's reported sigma with server's accuracy assessment
    pub server_sigma_effective: f64,

    /// Layer 2 Metadata: Agent's trust score snapshot at evaluation time
    /// Captures the EWMA accuracy state when this payload was processed
    pub agent_trust_snapshot: f64,

    /// Layer 2 Metadata: EWMA calibration state at time of evaluation
    pub ewma_calibration_at_time: Option<EwmaCalibration>,

    /// Layer 2 Metadata: Timestamp of server evaluation (UTC)
    pub evaluated_at_utc: chrono::DateTime<chrono::Utc>,

    /// Layer 2 Metadata: Parent payload hash (from immutable audit chain)
    pub parent_payload_hash: Option<String>,

    /// Layer 2 Metadata: Governance state snapshot at evaluation time
    pub governance_state_snapshot: Option<GovernanceSnapshot>,

    /// Layer 2 Metadata: Verdicts collected post-evaluation
    /// Used to update EWMA calibration after ground truth becomes available
    pub collected_verdicts: Vec<VerdictRecord>,
}

/// Snapshot of governance state (Couche 4) at evaluation time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceSnapshot {
    pub sender: String,
    pub breaker_trips: u32,
    pub circuit_state: String,  // "open", "closed", "half_open"
    pub drift_ratio: f64,
    pub drift_flagged: bool,
    pub semantic_warnings_count: u32,
}

/// Post-evaluation verdict record (ground truth)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictRecord {
    pub timestamp_utc: chrono::DateTime<chrono::Utc>,
    pub is_correct: bool,
    pub verdict_source: String,  // "human_validation", "cross_model_consensus", "later_reconciliation"
    pub confidence: f64,  // Verdict confidence (0.0..1.0)
}

impl EvaluatedPayload {
    /// Create a new evaluated payload
    pub fn new(
        raw_payload: CstlPayload,
        payload_hash: String,
        server_sigma_effective: f64,
        agent_trust_snapshot: f64,
    ) -> Self {
        Self {
            raw_payload,
            payload_hash,
            server_sigma_effective,
            agent_trust_snapshot,
            ewma_calibration_at_time: None,
            evaluated_at_utc: chrono::Utc::now(),
            parent_payload_hash: None,
            governance_state_snapshot: None,
            collected_verdicts: Vec::new(),
        }
    }

    /// Add EWMA calibration snapshot
    pub fn with_ewma_calibration(mut self, calibration: EwmaCalibration) -> Self {
        self.ewma_calibration_at_time = Some(calibration);
        self
    }

    /// Add governance state snapshot
    pub fn with_governance_snapshot(mut self, snapshot: GovernanceSnapshot) -> Self {
        self.governance_state_snapshot = Some(snapshot);
        self
    }

    /// Add parent hash reference (from audit chain)
    pub fn with_parent_hash(mut self, parent_hash: String) -> Self {
        self.parent_payload_hash = Some(parent_hash);
        self
    }

    /// Collect a verdict after the fact
    pub fn add_verdict(&mut self, is_correct: bool, source: String, confidence: f64) {
        self.collected_verdicts.push(VerdictRecord {
            timestamp_utc: chrono::Utc::now(),
            is_correct,
            verdict_source: source,
            confidence,
        });
    }

    /// Get all collected verdicts
    pub fn verdicts(&self) -> &[VerdictRecord] {
        &self.collected_verdicts
    }

    /// Check if evaluation has governance state (the most critical metadata)
    pub fn is_complete(&self) -> bool {
        self.governance_state_snapshot.is_some()
    }

    /// Get sender from the raw payload
    pub fn sender(&self) -> Option<String> {
        self.raw_payload.intent.get("sender").cloned()
    }

    /// Get purpose from the raw payload
    pub fn purpose(&self) -> Option<String> {
        self.raw_payload.intent.get("purpose").cloned()
    }

    /// Compute a confidence-weighted verdict status
    /// Returns: (overall_correct, confidence, sample_count)
    pub fn compute_verdict_consensus(&self) -> Option<(bool, f64, usize)> {
        if self.collected_verdicts.is_empty() {
            return None;
        }

        let total_weight: f64 = self.collected_verdicts.iter().map(|v| v.confidence).sum();
        if total_weight == 0.0 {
            return None;
        }

        let weighted_correct: f64 = self.collected_verdicts
            .iter()
            .map(|v| if v.is_correct { v.confidence } else { 0.0 })
            .sum();

        let consensus = weighted_correct / total_weight > 0.5;
        let avg_confidence = total_weight / self.collected_verdicts.len() as f64;

        Some((consensus, avg_confidence, self.collected_verdicts.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluated_payload_creation() {
        let raw = CstlPayload::default();
        let evaluated = EvaluatedPayload::new(
            raw,
            "abc123".to_string(),
            0.85,
            0.9,
        );

        assert_eq!(evaluated.server_sigma_effective, 0.85);
        assert_eq!(evaluated.agent_trust_snapshot, 0.9);
        assert!(!evaluated.is_complete(), "Should not be complete without governance snapshot");
    }

    #[test]
    fn test_verdict_consensus() {
        let raw = CstlPayload::default();
        let mut evaluated = EvaluatedPayload::new(
            raw,
            "abc123".to_string(),
            0.85,
            0.9,
        );

        evaluated.add_verdict(true, "consensus".to_string(), 0.95);
        evaluated.add_verdict(true, "consensus".to_string(), 0.90);
        evaluated.add_verdict(false, "consensus".to_string(), 0.10);

        let (is_correct, avg_conf, count) = evaluated.compute_verdict_consensus().unwrap();
        assert!(is_correct, "Majority correct verdicts");
        assert_eq!(count, 3);
        assert!(avg_conf > 0.6);
    }
}
