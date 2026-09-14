#!/usr/bin/env python3

import json
import sqlite3
import tempfile
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from cstl_graphify_bridge import GraphifyExporter, GraphifyNode, GraphifyEdge

def setup_test_db(db_path: str):
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()

    cursor.execute('''
        CREATE TABLE IF NOT EXISTS agent_registry (
            id TEXT PRIMARY KEY,
            label TEXT,
            public_key TEXT,
            custom_metadata TEXT
        )
    ''')

    cursor.execute('''
        CREATE TABLE IF NOT EXISTS audit_trail (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sender TEXT,
            receiver TEXT,
            purpose TEXT,
            payload_hash TEXT,
            created_at TEXT,
            payload_json TEXT
        )
    ''')

    cursor.execute('''
        CREATE TABLE IF NOT EXISTS deontic_modalities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            agent_id TEXT,
            modality_type TEXT,
            relation_name TEXT,
            active INTEGER,
            created_at TEXT
        )
    ''')

    agents = [
        ("alice", "Alice Agent", "key_alice_0123456789abcdef", json.dumps({"role": "coordinator"})),
        ("bob", "Bob Agent", "key_bob_0123456789abcdef", json.dumps({"role": "worker"})),
        ("charlie", "Charlie Agent", "key_charlie_0123456789ab", json.dumps({"role": "validator"}))
    ]

    for agent_id, label, pubkey, metadata in agents:
        cursor.execute(
            "INSERT OR REPLACE INTO agent_registry (id, label, public_key, custom_metadata) VALUES (?, ?, ?, ?)",
            (agent_id, label, pubkey, metadata)
        )

    audit_entries = [
        ("alice", "bob", "communication", "hash001", "2026-09-14T10:00:00Z", json.dumps({"msg": "hello"})),
        ("bob", "charlie", "consensus", "hash002", "2026-09-14T10:01:00Z", json.dumps({"msg": "confirm"})),
        ("charlie", "alice", "acknowledgement", "hash003", "2026-09-14T10:02:00Z", json.dumps({"msg": "ack"})),
        ("alice", "bob", "communication", "hash004", "2026-09-14T10:03:00Z", json.dumps({"msg": "query"})),
    ]

    for sender, receiver, purpose, phash, created, payload in audit_entries:
        cursor.execute(
            "INSERT INTO audit_trail (sender, receiver, purpose, payload_hash, created_at, payload_json) VALUES (?, ?, ?, ?, ?, ?)",
            (sender, receiver, purpose, phash, created, payload)
        )

    deontic_mods = [
        ("alice", "MUST", "consensus_protocol", 1, "2026-09-14T09:00:00Z"),
        ("bob", "MUST_NOT", "override_decision", 1, "2026-09-14T09:00:00Z"),
        ("charlie", "MAY", "propose_new_relation", 1, "2026-09-14T09:00:00Z"),
        ("alice", "MUST", "verify_signatures", 1, "2026-09-14T09:00:00Z"),
    ]

    for agent, mtype, rel, active, created in deontic_mods:
        cursor.execute(
            "INSERT INTO deontic_modalities (agent_id, modality_type, relation_name, active, created_at) VALUES (?, ?, ?, ?, ?)",
            (agent, mtype, rel, active, created)
        )

    conn.commit()
    conn.close()

def test_graphify_node_creation():
    node = GraphifyNode("agent_1", "agent", "Alice", {"role": "coordinator"})
    assert node.id == "agent_1"
    assert node.type == "agent"
    assert node.label == "Alice"
    assert node.metadata["role"] == "coordinator"

    graphify_format = node.to_graphify_format()
    assert graphify_format["id"] == "agent_1"
    assert graphify_format["type"] == "agent"
    assert "created_at" in graphify_format["metadata"]
    print("✓ test_graphify_node_creation PASS")

def test_graphify_edge_creation():
    edge = GraphifyEdge("alice", "bob", "sends_to", 1.0, {"purpose": "communication"})
    assert edge.source == "alice"
    assert edge.target == "bob"
    assert edge.type == "sends_to"

    graphify_format = edge.to_graphify_format()
    assert graphify_format["source"] == "alice"
    assert graphify_format["target"] == "bob"
    assert graphify_format["weight"] == 1.0
    print("✓ test_graphify_edge_creation PASS")

def test_deontic_coloring():
    must_node = GraphifyNode("deontic_1", "deontic_must", "MUST consensus", {"deontic_type": "MUST"})
    must_not_node = GraphifyNode("deontic_2", "deontic_must_not", "MUST_NOT override", {"deontic_type": "MUST_NOT"})
    may_node = GraphifyNode("deontic_3", "deontic_may", "MAY propose", {"deontic_type": "MAY"})

    must_format = must_node.to_graphify_format()
    must_not_format = must_not_node.to_graphify_format()
    may_format = may_node.to_graphify_format()

    assert must_format["color"] == "#DC143C"
    assert must_not_format["color"] == "#8B0000"
    assert may_format["color"] == "#32CD32"
    print("✓ test_deontic_coloring PASS")

