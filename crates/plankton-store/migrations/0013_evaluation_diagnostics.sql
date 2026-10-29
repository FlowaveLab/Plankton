-- Backfill missing diagnostics without changing approvals or acknowledging failures.
-- Batch rows share source evidence; only the source owns the provider failure.
INSERT OR IGNORE INTO diagnostic_errors (
    id, correlation_id, code, severity, source_json, user_message, internal_message,
    public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
)
SELECT
    'evaluation:' || r.id || ':' || (CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END), lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))),
    CASE WHEN lower(CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(r.llm_suggestion_json, '$.error') ELSE json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%deadline%' OR lower(CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(r.llm_suggestion_json, '$.error') ELSE json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%timed out%' OR lower(CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(r.llm_suggestion_json, '$.error') ELSE json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%timeout%' THEN 'timeout' ELSE 'backend_failed' END,
    CASE WHEN (CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END) = 'decision' THEN 'error' ELSE 'warning' END,
    json_object('kind', CASE WHEN r.provider_kind IN ('acp', 'codex_acp') THEN 'acp' ELSE 'daemon' END),
    CASE WHEN (CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END) = 'decision' THEN 'Automatic review failed; human review remains available' ELSE 'Approval evidence could not be completed; the recorded decision is unchanged' END,
    CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(r.llm_suggestion_json, '$.error') ELSE json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.error') END, json_object('request_id', r.id, 'phase', (CASE WHEN NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END)), '{}', 1,
    COALESCE(json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.updated_at'), json_extract(r.llm_suggestion_json, '$.generated_at'), r.updated_at), NULL
FROM access_requests AS r
WHERE COALESCE(json_extract(CASE WHEN json_valid(r.automatic_decision_json) THEN r.automatic_decision_json END, '$.decision_source'), '') != 'batch_ticket'
AND CASE WHEN json_valid(r.llm_suggestion_json) THEN (
NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NOT NULL
    OR (json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.state') IN ('failed', 'partial')
        AND NULLIF(json_extract(r.llm_suggestion_json, '$.provider_trace.review_progress.error'), '') IS NOT NULL)
) ELSE 0 END;

CREATE TRIGGER evaluation_diagnostic_after_insert
AFTER INSERT ON access_requests
WHEN COALESCE(json_extract(CASE WHEN json_valid(NEW.automatic_decision_json) THEN NEW.automatic_decision_json END, '$.decision_source'), '') != 'batch_ticket'
AND CASE WHEN json_valid(NEW.llm_suggestion_json) THEN (
NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL
    OR (json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.state') IN ('failed', 'partial')
        AND NULLIF(json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error'), '') IS NOT NULL)
) ELSE 0 END
BEGIN
INSERT OR IGNORE INTO diagnostic_errors (
    id, correlation_id, code, severity, source_json, user_message, internal_message,
    public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
)
SELECT
    'evaluation:' || NEW.id || ':' || (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END), lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))),
    CASE WHEN lower(CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%deadline%' OR lower(CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%timed out%' OR lower(CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%timeout%' THEN 'timeout' ELSE 'backend_failed' END,
    CASE WHEN (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END) = 'decision' THEN 'error' ELSE 'warning' END,
    json_object('kind', CASE WHEN NEW.provider_kind IN ('acp', 'codex_acp') THEN 'acp' ELSE 'daemon' END),
    CASE WHEN (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END) = 'decision' THEN 'Automatic review failed; human review remains available' ELSE 'Approval evidence could not be completed; the recorded decision is unchanged' END,
    CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END, json_object('request_id', NEW.id, 'phase', (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END)), '{}', 1,
    COALESCE(json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.updated_at'), json_extract(NEW.llm_suggestion_json, '$.generated_at'), NEW.updated_at), NULL
;
END;

CREATE TRIGGER evaluation_diagnostic_after_update
AFTER UPDATE OF llm_suggestion_json ON access_requests
WHEN COALESCE(json_extract(CASE WHEN json_valid(NEW.automatic_decision_json) THEN NEW.automatic_decision_json END, '$.decision_source'), '') != 'batch_ticket'
AND CASE WHEN json_valid(NEW.llm_suggestion_json) THEN (
NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL
    OR (json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.state') IN ('failed', 'partial')
        AND NULLIF(json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error'), '') IS NOT NULL)
) ELSE 0 END
BEGIN
INSERT OR IGNORE INTO diagnostic_errors (
    id, correlation_id, code, severity, source_json, user_message, internal_message,
    public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
)
SELECT
    'evaluation:' || NEW.id || ':' || (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END), lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))),
    CASE WHEN lower(CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%deadline%' OR lower(CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%timed out%' OR lower(CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END) LIKE '%timeout%' THEN 'timeout' ELSE 'backend_failed' END,
    CASE WHEN (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END) = 'decision' THEN 'error' ELSE 'warning' END,
    json_object('kind', CASE WHEN NEW.provider_kind IN ('acp', 'codex_acp') THEN 'acp' ELSE 'daemon' END),
    CASE WHEN (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END) = 'decision' THEN 'Automatic review failed; human review remains available' ELSE 'Approval evidence could not be completed; the recorded decision is unchanged' END,
    CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN json_extract(NEW.llm_suggestion_json, '$.error') ELSE json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.error') END, json_object('request_id', NEW.id, 'phase', (CASE WHEN NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NOT NULL THEN 'decision' ELSE 'review_details' END)), '{}', 1,
    COALESCE(json_extract(NEW.llm_suggestion_json, '$.provider_trace.review_progress.updated_at'), json_extract(NEW.llm_suggestion_json, '$.generated_at'), NEW.updated_at), NULL
;
END;

-- A recovered provider failure remains observable without marking the decision failed.
INSERT OR IGNORE INTO diagnostic_errors (
    id, correlation_id, code, severity, source_json, user_message, internal_message,
    public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
)
SELECT 'evaluation:' || r.id || ':decision_retry', lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))), 'backend_failed', 'warning',
    json_object('kind', 'acp'), 'Automatic review recovered after a retry',
    json_extract(r.llm_suggestion_json, '$.provider_trace.decision_attempt_errors'),
    json_object('request_id', r.id, 'phase', 'decision_retry'), '{}', 0,
    COALESCE(json_extract(r.llm_suggestion_json, '$.generated_at'), r.updated_at), NULL
FROM access_requests AS r
WHERE COALESCE(json_extract(CASE WHEN json_valid(r.automatic_decision_json) THEN r.automatic_decision_json END, '$.decision_source'), '') != 'batch_ticket'
AND CASE WHEN json_valid(r.llm_suggestion_json) THEN (
COALESCE(json_array_length(json_extract(r.llm_suggestion_json, '$.provider_trace.decision_attempt_errors')), 0) > 0 AND NULLIF(json_extract(r.llm_suggestion_json, '$.error'), '') IS NULL
) ELSE 0 END;

CREATE TRIGGER evaluation_retry_diagnostic_after_insert
AFTER INSERT ON access_requests
WHEN COALESCE(json_extract(CASE WHEN json_valid(NEW.automatic_decision_json) THEN NEW.automatic_decision_json END, '$.decision_source'), '') != 'batch_ticket'
AND CASE WHEN json_valid(NEW.llm_suggestion_json) THEN (
COALESCE(json_array_length(json_extract(NEW.llm_suggestion_json, '$.provider_trace.decision_attempt_errors')), 0) > 0 AND NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NULL
) ELSE 0 END
BEGIN
INSERT OR IGNORE INTO diagnostic_errors (
    id, correlation_id, code, severity, source_json, user_message, internal_message,
    public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
)
SELECT 'evaluation:' || NEW.id || ':decision_retry', lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))), 'backend_failed', 'warning',
    json_object('kind', 'acp'), 'Automatic review recovered after a retry',
    json_extract(NEW.llm_suggestion_json, '$.provider_trace.decision_attempt_errors'),
    json_object('request_id', NEW.id, 'phase', 'decision_retry'), '{}', 0,
    COALESCE(json_extract(NEW.llm_suggestion_json, '$.generated_at'), NEW.updated_at), NULL
