use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    RequestContext, SanitizedArgument, SanitizedCallChainEntry, SanitizedInlineSource,
    SanitizedPromptContext, SanitizedSourceLine,
};

// Compatibility names retained for persisted/API consumers. Approval evidence is lossless.
pub fn sanitize_audit_payload_for_display(payload: &Value) -> Value {
    payload.clone()
}

pub fn build_prompt_context(context: &RequestContext) -> SanitizedPromptContext {
    let safe_context = sanitize_request_context_for_storage(context);
    let resource = context.resource.clone();
    let resource_tags = context.resource_tags.clone();
    let metadata = context.resource_metadata.clone();
    let request_metadata = context.metadata.clone();
    let call_chain_details = safe_context
        .call_chain
        .iter()
        .map(|node| SanitizedCallChainEntry {
            pid: node.pid,
            ppid: node.ppid,
            source: node.source.clone(),
            process_name: node.process_name.clone(),
            executable_path: node.executable_path.clone(),
            arguments: node
                .argv
                .iter()
                .enumerate()
                .map(|(argument_index, value)| SanitizedArgument {
                    argument_index,
                    text: value.clone(),
                })
                .collect(),
            resolved_file_path: node.resolved_file_path.clone(),
        })
        .collect::<Vec<_>>();
    let call_chain = call_chain_details
        .iter()
        .map(summarize_prompt_call_chain_entry)
        .collect::<Vec<_>>();
    let env_var_names = context
        .env_vars
        .keys()
        .filter_map(|key| {
            let trimmed = key.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect::<Vec<_>>();
    let reason = context.reason.clone();
    let requested_by = context.requested_by.clone();
    let script_path = context.script_path.clone();
    // Keep the audit snapshot lossless; the provider projection replaces bodies with file references.
    let inline_sources = collect_inline_sources(&call_chain_details);
    SanitizedPromptContext {
        resource,
        resource_tags,
        metadata,
        request_metadata,
        reason,
        requested_by,
        script_path,
        call_chain,
        call_chain_details,
        inline_sources,
        env_vars: context.env_vars.clone(),
        env_var_names,
    }
}

pub(crate) fn collect_inline_sources(
    call_chain: &[SanitizedCallChainEntry],
) -> Vec<SanitizedInlineSource> {
    let mut sources = BTreeMap::<String, SanitizedInlineSource>::new();
    for (node_index, node) in call_chain.iter().enumerate() {
        for argument in &node.arguments {
            if let Ok(tokens) = shell_words::split(&argument.text) {
                collect_python_commands(&tokens, node_index, argument.argument_index, &mut sources);
            }
        }
        for argument_index in 0..node.arguments.len().saturating_sub(2) {
            let executable = &node.arguments[argument_index].text;
            if is_python_executable(executable) && node.arguments[argument_index + 1].text == "-c" {
                insert_inline_source(
                    node_index,
                    argument_index + 2,
                    &node.arguments[argument_index + 2].text,
                    &mut sources,
                );
            }
        }
    }
    sources.into_values().collect()
}

fn collect_python_commands(
    tokens: &[String],
    node_index: usize,
    argument_index: usize,
    sources: &mut BTreeMap<String, SanitizedInlineSource>,
) {
    for index in 0..tokens.len().saturating_sub(2) {
        if is_python_executable(&tokens[index]) && tokens[index + 1] == "-c" {
            insert_inline_source(node_index, argument_index, &tokens[index + 2], sources);
        }
    }
}

fn is_python_executable(value: &str) -> bool {
    let basename = value.rsplit('/').next().unwrap_or(value);
    basename == "python"
        || basename == "python3"
        || basename == "pythonw"
        || basename.strip_prefix("python").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        })
}

fn insert_inline_source(
    node_index: usize,
    argument_index: usize,
    source: &str,
    sources: &mut BTreeMap<String, SanitizedInlineSource>,
) {
    if source.trim().is_empty() {
        return;
    }
    let source_id = format!("call-chain:{node_index}:argv:{argument_index}:inline-python");
    sources
        .entry(source_id.clone())
        .or_insert_with(|| SanitizedInlineSource {
            source_id,
            node_index,
            argument_index,
            language: "python".to_string(),
            lines: source
                .split('\n')
                .enumerate()
                .map(|(index, text)| SanitizedSourceLine {
                    line: index + 1,
                    text: text.to_string(),
                })
                .collect(),
        });
}

pub fn sanitize_request_context_for_storage(context: &RequestContext) -> RequestContext {
    context.clone()
}

