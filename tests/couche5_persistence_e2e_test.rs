//! tests/couche5_persistence_e2e_test.rs
//! Comprehensive end-to-end test for Couche 5 (Persistent Memory / Provenance)
//!
//! This test exercises the full persistence lifecycle:
//! 1. Insert 10 ADN entries with various sigma values, relations, and deontic modalities
//! 2. Commit 5 of them via the human council
//! 3. Close and re-open the database (simulating a server restart)
//! 4. Verify all 10 entries are present with correct flags, hash chains, and relations
//! 5. Test audit trail continuity (parent_hash chains)
//! 6. Test delta detection on entries from before/after restart
//! 7. Verify council log persistence
//! 8. Verify deontic modality persistence

use cstl_parser::adn_store::AdnStore;
use cstl_parser::adn_delta_detector::ADNDeltaDetector;
use std::collections::HashMap;
use std::path::Path;

/// Test helper: generate a deterministic hash for an entry
fn make_test_hash(index: usize, seed: &str) -> String {
    format!("sha256:{:064x}", index as u64 * 1000 + seed.len() as u64)
}

/// Test helper: create test ADN entry with populated relations
fn create_test_entry(
    index: usize,
    sigma: f64,
    _parent_hash: Option<String>,
) -> (String, String, Vec<HashMap<String, String>>) {
    let hash = make_test_hash(index, "test");
    let payload = format!(
        "test_payload_{}: assertion about entity_{} with confidence {}",
        index, index, sigma
    );

    // Create deontic relations for even-indexed entries
    let mut relations = Vec::new();
    if index % 2 == 0 {
        relations.push({
            let mut r = HashMap::new();
            r.insert("subject".to_string(), format!("entity_{}", index));
            r.insert("type".to_string(), "MUST_RESPECT".to_string());
            r.insert("object".to_string(), format!("rule_{}", index));
            r.insert("modality".to_string(), "MUST".to_string());
            r
        });
    } else {
        relations.push({
            let mut r = HashMap::new();
            r.insert("subject".to_string(), format!("entity_{}", index));
            r.insert("type".to_string(), "ASSUMES".to_string());
            r.insert("object".to_string(), format!("fact_{}", index));
            r.insert("modality".to_string(), "PERMISSION".to_string());
            r
        });
    }

    (hash, payload, relations)
}

