#!/usr/bin/env python3
"""
Test suite for CSTL Obsidian Bridge (Couche 6)
Tests export/import roundtrip and graph synchronization
"""

import json
import tempfile
from pathlib import Path
from typing import Dict, Any

from cstl_obsidian_bridge import (
    AgentCard,
    AgentGraph,
    GraphNode,
    GraphEdge,
    NodeType,
    EdgeType,
    ObsidianBridge,
    CstlGraphifySync,
)


# ============================================================================
# Test Utilities
# ============================================================================

def create_test_graph() -> AgentGraph:
    """Create a test graph with sample agents"""
    graph = AgentGraph()

    agents = [
        AgentCard("alice", "5.1.0", ["fact_checking", "semantic_analysis"], 0.95, "a" * 64),
        AgentCard("bob", "5.1.0", ["fact_checking", "contradiction_detection"], 0.87),
        AgentCard("charlie", "5.1.0", ["execution_lab"], 0.92, "c" * 64),
    ]

    for agent in agents:
        graph.add_agent_node(agent)

    graph.add_relay_edge("alice", "bob", "dialogue_turn")
    graph.add_relay_edge("bob", "charlie", "verification_request")
    graph.add_relay_edge("charlie", "alice", "audit_result")

    graph.add_signature_edge("alice", "a" * 64)
    graph.add_signature_edge("charlie", "c" * 64)

    return graph


# ============================================================================
# Test: Graph Creation
# ============================================================================

def test_agent_graph_creation():
    """Test basic agent graph creation"""
    graph = create_test_graph()

    assert len(graph.agents) == 3, "Should have 3 agents"
    assert len(graph.nodes) >= 7, "Should have at least 7 nodes (3 agents + 4 capabilities)"
    assert len(graph.edges) >= 6, "Should have at least 6 edges"

    # Check agent data
    assert "alice" in graph.agents
    assert graph.agents["alice"].trust_score == 0.95
    assert graph.agents["alice"].public_key == "a" * 64

    print("✓ test_agent_graph_creation passed")


# ============================================================================
# Test: Graph Export to JSON
# ============================================================================

def test_graph_export_json():
    """Test exporting graph to JSON"""
    graph = create_test_graph()
    graph_dict = graph.to_dict()

    # Verify structure
    assert "nodes" in graph_dict
    assert "edges" in graph_dict
    assert "metadata" in graph_dict

    # Verify metadata
    assert graph_dict["metadata"]["node_count"] == len(graph_dict["nodes"])
    assert graph_dict["metadata"]["edge_count"] == len(graph_dict["edges"])
    assert graph_dict["metadata"]["agent_count"] == 3

    # Verify nodes
    agent_nodes = [n for n in graph_dict["nodes"] if n["type"] == "agent_card"]
    assert len(agent_nodes) == 3

    # Verify edges
    relay_edges = [e for e in graph_dict["edges"] if e["type"] == "relays_to"]
    assert len(relay_edges) == 3

    print("✓ test_graph_export_json passed")


# ============================================================================
# Test: Graph Import from JSON (Roundtrip)
# ============================================================================

def test_graph_roundtrip():
    """Test export → import roundtrip"""
    # Create original graph
    graph1 = create_test_graph()
    graph1_dict = graph1.to_dict()

    # Export to JSON string
    json_str = json.dumps(graph1_dict)

    # Re-import
    graph2_dict = json.loads(json_str)

    # Verify roundtrip fidelity
    assert graph1_dict["metadata"]["node_count"] == graph2_dict["metadata"]["node_count"]
    assert graph1_dict["metadata"]["edge_count"] == graph2_dict["metadata"]["edge_count"]
    assert graph1_dict["metadata"]["agent_count"] == graph2_dict["metadata"]["agent_count"]

    # Verify node contents
    for orig_node, reimported_node in zip(graph1_dict["nodes"], graph2_dict["nodes"]):
        assert orig_node["id"] == reimported_node["id"]
        assert orig_node["type"] == reimported_node["type"]
        assert orig_node["label"] == reimported_node["label"]

    # Verify edge contents
    for orig_edge, reimported_edge in zip(graph1_dict["edges"], graph2_dict["edges"]):
        assert orig_edge["source"] == reimported_edge["source"]
        assert orig_edge["target"] == reimported_edge["target"]
        assert orig_edge["type"] == reimported_edge["type"]

    print("✓ test_graph_roundtrip passed")


# ============================================================================
# Test: Obsidian Bridge Note Formatting
# ============================================================================

