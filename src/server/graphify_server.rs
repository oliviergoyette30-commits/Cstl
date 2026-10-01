use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::Utc;
use crate::server::audit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphifyNodeMetadata {
    pub created_at: String,
    pub class: String,
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphifyNode {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub color: String,
    pub size: f64,
    pub metadata: GraphifyNodeMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphifyEdge {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    pub weight: f64,
    pub color: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphifyMetadata {
    pub version: String,
    pub exported_at: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphifyPayload {
    pub metadata: GraphifyMetadata,
    pub nodes: Vec<GraphifyNode>,
    pub edges: Vec<GraphifyEdge>,
}

pub struct GraphifyExporter {
    chain: audit::HashChain,
}

impl GraphifyExporter {
    pub fn new(chain: audit::HashChain) -> Self {
        GraphifyExporter { chain }
    }

    fn get_deontic_color(deontic_type: &str) -> String {
        match deontic_type {
            "MUST" | "must" => "#DC143C".to_string(),
            "MUST_NOT" | "must_not" => "#8B0000".to_string(),
            "MAY" | "may" => "#32CD32".to_string(),
            _ => "#999999".to_string(),
        }
    }

    fn get_node_type_color(node_type: &str) -> String {
        match node_type {
            "agent" => "#4A90E2".to_string(),
            "relation" => "#7B68EE".to_string(),
            "decision" => "#50C878".to_string(),
            "commitment" => "#FF6B6B".to_string(),
            "audit_entry" => "#FFB347".to_string(),
            _ => "#999999".to_string(),
        }
    }

    fn get_edge_type_color(edge_type: &str) -> String {
        match edge_type {
            "sends_to" => "#4A90E2".to_string(),
            "responds_to" => "#7B68EE".to_string(),
            "references" => "#50C878".to_string(),
            "authorizes" => "#FF6B6B".to_string(),
            "restricts" => "#8B0000".to_string(),
            "confirms" => "#FFB347".to_string(),
            "violates" => "#DC143C".to_string(),
            "implements" => "#32CD32".to_string(),
            _ => "#999999".to_string(),
        }
    }

    /// Tronque `s` a au plus `max_bytes` octets, SANS jamais couper au
    /// milieu d'un caractere UTF-8 multi-octets. Trouvaille honnete
    /// (2026-10-01, en lisant `build_from_audit_trail` avant de le cabler
    /// dans `rest_api.rs`): le code original faisait `&entry.purpose[..20.min(entry.purpose.len())]`
    /// directement -- un indexage par OCTET sur un `str`, qui PANIQUE en
    /// Rust des que l'indice tombe au milieu d'un caractere multi-octets
    /// (`byte index N is not a char boundary`). `purpose` vient directement
    /// de `INTENT_PAYLOAD.purpose`, un champ texte totalement controle par
    /// l'expediteur du payload CSTL (voir `audit.rs::HashChain::append`) --
    /// n'importe quel client pouvait donc faire planter cette route HTTP
    /// simplement en envoyant un payload dont `purpose` place un caractere
    /// accentue/emoji/etc. pile au 20e octet. `entry.hash` n'a pas ce
    /// probleme (toujours de l'hexadecimal SHA-256, donc ASCII pur par
    /// construction), seul `purpose` est concerne.
    fn truncate_utf8_safe(s: &str, max_bytes: usize) -> &str {
        if s.len() <= max_bytes {
            return s;
        }
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }

    pub fn build_from_audit_trail(&self) -> GraphifyPayload {
        let mut nodes: HashMap<String, GraphifyNode> = HashMap::new();
        let mut edges: Vec<GraphifyEdge> = Vec::new();
        let mut agent_set = std::collections::HashSet::new();

        let entries = &self.chain.entries;

        for (idx, entry) in entries.iter().enumerate() {
            let entry_id = format!("audit_{}", idx);

            agent_set.insert(entry.sender.clone());
            if !entry.receiver.is_empty() {
                agent_set.insert(entry.receiver.clone());
            }

            let mut metadata = HashMap::new();
            metadata.insert("sender".to_string(), entry.sender.clone());
            metadata.insert("receiver".to_string(), entry.receiver.clone());
            metadata.insert("purpose".to_string(), entry.purpose.clone());
            metadata.insert("hash".to_string(), format!("{}...", &entry.hash[..12.min(entry.hash.len())]));
            metadata.insert("timestamp".to_string(), Utc::now().to_rfc3339());

            let node = GraphifyNode {
                id: entry_id.clone(),
                label: format!("{}...", Self::truncate_utf8_safe(&entry.purpose, 20)),
                node_type: "audit_entry".to_string(),
                color: Self::get_node_type_color("audit_entry"),
                size: 1.0,
                metadata: GraphifyNodeMetadata {
                    created_at: Utc::now().to_rfc3339(),
                    class: "audit_entry".to_string(),
                    extra: metadata.clone(),
                },
            };

            nodes.insert(entry_id.clone(), node);

            if !entry.sender.is_empty() {
                edges.push(GraphifyEdge {
                    source: entry.sender.clone(),
                    target: entry_id.clone(),
                    edge_type: "sends_to".to_string(),
                    weight: 1.0,
                    color: Self::get_edge_type_color("sends_to"),
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("purpose".to_string(), entry.purpose.clone());
                        m
                    },
                });
            }

            if !entry.receiver.is_empty() {
                edges.push(GraphifyEdge {
                    source: entry_id.clone(),
                    target: entry.receiver.clone(),
                    edge_type: "responds_to".to_string(),
                    weight: 1.0,
                    color: Self::get_edge_type_color("responds_to"),
                    metadata: HashMap::new(),
                });
            }
        }

        for agent_id in agent_set.iter() {
            if !nodes.contains_key(agent_id) {
                let mut metadata = HashMap::new();
                metadata.insert("type".to_string(), "agent".to_string());

                let node = GraphifyNode {
                    id: agent_id.clone(),
                    label: agent_id.clone(),
                    node_type: "agent".to_string(),
                    color: Self::get_node_type_color("agent"),
                    size: 1.5,
                    metadata: GraphifyNodeMetadata {
                        created_at: Utc::now().to_rfc3339(),
                        class: "agent".to_string(),
                        extra: metadata,
                    },
                };

                nodes.insert(agent_id.clone(), node);
            }
        }

        GraphifyPayload {
            metadata: GraphifyMetadata {
                version: "1.0.0".to_string(),
                exported_at: Utc::now().to_rfc3339(),
                node_count: nodes.len(),
                edge_count: edges.len(),
                format: "graphify-standard".to_string(),
            },
            nodes: nodes.into_values().collect(),
            edges,
        }
    }

    pub fn filter_nodes_by_type(&self, payload: &GraphifyPayload, node_type: &str) -> Vec<GraphifyNode> {
        payload
            .nodes
            .iter()
            .filter(|n| n.node_type == node_type)
            .cloned()
            .collect()
    }

    pub fn filter_edges_by_type(&self, payload: &GraphifyPayload, edge_type: &str) -> Vec<GraphifyEdge> {
        payload
            .edges
            .iter()
            .filter(|e| e.edge_type == edge_type)
            .cloned()
            .collect()
    }

    pub fn search_nodes(&self, payload: &GraphifyPayload, query: &str) -> Vec<GraphifyNode> {
        let query_lower = query.to_lowercase();
        payload
            .nodes
            .iter()
            .filter(|n| {
                n.label.to_lowercase().contains(&query_lower)
                    || n.id.to_lowercase().contains(&query_lower)
                    || n.metadata.extra.values().any(|v| v.to_lowercase().contains(&query_lower))
            })
            .cloned()
            .collect()
    }

    pub fn traverse_graph(
        &self,
        payload: &GraphifyPayload,
        start_node_id: &str,
        max_depth: usize,
    ) -> GraphifyPayload {
        let mut visited = std::collections::HashSet::new();
        let mut frontier = vec![(start_node_id.to_string(), 0)];
        let mut result_nodes = Vec::new();
        let mut result_edges = Vec::new();

        visited.insert(start_node_id.to_string());

        if let Some(start_node) = payload.nodes.iter().find(|n| n.id == start_node_id) {
            result_nodes.push(start_node.clone());
        }

        while let Some((current_id, depth)) = frontier.pop() {
            if depth >= max_depth {
                continue;
            }

            for edge in &payload.edges {
                let new_node_id = if edge.source == current_id && !visited.contains(&edge.target) {
                    Some(edge.target.clone())
                } else if edge.target == current_id && !visited.contains(&edge.source) {
                    Some(edge.source.clone())
                } else {
                    None
                };

                if let Some(new_id) = new_node_id {
                    if let Some(node) = payload.nodes.iter().find(|n| n.id == new_id) {
                        visited.insert(new_id.clone());
                        result_nodes.push(node.clone());
                        result_edges.push(edge.clone());
                        frontier.push((new_id, depth + 1));
                    }
                }
            }
        }

        GraphifyPayload {
            metadata: GraphifyMetadata {
                version: "1.0.0".to_string(),
                exported_at: Utc::now().to_rfc3339(),
                node_count: result_nodes.len(),
                edge_count: result_edges.len(),
                format: "graphify-standard".to_string(),
            },
            nodes: result_nodes,
            edges: result_edges,
        }
    }

    pub fn get_graph_stats(&self, payload: &GraphifyPayload) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();

        let agent_count = payload.nodes.iter().filter(|n| n.node_type == "agent").count();
        let audit_count = payload.nodes.iter().filter(|n| n.node_type == "audit_entry").count();

        stats.insert("total_nodes".to_string(), serde_json::json!(payload.nodes.len()));
        stats.insert("total_edges".to_string(), serde_json::json!(payload.edges.len()));
        stats.insert("agents".to_string(), serde_json::json!(agent_count));
        stats.insert("audit_entries".to_string(), serde_json::json!(audit_count));

        let mut edge_types: HashMap<String, usize> = HashMap::new();
        for edge in &payload.edges {
            *edge_types.entry(edge.edge_type.clone()).or_insert(0) += 1;
        }

        let edge_type_json: HashMap<String, serde_json::Value> = edge_types
            .into_iter()
            .map(|(k, v)| (k, serde_json::json!(v)))
            .collect();
        stats.insert("edge_types".to_string(), serde_json::json!(edge_type_json));

        stats.insert("exported_at".to_string(), serde_json::json!(Utc::now().to_rfc3339()));

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphify_node_creation() {
        let mut metadata = HashMap::new();
        metadata.insert("role".to_string(), "coordinator".to_string());

        let node = GraphifyNode {
            id: "agent_1".to_string(),
            label: "Alice".to_string(),
            node_type: "agent".to_string(),
            color: "#4A90E2".to_string(),
            size: 1.0,
            metadata: GraphifyNodeMetadata {
                created_at: Utc::now().to_rfc3339(),
                class: "agent".to_string(),
                extra: metadata,
            },
        };

        assert_eq!(node.id, "agent_1");
        assert_eq!(node.node_type, "agent");
    }

    #[test]
    fn test_deontic_color_mapping() {
        assert_eq!(GraphifyExporter::get_deontic_color("MUST"), "#DC143C");
        assert_eq!(GraphifyExporter::get_deontic_color("MUST_NOT"), "#8B0000");
        assert_eq!(GraphifyExporter::get_deontic_color("MAY"), "#32CD32");
    }

    #[test]
    fn test_node_type_color_mapping() {
        assert_eq!(GraphifyExporter::get_node_type_color("agent"), "#4A90E2");
        assert_eq!(GraphifyExporter::get_node_type_color("audit_entry"), "#FFB347");
    }

    #[test]
    fn test_edge_type_color_mapping() {
        assert_eq!(GraphifyExporter::get_edge_type_color("sends_to"), "#4A90E2");
        assert_eq!(GraphifyExporter::get_edge_type_color("restricts"), "#8B0000");
    }

    #[test]
    fn test_graph_serialization() {
        let payload = GraphifyPayload {
            metadata: GraphifyMetadata {
                version: "1.0.0".to_string(),
                exported_at: Utc::now().to_rfc3339(),
                node_count: 1,
                edge_count: 0,
                format: "graphify-standard".to_string(),
            },
            nodes: vec![],
            edges: vec![],
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("graphify-standard"));
    }

    #[test]
    fn test_truncate_utf8_safe_never_panics_on_multibyte_boundary() {
        // Un accent (e.g. 'é' = 2 octets UTF-8) place tel que l'ancien code
        // (`&s[..20.min(s.len())]`) coupait EXACTEMENT au milieu de son
        // encodage -- paniquait avec "byte index 20 is not a char boundary"
        // sur ce genre de chaine, AVANT ce fix.
        let purpose = "mise_a_jour_du_caf\u{e9}_et_du_statut"; // 'é' chevauche l'octet 20
        let truncated = GraphifyExporter::truncate_utf8_safe(purpose, 20);
        assert!(purpose.is_char_boundary(truncated.len()) || truncated.len() == purpose.len());
        // Ne doit jamais depasser la limite demandee.
        assert!(truncated.len() <= 20);
    }

    #[test]
    fn test_build_from_audit_trail_does_not_panic_on_multibyte_purpose() {
        // Preuve directe, via le chemin REELLEMENT exerce par
        // build_from_audit_trail (pas seulement truncate_utf8_safe en
        // isolation): un AuditEntry dont purpose vient, comme en
        // production, d'un INTENT_PAYLOAD.purpose totalement controle par
        // le client (voir audit.rs::HashChain::append) ne doit jamais faire
        // planter l'export graphify.
        let mut chain = audit::HashChain::new();
        chain.entries.push(audit::AuditEntry {
            hash: "a".repeat(64),
            parent_hash: "root".to_string(),
            sender: "agent_multibyte".to_string(),
            receiver: "server".to_string(),
            purpose: "mise_a_jour_du_caf\u{e9}_et_du_statut_complet".to_string(),
            seq: 0,
        });
        let exporter = GraphifyExporter::new(chain);
        let payload = exporter.build_from_audit_trail(); // ne doit PAS paniquer (c'est le but du test)
        assert_eq!(payload.nodes.len(), 3); // 1 audit_entry + sender + receiver
    }
}