/// Test: Full lifecycle persistence (INSERT → COMMIT → CLOSE → REOPEN → VERIFY)
#[test]
fn test_couche5_full_persistence_lifecycle() {
    let db_path = "/tmp/test_couche5_persist_e2e.db";

    // Cleanup before test
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to clean up test database");
    }

    // ========== PHASE 1: CREATE & INSERT 10 ENTRIES ==========
    {
        let store = AdnStore::open(db_path).expect("Failed to open database");

        // Insert 10 entries with varying sigma values
        for i in 0..10 {
            let (hash, payload, relations) = create_test_entry(
                i,
                0.5 + (i as f64 * 0.05),  // sigma: 0.5, 0.55, 0.6, ..., 0.95
                if i > 0 { Some(make_test_hash(i - 1, "test")) } else { None },
            );

            let parent_hash_str = if i > 0 { make_test_hash(i - 1, "test") } else { String::new() };
            let parent_hash_ref = if i > 0 { Some(parent_hash_str.as_str()) } else { None };
            let agent_str = format!("agent_{}", i % 3);
            let conv_str = format!("conv_{}", i / 2);

            store
                .put(
                    &hash,
                    &payload,
                    Some("test_encoder"),
                    Some(&agent_str),
                    0.5 + (i as f64 * 0.05),
                    parent_hash_ref,
                    Some(&conv_str),
                    Some(i as i64),
                )
                .expect("Failed to insert ADN entry");

            // Persist relations for this entry
            store
                .put_relations(&hash, &relations)
                .expect("Failed to insert relations");
        }

        // ========== PHASE 2: COMMIT 5 ENTRIES ==========
        // Commit entries 0, 2, 4, 6, 8 (even indices, or first 5)
        for i in [0, 2, 4, 6, 8].iter() {
            let hash = make_test_hash(*i, "test");
            store
                .commit(&hash, &format!("councilor_{}", i % 2), Some("Approved by council"))
                .expect("Failed to commit entry");
        }

        // Verify stats after phase 2
        let stats = store.stats().expect("Failed to get stats");
        assert_eq!(
            stats.total, 10,
            "Expected 10 total entries after insert phase"
        );
        assert_eq!(
            stats.committed, 5,
            "Expected 5 committed entries after commit phase"
        );
        assert_eq!(
            stats.pending, 5,
            "Expected 5 pending entries after commit phase"
        );
    }

    // ========== PHASE 3: CLOSE AND REOPEN DATABASE ==========
    // This simulates a server restart
    let store = AdnStore::open(db_path).expect("Failed to reopen database after restart");

    // ========== PHASE 4: VERIFY PERSISTENCE ==========

    // 4A: Check stats are preserved
    let stats = store.stats().expect("Failed to get stats after restart");
    assert_eq!(
        stats.total, 10,
        "Expected 10 entries to persist after restart"
    );
    assert_eq!(
        stats.committed, 5,
        "Expected committed flags to persist after restart"
    );
    assert_eq!(
        stats.pending, 5,
        "Expected pending count to persist after restart"
    );

    // 4B: Verify each entry exists and has correct properties
    for i in 0..10 {
        let hash = make_test_hash(i, "test");
        let entry = store
            .get(&hash)
            .expect("Failed to retrieve entry")
            .expect(&format!("Entry {} not found after restart", i));

        // Verify basic properties
        assert_eq!(entry.hash, hash, "Hash mismatch for entry {}", i);
        assert_eq!(
            entry.encoder,
            Some("test_encoder".to_string()),
            "Encoder not preserved for entry {}",
            i
        );
        assert_eq!(
            entry.produced_by,
            Some(format!("agent_{}", i % 3)),
            "producer not preserved for entry {}",
            i
        );

        // Verify sigma is preserved with precision
        let expected_sigma = 0.5 + (i as f64 * 0.05);
        assert!(
            (entry.sigma - expected_sigma).abs() < 0.001,
            "Sigma mismatch for entry {}: expected {}, got {}",
            i,
            expected_sigma,
            entry.sigma
        );

        // Verify conversation_id
        assert_eq!(
            entry.conversation_id,
            Some(format!("conv_{}", i / 2)),
            "conversation_id not preserved for entry {}",
            i
        );

        // Verify turn number
        assert_eq!(
            entry.turn,
            Some(i as i64),
            "turn not preserved for entry {}",
            i
        );

        // Verify committed flag
        let expected_committed = [0, 2, 4, 6, 8].contains(&i);
        assert_eq!(
            entry.committed, expected_committed,
            "committed flag not preserved for entry {}",
            i
        );

        // Verify parent_hash chain integrity (if not first entry)
        if i > 0 {
            let expected_parent = make_test_hash(i - 1, "test");
            assert_eq!(
                entry.parent_hash,
                Some(expected_parent),
                "parent_hash not preserved for entry {}",
                i
            );
        }

        // Verify payload content
        assert!(
            entry.payload.contains(&format!("test_payload_{}", i)),
            "Payload not preserved for entry {}",
            i
        );
    }

    // 4C: Verify hash chain continuity (hash[n].parent_hash == hash[n-1].hash)
    for i in 1..10 {
        let current_hash = make_test_hash(i, "test");
        let prev_hash = make_test_hash(i - 1, "test");

        let current_entry = store
            .get(&current_hash)
            .expect("Failed to get current entry for chain check")
            .expect(&format!("Current entry {} not found for chain check", i));

        assert_eq!(
            current_entry.parent_hash,
            Some(prev_hash),
            "Hash chain broken between entry {} and {}",
            i - 1,
            i
        );
    }

    // 4D: Verify council log entries are persistent
    for i in [0, 2, 4, 6, 8].iter() {
        let hash = make_test_hash(*i, "test");
        let log_entries = store
            .council_log_for(&hash)
            .expect("Failed to get council log");

        assert!(!log_entries.is_empty(), "Council log is empty for committed entry {}", i);

        // Check that at least one commit action exists
        let has_commit = log_entries
            .iter()
            .any(|log| log.action == "commit");
        assert!(
            has_commit,
            "No commit action found in council log for entry {}",
            i
        );

        // Verify log entry structure
        for log_entry in &log_entries {
            assert_eq!(log_entry.hash, hash);
            assert!(!log_entry.by_whom.is_empty());
            assert!(log_entry.timestamp > 0);
        }
    }

    // 4E: Verify relations are persistent (with modality)
    for i in 0..10 {
        let _hash = make_test_hash(i, "test");  // Create hash to match entry pattern below
        let relations = store
            .all_relations()
            .expect("Failed to get all relations");

        // Should have at least some relations total (all 10 entries have one relation each)
        assert!(!relations.is_empty(), "Relations not persisted");

        // Find relations for this specific entry
        let entry_relations: Vec<_> = relations
            .iter()
            .filter(|r| {
                r.get("subject")
                    .map(|s| s.contains(&format!("entity_{}", i)))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(
            entry_relations.len(), 1,
            "Expected 1 relation for entry {}, found {}",
            i,
            entry_relations.len()
        );
    }

    // 4F: Verify deontic modalities are preserved for relations
    let deontic_relations = store
        .deontic_relations_history()
        .expect("Failed to get deontic relations");

    // Should have relations with MUST and PERMISSION modalities
    let has_must = deontic_relations
        .iter()
        .any(|r| r.get("modality").map(|m| m == "MUST").unwrap_or(false));
    let has_permission = deontic_relations
        .iter()
        .any(|r| r.get("modality").map(|m| m == "PERMISSION").unwrap_or(false));

    assert!(has_must, "MUST modality not found in deontic relations");
    assert!(has_permission, "PERMISSION modality not found in deontic relations");

    // ========== PHASE 5: TEST DELTA DETECTION ACROSS RESTART ==========

    // Create a delta detector and test it
    let detector = ADNDeltaDetector::new(std::sync::Arc::new(store));

    // Test delta between entry 0 and entry 1
    let delta = detector
        .detect_deltas(&make_test_hash(0, "test"), &make_test_hash(1, "test"))
        .expect("Failed to detect deltas");

    assert!(delta.payload_changed, "Payloads should be different");
    assert!(delta.sigma_delta > 0.04, "Sigma should increase by ~0.05");
    assert_eq!(
        delta.old_hash,
        make_test_hash(0, "test"),
        "Delta report old_hash incorrect"
    );
    assert_eq!(
        delta.new_hash,
        make_test_hash(1, "test"),
        "Delta report new_hash incorrect"
    );

    // ========== PHASE 6: TEST COMMITTED FLAG WITH QUORUM ==========

    // Test cast_commit_vote on an uncommitted entry
    let store_vote = AdnStore::open(db_path).expect("Failed to reopen for vote test");
    let hash_7 = make_test_hash(7, "test");

    // First vote (distinct_voters = 1, quorum_size = 3)
    let outcome1 = store_vote
        .cast_commit_vote(&hash_7, "voter_1", Some("First vote"), 3)
        .expect("Failed to cast vote 1");

    assert_eq!(outcome1.distinct_voters, 1);
    assert_eq!(outcome1.quorum_size, 3);
    assert!(!outcome1.quorum_reached, "Quorum should not be reached with 1 voter");

    // Second vote from different voter
    let outcome2 = store_vote
        .cast_commit_vote(&hash_7, "voter_2", Some("Second vote"), 3)
        .expect("Failed to cast vote 2");

    assert_eq!(outcome2.distinct_voters, 2);
    assert!(!outcome2.quorum_reached, "Quorum should not be reached with 2 voters");

    // Third vote reaches quorum
    let outcome3 = store_vote
        .cast_commit_vote(&hash_7, "voter_3", Some("Third vote - quorum reached"), 3)
        .expect("Failed to cast vote 3");

    assert_eq!(outcome3.distinct_voters, 3);
    assert!(outcome3.quorum_reached, "Quorum should be reached with 3 voters");

    // Verify entry 7 is now committed
    let entry_7 = store_vote
        .get(&hash_7)
        .expect("Failed to get entry 7")
        .expect("Entry 7 not found");
    assert!(entry_7.committed, "Entry 7 should be committed after quorum");

    // ========== CLEANUP ==========
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to cleanup test database");
    }
}