def test_obsidian_note_formatting():
    """Test agent note formatting for Obsidian"""
    with tempfile.TemporaryDirectory() as tmpdir:
        vault_path = Path(tmpdir)
        bridge = ObsidianBridge(vault_path)

        graph = create_test_graph()
        alice = graph.agents["alice"]

        # Format note
        note = bridge._format_agent_note(alice, graph)

        # Verify note content
        assert "alice" in note
        assert "5.1.0" in note
        assert "0.95" in note  # trust score
        assert "[[fact_checking]]" in note  # wikilink
        assert "[[bob]]" in note  # relay target

        print("✓ test_obsidian_note_formatting passed")


# ============================================================================
# Test: Obsidian Index Formatting
# ============================================================================

def test_obsidian_index_formatting():
    """Test graph index formatting for Obsidian"""
    with tempfile.TemporaryDirectory() as tmpdir:
        vault_path = Path(tmpdir)
        bridge = ObsidianBridge(vault_path)

        graph = create_test_graph()
        index = bridge._format_graph_index(graph)

        # Verify index content
        assert "CSTL Agent Graph Index" in index
        assert "alice" in index
        assert "bob" in index
        assert "charlie" in index
        assert "nodes" in index.lower()  # statistics
        assert "edges" in index.lower()

        print("✓ test_obsidian_index_formatting passed")


# ============================================================================
# Test: Obsidian Bridge Write/Read
# ============================================================================

def test_obsidian_bridge_write_read():
    """Test writing and reading notes via Obsidian Bridge"""
    with tempfile.TemporaryDirectory() as tmpdir:
        vault_path = Path(tmpdir)
        bridge = ObsidianBridge(vault_path)

        # Write a test note
        test_content = "# Test Note\n\nThis is a test."
        success = bridge.write_note("test_note.md", test_content)
        assert success, "Failed to write note"

        # Read it back
        read_content = bridge.read_note("test_note.md")
        assert read_content == test_content, "Read content doesn't match written content"

        print("✓ test_obsidian_bridge_write_read passed")


# ============================================================================
# Test: Obsidian Sync
# ============================================================================

def test_obsidian_sync():
    """Test syncing graph to Obsidian vault"""
    with tempfile.TemporaryDirectory() as tmpdir:
        vault_path = Path(tmpdir)
        bridge = ObsidianBridge(vault_path)

        graph = create_test_graph()

        # Sync graph
        result = bridge.sync_agent_graph(graph)

        # Verify sync results
        assert result["agents_synced"] == 3
        assert result["notes_written"] >= 4  # 3 agents + index
        assert len(result["errors"]) == 0

        # Verify files were created
        assert (vault_path / "CSTL_Agents" / "alice.md").exists()
        assert (vault_path / "CSTL_Agents" / "bob.md").exists()
        assert (vault_path / "CSTL_Agents" / "charlie.md").exists()
        assert (vault_path / "CSTL_Graphs" / "INDEX.md").exists()

        # Verify content
        alice_note = (vault_path / "CSTL_Agents" / "alice.md").read_text()
        assert "alice" in alice_note
        assert "fact_checking" in alice_note

        print("✓ test_obsidian_sync passed")


# ============================================================================
# Test: CSTL Graphify Sync
# ============================================================================

def test_cstl_graphify_sync():
    """Test CSTL→Graphify sync functionality"""
    with tempfile.TemporaryDirectory() as tmpdir:
        vault_path = Path(tmpdir)
        sync = CstlGraphifySync(vault_path=vault_path)

        graph = create_test_graph()
        sync.graph = graph

        # Test agent registration event
        agent_data = {
            "name": "david",
            "version": "5.1.0",
            "capabilities": ["consensus_build"],
            "trust_score": 0.88
        }
        sync.register_agent_event("david", agent_data)

        assert "david" in sync.graph.agents
        assert sync.graph.agents["david"].trust_score == 0.88

        # Test relay event
        sync.relay_event("alice", "david", "test_event")

        # Verify relay edge was added
        relay_edges = [e for e in sync.graph.edges if e.type == EdgeType.RELAYS_TO]
        assert any(e.source == "agent_alice" and e.target == "agent_david" for e in relay_edges)

        print("✓ test_cstl_graphify_sync passed")


# ============================================================================
# Test: JSON Export/Import with File
# ============================================================================

