CREATE TABLE remembered_exposure_policies (
    resource_id TEXT PRIMARY KEY NOT NULL,
    catalog_fingerprint TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    source_request_id TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