/// Test: Verify parent_hash chain remains intact across updates
#[test]
fn test_couche5_parent_hash_chain_integrity() {
    let db_path = "/tmp/test_couche5_chain_integrity.db";

    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to clean up test database");
    }

    // Create a chain of 5 entries
    {
        let store = AdnStore::open(db_path).expect("Failed to open database");

        let mut prev_hash: Option<String> = None;

        for i in 0..5 {
            let hash = make_test_hash(i, "chain");
            let payload = format!("chain_entry_{}", i);

            store
                .put(
                    &hash,
                    &payload,
                    None,
                    Some("chain_agent"),
                    0.8,
                    prev_hash.as_deref(),
                    None,
                    None,
                )
                .expect("Failed to insert chain entry");

            prev_hash = Some(hash);
        }
    }

    // Reopen and verify chain is intact
    let store = AdnStore::open(db_path).expect("Failed to reopen database");

    for i in 1..5 {
        let current = store
            .get(&make_test_hash(i, "chain"))
            .expect("Failed to get entry")
            .expect("Entry not found");

        let expected_parent = make_test_hash(i - 1, "chain");
        assert_eq!(
            current.parent_hash,
            Some(expected_parent),
            "Chain broken at position {}",
            i
        );
    }

    // Cleanup
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to cleanup test database");
    }
}

