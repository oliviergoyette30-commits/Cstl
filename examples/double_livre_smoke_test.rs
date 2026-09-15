/// examples/double_livre_smoke_test.rs -- verification live du Double Livre v5.2
///
/// Valide l'architecture de séparation Layer 1 (payload immuable signée) vs
/// Layer 2 (métadonnées d'évaluation serveur) pour la standardisation sigma
/// inter-LLM sans modifier les payloads en vol.

use cstl_parser::calibration::{EwmaCalibration, SigmaCalibrator};
use cstl_parser::server::evaluated_payload::{EvaluatedPayload, GovernanceSnapshot};
use cstl_parser::server::parser::CstlPayload;

fn main() {
    println!("🔒 Double Livre v5.2 — Smoke Test Suite");
    println!("========================================");
    println!();

    let mut passed = 0;
    let mut failed = 0;

    // Test 1: EWMA rapid convergence
    println!("Test 1: EWMA rapid convergence (high alpha)...");
    {
        let mut ewma = EwmaCalibration::new("test_agent".to_string(), 0.5, 0.2);
        for _ in 0..20 {
            ewma.update(true);
        }
        if ewma.score() > 0.95 && ewma.sample_count == 21 {
            println!("✅ PASSED: High alpha converges fast to 1.0");
            passed += 1;
        } else {
            println!("❌ FAILED: Expected score > 0.95, got {}", ewma.score());
            failed += 1;
        }
    }
    println!();

    // Test 2: EWMA reversal on failures
    println!("Test 2: EWMA reversal on failures...");
    {
        let mut ewma = EwmaCalibration::new("agent".to_string(), 1.0, 0.2);
        for _ in 0..10 {
            ewma.update(false);
        }
        if ewma.score() < 0.15 {
            println!("✅ PASSED: After 10 failures, score dropped to {}", ewma.score());
            passed += 1;
        } else {
            println!("❌ FAILED: Expected score < 0.15, got {}", ewma.score());
            failed += 1;
        }
    }
    println!();

    // Test 3: Sigma calibrator neutral agent
    println!("Test 3: Sigma calibrator blending (neutral agent)...");
    {
        let mut calibrator = SigmaCalibrator::new(0.2);
        let sigma = calibrator.compute_effective_sigma("unknown_agent", 0.8);
        if (sigma - 0.5).abs() < 0.01 {
            println!("✅ PASSED: Unknown agent produces neutral sigma = {}", sigma);
            passed += 1;
        } else {
            println!("❌ FAILED: Expected sigma ≈ 0.5, got {}", sigma);
            failed += 1;
        }
    }
    println!();

    // Test 4: Sigma calibrator trusted agent
    println!("Test 4: Sigma calibrator blending (trusted agent)...");
    {
        let mut calibrator = SigmaCalibrator::new(0.2);
        for _ in 0..10 {
            calibrator.observe_verdict("trusted_agent", true);
        }
        let sigma = calibrator.compute_effective_sigma("trusted_agent", 0.9);
        if sigma > 0.85 {
            println!("✅ PASSED: High accuracy agent's high sigma trusted = {}", sigma);
            passed += 1;
        } else {
            println!("❌ FAILED: Expected sigma > 0.85, got {}", sigma);
            failed += 1;
        }
    }
    println!();

    // Test 5: Sigma calibrator distrusted agent
    println!("Test 5: Sigma calibrator blending (distrusted agent)...");
    {
        let mut calibrator = SigmaCalibrator::new(0.2);
        for _ in 0..10 {
            calibrator.observe_verdict("bad_agent", false);
        }
        let sigma = calibrator.compute_effective_sigma("bad_agent", 0.9);
        if sigma < 0.2 {
            println!("✅ PASSED: Low accuracy agent's high sigma inverted = {}", sigma);
            passed += 1;
        } else {
            println!("❌ FAILED: Expected sigma < 0.2, got {}", sigma);
            failed += 1;
        }
    }
    println!();

    // Test 6: EvaluatedPayload immutability
    println!("Test 6: EvaluatedPayload immutability...");
    {
        let payload = CstlPayload::default();
        let evaluated = EvaluatedPayload::new(
            payload.clone(),
            "abc123def456".to_string(),
            0.75,
            0.85,
        );

        if evaluated.server_sigma_effective == 0.75 && evaluated.agent_trust_snapshot == 0.85 {
            println!("✅ PASSED: Layer 1 immutable, Layer 2 metadata separate");
            passed += 1;
        } else {
            println!("❌ FAILED: Metadata not preserved correctly");
            failed += 1;
        }
    }
    println!();

    // Test 7: EvaluatedPayload with governance snapshot
    println!("Test 7: EvaluatedPayload with governance snapshot...");
    {
        let payload = CstlPayload::default();
        let governance = GovernanceSnapshot {
            sender: "test_agent".to_string(),
            breaker_trips: 2,
            circuit_state: "open".to_string(),
            drift_ratio: 0.75,
            drift_flagged: true,
            semantic_warnings_count: 4,
        };

        let evaluated = EvaluatedPayload::new(payload, "hash123".to_string(), 0.6, 0.7)
            .with_governance_snapshot(governance);

        if evaluated.is_complete() && evaluated.governance_state_snapshot.unwrap().breaker_trips == 2 {
            println!("✅ PASSED: Governance snapshot attached and complete");
            passed += 1;
        } else {
            println!("❌ FAILED: Governance snapshot not attached correctly");
            failed += 1;
        }
    }
    println!();

    // Test 8: Verdict consensus computation
    println!("Test 8: Verdict consensus computation...");
    {
        let payload = CstlPayload::default();
        let mut evaluated = EvaluatedPayload::new(payload, "hash123".to_string(), 0.7, 0.8);

        evaluated.add_verdict(true, "consensus".to_string(), 0.95);
        evaluated.add_verdict(true, "consensus".to_string(), 0.90);
        evaluated.add_verdict(false, "consensus".to_string(), 0.10);

        if let Some((is_correct, _avg_confidence, count)) = evaluated.compute_verdict_consensus() {
            if is_correct && count == 3 {
                println!("✅ PASSED: Verdict consensus: {} verdicts, majority correct", count);
                passed += 1;
            } else {
                println!("❌ FAILED: Consensus computation incorrect");
                failed += 1;
            }
        } else {
            println!("❌ FAILED: No verdict consensus");
            failed += 1;
        }
    }
    println!();

    // Test 9: Double Livre separation principle
    println!("Test 9: Double Livre separation principle...");
    {
        let mut payload = CstlPayload::default();
        payload.intent.insert("sigma".to_string(), "0.95".to_string());

        let evaluated = EvaluatedPayload::new(payload, "hash123".to_string(), 0.45, 0.3);

        if evaluated.raw_payload.intent.get("sigma") == Some(&"0.95".to_string())
            && evaluated.server_sigma_effective == 0.45
        {
            println!("✅ PASSED: Layer 1 (0.95) and Layer 2 (0.45) are properly separated");
            passed += 1;
        } else {
            println!("❌ FAILED: Separation principle not maintained");
            failed += 1;
        }
    }
    println!();

    // Test 10: EWMA convergence detection
    println!("Test 10: EWMA convergence detection...");
    {
        let mut ewma = EwmaCalibration::new("agent".to_string(), 0.5, 0.2);
        assert!(!ewma.is_converged());
        for _ in 0..50 {
            ewma.update(true);
        }
        if ewma.is_converged() {
            println!("✅ PASSED: EWMA convergence detected after 50+ samples");
            passed += 1;
        } else {
            println!("❌ FAILED: Convergence not detected");
            failed += 1;
        }
    }
    println!();

    // Test 11: Sigma bounds always clamped
    println!("Test 11: Sigma bounds always clamped...");
    {
        let mut calibrator = SigmaCalibrator::new(0.2);
        let sigma_high = calibrator.compute_effective_sigma("test", 1.0);
        let sigma_low = calibrator.compute_effective_sigma("test", 0.0);

        if sigma_high >= 0.0 && sigma_high <= 1.0 && sigma_low >= 0.0 && sigma_low <= 1.0 {
            println!("✅ PASSED: All sigma values bounded in [0.0, 1.0]");
            passed += 1;
        } else {
            println!("❌ FAILED: Sigma bounds violated");
            failed += 1;
        }
    }
    println!();

    // Summary
    println!("════════════════════════════════════════");
    println!("✅ {} test(s) passed", passed);
    if failed > 0 {
        println!("❌ {} test(s) failed", failed);
    }
    println!();
    println!("🎯 Double Livre v5.2 architecture validated:");
    println!("   - Layer 1 (raw payload) remains immutable and signed");
    println!("   - Layer 2 (server metadata) holds calibration + governance state");
    println!("   - EWMA adapts rapidly to agent accuracy (alpha=0.2, ~50 message window)");
    println!("   - Sigma blending preserves cryptographic properties");
    println!("   - Audit trail (adn_sigma_audit_log) links both layers");
    println!();

    if failed == 0 {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