;
END;

CREATE TRIGGER evaluation_retry_diagnostic_after_update
AFTER UPDATE OF llm_suggestion_json ON access_requests
WHEN COALESCE(json_extract(CASE WHEN json_valid(NEW.automatic_decision_json) THEN NEW.automatic_decision_json END, '$.decision_source'), '') != 'batch_ticket'
AND CASE WHEN json_valid(NEW.llm_suggestion_json) THEN (
COALESCE(json_array_length(json_extract(NEW.llm_suggestion_json, '$.provider_trace.decision_attempt_errors')), 0) > 0 AND NULLIF(json_extract(NEW.llm_suggestion_json, '$.error'), '') IS NULL
) ELSE 0 END
BEGIN
INSERT OR IGNORE INTO diagnostic_errors (
    id, correlation_id, code, severity, source_json, user_message, internal_message,
    public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
)
SELECT 'evaluation:' || NEW.id || ':decision_retry', lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))), 'backend_failed', 'warning',
    json_object('kind', 'acp'), 'Automatic review recovered after a retry',
    json_extract(NEW.llm_suggestion_json, '$.provider_trace.decision_attempt_errors'),
    json_object('request_id', NEW.id, 'phase', 'decision_retry'), '{}', 0,
    COALESCE(json_extract(NEW.llm_suggestion_json, '$.generated_at'), NEW.updated_at), NULL
;
END;