/// Test: Verify committed flag persistence across multiple restart cycles
#[test]
fn test_couche5_committed_flag_persistence_cycles() {
    let db_path = "/tmp/test_couche5_commit_cycles.db";

    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to clean up test database");
    }

    let hash = make_test_hash(42, "cycles");

    // Cycle 1: Insert uncommitted
    {
        let store = AdnStore::open(db_path).expect("Failed to open database (cycle 1)");
        store
            .put(&hash, "payload_cycle_1", None, None, 0.7, None, None, None)
            .expect("Failed to insert entry (cycle 1)");

        let entry = store
            .get(&hash)
            .expect("Failed to get entry (cycle 1)")
            .expect("Entry not found (cycle 1)");
        assert!(!entry.committed, "Entry should be uncommitted in cycle 1");
    }

    // Cycle 2: Commit it
    {
        let store = AdnStore::open(db_path).expect("Failed to open database (cycle 2)");
        store
            .commit(&hash, "councilor_1", Some("Committed in cycle 2"))
            .expect("Failed to commit entry (cycle 2)");

        let entry = store
            .get(&hash)
            .expect("Failed to get entry (cycle 2)")
            .expect("Entry not found (cycle 2)");
        assert!(entry.committed, "Entry should be committed in cycle 2");
    }

    // Cycle 3: Revoke it
    {
        let store = AdnStore::open(db_path).expect("Failed to open database (cycle 3)");
        store
            .revoke(&hash, "councilor_2", Some("Revoked in cycle 3"))
            .expect("Failed to revoke entry (cycle 3)");

        let entry = store
            .get(&hash)
            .expect("Failed to get entry (cycle 3)")
            .expect("Entry not found (cycle 3)");
        assert!(!entry.committed, "Entry should be revoked (uncommitted) in cycle 3");
    }

    // Cycle 4: Verify final state
    {
        let store = AdnStore::open(db_path).expect("Failed to open database (cycle 4)");
        let entry = store
            .get(&hash)
            .expect("Failed to get entry (cycle 4)")
            .expect("Entry not found (cycle 4)");

        assert!(!entry.committed, "Entry should remain uncommitted after revoke");

        let log = store
            .council_log_for(&hash)
            .expect("Failed to get council log");

        // Should have commit and revoke actions
        assert_eq!(log.len(), 2, "Should have 2 log entries (commit + revoke)");
        assert_eq!(log[0].action, "commit");
        assert_eq!(log[1].action, "revoke");
    }

    // Cleanup
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to cleanup test database");
    }
}

