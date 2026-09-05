use clap::Parser;
use plankton_core::{load_settings, AcpSessionClient};
use serde_json::json;

#[derive(Debug, Parser)]
struct Args {
    #[arg(
        long,
        default_value = "Return strict JSON only: {\"suggested_decision\":\"allow|deny|escalate\",\"rationale_summary\":\"short rationale\",\"risk_score\":0-100}. Evaluate this request summary: low-risk dev smoke test with no secret exposure."
    )]
    prompt: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let settings = load_settings()?;
    let client = AcpSessionClient::from_settings(&settings)?;
    let result = client.prompt_json_suggestion(args.prompt).await?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "provider_model": result.provider_model,
            "provider_trace": result.trace,
            "content": result.content,
        }))?
    );

    Ok(())
}