fn summarize_prompt_call_chain_entry(entry: &SanitizedCallChainEntry) -> String {
    let subject = entry
        .resolved_file_path
        .as_deref()
        .or(entry.executable_path.as_deref())
        .or(entry.process_name.as_deref())
        .unwrap_or("unknown");

    if entry.arguments.is_empty() {
        subject.to_string()
    } else {
        format!(
            "{subject} :: {}",
            entry
                .arguments
                .iter()
                .map(|argument| argument.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{CallChainNode, RequestContext};

    use super::{
        build_prompt_context, sanitize_audit_payload_for_display,
        sanitize_request_context_for_storage,
    };

    #[test]
    fn audit_payload_preserves_all_evidence() {
        let payload =
            serde_json::json!({"password":"example", "nested":{"authorization":"example"}});
        assert_eq!(sanitize_audit_payload_for_display(&payload), payload);
    }

    #[test]
    fn exposes_request_metadata_and_call_chain_arguments_to_provider_context() {
        let mut context = RequestContext::new(
            "secret/demo".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context.resource_tags = vec!["prod".to_string(), "db".to_string()];
        context
            .resource_metadata
            .insert("environment".to_string(), "dev".to_string());
        context.env_vars.insert(
            "OPENAI_API_KEY".to_string(),
            "sk-test-super-secret-value".to_string(),
        );
        context
            .metadata
            .insert("environment".to_string(), "dev".to_string());
        context.metadata.insert(
            "api_token".to_string(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        );
        context.script_path = Some("/Users/zqqqqz2000/private/run-secret.sh".to_string());
        context.call_chain = vec![CallChainNode {
            pid: None,
            ppid: None,
            process_name: Some("bash".to_string()),
            executable_path: Some("/bin/bash".to_string()),
            argv: vec![
                "bash".to_string(),
                "/Users/zqqqqz2000/private/run-secret.sh".to_string(),
                "--token=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ],
            resolved_file_path: Some("/Users/zqqqqz2000/private/run-secret.sh".to_string()),
            source: crate::CallChainNodeSource::BestEffort,
            previewable: true,
            preview_status: crate::CallChainPreviewStatus::PathOnly,
            preview_text: None,
            preview_error: None,
        }];

        let prompt_context = build_prompt_context(&context);

        assert_eq!(prompt_context.resource, "secret/demo");
        assert_eq!(
            prompt_context.resource_tags,
            vec!["prod".to_string(), "db".to_string()]
        );
        assert_eq!(
            prompt_context.metadata.get("environment"),
            Some(&"dev".to_string())
        );
        assert_eq!(prompt_context.reason, "Need smoke test access");
        assert_eq!(prompt_context.requested_by, "alice");
        assert_eq!(
            prompt_context.script_path.as_deref(),
            Some("/Users/zqqqqz2000/private/run-secret.sh")
        );
        assert_eq!(
            prompt_context.request_metadata.get("environment"),
            Some(&"dev".to_string())
        );
        assert_eq!(prompt_context.request_metadata, context.metadata);
        assert_eq!(prompt_context.env_vars, context.env_vars);
        assert_eq!(
            prompt_context.env_var_names,
            vec!["OPENAI_API_KEY".to_string()]
        );
        assert_eq!(prompt_context.call_chain_details.len(), 1);
        assert_eq!(
            prompt_context.call_chain_details[0]
                .resolved_file_path
                .as_deref(),
            Some("/Users/zqqqqz2000/private/run-secret.sh")
        );
        assert_eq!(
            prompt_context.call_chain_details[0].arguments[2].text,
            "--token=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            prompt_context.call_chain_details[0].arguments[2].argument_index,
            2
        );
    }

    #[test]
    fn keeps_original_paths_and_preview_evidence() {
        let mut context = RequestContext::new(
            "secret/demo".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context.script_path = Some("/Users/zqqqqz2000/private/run-secret.sh".to_string());
        context.call_chain = vec![
            CallChainNode::best_effort_path("/Users/zqqqqz2000/private/run-secret.sh"),
            CallChainNode::best_effort_path("bash"),
        ];
        context.call_chain[0].preview_text = Some("echo secret".to_string());
        context.call_chain[0].preview_error = Some("Preview unavailable".to_string());

        let prompt_context = build_prompt_context(&context);
        let stored_context = sanitize_request_context_for_storage(&context);

        assert_eq!(
            prompt_context.script_path.as_deref(),
            Some("/Users/zqqqqz2000/private/run-secret.sh")
        );
        assert_eq!(
            stored_context.script_path.as_deref(),
            Some("/Users/zqqqqz2000/private/run-secret.sh")
        );
        assert_eq!(prompt_context.call_chain_details.len(), 2);
        assert_eq!(
            stored_context.call_chain[0].resolved_file_path.as_deref(),
            Some("/Users/zqqqqz2000/private/run-secret.sh")
        );
        assert_eq!(stored_context, context);
    }

    #[test]
    fn materializes_inline_python_with_stable_source_lines() {
        let mut context = RequestContext::new(
            "secret/demo".to_string(),
            "Inspect inline request".to_string(),
            "alice".to_string(),
        );
        context.call_chain = vec![CallChainNode {
            pid: None,
            ppid: None,
            process_name: Some("zsh".to_string()),
            executable_path: Some("/bin/zsh".to_string()),
            argv: vec![
                "zsh".to_string(),
                "-c".to_string(),
                "python3 -c 'import requests\nrequests.post(\"https://example.test\")'".to_string(),
            ],
            resolved_file_path: None,
            source: crate::CallChainNodeSource::BestEffort,
            previewable: false,
            preview_status: crate::CallChainPreviewStatus::NotPreviewable,
            preview_text: None,
            preview_error: None,
        }];

        let prompt_context = build_prompt_context(&context);

        assert_eq!(prompt_context.inline_sources.len(), 1);
        let sources = super::collect_inline_sources(&prompt_context.call_chain_details);
        let source = &sources[0];
        assert_eq!(source.source_id, "call-chain:0:argv:2:inline-python");
        assert_eq!(source.language, "python");
        assert_eq!(source.lines[0].text, "import requests");
        assert_eq!(
            source.lines[1].text,
            "requests.post(\"https://example.test\")"
        );
    }
}
