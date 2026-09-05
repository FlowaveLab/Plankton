use clap::Parser;
use plankton_core::{
    build_prompt_context, generate_llm_suggestion, load_settings, PolicyMode, RequestContext,
    CLAUDE_PROVIDER_KIND,
};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "config/dev-readonly")]
    resource: String,
    #[arg(
        long,
        default_value = "Claude smoke request for readonly dev config without secret exposure"
    )]
    reason: String,
    #[arg(long, default_value = "smoke-runner")]
    requested_by: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut settings = load_settings()?;
    settings.provider_kind = CLAUDE_PROVIDER_KIND.to_string();

    let raw_context = RequestContext::new(args.resource, args.reason, args.requested_by);
    let context = build_prompt_context(&raw_context);
    let (_, suggestion) =
        generate_llm_suggestion(&settings, PolicyMode::Assisted, &raw_context, &context).await?;

    println!("{}", serde_json::to_string_pretty(&suggestion)?);
    Ok(())
}
