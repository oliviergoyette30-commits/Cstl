/// Message router - sends CSTL payloads to correct agent
use crate::agent_discovery::AgentRegistry;

pub fn route_payload(
    payload: &str,
    registry: &AgentRegistry,
) -> Option<String> {
    // TODO: Parse CSTL payload
    // TODO: Extract destination agent
    // TODO: Find in registry
    // TODO: Route accordingly
    
    eprintln!("[Router] Routing payload: {}", payload);
    Some("routed".to_string())
}
