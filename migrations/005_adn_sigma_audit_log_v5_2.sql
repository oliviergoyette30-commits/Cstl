-- Migration v5.2: ADN Sigma Audit Log (Double Livre Layer 2 Metadata)
-- ====================================================================
--
-- Purpose: Implement Layer 2 of the Double Livre pattern for CSTL v5.2
-- - Separates server-side confidence evaluation from immutable payloads
-- - Stores EWMA calibration history per agent
-- - Enables post-evaluation verdict collection for continuous learning
-- - Preserves cryptographic chain (parent hash references)
--
-- Created: 2026-09-15
-- Compatibility: PostgreSQL 12+, SQLite 3.28+

-- Main audit log table: immutable append-only log
CREATE TABLE IF NOT EXISTS adn_sigma_audit_log (
  -- Primary key: composite (payload hash, agent name)
  payload_hash_sha256 CHAR(64) NOT NULL,
  agent_name VARCHAR(255) NOT NULL,

  -- Cryptographic chain reference (nullable for first entry)
  payload_hash_parent CHAR(64),

  -- Agent state snapshot at evaluation time
  agent_trust_score_at_time FLOAT8 NOT NULL CHECK (agent_trust_score_at_time >= 0.0 AND agent_trust_score_at_time <= 1.0),

  -- Double Livre Layer 1 vs Layer 2 distinction
  sigma_provided_by_agent FLOAT8 NOT NULL CHECK (sigma_provided_by_agent >= 0.0 AND sigma_provided_by_agent <= 1.0),
  sigma_effective_server FLOAT8 NOT NULL CHECK (sigma_effective_server >= 0.0 AND sigma_effective_server <= 1.0),

  -- EWMA calibration state at evaluation time
  accuracy_ewma_score FLOAT8 NOT NULL CHECK (accuracy_ewma_score >= 0.0 AND accuracy_ewma_score <= 1.0),
  ewma_sample_count INTEGER NOT NULL DEFAULT 1 CHECK (ewma_sample_count >= 1),
  ewma_alpha FLOAT8 NOT NULL DEFAULT 0.2 CHECK (ewma_alpha > 0.0 AND ewma_alpha <= 1.0),

  -- Post-evaluation verdicts (nullable until ground truth arrives)
  is_correct_verdict BOOLEAN,
  verdict_source VARCHAR(64),  -- "human_validation", "cross_model_consensus", "later_reconciliation"
  verdict_confidence FLOAT8 CHECK (verdict_confidence IS NULL OR (verdict_confidence >= 0.0 AND verdict_confidence <= 1.0)),

  -- Evaluated payload snapshot (JSON for flexibility)
  evaluated_payload_json TEXT NOT NULL,

  -- Audit trail metadata
  timestamp_utc TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  evaluation_version VARCHAR(16) NOT NULL DEFAULT 'v5.2',

  -- Governance state snapshot (optional, for correlation)
  governance_breaker_trips_at_time INTEGER,
  governance_circuit_state_at_time VARCHAR(32),  -- "open", "closed", "half_open"
  governance_drift_ratio_at_time FLOAT8 CHECK (governance_drift_ratio_at_time IS NULL OR (governance_drift_ratio_at_time >= 0.0 AND governance_drift_ratio_at_time <= 1.0)),

  -- Constraints
  PRIMARY KEY (payload_hash_sha256, agent_name),
  UNIQUE (payload_hash_sha256, agent_name, timestamp_utc)
);

-- Indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_agent_timestamp
  ON adn_sigma_audit_log (agent_name, timestamp_utc DESC);

CREATE INDEX IF NOT EXISTS idx_payload_parent_chain
  ON adn_sigma_audit_log (payload_hash_parent)
  WHERE payload_hash_parent IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_verdict_pending
  ON adn_sigma_audit_log (payload_hash_sha256)
  WHERE is_correct_verdict IS NULL;

CREATE INDEX IF NOT EXISTS idx_convergence_scan
  ON adn_sigma_audit_log (agent_name, ewma_sample_count DESC);

-- Sigma divergence tracking: standard deviation of sigma_effective per agent
CREATE TABLE IF NOT EXISTS adn_sigma_divergence_stats (
  agent_name VARCHAR(255) PRIMARY KEY,

  -- Rolling statistics (updated after each new entry)
  sigma_mean FLOAT8 NOT NULL DEFAULT 0.5,
  sigma_stddev FLOAT8 NOT NULL DEFAULT 0.0,
  sigma_min FLOAT8 NOT NULL DEFAULT 0.0,
  sigma_max FLOAT8 NOT NULL DEFAULT 1.0,

  -- Sample count for Welford's online algorithm
  sample_count INTEGER NOT NULL DEFAULT 0,

  -- Last update timestamp
  updated_at_utc TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

  -- Alert status
  stddev_alert_threshold_exceeded BOOLEAN DEFAULT FALSE,
  convergence_status VARCHAR(32) DEFAULT 'unknown'  -- "converged", "adapting", "unstable"
);

-- Verdict correlation: cross-check ground truth against server predictions
CREATE TABLE IF NOT EXISTS adn_sigma_verdict_correlation (
  correlation_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

  payload_hash_sha256 CHAR(64) NOT NULL,
  agent_name VARCHAR(255) NOT NULL,

  -- Server's predictions at evaluation time
  server_sigma_predicted FLOAT8 NOT NULL CHECK (server_sigma_predicted >= 0.0 AND server_sigma_predicted <= 1.0),
  agent_trust_predicted FLOAT8 NOT NULL CHECK (agent_trust_predicted >= 0.0 AND agent_trust_predicted <= 1.0),

  -- Ground truth that arrived later
  is_correct_actual BOOLEAN NOT NULL,
  verdict_source VARCHAR(64),
  verdict_arrival_delay_seconds INTEGER,

  -- Accuracy metric
  prediction_error FLOAT8,  -- ABS(server_sigma - actual), computed on insert

  -- Metadata
  recorded_at_utc TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

  FOREIGN KEY (payload_hash_sha256, agent_name) REFERENCES adn_sigma_audit_log (payload_hash_sha256, agent_name)
);

