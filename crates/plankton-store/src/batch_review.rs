use std::collections::BTreeMap;

use plankton_core::{AccessRequest, AutomaticDecisionSource, LlmSuggestion};
use sqlx::SqlitePool;

use crate::StoreError;

/// Batch authorization is immutable; only its shared, progressively enriched evidence is live.
/// Resolve that evidence at read time so older rows cannot freeze a copied `running` snapshot.
pub(crate) async fn project_batch_reviews(
    pool: &SqlitePool,
    requests: &mut [AccessRequest],
) -> Result<(), StoreError> {
    let mut sources = BTreeMap::<String, Option<LlmSuggestion>>::new();
    for request in requests.iter() {
        let Some(source_id) = batch_source(request) else {
            continue;
        };
        if sources.contains_key(source_id) {
            continue;
        }
        let encoded: Option<Option<String>> =
            sqlx::query_scalar("SELECT llm_suggestion_json FROM access_requests WHERE id = ?")
                .bind(source_id)
                .fetch_optional(pool)
                .await?;
        let suggestion = encoded
            .flatten()
            .map(|value| serde_json::from_str(&value))
            .transpose()?;
        sources.insert(source_id.to_string(), suggestion);
    }
    for request in requests {
        let Some(source) = batch_source(request)
            .and_then(|id| sources.get(id))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let Some(trace) = request.automatic_decision.as_ref() else {
            continue;
        };
        if request.llm_suggestion.is_none() {
            let (Some(decision), Some(risk)) = (trace.suggested_decision, trace.risk_score) else {
                continue;
            };
            let mut inherited = source.clone();
            inherited.suggested_decision = decision;
            inherited.rationale_summary = trace.auto_rationale_summary.clone();
            inherited.risk_score = risk;
            inherited.batch_decisions.clear();
            inherited.usage = None;
            request.llm_suggestion = Some(inherited);
        }
        if let Some(suggestion) = &mut request.llm_suggestion {
            suggestion.exposure_report = source.exposure_report.clone();
            // A batch shares its source session, tool audit and progressive evidence.
            // Keep the target's per-resource authorization fields unchanged.
            suggestion.provider_trace = source.provider_trace.clone();
            suggestion.usage = source.usage.clone();
        }
    }
    Ok(())
}

fn batch_source(request: &AccessRequest) -> Option<&str> {
    let trace = request.automatic_decision.as_ref()?;
    if trace.decision_source != AutomaticDecisionSource::BatchTicket {
        return None;
    }
    trace
        .batch_source_request_id
        .as_deref()
        .filter(|id| *id != request.id)
}
