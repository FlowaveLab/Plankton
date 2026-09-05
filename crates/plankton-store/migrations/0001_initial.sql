CREATE TABLE IF NOT EXISTS access_requests (
    id TEXT PRIMARY KEY NOT NULL,
    resource TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    reason TEXT NOT NULL,
    policy_mode TEXT NOT NULL,
    approval_status TEXT NOT NULL,
    final_decision TEXT,
    provider_kind TEXT,
    rendered_prompt TEXT NOT NULL,
    context_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS audit_records (
    id TEXT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL,
    action TEXT NOT NULL,
    actor TEXT NOT NULL,
    note TEXT,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (request_id) REFERENCES access_requests(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_access_requests_status_created_at
    ON access_requests(approval_status, created_at);

CREATE INDEX IF NOT EXISTS idx_audit_records_request_id_created_at
    ON audit_records(request_id, created_at);

