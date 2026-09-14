#!/usr/bin/env python3
"""
Couche 6: Graphify/Obsidian UI Bridge for CSTL v5.1
Connects agent graph (nodes=AgentCard, edges=relations) to Obsidian vault
Auto-sync on agent_register / relay events
"""

import json
import socket
import requests
import hashlib
from pathlib import Path
from typing import Optional, Dict, List, Tuple, Any
from dataclasses import dataclass, asdict
from datetime import datetime
from enum import Enum


# ============================================================================
# Graph Data Structures
# ============================================================================

class NodeType(Enum):
    """Agent node types in the CSTL graph"""
    AGENT_CARD = "agent_card"
    CAPABILITY = "capability"
    RELAY_POINT = "relay_point"
    AUDIT_EVENT = "audit_event"


class EdgeType(Enum):
    """Relation types between nodes"""
    HAS_CAPABILITY = "has_capability"
    RELAYS_TO = "relays_to"
    SIGNS = "signs"
    VERIFIES = "verifies"
    AUDITS = "audits"
    CONTRADICTS = "contradicts"


@dataclass
class GraphNode:
    """Graph node representing an agent, capability, or event"""
    id: str
    type: NodeType
    label: str
    metadata: Dict[str, Any]
    timestamp: str = None

    def __post_init__(self):
        if self.timestamp is None:
            self.timestamp = datetime.utcnow().isoformat()

    def to_dict(self) -> Dict:
        return {
            "id": self.id,
            "type": self.type.value,
            "label": self.label,
            "metadata": self.metadata,
            "timestamp": self.timestamp
        }


@dataclass
class GraphEdge:
    """Graph edge representing a relation between nodes"""
    source: str
    target: str
    type: EdgeType
    properties: Dict[str, Any] = None

    def __post_init__(self):
        if self.properties is None:
            self.properties = {}

    def to_dict(self) -> Dict:
        return {
            "source": self.source,
            "target": self.target,
            "type": self.type.value,
            "properties": self.properties
        }


@dataclass
class AgentCard:
    """CSTL Agent Card (matches src/agent_discovery.rs::AgentCard)"""
    name: str
    version: str
    capabilities: List[str]
    trust_score: float
    public_key: Optional[str] = None

    @classmethod
    def from_response(cls, data: Dict) -> "AgentCard":
        return cls(
            name=data.get("name"),
            version=data.get("version", "unknown"),
            capabilities=data.get("capabilities", []),
            trust_score=float(data.get("trust_score", 0.5)),
            public_key=data.get("public_key")
        )


# ============================================================================
# Agent Graph Manager
# ============================================================================