/// Test: Verify council log audit trail completeness
#[test]
fn test_couche5_council_log_audit_trail() {
    let db_path = "/tmp/test_couche5_council_log.db";

    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to clean up test database");
    }

    let hash = make_test_hash(99, "audit");

    {
        let store = AdnStore::open(db_path).expect("Failed to open database");

        // Insert entry
        store
            .put(&hash, "test_payload", None, None, 0.9, None, None, None)
            .expect("Failed to insert entry");

        // Simulate multiple commits and revokes with different councilors
        store
            .commit(&hash, "alice", Some("Initial approval"))
            .expect("Failed to commit (alice)");

        store
            .commit(&hash, "bob", Some("Secondary approval"))
            .expect("Failed to commit (bob)");

        store
            .revoke(&hash, "charlie", Some("Revoked due to contradiction"))
            .expect("Failed to revoke (charlie)");

        // Verify complete audit trail
        let log = store
            .council_log_for(&hash)
            .expect("Failed to get council log");

        assert_eq!(log.len(), 3, "Expected 3 log entries");

        // Verify sequence and content
        assert_eq!(log[0].action, "commit");
        assert_eq!(log[0].by_whom, "alice");
        assert_eq!(log[0].note, Some("Initial approval".to_string()));

        assert_eq!(log[1].action, "commit");
        assert_eq!(log[1].by_whom, "bob");

        assert_eq!(log[2].action, "revoke");
        assert_eq!(log[2].by_whom, "charlie");

        // Verify timestamps are monotonically increasing
        for i in 1..log.len() {
            assert!(
                log[i].timestamp >= log[i - 1].timestamp,
                "Timestamps should be monotonically increasing"
            );
        }
    }

    // Cleanup
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to cleanup test database");
    }
}

/// Test: Verify relations and deontic modalities survive across restarts
#[test]
fn test_couche5_relations_modality_persistence() {
    let db_path = "/tmp/test_couche5_relations_persist.db";

    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to clean up test database");
    }

    let hash = make_test_hash(777, "relations");

    // Phase 1: Insert with relations
    {
        let store = AdnStore::open(db_path).expect("Failed to open database");

        store
            .put(&hash, "entity_payload", None, None, 0.85, None, None, None)
            .expect("Failed to insert entry");

        let relations = vec![
            {
                let mut r = HashMap::new();
                r.insert("subject".to_string(), "person_alice".to_string());
                r.insert("type".to_string(), "MUST_COMPLY".to_string());
                r.insert("object".to_string(), "regulation_x".to_string());
                r.insert("modality".to_string(), "MUST".to_string());
                r
            },
            {
                let mut r = HashMap::new();
                r.insert("subject".to_string(), "person_bob".to_string());
                r.insert("type".to_string(), "MAY_USE".to_string());
                r.insert("object".to_string(), "resource_y".to_string());
                r.insert("modality".to_string(), "PERMISSION".to_string());
                r
            },
            {
                let mut r = HashMap::new();
                r.insert("subject".to_string(), "person_charlie".to_string());
                r.insert("type".to_string(), "MUST_NOT_ACCESS".to_string());
                r.insert("object".to_string(), "data_z".to_string());
                r.insert("modality".to_string(), "MUST_NOT".to_string());
                r
            },
        ];

        store
            .put_relations(&hash, &relations)
            .expect("Failed to insert relations");
    }

    // Phase 2: Verify relations persist after restart
    {
        let store = AdnStore::open(db_path).expect("Failed to reopen database");

        let all_relations = store
            .all_relations()
            .expect("Failed to get all relations");

        assert_eq!(all_relations.len(), 3, "Expected 3 relations to persist");

        // Find relations for this entry
        let entry_relations: Vec<_> = all_relations
            .iter()
            .filter(|r| {
                r.get("subject")
                    .map(|s| s.starts_with("person_"))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(entry_relations.len(), 3);
    }

    // Phase 3: Verify deontic modalities are intact
    {
        let store = AdnStore::open(db_path).expect("Failed to reopen database for modality check");

        let deontic_rels = store
            .deontic_relations_history()
            .expect("Failed to get deontic relations");

        assert_eq!(deontic_rels.len(), 3, "Expected 3 deontic relations");

        // Verify all three modalities are present
        let modalities: Vec<String> = deontic_rels
            .iter()
            .filter_map(|r| r.get("modality").cloned())
            .collect();

        assert!(modalities.contains(&"MUST".to_string()));
        assert!(modalities.contains(&"PERMISSION".to_string()));
        assert!(modalities.contains(&"MUST_NOT".to_string()));
    }

    // Cleanup
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to cleanup test database");
    }
}