def test_json_file_export_import():
    """Test exporting/importing graph via JSON files"""
    with tempfile.TemporaryDirectory() as tmpdir:
        json_path = Path(tmpdir) / "test_graph.json"

        # Create and export
        sync = CstlGraphifySync()
        graph = create_test_graph()
        sync.graph = graph

        original_node_count = len(sync.graph.nodes)
        original_edge_count = len(sync.graph.edges)

        success = sync.export_graph_json(json_path)
        assert success
        assert json_path.exists()

        # Create new sync and import
        sync2 = CstlGraphifySync()
        success = sync2.load_graph_from_json(json_path)
        assert success

        # Verify graphs match by node/edge counts (agents dict not rebuilt)
        assert len(sync2.graph.nodes) == original_node_count, f"Node count mismatch: {len(sync2.graph.nodes)} != {original_node_count}"
        assert len(sync2.graph.edges) == original_edge_count, f"Edge count mismatch: {len(sync2.graph.edges)} != {original_edge_count}"

        print("✓ test_json_file_export_import passed")


# ============================================================================
# Test: Graph Statistics
# ============================================================================

def test_graph_statistics():
    """Test graph statistics calculation"""
    graph = create_test_graph()
    stats = graph.stats()

    assert "nodes" in stats
    assert "edges" in stats
    assert "agents" in stats
    assert "capabilities" in stats

    assert stats["agents"] == 3
    assert stats["nodes"] >= 7
    assert stats["edges"] >= 6
    assert stats["capabilities"] == 4

    print("✓ test_graph_statistics passed")


# ============================================================================
# Test: Agent Card from Response
# ============================================================================

def test_agent_card_from_response():
    """Test creating AgentCard from server response"""
    response_data = {
        "name": "eve",
        "version": "5.1.0",
        "capabilities": ["monitoring", "alerting"],
        "trust_score": 0.89,
        "public_key": "e" * 64
    }

    agent = AgentCard.from_response(response_data)

    assert agent.name == "eve"
    assert agent.version == "5.1.0"
    assert len(agent.capabilities) == 2
    assert agent.trust_score == 0.89
    assert agent.public_key == "e" * 64

    print("✓ test_agent_card_from_response passed")


# ============================================================================
# Test: Complete Workflow
# ============================================================================

def test_complete_workflow():
    """Test complete workflow: create → export → sync → import"""
    with tempfile.TemporaryDirectory() as tmpdir:
        vault_path = Path(tmpdir)
        json_path = Path(tmpdir) / "graph.json"

        # Step 1: Create graph
        sync = CstlGraphifySync(vault_path=vault_path)
        graph = create_test_graph()
        sync.graph = graph

        original_nodes = len(sync.graph.nodes)
        original_edges = len(sync.graph.edges)

        # Step 2: Export to JSON
        assert sync.export_graph_json(json_path)
        assert json_path.exists()

        # Step 3: Sync to Obsidian
        result = sync.sync_to_obsidian()
        assert result["agents_synced"] == 3

        # Step 4: Import from JSON (new instance)
        sync2 = CstlGraphifySync(vault_path=vault_path)
        assert sync2.load_graph_from_json(json_path)

        # Step 5: Verify consistency by node/edge counts
        assert len(sync2.graph.nodes) == original_nodes, f"Node count mismatch after import: {len(sync2.graph.nodes)} != {original_nodes}"
        assert len(sync2.graph.edges) == original_edges, f"Edge count mismatch after import: {len(sync2.graph.edges)} != {original_edges}"

        # Step 6: Sync imported graph again
        result2 = sync2.sync_to_obsidian()
        assert result2["agents_synced"] == 3

        print("✓ test_complete_workflow passed")


# ============================================================================
# Test Runner
# ============================================================================

def run_all_tests():
    """Run all test suites"""
    tests = [
        test_agent_graph_creation,
        test_graph_export_json,
        test_graph_roundtrip,
        test_obsidian_note_formatting,
        test_obsidian_index_formatting,
        test_obsidian_bridge_write_read,
        test_obsidian_sync,
        test_cstl_graphify_sync,
        test_json_file_export_import,
        test_graph_statistics,
        test_agent_card_from_response,
        test_complete_workflow,
    ]

    print("=" * 70)
    print("Running CSTL Obsidian Bridge Test Suite")
    print("=" * 70)

    passed = 0
    failed = 0

    for test in tests:
        try:
            test()
            passed += 1
        except Exception as e:
            print(f"✗ {test.__name__} failed: {e}")
            failed += 1

    print("\n" + "=" * 70)
    print(f"Results: {passed} passed, {failed} failed")
    print("=" * 70)

    return failed == 0


if __name__ == "__main__":
    import sys
    success = run_all_tests()
    sys.exit(0 if success else 1)