class AgentGraph:
    """In-memory representation of CSTL agent network as a graph"""

    def __init__(self):
        self.nodes: Dict[str, GraphNode] = {}
        self.edges: List[GraphEdge] = []
        self.agents: Dict[str, AgentCard] = {}

    def add_agent_node(self, agent: AgentCard) -> None:
        """Add an agent as a node to the graph"""
        agent_id = f"agent_{agent.name}"

        node = GraphNode(
            id=agent_id,
            type=NodeType.AGENT_CARD,
            label=f"{agent.name} (v{agent.version})",
            metadata={
                "name": agent.name,
                "version": agent.version,
                "trust_score": agent.trust_score,
                "public_key": agent.public_key or "none",
                "capabilities_count": len(agent.capabilities)
            }
        )
        self.nodes[agent_id] = node
        self.agents[agent.name] = agent

        # Add edges to capabilities
        for cap in agent.capabilities:
            self._add_capability_node(cap)
            cap_id = f"cap_{cap}"
            edge = GraphEdge(
                source=agent_id,
                target=cap_id,
                type=EdgeType.HAS_CAPABILITY,
                properties={"directional": True}
            )
            self.edges.append(edge)

    def _add_capability_node(self, capability: str) -> None:
        """Add a capability node if not already present"""
        cap_id = f"cap_{capability}"
        if cap_id not in self.nodes:
            node = GraphNode(
                id=cap_id,
                type=NodeType.CAPABILITY,
                label=capability,
                metadata={"capability_type": capability}
            )
            self.nodes[cap_id] = node

    def add_relay_edge(self, sender: str, receiver: str, purpose: str) -> None:
        """Add a relay/communication edge between two agents"""
        sender_id = f"agent_{sender}"
        receiver_id = f"agent_{receiver}"

        if sender_id in self.nodes and receiver_id in self.nodes:
            edge = GraphEdge(
                source=sender_id,
                target=receiver_id,
                type=EdgeType.RELAYS_TO,
                properties={"purpose": purpose, "directional": True}
            )
            self.edges.append(edge)

    def add_signature_edge(self, agent: str, public_key: str) -> None:
        """Add an edge indicating an agent's public key for signing"""
        agent_id = f"agent_{agent}"
        key_id = f"key_{hashlib.sha256(public_key.encode()).hexdigest()[:12]}"

        if key_id not in self.nodes:
            node = GraphNode(
                id=key_id,
                type=NodeType.AUDIT_EVENT,
                label=f"Key [{public_key[:8]}...]",
                metadata={"key_hash": public_key[:16], "agent": agent}
            )
            self.nodes[key_id] = node

        if agent_id in self.nodes:
            edge = GraphEdge(
                source=agent_id,
                target=key_id,
                type=EdgeType.SIGNS,
                properties={"public_key": public_key}
            )
            self.edges.append(edge)

    def to_dict(self) -> Dict:
        """Export graph as dictionary (Graphify-compatible JSON)"""
        return {
            "nodes": [node.to_dict() for node in self.nodes.values()],
            "edges": [edge.to_dict() for edge in self.edges],
            "metadata": {
                "node_count": len(self.nodes),
                "edge_count": len(self.edges),
                "agent_count": len(self.agents),
                "timestamp": datetime.utcnow().isoformat(),
                "version": "5.1.0"
            }
        }

    def stats(self) -> Dict:
        """Return graph statistics"""
        return {
            "nodes": len(self.nodes),
            "edges": len(self.edges),
            "agents": len(self.agents),
            "capabilities": len([n for n in self.nodes.values() if n.type == NodeType.CAPABILITY])
        }


# ============================================================================
# Obsidian Vault Bridge
# ============================================================================