/// Test: Large-scale persistence (100 entries, various states)
#[test]
fn test_couche5_large_scale_persistence() {
    let db_path = "/tmp/test_couche5_large_scale.db";

    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to clean up test database");
    }

    const NUM_ENTRIES: usize = 100;

    // Phase 1: Insert 100 entries with varying states
    {
        let store = AdnStore::open(db_path).expect("Failed to open database");

        for i in 0..NUM_ENTRIES {
            let hash = make_test_hash(i, "large");
            let payload = format!("large_scale_entry_{}_sigma_{}", i, i as f64 / 100.0);
            let parent = if i > 0 {
                Some(make_test_hash(i - 1, "large"))
            } else {
                None
            };

            store
                .put(&hash, &payload, None, None, i as f64 / 100.0, parent.as_deref(), None, Some(i as i64))
                .expect("Failed to insert entry");

            // Commit every 5th entry
            if i % 5 == 0 {
                store
                    .commit(&hash, &format!("councilor_{}", i % 3), None)
                    .expect("Failed to commit entry");
            }
        }
    }

    // Phase 2: Verify all entries persist
    {
        let store = AdnStore::open(db_path).expect("Failed to reopen database");

        let stats = store.stats().expect("Failed to get stats");
        assert_eq!(stats.total, NUM_ENTRIES as u64, "Not all entries persisted");

        // Should have committed every 5th entry (0, 5, 10, ..., 95)
        let expected_committed = (NUM_ENTRIES as f64 / 5.0).ceil() as u64;
        assert_eq!(
            stats.committed, expected_committed,
            "Committed count mismatch"
        );

        // Spot-check a few entries
        for test_idx in [0, 25, 50, 75, 99] {
            let hash = make_test_hash(test_idx, "large");
            let entry = store
                .get(&hash)
                .expect("Failed to get entry")
                .expect(&format!("Entry {} not found", test_idx));

            assert!(entry.payload.contains(&format!("entry_{}", test_idx)));
            assert_eq!(entry.turn, Some(test_idx as i64));

            // Verify parent hash for non-zero entries
            if test_idx > 0 {
                let expected_parent = make_test_hash(test_idx - 1, "large");
                assert_eq!(entry.parent_hash, Some(expected_parent));
            }

            let should_be_committed = test_idx % 5 == 0;
            assert_eq!(entry.committed, should_be_committed, "Committed flag mismatch for entry {}", test_idx);
        }
    }

    // Cleanup
    if Path::new(db_path).exists() {
        std::fs::remove_file(db_path).expect("Failed to cleanup test database");
    }
}
