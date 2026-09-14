#!/usr/bin/env python3

import json
import sqlite3
import hashlib
from datetime import datetime
from typing import Dict, List, Optional, Tuple, Set
from collections import defaultdict
import threading
import time

class GraphifyNode:
    def __init__(self, node_id: str, node_type: str, label: str, metadata: Dict = None):
        self.id = node_id
        self.type = node_type
        self.label = label
        self.metadata = metadata or {}
        self.created_at = datetime.utcnow().isoformat()

    def to_graphify_format(self) -> Dict:
        color_map = {
            "agent": "#4A90E2",
            "relation": "#7B68EE",
            "decision": "#50C878",
            "commitment": "#FF6B6B",
            "audit_entry": "#FFB347",
            "deontic_must": "#DC143C",
            "deontic_must_not": "#8B0000",
            "deontic_may": "#32CD32"
        }

        deontic_type = self.metadata.get("deontic_type", "")
        if deontic_type in ("MUST", "must"):
            color = color_map["deontic_must"]
        elif deontic_type in ("MUST_NOT", "must_not"):
            color = color_map["deontic_must_not"]
        elif deontic_type in ("MAY", "may"):
            color = color_map["deontic_may"]
        else:
            color = color_map.get(self.type, "#999999")

        return {
            "id": self.id,
            "label": self.label,
            "type": self.type,
            "color": color,
            "size": self.metadata.get("size", 1.0),
            "metadata": {
                **self.metadata,
                "created_at": self.created_at,
                "class": self.type
            }
        }

class GraphifyEdge:
    def __init__(self, source: str, target: str, edge_type: str, weight: float = 1.0, metadata: Dict = None):
        self.source = source
        self.target = target
        self.type = edge_type
        self.weight = weight
        self.metadata = metadata or {}

    def to_graphify_format(self) -> Dict:
        edge_type_colors = {
            "sends_to": "#4A90E2",
            "responds_to": "#7B68EE",
            "references": "#50C878",
            "authorizes": "#FF6B6B",
            "restricts": "#8B0000",
            "confirms": "#FFB347",
            "violates": "#DC143C",
            "implements": "#32CD32"
        }

        return {
            "source": self.source,
            "target": self.target,
            "type": self.type,
            "weight": self.weight,
            "color": edge_type_colors.get(self.type, "#999999"),
            "metadata": self.metadata
        }

