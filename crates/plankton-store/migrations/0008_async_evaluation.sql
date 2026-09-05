ALTER TABLE access_requests
    ADD COLUMN evaluation_state TEXT NOT NULL DEFAULT 'not_required';

UPDATE access_requests
SET evaluation_state = 'completed'
WHERE policy_mode IN ('assisted', 'llm_automatic')
  AND (llm_suggestion_json IS NOT NULL OR automatic_decision_json IS NOT NULL);

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
    evaluation_state,
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

CREATE INDEX idx_access_requests_evaluation_state_created_at
    ON access_requests(evaluation_state, created_at, id);

CREATE INDEX idx_interrupted_operations_kind_key_status
    ON interrupted_operations(operation_kind, operation_key, status);
