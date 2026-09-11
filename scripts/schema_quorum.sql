-- ============================================================================
-- CSTL Layer 2: Quorum Governance Schema
-- Version: 1.0.0
-- Purpose: Byzantine Fault Tolerant voting consensus persistence layer
-- ============================================================================

-- ============================================================================
-- TABLE: quorum_members
-- ============================================================================
CREATE TABLE IF NOT EXISTS quorum_members (
    member_id TEXT PRIMARY KEY NOT NULL,
    public_key TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'validator'
        CHECK (role IN ('initiator', 'validator', 'observer', 'admin')),
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'degraded', 'inactive', 'deregistered')),
    health_score REAL NOT NULL DEFAULT 1.0,
    registered_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_verified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_updated TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    vote_count INTEGER NOT NULL DEFAULT 0,
    rejected_vote_count INTEGER NOT NULL DEFAULT 0,
    misbehavior_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_quorum_members_public_key ON quorum_members(public_key);
CREATE INDEX idx_quorum_members_status ON quorum_members(status, health_score DESC);
CREATE INDEX idx_quorum_members_role ON quorum_members(role);

-- ============================================================================
-- TABLE: quorum_proposals
-- ============================================================================
CREATE TABLE IF NOT EXISTS quorum_proposals (
    proposal_id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    initiator_id TEXT NOT NULL,
        FOREIGN KEY (initiator_id) REFERENCES quorum_members(member_id)
            ON DELETE RESTRICT ON UPDATE CASCADE,
    required_threshold INTEGER NOT NULL,
    total_eligible_voters INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'voting', 'finalized', 'cancelled', 'deadlock')),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    voting_started_at TIMESTAMP,
    finalized_at TIMESTAMP,
    final_decision TEXT,
    decision_confidence REAL,
    metadata_json TEXT
);

CREATE INDEX idx_quorum_proposals_status ON quorum_proposals(status, created_at DESC);
CREATE INDEX idx_quorum_proposals_initiator ON quorum_proposals(initiator_id);
CREATE INDEX idx_quorum_proposals_finalized ON quorum_proposals(finalized_at DESC)
    WHERE finalized_at IS NOT NULL;

-- ============================================================================
-- TABLE: quorum_votes (IMMUTABLE LEDGER)
-- ============================================================================
CREATE TABLE IF NOT EXISTS quorum_votes (
    vote_id TEXT PRIMARY KEY NOT NULL,
    proposal_id TEXT NOT NULL,
        FOREIGN KEY (proposal_id) REFERENCES quorum_proposals(proposal_id)
            ON DELETE RESTRICT ON UPDATE CASCADE,
    voter_id TEXT NOT NULL,
        FOREIGN KEY (voter_id) REFERENCES quorum_members(member_id)
            ON DELETE RESTRICT ON UPDATE CASCADE,
    decision TEXT NOT NULL
        CHECK (decision IN ('yea', 'nay', 'abstain')),
    signature TEXT NOT NULL,
    public_key_at_vote TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    sequence_number INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    received_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    verified_at TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'accepted'
        CHECK (status IN ('accepted', 'rejected', 'pending')),
    rejection_reason TEXT,
    created_by_agent TEXT
);

CREATE INDEX idx_quorum_votes_proposal ON quorum_votes(proposal_id, timestamp ASC);
CREATE INDEX idx_quorum_votes_voter ON quorum_votes(voter_id, proposal_id, sequence_number DESC);
CREATE UNIQUE INDEX idx_quorum_votes_replay_detection
    ON quorum_votes(proposal_id, voter_id, sequence_number)
    WHERE status = 'accepted';
CREATE INDEX idx_quorum_votes_rejected ON quorum_votes(status, rejection_reason)
    WHERE status = 'rejected';

