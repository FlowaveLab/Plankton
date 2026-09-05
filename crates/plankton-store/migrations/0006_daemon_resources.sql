CREATE TABLE backend_bindings (
    id TEXT PRIMARY KEY NOT NULL,
    backend_kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL DEFAULT '{}',
    capabilities_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_backend_bindings_kind_name
    ON backend_bindings(backend_kind, display_name);

CREATE TABLE vault_manifests (
    id TEXT PRIMARY KEY NOT NULL,
    backend_binding_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    format_version INTEGER NOT NULL DEFAULT 1,
    local_path TEXT,
    revision INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (backend_binding_id) REFERENCES backend_bindings(id) ON DELETE RESTRICT
);

CREATE INDEX idx_vault_manifests_backend
    ON vault_manifests(backend_binding_id, archived, display_name);

CREATE TABLE resource_items (
    id TEXT PRIMARY KEY NOT NULL,
    vault_id TEXT NOT NULL,
    backend_locator_json TEXT NOT NULL,
    display_name TEXT NOT NULL,
    category TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (vault_id) REFERENCES vault_manifests(id) ON DELETE CASCADE
);

CREATE INDEX idx_resource_items_vault_updated
    ON resource_items(vault_id, archived, updated_at DESC, id);

CREATE TABLE resource_sections (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    label TEXT NOT NULL,
    position INTEGER NOT NULL,
    FOREIGN KEY (item_id) REFERENCES resource_items(id) ON DELETE CASCADE,
    UNIQUE (item_id, position)
);

CREATE TABLE resource_fields (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    section_id TEXT,
    field_key TEXT NOT NULL,
    label TEXT NOT NULL,
    field_kind TEXT NOT NULL,
    is_concealed INTEGER NOT NULL DEFAULT 1 CHECK (is_concealed IN (0, 1)),
    position INTEGER NOT NULL,
    resource_uri TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (item_id) REFERENCES resource_items(id) ON DELETE CASCADE,
    FOREIGN KEY (section_id) REFERENCES resource_sections(id) ON DELETE SET NULL,
    UNIQUE (item_id, field_key)
);

CREATE INDEX idx_resource_fields_item_position
    ON resource_fields(item_id, position, id);

CREATE TABLE resource_tags (
    id TEXT PRIMARY KEY NOT NULL,
    normalized_name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL
);

CREATE TABLE resource_item_tags (
    item_id TEXT NOT NULL,
    tag_id TEXT NOT NULL,
    PRIMARY KEY (item_id, tag_id),
    FOREIGN KEY (item_id) REFERENCES resource_items(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES resource_tags(id) ON DELETE CASCADE
);

CREATE INDEX idx_resource_item_tags_tag
    ON resource_item_tags(tag_id, item_id);

CREATE TABLE resource_aliases (
    item_id TEXT NOT NULL,
    alias TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,
    PRIMARY KEY (item_id, normalized_alias),
    FOREIGN KEY (item_id) REFERENCES resource_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_resource_aliases_normalized
    ON resource_aliases(normalized_alias, item_id);

CREATE TABLE resource_search_documents (
    field_id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    document_json TEXT NOT NULL,
    normalized_text TEXT NOT NULL,
    index_generation INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (field_id) REFERENCES resource_fields(id) ON DELETE CASCADE,
    FOREIGN KEY (item_id) REFERENCES resource_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_resource_search_documents_generation
    ON resource_search_documents(index_generation, item_id, field_id);

CREATE TABLE sync_states (
    vault_id TEXT NOT NULL,
    adapter_id TEXT NOT NULL,
    remote_revision TEXT,
    base_hash TEXT,
    local_hash TEXT,
    last_attempt_at TEXT,
    last_success_at TEXT,
    status TEXT NOT NULL,
    error_id TEXT,
    config_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (vault_id, adapter_id),
    FOREIGN KEY (vault_id) REFERENCES vault_manifests(id) ON DELETE CASCADE
);

CREATE TABLE interrupted_operations (
    id TEXT PRIMARY KEY NOT NULL,
    operation_kind TEXT NOT NULL,
    operation_key TEXT NOT NULL,
    state_json TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE INDEX idx_interrupted_operations_status
    ON interrupted_operations(status, heartbeat_at, id);
