CREATE TABLE approval_batch_tickets (
    id TEXT PRIMARY KEY NOT NULL,
    source_request_id TEXT NOT NULL,
    semantic_call_chain_sha256 TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    reason TEXT NOT NULL,
    shared_resource_metadata_sha256 TEXT NOT NULL,
    resource_selector TEXT NOT NULL,
    suggested_decision TEXT NOT NULL,
    rationale_summary TEXT NOT NULL,
    risk_score INTEGER NOT NULL,
    provider_kind TEXT,
    provider_model TEXT,
    template_version TEXT NOT NULL,
    prompt_contract_version TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    FOREIGN KEY (source_request_id) REFERENCES access_requests(id) ON DELETE CASCADE
);

CREATE INDEX idx_approval_batch_ticket_match
ON approval_batch_tickets (
    semantic_call_chain_sha256,
    requested_by,
    reason,
    shared_resource_metadata_sha256,
    expires_at DESC
);
