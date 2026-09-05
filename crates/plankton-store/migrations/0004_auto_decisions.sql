ALTER TABLE access_requests ADD COLUMN automatic_decision_json TEXT;

UPDATE audit_records
SET
    event_type = action,
    decision = COALESCE(
        json_extract(payload_json, '$.decision'),
        json_extract(payload_json, '$.suggested_decision')
    ),
    actor_type = CASE
        WHEN actor = 'system_auto' THEN 'system_auto'
        WHEN action = 'request_submitted' THEN 'requester'
        WHEN action IN ('approval_recorded', 'human_decision_overrode_llm') THEN 'human'
        WHEN action IN ('llm_suggestion_generated', 'llm_suggestion_failed') THEN 'llm'
        WHEN action IN ('automatic_decision_recorded', 'automatic_escalated_to_human') THEN 'system_auto'
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

DROP TRIGGER IF EXISTS trg_audit_records_fill_read_fields;

CREATE TRIGGER trg_audit_records_fill_read_fields
AFTER INSERT ON audit_records
BEGIN
    UPDATE audit_records
    SET
        event_type = NEW.action,
        decision = COALESCE(
            json_extract(NEW.payload_json, '$.decision'),
            json_extract(NEW.payload_json, '$.suggested_decision')
        ),
        actor_type = CASE
            WHEN NEW.actor = 'system_auto' THEN 'system_auto'
            WHEN NEW.action = 'request_submitted' THEN 'requester'
            WHEN NEW.action IN ('approval_recorded', 'human_decision_overrode_llm') THEN 'human'
            WHEN NEW.action IN ('llm_suggestion_generated', 'llm_suggestion_failed') THEN 'llm'
            WHEN NEW.action IN ('automatic_decision_recorded', 'automatic_escalated_to_human') THEN 'system_auto'
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

DROP VIEW IF EXISTS approval_requests;

CREATE VIEW approval_requests AS
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
    provider_input_json,
    llm_suggestion_json,
    automatic_decision_json,
    context_json,
    created_at,
    updated_at,
    resolved_at
FROM access_requests;
