/// Wire RestrictedCouncil conflicts to human arbiter via Telegram

pub struct ConflictQuestion {
    pub agent_a: String,
    pub agent_a_position: String,
    pub agent_b: String,
    pub agent_b_position: String,
    pub context: String,
}

pub struct ArbitrationDecision {
    pub decided_by: String,
    pub decision: String,
    pub reasoning: String,
    pub timestamp: i64,
    pub immutable_hash: String,
}

pub fn format_conflict_cstl(q: &ConflictQuestion) -> String {
    format!(
        "#!CSTL v5.0.0 MODE=A\nINTENT_PAYLOAD [\npurpose=request_arbitration,\nsender=RestrictedCouncil,\nreceiver=human_arbiter\n]\n\nQUESTION [\nagent_a={},\nposition={},\nagent_b={},\nposition={},\ncontext={}\n]\n\n---END---",
        q.agent_a, q.agent_a_position, q.agent_b, q.agent_b_position, q.context
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_conflict_cstl() {
        let q = ConflictQuestion {
            agent_a: "alice".to_string(),
            agent_a_position: "user_is_authorized".to_string(),
            agent_b: "bob".to_string(),
            agent_b_position: "user_needs_approval".to_string(),
            context: "access_request".to_string(),
        };
        
        let formatted = format_conflict_cstl(&q);
        assert!(formatted.contains("QUESTION"));
        assert!(formatted.contains("alice"));
    }
}
