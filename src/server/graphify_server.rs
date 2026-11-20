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
            "entity" => "#20B2AA".to_string(),
            "key" => "#DAA520".to_string(),
            "relation" => "#7B68EE".to_string(),
            "chains_to" => "#708090".to_string(),
            "holds_key" => "#DAA520".to_string(),
            "signed_by" => "#B8860B".to_string(),
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
            "asserts" => "#BBBBBB".to_string(),
            "relation" => "#7B68EE".to_string(),
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

    /// Graphe complet + PROVENANCE CRYPTOGRAPHIQUE (2026-11-20) : en plus de
    /// `build_with_content`, rend visibles les garanties Ed25519 et SHA-256
    /// qui etaient absentes du graphe :
    /// - par message (`audit_entry`) : `signature` = valid|invalid|absent
    ///   (REVERIFIEE a l'export, voir `AdnStore::crypto_provenance`),
    ///   `key_fp` (16 premiers hex de la cle), `chain_ok` (son parent_hash est
    ///   bien le hash du message precedent, ou "root" pour le premier) ;
    /// - arc `chains_to` de chaque message vers son predecesseur : la chaine
    ///   de hachage elle-meme ;
    /// - noeuds `key:<fp>`, arc `holds_key` agent -> cle enregistree, arc
    ///   `signed_by` message -> cle qui l'a signe (signature valide). Une
    ///   usurpation apparait comme `key_matches_sender=false` : message signe
    ///   par une cle qui n'est pas celle enregistree pour son expediteur.
    pub fn build_with_crypto(
        &self,
        rows: &[crate::adn_store::GraphRelationRow],
        provenance: &HashMap<String, (String, Option<String>)>,
        agent_keys: &HashMap<String, String>,
    ) -> GraphifyPayload {
        let mut payload = self.build_with_content(rows);
        let fp = |k: &str| k.chars().take(16).collect::<String>();
        let mut key_ids = std::collections::HashSet::new();
        let mut add_key = |payload: &mut GraphifyPayload, key: &str| -> String {
            let id = format!("key:{}", fp(key));
            if key_ids.insert(id.clone()) {
                let mut extra = HashMap::new();
                extra.insert("algorithm".to_string(), "Ed25519".to_string());
                extra.insert("public_key".to_string(), key.to_string());
                payload.nodes.push(GraphifyNode {
                    id: id.clone(),
                    label: format!("Ed25519 {}", fp(key)),
                    node_type: "key".to_string(),
                    color: Self::get_node_type_color("key"),
                    size: 1.0,
                    metadata: GraphifyNodeMetadata {
                        created_at: "unknown".to_string(),
                        class: "key".to_string(),
                        extra,
                    },
                });
            }
            id
        };

        for (agent, key) in agent_keys {
            let key_id = add_key(&mut payload, key);
            if let Some(node) = payload.nodes.iter_mut().find(|n| &n.id == agent && n.node_type == "agent") {
                node.metadata.extra.insert("key_fp".to_string(), fp(key));
            }
            payload.edges.push(GraphifyEdge {
                source: agent.clone(),
                target: key_id,
                edge_type: "holds_key".to_string(),
                weight: 1.0,
                color: Self::get_edge_type_color("holds_key"),
                metadata: HashMap::new(),
            });
        }

        let entries = &self.chain.entries;
        for (idx, entry) in entries.iter().enumerate() {
            let audit_id = format!("audit_{}", idx);
            let expected_parent = if idx == 0 { "root".to_string() } else { entries[idx - 1].hash.clone() };
            let chain_ok = entry.parent_hash == expected_parent;
            let (status, key) = provenance.get(&entry.hash).cloned().unwrap_or_else(|| ("unknown".to_string(), None));
            let mut signed_by = None;
            if let Some(node) = payload.nodes.iter_mut().find(|n| n.id == audit_id) {
                node.metadata.extra.insert("signature".to_string(), status.clone());
                node.metadata.extra.insert("chain_ok".to_string(), chain_ok.to_string());
                if let Some(k) = &key {
                    node.metadata.extra.insert("key_fp".to_string(), fp(k));
                    if let Some(registered) = agent_keys.get(&entry.sender) {
                        node.metadata.extra.insert("key_matches_sender".to_string(), (registered == k).to_string());
                    }
                    if status == "valid" { signed_by = Some(k.clone()); }
                }
            }
            if let Some(k) = signed_by {
                let key_id = add_key(&mut payload, &k);
                payload.edges.push(GraphifyEdge {
                    source: audit_id.clone(),
                    target: key_id,
                    edge_type: "signed_by".to_string(),
                    weight: 1.0,
                    color: Self::get_edge_type_color("signed_by"),
                    metadata: HashMap::new(),
                });
            }
            if idx > 0 {
                let mut meta = HashMap::new();
                meta.insert("parent_hash".to_string(), entry.parent_hash.clone());
                meta.insert("chain_ok".to_string(), chain_ok.to_string());
                payload.edges.push(GraphifyEdge {
                    source: audit_id,
                    target: format!("audit_{}", idx - 1),
                    edge_type: "chains_to".to_string(),
                    weight: 1.0,
                    color: if chain_ok { Self::get_edge_type_color("chains_to") } else { Self::get_edge_type_color("violates") },
                    metadata: meta,
                });
            }
        }
        payload.metadata.node_count = payload.nodes.len();
        payload.metadata.edge_count = payload.edges.len();
        payload
    }

    /// Graphe COMPLET (2026-11-17) : la chaine d'audit (qui parle a qui) PLUS
    /// le contenu des messages -- entites (`entity:<nom>`), relations entre
    /// elles (arc `relation`, predicat/modalite en metadonnees), arc `asserts`
    /// du message vers le sujet de chaque relation qu'il affirme, et les
    /// contradictions marquees (`conflict=fact|deontic`, couleur `violates`).
    /// Les contradictions viennent des MEMES detecteurs que le pipeline live
    /// (`execution_lab::check_consistency_with_history` /
    /// `check_deontic_consistency_with_history`, appeles sur l'ensemble des
    /// relations) -- aucune logique dupliquee ici.
    pub fn build_with_content(&self, rows: &[crate::adn_store::GraphRelationRow]) -> GraphifyPayload {
        let mut payload = self.build_from_audit_trail();

        let audit_id_by_hash: HashMap<&str, String> = self.chain.entries.iter().enumerate()
            .map(|(i, e)| (e.hash.as_str(), format!("audit_{}", i)))
            .collect();
        let mut created_by_hash: HashMap<&str, i64> = HashMap::new();
        for (hash, _, _, _, _, created) in rows {
            if let Some(t) = created { created_by_hash.insert(hash.as_str(), *t); }
        }
        for node in payload.nodes.iter_mut() {
            if let Some(hash) = node.metadata.extra.get("full_hash") {
                if let Some(t) = created_by_hash.get(hash.as_str()) {
                    if let Some(dt) = chrono::DateTime::from_timestamp(*t, 0) {
                        node.metadata.created_at = dt.to_rfc3339();
                    }
                }
            }
        }

        // Memes detecteurs que le pipeline live.
        let rels: Vec<HashMap<String, String>> = rows.iter().map(|(_, s, p, o, m, _)| {
            let mut r = HashMap::new();
            r.insert("subject".to_string(), s.clone());
            r.insert("type".to_string(), p.clone());
            r.insert("object".to_string(), o.clone());
            if let Some(m) = m { r.insert("modality".to_string(), m.clone()); }
            r
        }).collect();
        let facts = crate::execution_lab::check_consistency_with_history(&rels, &[]);
        let fact_conflicts: std::collections::HashSet<(String, String)> = facts.contradictions.iter()
            .map(|c| (c.subject.clone(), c.predicate.clone())).collect();
        let deontic = crate::execution_lab::check_deontic_consistency_with_history(&rels, &[]);
        let deontic_conflicts: std::collections::HashSet<(String, String)> = deontic.violations.iter()
            .map(|v| (v.subject.clone(), v.object.clone())).collect();

        let mut entity_ids = std::collections::HashSet::new();
        for (hash, subject, predicate, object, modality, _) in rows {
            for name in [subject, object] {
                let id = format!("entity:{}", name);
                if entity_ids.insert(id.clone()) {
                    payload.nodes.push(GraphifyNode {
                        id: id.clone(),
                        label: name.clone(),
                        node_type: "entity".to_string(),
                        color: Self::get_node_type_color("entity"),
                        size: 1.2,
                        metadata: GraphifyNodeMetadata {
                            created_at: "unknown".to_string(),
                            class: "entity".to_string(),
                            extra: HashMap::new(),
                        },
                    });
                }
            }
            let conflict = if fact_conflicts.contains(&(subject.clone(), predicate.clone())) {
                Some("fact")
            } else if modality.is_some() && deontic_conflicts.contains(&(subject.clone(), object.clone())) {
                Some("deontic")
            } else {
                None
            };
            let mut meta = HashMap::new();
            meta.insert("predicate".to_string(), predicate.clone());
            meta.insert("hash".to_string(), hash.clone());
            if let Some(m) = modality { meta.insert("modality".to_string(), m.clone()); }
            if let Some(c) = conflict { meta.insert("conflict".to_string(), c.to_string()); }
            let color = match (conflict, modality) {
                (Some(_), _) => Self::get_edge_type_color("violates"),
                (None, Some(m)) => Self::get_deontic_color(m),
                (None, None) => Self::get_edge_type_color("relation"),
            };
            payload.edges.push(GraphifyEdge {
                source: format!("entity:{}", subject),
                target: format!("entity:{}", object),
                edge_type: "relation".to_string(),
                weight: 1.0,
                color,
                metadata: meta,
            });
            if let Some(audit_id) = audit_id_by_hash.get(hash.as_str()) {
                payload.edges.push(GraphifyEdge {
                    source: audit_id.clone(),
                    target: format!("entity:{}", subject),
                    edge_type: "asserts".to_string(),
                    weight: 0.5,
                    color: Self::get_edge_type_color("asserts"),
                    metadata: HashMap::new(),
                });
            }
        }
        payload.metadata.node_count = payload.nodes.len();
        payload.metadata.edge_count = payload.edges.len();
        payload
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
            metadata.insert("full_hash".to_string(), entry.hash.clone());

            let node = GraphifyNode {
                id: entry_id.clone(),
                label: format!("{}...", Self::truncate_utf8_safe(&entry.purpose, 20)),
                node_type: "audit_entry".to_string(),
                color: Self::get_node_type_color("audit_entry"),
                size: 1.0,
                metadata: GraphifyNodeMetadata {
                    // Date reelle du message renseignee par `build_with_content`
                    // (adn_store.created_at) ; avant le 2026-11-17 : l'heure de
                    // l'EXPORT, presentee comme celle du message.
                    created_at: "unknown".to_string(),
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
    #[test]
    fn test_build_with_content_shows_entities_relations_conflicts_and_real_dates() {
        let mut chain = audit::HashChain::new();
        let p = crate::server::parser::parse_payload(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=t, produced_by=t]\nINTENT_PAYLOAD [purpose=inform, sender=alice, receiver=bob]\n---END---\n",
        ).unwrap();
        let entry = chain.append(&p);
        let exporter = GraphifyExporter::new(chain);
        let h = entry.hash.clone();
        let rows = vec![
            (h.clone(), "agent_z".into(), "born_in".into(), "Paris".into(), None, Some(1_794_976_491)),
            (h.clone(), "agent_z".into(), "born_in".into(), "Berlin".into(), None, Some(1_794_976_491)),
            (h.clone(), "bot".into(), "PERFORM".into(), "wipe_db".into(), Some("MUST_NOT".into()), Some(1_794_976_491)),
            (h.clone(), "bot".into(), "PERFORM".into(), "wipe_db".into(), Some("MUST".into()), Some(1_794_976_491)),
            (h.clone(), "order_7".into(), "STATE".into(), "shipped".into(), None, Some(1_794_976_491)),
        ];
        let g = exporter.build_with_content(&rows);

        let entities = g.nodes.iter().filter(|n| n.node_type == "entity").count();
        assert_eq!(entities, 7, "agent_z, Paris, Berlin, bot, wipe_db, order_7, shipped");
        let rel: Vec<_> = g.edges.iter().filter(|e| e.edge_type == "relation").collect();
        assert_eq!(rel.len(), 5);
        let conflict = |c: &str| rel.iter().filter(|e| e.metadata.get("conflict").map(String::as_str) == Some(c)).count();
        assert_eq!(conflict("fact"), 2, "Paris vs Berlin");
        assert_eq!(conflict("deontic"), 2, "MUST vs MUST_NOT");
        assert!(rel.iter().any(|e| !e.metadata.contains_key("conflict")), "order_7 STATE shipped sans conflit");
        assert_eq!(g.edges.iter().filter(|e| e.edge_type == "asserts").count(), 5);

        let audit = g.nodes.iter().find(|n| n.node_type == "audit_entry").unwrap();
        assert_eq!(audit.metadata.created_at, "2026-11-18T04:34:51+00:00", "date du message, pas de l'export");
        assert_eq!(g.metadata.node_count, g.nodes.len());
        assert_eq!(g.metadata.edge_count, g.edges.len());
    }
    #[test]
    fn test_build_with_crypto_exposes_signatures_keys_chain_and_tampering() {
        let mk = |sender: &str, parent: &str, hash: &str, seq: u64| audit::AuditEntry {
            hash: hash.to_string(), parent_hash: parent.to_string(), sender: sender.to_string(),
            receiver: "bob".to_string(), purpose: "inform".to_string(), seq,
        };
        let mut chain = audit::HashChain::new();
        chain.entries.push(mk("signer", "root", "sha256:a", 0));
        chain.entries.push(mk("alice", "sha256:a", "sha256:b", 1));
        chain.entries.push(mk("signer", "sha256:TAMPERED", "sha256:c", 2)); // chaine rompue
        chain.entries.push(mk("signer", "sha256:c", "sha256:d", 3));
        let key = "ab".repeat(32);
        let other = "cd".repeat(32);
        let mut prov = HashMap::new();
        prov.insert("sha256:a".to_string(), ("valid".to_string(), Some(key.clone())));
        prov.insert("sha256:b".to_string(), ("absent".to_string(), None));
        prov.insert("sha256:c".to_string(), ("invalid".to_string(), Some(key.clone())));
        prov.insert("sha256:d".to_string(), ("valid".to_string(), Some(other.clone()))); // cle d'un autre
        let mut agents = HashMap::new();
        agents.insert("signer".to_string(), key.clone());

        let g = GraphifyExporter::new(chain).build_with_crypto(&[], &prov, &agents);
        let node = |id: &str| g.nodes.iter().find(|n| n.id == id).unwrap().metadata.extra.clone();
        assert_eq!(node("audit_0")["signature"], "valid");
        assert_eq!(node("audit_0")["key_matches_sender"], "true");
        assert_eq!(node("audit_1")["signature"], "absent");
        assert_eq!(node("audit_2")["signature"], "invalid");
        assert_eq!(node("audit_2")["chain_ok"], "false", "parent_hash altere");
        assert_eq!(node("audit_3")["key_matches_sender"], "false", "usurpation visible");

        let count = |t: &str| g.edges.iter().filter(|e| e.edge_type == t).count();
        assert_eq!(count("chains_to"), 3);
        assert_eq!(count("holds_key"), 1);
        assert_eq!(count("signed_by"), 2, "seulement les signatures VALIDES");
        assert_eq!(g.nodes.iter().filter(|n| n.node_type == "key").count(), 2);
        let broken = g.edges.iter().find(|e| e.edge_type == "chains_to" && e.source == "audit_2").unwrap();
        assert_eq!(broken.metadata["chain_ok"], "false");
        assert_eq!(g.metadata.edge_count, g.edges.len());
    }
}