-- ============================================================================
-- TABLE: quorum_state
-- ============================================================================
CREATE TABLE IF NOT EXISTS quorum_state (
    proposal_id TEXT NOT NULL,
    round INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (proposal_id, round),
        FOREIGN KEY (proposal_id) REFERENCES quorum_proposals(proposal_id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    yea_count INTEGER NOT NULL DEFAULT 0,
    nay_count INTEGER NOT NULL DEFAULT 0,
    abstain_count INTEGER NOT NULL DEFAULT 0,
    threshold INTEGER NOT NULL,
    participating_members INTEGER NOT NULL DEFAULT 0,
    circuit_breaker_active BOOLEAN NOT NULL DEFAULT 0,
    circuit_breaker_reason TEXT,
    consensus_reached BOOLEAN NOT NULL DEFAULT 0,
    final_decision TEXT
        CHECK (final_decision IS NULL OR final_decision IN ('yea', 'nay', 'abstain')),
    average_member_health REAL NOT NULL DEFAULT 1.0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_quorum_state_proposal_round ON quorum_state(proposal_id, round DESC);
CREATE INDEX idx_quorum_state_circuit_breaker ON quorum_state(circuit_breaker_active)
    WHERE circuit_breaker_active = 1;

-- ============================================================================
-- VIEW: quorum_vote_summary
-- ============================================================================
CREATE VIEW IF NOT EXISTS quorum_vote_summary AS
SELECT
    v.proposal_id,
    COUNT(*) AS total_votes,
    SUM(CASE WHEN v.decision = 'yea' THEN 1 ELSE 0 END) AS yea_count,
    SUM(CASE WHEN v.decision = 'nay' THEN 1 ELSE 0 END) AS nay_count,
    SUM(CASE WHEN v.decision = 'abstain' THEN 1 ELSE 0 END) AS abstain_count,
    COUNT(DISTINCT v.voter_id) AS unique_voters,
    MAX(v.timestamp) AS last_vote_at,
    p.required_threshold,
    p.status AS proposal_status,
    p.final_decision
FROM quorum_votes v
LEFT JOIN quorum_proposals p ON v.proposal_id = p.proposal_id
WHERE v.status = 'accepted'
GROUP BY v.proposal_id
ORDER BY v.proposal_id DESC;

-- ============================================================================
-- VIEW: quorum_member_metrics
-- ============================================================================
CREATE VIEW IF NOT EXISTS quorum_member_metrics AS
SELECT
    m.member_id,
    m.public_key,
    m.role,
    m.status,
    m.health_score,
    m.vote_count,
    m.rejected_vote_count,
    m.misbehavior_count,
    CASE
        WHEN m.vote_count = 0 THEN 0.0
        ELSE CAST(m.rejected_vote_count AS REAL) / (m.vote_count + m.rejected_vote_count)
    END AS rejection_rate,
    m.last_verified,
    m.last_updated
FROM quorum_members m
WHERE m.status != 'deregistered'
ORDER BY m.health_score DESC;

-- ============================================================================
-- TRIGGERS
-- ============================================================================

CREATE TRIGGER IF NOT EXISTS trigger_quorum_members_updated_at
AFTER UPDATE ON quorum_members
FOR EACH ROW
BEGIN
    UPDATE quorum_members
    SET last_updated = CURRENT_TIMESTAMP
    WHERE member_id = NEW.member_id;
END;

CREATE TRIGGER IF NOT EXISTS trigger_quorum_votes_no_delete
BEFORE DELETE ON quorum_votes
BEGIN
    SELECT RAISE(ABORT, 'quorum_votes is immutable ledger; DELETE not allowed');
END;

CREATE TRIGGER IF NOT EXISTS trigger_quorum_votes_sequence_validation
BEFORE INSERT ON quorum_votes
FOR EACH ROW
WHEN (NEW.status = 'accepted')
BEGIN
    SELECT CASE
        WHEN EXISTS(
            SELECT 1 FROM quorum_votes
            WHERE proposal_id = NEW.proposal_id
              AND voter_id = NEW.voter_id
              AND sequence_number = NEW.sequence_number
              AND status = 'accepted'
        )
        THEN RAISE(ABORT, 'Duplicate vote: sequence number already used')
    END;
END;

-- ============================================================================
-- PRAGMAS (run at connection init)
-- ============================================================================
-- PRAGMA journal_mode = WAL;
-- PRAGMA foreign_keys = ON;
-- PRAGMA synchronous = FULL;
-- PRAGMA cache_size = -64000;
