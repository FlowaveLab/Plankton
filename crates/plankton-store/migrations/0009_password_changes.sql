CREATE TABLE password_changes (
    id TEXT PRIMARY KEY NOT NULL,
    batch_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    state TEXT NOT NULL,
    version INTEGER NOT NULL,
    confirmed_version INTEGER,
    base_revision TEXT NOT NULL,
    operations_json TEXT NOT NULL,
    operation_ids_json TEXT NOT NULL,
    diff_json TEXT NOT NULL,
    successor_change_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    confirmed_at TEXT,
    committed_at TEXT,
    error_message TEXT
);

CREATE INDEX idx_password_changes_state_created_at
    ON password_changes(state, created_at);

CREATE INDEX idx_password_changes_batch_created_at
    ON password_changes(batch_id, created_at);