CREATE INDEX IF NOT EXISTS idx_verdict_correlation_agent
  ON adn_sigma_verdict_correlation (agent_name);

CREATE INDEX IF NOT EXISTS idx_prediction_error
  ON adn_sigma_verdict_correlation (prediction_error DESC);

-- Gossip protocol telemetry: Couche 7 agent_telemetry messages
CREATE TABLE IF NOT EXISTS adn_sigma_gossip_telemetry (
  telemetry_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

  -- Source and receiving agents
  source_agent VARCHAR(255) NOT NULL,
  receiving_agent VARCHAR(255),  -- NULL if broadcast

  -- Sigma distribution claim
  agent_name VARCHAR(255) NOT NULL,
  sigma_distribution_mean FLOAT8 NOT NULL,
  sigma_distribution_stddev FLOAT8 NOT NULL,
  sample_size INTEGER NOT NULL CHECK (sample_size > 0),

  -- Message signature verification
  message_signature_valid BOOLEAN,
  signature_verification_error TEXT,

  -- Byzantine fault tolerance
  consensus_votes_for INTEGER DEFAULT 0,
  consensus_votes_against INTEGER DEFAULT 0,
  quorum_threshold_met BOOLEAN DEFAULT FALSE,

  -- Telemetry
  received_at_utc TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  latency_milliseconds INTEGER
);

CREATE INDEX IF NOT EXISTS idx_gossip_source_agent
  ON adn_sigma_gossip_telemetry (source_agent, received_at_utc DESC);

CREATE INDEX IF NOT EXISTS idx_gossip_consensus
  ON adn_sigma_gossip_telemetry (quorum_threshold_met)
  WHERE quorum_threshold_met = TRUE;

-- Table audit triggers (for compliance and forensics)
CREATE TABLE IF NOT EXISTS adn_sigma_audit_log_changelog (
  changelog_id BIGSERIAL PRIMARY KEY,

  operation VARCHAR(10) NOT NULL CHECK (operation IN ('INSERT', 'UPDATE', 'DELETE')),
  payload_hash_sha256 CHAR(64),
  agent_name VARCHAR(255),

  -- Before/after state (JSON)
  old_values TEXT,
  new_values TEXT,

  -- Who made the change
  modified_by VARCHAR(255),
  modified_at_utc TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Archive table for retention: yearly cold storage
CREATE TABLE IF NOT EXISTS adn_sigma_audit_log_archive (
  -- Same schema as main table, plus archival metadata
  payload_hash_sha256 CHAR(64) NOT NULL,
  agent_name VARCHAR(255) NOT NULL,

  payload_hash_parent CHAR(64),
  agent_trust_score_at_time FLOAT8 NOT NULL,
  sigma_provided_by_agent FLOAT8 NOT NULL,
  sigma_effective_server FLOAT8 NOT NULL,
  accuracy_ewma_score FLOAT8 NOT NULL,
  ewma_sample_count INTEGER NOT NULL,
  ewma_alpha FLOAT8 NOT NULL,

  is_correct_verdict BOOLEAN,
  verdict_source VARCHAR(64),
  verdict_confidence FLOAT8,

  evaluated_payload_json TEXT NOT NULL,
  timestamp_utc TIMESTAMPTZ NOT NULL,
  evaluation_version VARCHAR(16),

  governance_breaker_trips_at_time INTEGER,
  governance_circuit_state_at_time VARCHAR(32),
  governance_drift_ratio_at_time FLOAT8,

  -- Archival metadata
  archived_at_utc TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  archive_year INTEGER NOT NULL,

  PRIMARY KEY (archive_year, payload_hash_sha256, agent_name)
);

CREATE INDEX IF NOT EXISTS idx_archive_by_year
  ON adn_sigma_audit_log_archive (archive_year DESC);

-- Materialized view for easy monitoring
CREATE VIEW adn_sigma_audit_summary AS
SELECT
  agent_name,
  COUNT(*) as total_evaluations,
  COUNT(CASE WHEN is_correct_verdict IS NOT NULL THEN 1 END) as verdicts_received,
  COUNT(CASE WHEN is_correct_verdict = true THEN 1 END) as correct_count,
  ROUND(AVG(CAST(CASE WHEN is_correct_verdict = true THEN 1.0 ELSE 0.0 END AS FLOAT8)), 4) as accuracy_rate,
  ROUND(AVG(sigma_effective_server)::NUMERIC, 4) as avg_sigma_effective,
  ROUND(STDDEV(sigma_effective_server)::NUMERIC, 4) as stddev_sigma_effective,
  ROUND(AVG(accuracy_ewma_score)::NUMERIC, 4) as avg_ewma_score,
  MAX(timestamp_utc) as last_evaluation_utc
FROM adn_sigma_audit_log
GROUP BY agent_name
ORDER BY total_evaluations DESC;

-- Production safety: prevent direct deletes (must use archive strategy)
CREATE RULE adn_sigma_audit_log_no_delete AS ON DELETE TO adn_sigma_audit_log DO INSTEAD
  RAISE EXCEPTION 'Direct deletion from adn_sigma_audit_log is prohibited. Use archive strategy.';

-- End of migration v5.2