class ObsidianBridge:
    """Bridge to Obsidian vault via HTTP API"""

    def __init__(self, vault_path: Path, obsidian_port: int = 27123):
        """
        Initialize bridge to Obsidian vault

        Args:
            vault_path: Path to Obsidian vault directory
            obsidian_port: HTTP port Obsidian runs on (default 27123)
        """
        self.vault_path = Path(vault_path)
        self.obsidian_port = obsidian_port
        self.api_url = f"http://localhost:{obsidian_port}"
        self.graph_dir = self.vault_path / "CSTL_Graphs"
        self.agent_dir = self.vault_path / "CSTL_Agents"

        # Ensure directories exist
        self.graph_dir.mkdir(parents=True, exist_ok=True)
        self.agent_dir.mkdir(parents=True, exist_ok=True)

    def health_check(self) -> bool:
        """Check if Obsidian is running and accessible"""
        try:
            resp = requests.get(f"{self.api_url}/", timeout=2)
            return resp.status_code in [200, 404]  # 404 ok for root
        except Exception:
            return False

    def write_note(self, path: str, content: str) -> bool:
        """
        Write a note to Obsidian vault (fallback: direct file write)

        Args:
            path: Note path relative to vault root
            content: Note markdown content

        Returns:
            True if successful
        """
        try:
            # Try HTTP API first (Obsidian community plugin)
            headers = {"Content-Type": "text/plain"}
            resp = requests.post(
                f"{self.api_url}/vault/create",
                json={"path": path, "content": content},
                headers=headers,
                timeout=5
            )
            if resp.status_code == 200:
                return True
        except Exception:
            pass

        # Fallback: direct file write to vault
        try:
            note_path = self.vault_path / path
            note_path.parent.mkdir(parents=True, exist_ok=True)

            # Append .md if needed
            if not note_path.suffix:
                note_path = note_path.with_suffix('.md')

            note_path.write_text(content, encoding='utf-8')
            return True
        except Exception as e:
            print(f"❌ Failed to write note {path}: {e}")
            return False

    def read_note(self, path: str) -> Optional[str]:
        """Read a note from Obsidian vault"""
        try:
            # Try HTTP API first
            resp = requests.get(
                f"{self.api_url}/vault/read",
                params={"path": path},
                timeout=5
            )
            if resp.status_code == 200:
                return resp.text
        except Exception:
            pass

        # Fallback: direct file read
        try:
            note_path = self.vault_path / path
            if not note_path.suffix:
                note_path = note_path.with_suffix('.md')

            if note_path.exists():
                return note_path.read_text(encoding='utf-8')
        except Exception as e:
            print(f"❌ Failed to read note {path}: {e}")

        return None

    def sync_agent_graph(self, graph: AgentGraph, update_index: bool = True) -> Dict:
        """
        Sync agent graph to Obsidian vault as interlinked notes

        Args:
            graph: AgentGraph to sync
            update_index: Whether to update graph index

        Returns:
            Dict with sync results
        """
        results = {
            "agents_synced": 0,
            "notes_written": 0,
            "errors": []
        }

        # Sync each agent as a separate note
        for agent_name, agent in graph.agents.items():
            note_path = f"CSTL_Agents/{agent_name}.md"
            content = self._format_agent_note(agent, graph)

            if self.write_note(note_path, content):
                results["agents_synced"] += 1
                results["notes_written"] += 1
            else:
                results["errors"].append(f"Failed to sync agent {agent_name}")

        # Sync graph index
        if update_index:
            index_path = "CSTL_Graphs/INDEX.md"
            index_content = self._format_graph_index(graph)

            if self.write_note(index_path, index_content):
                results["notes_written"] += 1
            else:
                results["errors"].append("Failed to write graph index")

        return results

    def _format_agent_note(self, agent: AgentCard, graph: AgentGraph) -> str:
        """Format an agent as an Obsidian note with wikilinks"""
        caps_list = "\n".join([f"- [[{cap}]]" for cap in agent.capabilities])
        key_info = f"**Public Key**: `{agent.public_key[:32]}...`" if agent.public_key else "*Not signed*"

        # Find relay targets
        agent_id = f"agent_{agent.name}"
        relays = [e for e in graph.edges if e.source == agent_id and e.type == EdgeType.RELAYS_TO]
        relay_list = "\n".join([
            f"- [[{e.target.replace('agent_', '')}]] ({e.properties.get('purpose', 'relay')})"
            for e in relays
        ]) if relays else "*None*"

        note = f"""# {agent.name}

**Version**: {agent.version}
**Trust Score**: {agent.trust_score:.2f}
{key_info}

## Capabilities
{caps_list}

## Relay Connections
{relay_list}

## Metadata
- Created: {datetime.utcnow().isoformat()}
- Layer: 7 (Agent Discovery & Routing)
- Part of: [[CSTL_Graphs/INDEX|CSTL Graph]]

---
*Auto-generated by CSTL Couche 6 Graphify Bridge*
"""
        return note

    def _format_graph_index(self, graph: AgentGraph) -> str:
        """Format graph index/TOC for Obsidian"""
        agent_list = "\n".join([
            f"- [[{name}]] (trust: {agent.trust_score:.2f})"
            for name, agent in sorted(graph.agents.items())
        ])

        stats = graph.stats()

        index = f"""# CSTL Agent Graph Index

**Generated**: {datetime.utcnow().isoformat()}
**Version**: CSTL v5.1.0

## Graph Statistics
- **Nodes**: {stats['nodes']}
- **Edges**: {stats['edges']}
- **Agents**: {stats['agents']}
- **Capabilities**: {stats['capabilities']}

## Registered Agents
{agent_list}

## Architecture
This graph represents Layer 6 (Graphify UI) and Layer 7 (Agent Discovery & Routing) of the CSTL system:

- **Nodes**: Agent cards with capabilities, trust scores, public keys
- **Edges**: Relations (has_capability, relays_to, signs, verifies, audits)
- **Metadata**: Timestamps, signatures, audit trails

## Export Formats
- **JSON**: `cstl_agent_graph.json` (raw graph structure)
- **Markdown**: This index + individual agent notes
- **Graph Visualization**: Graphify format (compatible with D3.js, Obsidian Graph View)

## Layer References
- [[Layer_6_Graphify|Layer 6: Graphify UI Connection]]
- [[Layer_7_AgentDiscovery|Layer 7: Agent Discovery & Routing]]
- [[Layer_5c_ADN|Layer 5c: Audit Trail Server]]

---
*Last updated: {datetime.utcnow().isoformat()}*
"""
        return index


