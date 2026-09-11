-- schema_arbitrage.sql — Couche 3b: Arbitrage Réel
--
-- Schéma DDL pour le système d'arbitrage décentralisé
-- S'intègre dans la même base SQLite que adn_store et audit_trail
-- Tous les timestamps en UTC (ISO 8601)

-- Table: arbiters
-- Registre des arbitres qualifiés ayant le droit de voter sur les dossiers
CREATE TABLE IF NOT EXISTS arbiters (
    arbiter_id TEXT PRIMARY KEY,
    public_key TEXT NOT NULL UNIQUE,
    authority_level INTEGER NOT NULL, -- 1=Trainee, 2=Senior, 3=Expert
    stake_amount INTEGER NOT NULL DEFAULT 0, -- en satoshis/tokens
    registered_at TEXT NOT NULL, -- ISO 8601 UTC
    is_active BOOLEAN NOT NULL DEFAULT 1,
    
    CHECK (authority_level IN (1, 2, 3)),
    CHECK (stake_amount >= 0)
);

CREATE INDEX IF NOT EXISTS idx_arbiters_active ON arbiters(is_active);
CREATE INDEX IF NOT EXISTS idx_arbiters_authority ON arbiters(authority_level);
CREATE INDEX IF NOT EXISTS idx_arbiters_public_key ON arbiters(public_key);

-- Table: arbitrage_cases
-- Dossiers d'arbitrage ouverts suite à détection de contradictions
CREATE TABLE IF NOT EXISTS arbitrage_cases (
    case_id TEXT PRIMARY KEY,
    escalation_source TEXT NOT NULL,
    contradiction_type TEXT NOT NULL,
    status TEXT NOT NULL,
    description TEXT NOT NULL,
    opened_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    
    CHECK (status IN ('Open', 'InProgress', 'RulingSubmitted', 'Finalized', 'EscalatedToCouncil')),
    CHECK (
        contradiction_type IN (
            'MutuallyExclusive',
            'LogicalBreak',
            'RefusalToComply',
            'InvalidProof'
        )
    )
);

CREATE INDEX IF NOT EXISTS idx_cases_status ON arbitrage_cases(status);
CREATE INDEX IF NOT EXISTS idx_cases_opened_at ON arbitrage_cases(opened_at);
CREATE INDEX IF NOT EXISTS idx_cases_escalation_source ON arbitrage_cases(escalation_source);

