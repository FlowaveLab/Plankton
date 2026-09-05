-- The evidence contract changed incompatibly in prompt_context.v11.
-- Approval history is intentionally reset instead of migrating unverifiable targets.
DELETE FROM audit_records;
DELETE FROM approval_batch_tickets;
DELETE FROM access_requests;