# ============================================================================
# CSTL Client Integration
# ============================================================================

class CstlGraphifySync:
    """Synchronize CSTL agent events to Graphify graph"""

    def __init__(self, cstl_host: str = "127.0.0.1", cstl_port: int = 5050,
                 vault_path: Path = None):
        """
        Initialize CSTL<->Obsidian sync

        Args:
            cstl_host: CSTL server host
            cstl_port: CSTL server port
            vault_path: Path to Obsidian vault (auto-detected if None)
        """
        self.cstl_host = cstl_host
        self.cstl_port = cstl_port
        self.graph = AgentGraph()

        # Auto-detect vault path
        if vault_path is None:
            vault_path = Path.cwd()  # Assume current dir is vault root
            if not (vault_path / ".obsidian").exists():
                # Try looking for .obsidian in parent
                vault_path = Path.home() / "vault" / "cstl"

        self.vault_path = Path(vault_path)
        self.obsidian = ObsidianBridge(self.vault_path)

    def load_graph_from_json(self, json_path: Path) -> bool:
        """Load pre-built graph from JSON export"""
        try:
            data = json.loads(json_path.read_text())

            # Reconstruct nodes
            for node_data in data.get("nodes", []):
                node = GraphNode(
                    id=node_data["id"],
                    type=NodeType(node_data["type"]),
                    label=node_data["label"],
                    metadata=node_data.get("metadata", {}),
                    timestamp=node_data.get("timestamp")
                )
                self.graph.nodes[node.id] = node

                # Reconstruct agents dict from agent_card nodes
                if node.type == NodeType.AGENT_CARD:
                    metadata = node.metadata
                    agent = AgentCard(
                        name=metadata.get("name"),
                        version=metadata.get("version", "unknown"),
                        capabilities=[],  # Will be filled from edges
                        trust_score=float(metadata.get("trust_score", 0.5)),
                        public_key=metadata.get("public_key") if metadata.get("public_key") != "none" else None
                    )
                    self.graph.agents[agent.name] = agent

            # Reconstruct edges
            for edge_data in data.get("edges", []):
                edge = GraphEdge(
                    source=edge_data["source"],
                    target=edge_data["target"],
                    type=EdgeType(edge_data["type"]),
                    properties=edge_data.get("properties", {})
                )
                self.graph.edges.append(edge)

                # Rebuild capabilities list from HAS_CAPABILITY edges
                if edge.type == EdgeType.HAS_CAPABILITY and edge.source.startswith("agent_"):
                    agent_name = edge.source.replace("agent_", "")
                    cap_name = edge.target.replace("cap_", "")
                    if agent_name in self.graph.agents:
                        if cap_name not in self.graph.agents[agent_name].capabilities:
                            self.graph.agents[agent_name].capabilities.append(cap_name)

            print(f"✓ Loaded graph from {json_path}: {len(self.graph.nodes)} nodes, {len(self.graph.edges)} edges")
            return True
        except Exception as e:
            print(f"❌ Failed to load graph JSON: {e}")
            return False

    def export_graph_json(self, output_path: Path) -> bool:
        """Export current graph as JSON (Graphify format)"""
        try:
            graph_dict = self.graph.to_dict()
            output_path.write_text(json.dumps(graph_dict, indent=2))
            print(f"✓ Exported graph to {output_path}")
            return True
        except Exception as e:
            print(f"❌ Failed to export graph: {e}")
            return False

    def sync_to_obsidian(self) -> Dict:
        """Sync current graph to Obsidian vault"""
        if not self.obsidian.health_check():
            print("⚠️  Obsidian not running, falling back to direct file write")

        return self.obsidian.sync_agent_graph(self.graph)

    def register_agent_event(self, agent_name: str, agent_data: Dict) -> None:
        """
        Handle agent_register event from CSTL server

        Args:
            agent_name: Name of registered agent
            agent_data: Agent data from server response
        """
        agent = AgentCard.from_response(agent_data)
        self.graph.add_agent_node(agent)
        print(f"✓ Registered agent '{agent_name}' in graph")

    def relay_event(self, sender: str, receiver: str, purpose: str) -> None:
        """
        Handle relay/communication event

        Args:
            sender: Sending agent name
            receiver: Receiving agent name
            purpose: Purpose of relay
        """
        self.graph.add_relay_edge(sender, receiver, purpose)
        print(f"✓ Added relay edge {sender} -> {receiver} ({purpose})")