-- Table: case_assignments
-- Assignation d'arbitres à des dossiers (relation N:M)
CREATE TABLE IF NOT EXISTS case_assignments (
    case_id TEXT NOT NULL,
    arbiter_id TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    
    PRIMARY KEY (case_id, arbiter_id),
    FOREIGN KEY (case_id) REFERENCES arbitrage_cases(case_id) ON DELETE CASCADE,
    FOREIGN KEY (arbiter_id) REFERENCES arbiters(arbiter_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_assignments_arbiter ON case_assignments(arbiter_id);
CREATE INDEX IF NOT EXISTS idx_assignments_case ON case_assignments(case_id);

-- Table: arbitration_rulings
-- Décisions signées d'arbitres sur les cas
CREATE TABLE IF NOT EXISTS arbitration_rulings (
    ruling_id TEXT PRIMARY KEY,
    case_id TEXT NOT NULL,
    arbiter_id TEXT NOT NULL,
    decision TEXT NOT NULL,
    justification TEXT NOT NULL,
    signature TEXT NOT NULL,
    ruled_at TEXT NOT NULL,
    
    UNIQUE (case_id, arbiter_id),
    FOREIGN KEY (case_id) REFERENCES arbitrage_cases(case_id) ON DELETE CASCADE,
    FOREIGN KEY (arbiter_id) REFERENCES arbiters(arbiter_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_rulings_case ON arbitration_rulings(case_id);
CREATE INDEX IF NOT EXISTS idx_rulings_arbiter ON arbitration_rulings(arbiter_id);
CREATE INDEX IF NOT EXISTS idx_rulings_ruled_at ON arbitration_rulings(ruled_at);

-- Table: peer_review_signatures
-- Signatures de validation croisée d'autres arbitres (peer review)
-- Utilisé pour atteindre le quorum de finality
CREATE TABLE IF NOT EXISTS peer_review_signatures (
    review_id INTEGER PRIMARY KEY AUTOINCREMENT,
    ruling_id TEXT NOT NULL,
    reviewing_arbiter_id TEXT NOT NULL,
    review_signature TEXT NOT NULL,
    reviewed_at TEXT NOT NULL,
    
    UNIQUE (ruling_id, reviewing_arbiter_id),
    FOREIGN KEY (ruling_id) REFERENCES arbitration_rulings(ruling_id) ON DELETE CASCADE,
    FOREIGN KEY (reviewing_arbiter_id) REFERENCES arbiters(arbiter_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_reviews_ruling ON peer_review_signatures(ruling_id);
CREATE INDEX IF NOT EXISTS idx_reviews_arbiter ON peer_review_signatures(reviewing_arbiter_id);
CREATE INDEX IF NOT EXISTS idx_reviews_reviewed_at ON peer_review_signatures(reviewed_at);

-- Table: arbitrage_audit
-- Trace d'audit des actions effectuées dans le système d'arbitrage
CREATE TABLE IF NOT EXISTS arbitrage_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id TEXT,
    ruling_id TEXT,
    arbiter_id TEXT NOT NULL,
    action TEXT NOT NULL,
    details TEXT,
    occurred_at TEXT NOT NULL,
    
    FOREIGN KEY (case_id) REFERENCES arbitrage_cases(case_id) ON DELETE SET NULL,
    FOREIGN KEY (ruling_id) REFERENCES arbitration_rulings(ruling_id) ON DELETE SET NULL,
    FOREIGN KEY (arbiter_id) REFERENCES arbiters(arbiter_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_audit_case ON arbitrage_audit(case_id);
CREATE INDEX IF NOT EXISTS idx_audit_action ON arbitrage_audit(action);
CREATE INDEX IF NOT EXISTS idx_audit_occurred_at ON arbitrage_audit(occurred_at);
CREATE INDEX IF NOT EXISTS idx_audit_arbiter ON arbitrage_audit(arbiter_id);

-- View: v_case_finality_status
CREATE VIEW IF NOT EXISTS v_case_finality_status AS
SELECT
    c.case_id,
    c.status,
    COUNT(DISTINCT pr.reviewing_arbiter_id) AS peer_review_count,
    2 AS quorum_size_for_finality,
    CASE
        WHEN COUNT(DISTINCT pr.reviewing_arbiter_id) >= 2 THEN 'READY_FOR_FINALIZATION'
        ELSE 'PENDING_PEER_REVIEWS'
    END AS finality_status
FROM
    arbitrage_cases c
    LEFT JOIN arbitration_rulings r ON c.case_id = r.case_id
    LEFT JOIN peer_review_signatures pr ON r.ruling_id = pr.ruling_id
GROUP BY
    c.case_id;

-- View: v_arbiter_stats
CREATE VIEW IF NOT EXISTS v_arbiter_stats AS
SELECT
    a.arbiter_id,
    a.authority_level,
    a.is_active,
    COUNT(DISTINCT r.ruling_id) AS rulings_submitted,
    COUNT(DISTINCT pr.review_id) AS peer_reviews_given,
    CAST(COUNT(DISTINCT pr.review_id) AS REAL) / NULLIF(COUNT(DISTINCT r.ruling_id), 0) AS peer_review_ratio
FROM
    arbiters a
    LEFT JOIN arbitration_rulings r ON a.arbiter_id = r.arbiter_id
    LEFT JOIN peer_review_signatures pr ON a.arbiter_id = pr.reviewing_arbiter_id
GROUP BY
    a.arbiter_id;

-- View: v_open_cases
CREATE VIEW IF NOT EXISTS v_open_cases AS
SELECT
    c.case_id,
    c.contradiction_type,
    c.status,
    c.escalation_source,
    c.opened_at,
    CAST((julianday('now') - julianday(c.opened_at)) * 24 AS INTEGER) AS age_hours
FROM
    arbitrage_cases c
WHERE
    c.status IN ('Open', 'InProgress', 'RulingSubmitted');