class GraphifyExporter:
    def __init__(self, db_path: str = "cstl_adn.db"):
        self.db_path = db_path
        self.nodes: Dict[str, GraphifyNode] = {}
        self.edges: List[GraphifyEdge] = []
        self.lock = threading.Lock()
        self.last_sync = None

    def _get_connection(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def _load_agents_from_registry(self) -> Dict[str, Dict]:
        agents = {}
        try:
            conn = self._get_connection()
            cursor = conn.cursor()
            cursor.execute("SELECT id, label, public_key, custom_metadata FROM agent_registry")
            for row in cursor.fetchall():
                agent_id = row[0]
                agents[agent_id] = {
                    "label": row[1],
                    "public_key": row[2],
                    "metadata": json.loads(row[3] or "{}")
                }
            conn.close()
        except Exception:
            pass
        return agents

    def _load_audit_trail(self) -> List[Dict]:
        entries = []
        try:
            conn = self._get_connection()
            cursor = conn.cursor()
            cursor.execute("""
                SELECT id, sender, receiver, purpose, payload_hash, created_at, payload_json
                FROM audit_trail
                ORDER BY created_at DESC
                LIMIT 1000
            """)
            for row in cursor.fetchall():
                entries.append({
                    "id": row[0],
                    "sender": row[1],
                    "receiver": row[2],
                    "purpose": row[3],
                    "payload_hash": row[4],
                    "created_at": row[5],
                    "payload": json.loads(row[6] or "{}")
                })
            conn.close()
        except Exception:
            pass
        return entries

    def _load_deontic_modalities(self) -> List[Dict]:
        modalities = []
        try:
            conn = self._get_connection()
            cursor = conn.cursor()
            cursor.execute("""
                SELECT id, agent_id, modality_type, relation_name, active
                FROM deontic_modalities
                ORDER BY created_at DESC
                LIMIT 500
            """)
            for row in cursor.fetchall():
                modalities.append({
                    "id": row[0],
                    "agent_id": row[1],
                    "type": row[2],
                    "relation": row[3],
                    "active": row[4]
                })
            conn.close()
        except Exception:
            pass
        return modalities

    def build_graph(self) -> Tuple[Dict[str, GraphifyNode], List[GraphifyEdge]]:
        with self.lock:
            self.nodes.clear()
            self.edges.clear()

            agents = self._load_agents_from_registry()
            audit_entries = self._load_audit_trail()
            deontic_mods = self._load_deontic_modalities()

            for agent_id, agent_info in agents.items():
                node = GraphifyNode(
                    node_id=agent_id,
                    node_type="agent",
                    label=agent_info["label"],
                    metadata={
                        "public_key": agent_info["public_key"][:16] + "..." if agent_info["public_key"] else "",
                        **agent_info["metadata"]
                    }
                )
                self.nodes[agent_id] = node

            entry_ids = set()
            for entry in audit_entries:
                entry_id = f"audit_{entry['id']}"
                entry_ids.add(entry_id)

                node = GraphifyNode(
                    node_id=entry_id,
                    node_type="audit_entry",
                    label=f"{entry['purpose'][:20]}...",
                    metadata={
                        "sender": entry["sender"],
                        "receiver": entry["receiver"],
                        "purpose": entry["purpose"],
                        "hash": entry["payload_hash"][:12] + "...",
                        "timestamp": entry["created_at"]
                    }
                )
                self.nodes[entry_id] = node

                if entry["sender"] in agents:
                    edge = GraphifyEdge(
                        source=entry["sender"],
                        target=entry_id,
                        edge_type="sends_to",
                        metadata={"purpose": entry["purpose"]}
                    )
                    self.edges.append(edge)

                if entry["receiver"] and entry["receiver"] in agents:
                    edge = GraphifyEdge(
                        source=entry_id,
                        target=entry["receiver"],
                        edge_type="responds_to"
                    )
                    self.edges.append(edge)

            for mod in deontic_mods:
                mod_id = f"deontic_{mod['id']}"
                mod_type_label = mod["type"].lower()

                node = GraphifyNode(
                    node_id=mod_id,
                    node_type=f"deontic_{mod_type_label}",
                    label=f"{mod['type']}: {mod['relation'][:15]}",
                    metadata={
                        "deontic_type": mod["type"],
                        "relation": mod["relation"],
                        "active": mod["active"]
                    }
                )
                self.nodes[mod_id] = node

                if mod["agent_id"] in agents:
                    edge_type = {
                        "MUST": "authorizes",
                        "MUST_NOT": "restricts",
                        "MAY": "confirms"
                    }.get(mod["type"], "references")

                    edge = GraphifyEdge(
                        source=mod["agent_id"],
                        target=mod_id,
                        edge_type=edge_type,
                        metadata={"modality": mod["type"]}
                    )
                    self.edges.append(edge)

            self.last_sync = datetime.utcnow().isoformat()
            return self.nodes.copy(), self.edges.copy()

    def export_to_json(self, output_file: Optional[str] = None) -> Dict:
        nodes_dict, edges_list = self.build_graph()

        graph_data = {
            "metadata": {
                "version": "1.0.0",
                "exported_at": datetime.utcnow().isoformat(),
                "node_count": len(nodes_dict),
                "edge_count": len(edges_list),
                "format": "graphify-standard"
            },
            "nodes": [node.to_graphify_format() for node in nodes_dict.values()],
            "edges": [edge.to_graphify_format() for edge in edges_list]
        }

        if output_file:
            with open(output_file, 'w') as f:
                json.dump(graph_data, f, indent=2)

        return graph_data

    def export_for_obsidian(self, vault_path: str) -> Dict[str, str]:
        nodes_dict, edges_list = self.build_graph()

        vault_content = {}

        vault_content["_index.md"] = self._generate_index(nodes_dict, edges_list)

        for agent_id, node in nodes_dict.items():
            if node.type == "agent":
                vault_content[f"agents/{agent_id}.md"] = self._generate_agent_note(node, edges_list)

        agent_map = {n.id: n for n in nodes_dict.values() if n.type == "agent"}
        for edge in edges_list:
            if edge.source in agent_map and edge.target in agent_map:
                rel_file = f"relations/{edge.source}_{edge.target}.md"
                if rel_file not in vault_content:
                    vault_content[rel_file] = self._generate_relation_note(
                        edge, agent_map[edge.source], agent_map[edge.target]
                    )

        for node_id, node in nodes_dict.items():
            if node.type.startswith("deontic_"):
                vault_content[f"modalities/{node_id}.md"] = self._generate_deontic_note(node)

        return vault_content

    def _generate_index(self, nodes: Dict, edges: List) -> str:
        agent_nodes = [n for n in nodes.values() if n.type == "agent"]
        deontic_nodes = [n for n in nodes.values() if n.type.startswith("deontic_")]
        audit_nodes = [n for n in nodes.values() if n.type == "audit_entry"]

        md = "# CSTL Graph Index\n\n"
        md += f"**Generated**: {datetime.utcnow().isoformat()}\n\n"
        md += f"**Nodes**: {len(nodes)} | **Edges**: {len(edges)}\n\n"

        if agent_nodes:
            md += "## Agents\n\n"
            for node in sorted(agent_nodes, key=lambda n: n.label):
                md += f"- [[{node.id}]] {node.label}\n"
            md += "\n"

        if deontic_nodes:
            must_nodes = [n for n in deontic_nodes if "must" in n.type.lower() and "not" not in n.type.lower()]
            must_not_nodes = [n for n in deontic_nodes if "must_not" in n.type.lower()]
            may_nodes = [n for n in deontic_nodes if "may" in n.type.lower()]

            if must_nodes:
                md += "### MUST Commitments\n"
                for node in must_nodes:
                    md += f"- [[{node.id}]] {node.label}\n"
                md += "\n"

            if must_not_nodes:
                md += "### MUST_NOT Restrictions\n"
                for node in must_not_nodes:
                    md += f"- [[{node.id}]] {node.label}\n"
                md += "\n"

            if may_nodes:
                md += "### MAY Permissions\n"
                for node in may_nodes:
                    md += f"- [[{node.id}]] {node.label}\n"
                md += "\n"

        if audit_nodes:
            md += f"## Recent Activity ({len(audit_nodes)} entries)\n\n"
            for node in sorted(audit_nodes, key=lambda n: n.metadata.get("timestamp", ""), reverse=True)[:20]:
                md += f"- {node.label} ({node.metadata.get('purpose', 'unknown')})\n"
            md += "\n"

        return md

    def _generate_agent_note(self, node: GraphifyNode, edges: List) -> str:
        related_edges = [e for e in edges if e.source == node.id or e.target == node.id]

        md = f"# {node.label}\n\n"
        md += f"**Agent ID**: {node.id}\n"
        md += f"**Type**: agent\n"
        md += f"**Created**: {node.created_at}\n\n"

        if node.metadata.get("public_key"):
            md += f"**Public Key**: `{node.metadata['public_key']}`\n\n"

        md += "## Relations\n\n"
        if related_edges:
            for edge in related_edges:
                other_id = edge.target if edge.source == node.id else edge.source
                md += f"- **{edge.type}**: [[{other_id}]]\n"
        else:
            md += "No relations\n"

        return md

    def _generate_relation_note(self, edge: GraphifyEdge, source_node: GraphifyNode, target_node: GraphifyNode) -> str:
        md = f"# {source_node.label} → {target_node.label}\n\n"
        md += f"**Type**: {edge.type}\n"
        md += f"**Weight**: {edge.weight}\n\n"
        md += f"**Source**: [[{source_node.id}]]\n"
        md += f"**Target**: [[{target_node.id}]]\n\n"
        md += f"**Metadata**: {json.dumps(edge.metadata, indent=2)}\n"
        return md

    def _generate_deontic_note(self, node: GraphifyNode) -> str:
        md = f"# {node.label}\n\n"
        modality = node.metadata.get("deontic_type", "UNKNOWN")
        md += f"**Modality**: `{modality}`\n"
        md += f"**Relation**: {node.metadata.get('relation', 'N/A')}\n"
        md += f"**Active**: {node.metadata.get('active', True)}\n\n"
        return md

    def filter_nodes_by_type(self, node_type: str) -> List[GraphifyNode]:
        return [n for n in self.nodes.values() if n.type == node_type]

    def filter_edges_by_type(self, edge_type: str) -> List[GraphifyEdge]:
        return [e for e in self.edges if e.type == edge_type]

    def search_nodes(self, query: str) -> List[GraphifyNode]:
        query_lower = query.lower()
        results = []
        for node in self.nodes.values():
            if query_lower in node.label.lower() or query_lower in node.id.lower():
                results.append(node)
            elif any(query_lower in str(v).lower() for v in node.metadata.values()):
                results.append(node)
        return results

    def traverse_graph(self, start_node_id: str, max_depth: int = 3) -> Dict:
        if start_node_id not in self.nodes:
            return {"error": "Node not found"}

        visited = {start_node_id}
        frontier = [(start_node_id, 0)]
        result_nodes = [self.nodes[start_node_id]]
        result_edges = []

        while frontier:
            current_id, depth = frontier.pop(0)
            if depth >= max_depth:
                continue

            for edge in self.edges:
                new_node_id = None
                if edge.source == current_id and edge.target not in visited:
                    new_node_id = edge.target
                    result_edges.append(edge)
                elif edge.target == current_id and edge.source not in visited:
                    new_node_id = edge.source
                    result_edges.append(edge)

                if new_node_id and new_node_id in self.nodes:
                    visited.add(new_node_id)
                    result_nodes.append(self.nodes[new_node_id])
                    frontier.append((new_node_id, depth + 1))

        return {
            "root": start_node_id,
            "depth": max_depth,
            "nodes": [n.to_graphify_format() for n in result_nodes],
            "edges": [e.to_graphify_format() for e in result_edges]
        }

    def get_graph_stats(self) -> Dict:
        agent_count = len([n for n in self.nodes.values() if n.type == "agent"])
        audit_count = len([n for n in self.nodes.values() if n.type == "audit_entry"])
        deontic_count = len([n for n in self.nodes.values() if n.type.startswith("deontic_")])

        edge_types = defaultdict(int)
        for edge in self.edges:
            edge_types[edge.type] += 1

        must_count = len([n for n in self.nodes.values() if n.type == "deontic_must"])
        must_not_count = len([n for n in self.nodes.values() if n.type == "deontic_must_not"])
        may_count = len([n for n in self.nodes.values() if n.type == "deontic_may"])

        return {
            "total_nodes": len(self.nodes),
            "total_edges": len(self.edges),
            "agents": agent_count,
            "audit_entries": audit_count,
            "deontic_modalities": deontic_count,
            "deontic_must": must_count,
            "deontic_must_not": must_not_count,
            "deontic_may": may_count,
            "edge_types": dict(edge_types),
            "last_sync": self.last_sync
        }

def main():
    exporter = GraphifyExporter()

    print("Building graph from CSTL audit trail...")
    exporter.build_graph()

    print("Exporting to Graphify JSON...")
    exporter.export_to_json("cstl_graph.json")

    print("Exporting to Obsidian vault format...")
    vault = exporter.export_for_obsidian("/tmp/cstl_vault")

    print("\nGraph Statistics:")
    stats = exporter.get_graph_stats()
    for key, value in stats.items():
        print(f"  {key}: {value}")

    print("\nVault files to create:")
    for filename in sorted(vault.keys())[:10]:
        print(f"  {filename}")
    if len(vault) > 10:
        print(f"  ... and {len(vault) - 10} more")

if __name__ == "__main__":
    main()