def test_graph_building():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        nodes, edges = exporter.build_graph()

        assert len(nodes) > 0
        assert len(edges) > 0

        agent_nodes = [n for n in nodes.values() if n.type == "agent"]
        assert len(agent_nodes) == 3
        print("✓ test_graph_building PASS")

def test_export_to_json():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        output_file = os.path.join(tmpdir, "graph.json")
        graph_data = exporter.export_to_json(output_file)

        assert "metadata" in graph_data
        assert "nodes" in graph_data
        assert "edges" in graph_data

        assert graph_data["metadata"]["version"] == "1.0.0"
        assert graph_data["metadata"]["format"] == "graphify-standard"

        assert os.path.exists(output_file)
        with open(output_file) as f:
            loaded = json.load(f)
            assert loaded == graph_data

        print("✓ test_export_to_json PASS")

def test_obsidian_vault_export():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        vault = exporter.export_for_obsidian(tmpdir)

        assert "_index.md" in vault
        assert any("agents/" in key for key in vault.keys())
        assert any("modalities/" in key for key in vault.keys())

        index_content = vault["_index.md"]
        assert "# CSTL Graph Index" in index_content
        assert "Agents" in index_content
        assert "alice" in index_content or "Bob" in index_content
        print("✓ test_obsidian_vault_export PASS")

def test_filter_nodes_by_type():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        exporter.build_graph()

        agent_nodes = exporter.filter_nodes_by_type("agent")
        assert len(agent_nodes) == 3

        deontic_must_nodes = exporter.filter_nodes_by_type("deontic_must")
        assert len(deontic_must_nodes) == 2

        print("✓ test_filter_nodes_by_type PASS")

def test_search_nodes():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        exporter.build_graph()

        alice_results = exporter.search_nodes("alice")
        assert len(alice_results) > 0
        assert any("alice" in n.label.lower() for n in alice_results)

        coordinator_results = exporter.search_nodes("coordinator")
        assert len(coordinator_results) > 0

        print("✓ test_search_nodes PASS")

def test_graph_traversal():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        exporter.build_graph()

        result = exporter.traverse_graph("alice", max_depth=2)

        assert "root" in result
        assert "nodes" in result
        assert "edges" in result
        assert result["root"] == "alice"

        result_invalid = exporter.traverse_graph("nonexistent")
        assert "error" in result_invalid

        print("✓ test_graph_traversal PASS")

def test_graph_statistics():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)
        exporter.build_graph()

        stats = exporter.get_graph_stats()

        assert "total_nodes" in stats
        assert "total_edges" in stats
        assert "agents" in stats
        assert "audit_entries" in stats
        assert "deontic_modalities" in stats
        assert "deontic_must" in stats
        assert "deontic_must_not" in stats
        assert "deontic_may" in stats

        assert stats["agents"] == 3
        assert stats["deontic_must"] == 2
        assert stats["deontic_must_not"] == 1
        assert stats["deontic_may"] == 1

        print("✓ test_graph_statistics PASS")

def test_bidirectional_sync_consistency():
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.db")
        setup_test_db(db_path)

        exporter = GraphifyExporter(db_path)

        graph_json = exporter.export_to_json()
        vault_content = exporter.export_for_obsidian(tmpdir)

        nodes_in_json = set(n["id"] for n in graph_json["nodes"])

        assert len(nodes_in_json) > 0
        assert len(vault_content) > 0

        assert "alice" in str(vault_content).lower()
        assert "bob" in str(vault_content).lower()

        print("✓ test_bidirectional_sync_consistency PASS")

def run_all_tests():
    tests = [
        test_graphify_node_creation,
        test_graphify_edge_creation,
        test_deontic_coloring,
        test_graph_building,
        test_export_to_json,
        test_obsidian_vault_export,
        test_filter_nodes_by_type,
        test_search_nodes,
        test_graph_traversal,
        test_graph_statistics,
        test_bidirectional_sync_consistency,
    ]

    passed = 0
    failed = 0

    for test in tests:
        try:
            test()
            passed += 1
        except Exception as e:
            print(f"✗ {test.__name__} FAIL: {e}")
            failed += 1

    print(f"\n{'='*50}")
    print(f"Test Results: {passed} passed, {failed} failed")
    print(f"{'='*50}")

    return failed == 0

if __name__ == "__main__":
    success = run_all_tests()
    sys.exit(0 if success else 1)
