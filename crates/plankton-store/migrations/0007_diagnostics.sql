CREATE TABLE diagnostic_errors (
    id TEXT PRIMARY KEY NOT NULL,
    correlation_id TEXT NOT NULL,
    code TEXT NOT NULL,
    severity TEXT NOT NULL,
    source_json TEXT NOT NULL,
    user_message TEXT NOT NULL,
    internal_message TEXT,
    public_context_json TEXT NOT NULL DEFAULT '{}',
    internal_context_json TEXT NOT NULL DEFAULT '{}',
    retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
    occurred_at TEXT NOT NULL,
    acknowledged_at TEXT
);

CREATE INDEX idx_diagnostic_errors_unacknowledged
    ON diagnostic_errors(acknowledged_at, occurred_at DESC, id);

CREATE INDEX idx_diagnostic_errors_correlation
    ON diagnostic_errors(correlation_id, occurred_at, id);
