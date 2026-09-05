ALTER TABLE audit_records ADD COLUMN event_type TEXT;
ALTER TABLE audit_records ADD COLUMN decision TEXT;
ALTER TABLE audit_records ADD COLUMN actor_type TEXT;
ALTER TABLE audit_records ADD COLUMN actor_id TEXT;
ALTER TABLE audit_records ADD COLUMN resource TEXT;
ALTER TABLE audit_records ADD COLUMN policy_mode TEXT;
ALTER TABLE audit_records ADD COLUMN approval_status TEXT;
ALTER TABLE audit_records ADD COLUMN request_reason TEXT;
ALTER TABLE audit_records ADD COLUMN approval_note TEXT;

UPDATE audit_records
SET
    event_type = action,
    decision = json_extract(payload_json, '$.decision'),
    actor_type = CASE
        WHEN action = 'request_submitted' THEN 'requester'
        WHEN action = 'approval_recorded' THEN 'human'
        WHEN action = 'status_viewed' THEN 'viewer'
        ELSE 'system'
    END,
    actor_id = actor,
    resource = COALESCE(
        json_extract(payload_json, '$.resource'),
        (SELECT resource FROM access_requests WHERE id = audit_records.request_id)
    ),
    policy_mode = COALESCE(
        json_extract(payload_json, '$.policy_mode'),
        (SELECT policy_mode FROM access_requests WHERE id = audit_records.request_id)
    ),
    approval_status = COALESCE(
        json_extract(payload_json, '$.approval_status'),
        (SELECT approval_status FROM access_requests WHERE id = audit_records.request_id)
    ),
    request_reason = (SELECT reason FROM access_requests WHERE id = audit_records.request_id),
    approval_note = note;

CREATE TRIGGER IF NOT EXISTS trg_audit_records_fill_read_fields
AFTER INSERT ON audit_records
BEGIN
    UPDATE audit_records
    SET
        event_type = NEW.action,
        decision = json_extract(NEW.payload_json, '$.decision'),
        actor_type = CASE
            WHEN NEW.action = 'request_submitted' THEN 'requester'
            WHEN NEW.action = 'approval_recorded' THEN 'human'
            WHEN NEW.action = 'status_viewed' THEN 'viewer'
            ELSE 'system'
        END,
        actor_id = NEW.actor,
        resource = COALESCE(
            json_extract(NEW.payload_json, '$.resource'),
            (SELECT resource FROM access_requests WHERE id = NEW.request_id)
        ),
        policy_mode = COALESCE(
            json_extract(NEW.payload_json, '$.policy_mode'),
            (SELECT policy_mode FROM access_requests WHERE id = NEW.request_id)
        ),
        approval_status = COALESCE(
            json_extract(NEW.payload_json, '$.approval_status'),
            (SELECT approval_status FROM access_requests WHERE id = NEW.request_id)
        ),
        request_reason = (SELECT reason FROM access_requests WHERE id = NEW.request_id),
        approval_note = NEW.note
    WHERE id = NEW.id;
END;

CREATE VIEW IF NOT EXISTS approval_requests AS
SELECT
    id,
    id AS request_id,
    resource,
    requested_by,
    requested_by AS requester_id,
    reason,
    policy_mode,
    approval_status,
    final_decision AS decision,
    provider_kind,
    rendered_prompt,
    context_json,
    created_at,
    updated_at,
    resolved_at
FROM access_requests;

CREATE INDEX IF NOT EXISTS idx_access_requests_updated_at
    ON access_requests(updated_at);

CREATE INDEX IF NOT EXISTS idx_audit_records_event_type_created_at
    ON audit_records(event_type, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_records_request_id_event_type_created_at
    ON audit_records(request_id, event_type, created_at DESC);