# ============================================================================
# CLI Interface
# ============================================================================

def cli_export_graph(json_path: Path = None) -> None:
    """Export current agent graph as JSON"""
    if json_path is None:
        json_path = Path("cstl_agent_graph.json")

    sync = CstlGraphifySync()

    # For demo: add some sample agents
    sample_agents = [
        AgentCard("alice", "5.1.0", ["fact_checking", "semantic_analysis"], 0.95, "a" * 64),
        AgentCard("bob", "5.1.0", ["fact_checking", "contradiction_detection"], 0.87),
        AgentCard("charlie", "5.1.0", ["execution_lab"], 0.92, "c" * 64),
    ]

    for agent in sample_agents:
        sync.graph.add_agent_node(agent)

    # Add some relay edges
    sync.graph.add_relay_edge("alice", "bob", "dialogue_turn")
    sync.graph.add_relay_edge("bob", "charlie", "verification_request")
    sync.graph.add_relay_edge("charlie", "alice", "audit_result")

    sync.export_graph_json(json_path)
    print(f"\n📊 Graph statistics:\n{json.dumps(sync.graph.stats(), indent=2)}")


def cli_sync_obsidian(json_path: Path = None, vault_path: Path = None) -> None:
    """Sync graph to Obsidian vault"""
    if json_path is None:
        json_path = Path("cstl_agent_graph.json")

    if vault_path is None:
        vault_path = Path.cwd()

    sync = CstlGraphifySync(vault_path=vault_path)

    if json_path.exists():
        sync.load_graph_from_json(json_path)
    else:
        print(f"⚠️  JSON graph not found at {json_path}, using defaults")
        # Build sample graph
        sample_agents = [
            AgentCard("alice", "5.1.0", ["fact_checking"], 0.95),
            AgentCard("bob", "5.1.0", ["contradiction_detection"], 0.87),
        ]
        for agent in sample_agents:
            sync.graph.add_agent_node(agent)

    result = sync.sync_to_obsidian()
    print(f"\n✓ Sync complete:")
    print(f"  - Agents synced: {result['agents_synced']}")
    print(f"  - Notes written: {result['notes_written']}")
    if result['errors']:
        print(f"  - Errors: {', '.join(result['errors'])}")


if __name__ == "__main__":
    import sys

    if len(sys.argv) > 1 and sys.argv[1] == "export":
        output = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("cstl_agent_graph.json")
        cli_export_graph(output)
    elif len(sys.argv) > 1 and sys.argv[1] == "sync":
        json_input = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("cstl_agent_graph.json")
        vault = Path(sys.argv[3]) if len(sys.argv) > 3 else Path.cwd()
        cli_sync_obsidian(json_input, vault)
    else:
        print("Usage:")
        print("  python cstl_obsidian_bridge.py export [output.json]")
        print("  python cstl_obsidian_bridge.py sync [input.json] [vault_path]")
