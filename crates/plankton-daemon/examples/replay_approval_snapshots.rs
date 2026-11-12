//! Review frozen, sanitized approval inputs without fetching credentials or executing consumers.
//! Input: JSON array of batches (arrays of BatchReviewInput). Output: review results and timings.
use std::{path::PathBuf, time::Instant};

use anyhow::{ensure, Context, Result};
use plankton_core::{load_settings, request_llm_batch_suggestions, BatchReviewInput};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().context("input JSON required")?).canonicalize()?;
    let output = PathBuf::from(args.next().context("output JSON required")?);
    let batches: Vec<Vec<BatchReviewInput>> = serde_json::from_slice(&std::fs::read(input)?)?;
    let settings = load_settings()?;
    ensure!(settings.provider_kind == "acp", "replay requires ACP");
    let mut results = Vec::new();
    for (index, batch) in batches.iter().enumerate() {
        let start = Instant::now();
        let suggestions = request_llm_batch_suggestions(&settings, batch).await;
        let seconds = start.elapsed().as_secs_f64();
        let errors = suggestions
            .values()
            .filter(|item| item.error.is_some())
            .count();
        println!(
            "batch={index} requests={} seconds={seconds:.2} errors={errors}",
            batch.len()
        );
        results
            .push(serde_json::json!({"batch":index,"seconds":seconds,"suggestions":suggestions}));
        std::fs::write(&output, serde_json::to_vec_pretty(&results)?)?;
    }
    Ok(())
}
