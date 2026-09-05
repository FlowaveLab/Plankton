use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::call_chain::read_review_file;

pub(crate) const RUN_COMMAND_TOOL_NAME: &str = "run_command";
pub(crate) const READ_FILE_TOOL_NAME: &str = "read_file";
pub(crate) const GREP_FILES_TOOL_NAME: &str = "grep_files";
pub(crate) const FIND_FILES_TOOL_NAME: &str = "find_files";
pub(crate) const WRITE_REVIEW_FILE_TOOL_NAME: &str = "write_review_file";
pub(crate) const VALIDATE_REVIEW_WORKSPACE_TOOL_NAME: &str = "validate_review_workspace";
pub(crate) const MIN_TOOL_RESULT_CHARS: usize = 256;
pub(crate) const DEFAULT_TOOL_RESULT_CHARS: usize = 4_000;
pub(crate) const MAX_TOOL_RESULT_CHARS: usize = 12_000;
pub(crate) const MAX_TOOL_TOTAL_CHARS: usize = 24_000;
pub(crate) const MAX_TOOL_CALLS: usize = 12;
pub(crate) const MAX_TOOL_ROUNDS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProviderFileToolError {
    #[error("unsupported provider file tool {0}")]
    UnsupportedTool(String),
    #[error("invalid arguments for provider file tool {tool}: {message}")]
    InvalidArguments { tool: String, message: String },
    #[error("provider file tool {tool} failed: {message}")]
    Execution { tool: String, message: String },
    #[error("provider file tool call limit of {MAX_TOOL_CALLS} was exceeded")]
    CallLimitExceeded,
    #[error(
        "provider file tool total result limit of {MAX_TOOL_TOTAL_CHARS} characters was exhausted"
    )]
    TotalResultLimitExceeded,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderFileToolExecutor {
    allowed_paths: Vec<String>,
    calls: usize,
    returned_chars: usize,
    review_files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadFileArgs {
    path: String,
    #[serde(default = "default_start_line")]
    start_line: usize,
    #[serde(default = "default_max_lines")]
    max_lines: usize,
    #[serde(default = "default_result_chars")]
    max_chars: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrepFilesArgs {
    query: String,
    path: Option<String>,
    #[serde(default = "default_true")]
    case_sensitive: bool,
    #[serde(default = "default_max_matches")]
    max_matches: usize,
    #[serde(default = "default_result_chars")]
    max_chars: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindFilesArgs {
    pattern: Option<String>,
    #[serde(default = "default_max_results")]
    max_results: usize,
    #[serde(default = "default_result_chars")]
    max_chars: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteReviewFileArgs {
    path: String,
    content: String,
}

impl ProviderFileToolExecutor {
    pub(crate) fn new(allowed_paths: Vec<String>) -> Self {
        let allowed_paths = allowed_paths
            .into_iter()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            allowed_paths,
            calls: 0,
            returned_chars: 0,
            review_files: BTreeMap::new(),
        }
    }

    pub(crate) fn begin_phase(&mut self) {
        self.calls = 0;
        self.returned_chars = 0;
    }

    pub(crate) async fn execute_async(
        &mut self,
        tool: &str,
        arguments: &str,
    ) -> Result<String, ProviderFileToolError> {
        let mut executor = self.clone();
        let tool = tool.to_string();
        let arguments = arguments.to_string();
        let (executor, result) = tokio::task::spawn_blocking(move || {
            let result = executor.execute(&tool, &arguments);
            (executor, result)
        })
        .await
        .map_err(|error| ProviderFileToolError::Execution {
            tool: "tool worker".into(),
            message: error.to_string(),
        })?;
        *self = executor;
        result
    }

    pub(crate) fn execute(
        &mut self,
        tool_name: &str,
        arguments: &str,
    ) -> Result<String, ProviderFileToolError> {
        if self.calls >= MAX_TOOL_CALLS {
            return Err(ProviderFileToolError::CallLimitExceeded);
        }
        self.calls += 1;

        let (content, source_truncated, requested_chars) = match tool_name {
            RUN_COMMAND_TOOL_NAME => self.execute_command(arguments)?,
            READ_FILE_TOOL_NAME => self.execute_read(arguments)?,
            GREP_FILES_TOOL_NAME => self.execute_grep(arguments)?,
            FIND_FILES_TOOL_NAME => self.execute_find(arguments)?,
            WRITE_REVIEW_FILE_TOOL_NAME => self.execute_write_review_file(arguments)?,
            VALIDATE_REVIEW_WORKSPACE_TOOL_NAME => self.execute_validate_workspace(arguments)?,
            other => return Err(ProviderFileToolError::UnsupportedTool(other.to_string())),
        };
        validate_max_chars(tool_name, requested_chars)?;

        let remaining = MAX_TOOL_TOTAL_CHARS.saturating_sub(self.returned_chars);
        if remaining < MIN_TOOL_RESULT_CHARS {
            return Err(ProviderFileToolError::TotalResultLimitExceeded);
        }
        let result_limit = requested_chars.min(remaining);
        let output = bounded_tool_output(tool_name, &content, source_truncated, result_limit)?;
        self.returned_chars += output.chars().count();
        Ok(output)
    }

    fn execute_command(
        &self,
        arguments: &str,
    ) -> Result<(String, bool, usize), ProviderFileToolError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CommandArgs {
            command: String,
            cwd: Option<String>,
            #[serde(default = "default_result_chars")]
            max_chars: usize,
        }
        let args: CommandArgs = parse_args(RUN_COMMAND_TOOL_NAME, arguments)?;
        #[cfg(unix)]
        let mut command = {
            let mut command = std::process::Command::new("/bin/sh");
            command.args(["-c", &args.command]);
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", &args.command]);
            command
        };
        if let Some(cwd) = args.cwd {
            command.current_dir(cwd);
        }
        let output = command
            .output()
            .map_err(|error| ProviderFileToolError::Execution {
                tool: RUN_COMMAND_TOOL_NAME.to_string(),
                message: error.to_string(),
            })?;
        Ok((
            format!(
                "exit_code: {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            false,
            args.max_chars,
        ))
    }

    fn execute_read(
        &self,
        arguments: &str,
    ) -> Result<(String, bool, usize), ProviderFileToolError> {
        let args: ReadFileArgs = parse_args(READ_FILE_TOOL_NAME, arguments)?;
        if args.path.trim().is_empty()
            || args.start_line == 0
            || !(1..=1_000).contains(&args.max_lines)
        {
            return Err(invalid_args(
                READ_FILE_TOOL_NAME,
                "path must be non-empty, start_line must be >= 1, and max_lines must be 1..=1000",
            ));
        }
        let file =
            read_review_file(&args.path).map_err(|error| ProviderFileToolError::Execution {
                tool: READ_FILE_TOOL_NAME.to_string(),
                message: error.to_string(),
            })?;
        let lines = file.content.lines().collect::<Vec<_>>();
        let start_index = args.start_line.saturating_sub(1).min(lines.len());
        let end_index = start_index.saturating_add(args.max_lines).min(lines.len());
        let mut content = format!(
            "path: {}\nencoding: {}\nlines: {}-{}\n---",
            file.path,
            file.encoding,
            if start_index < lines.len() {
                start_index + 1
            } else {
                0
            },
            end_index
        );
        for (index, line) in lines[start_index..end_index].iter().enumerate() {
            content.push_str(&format!("\n{}: {}", start_index + index + 1, line));
        }
        let source_truncated = file.truncated || start_index > 0 || end_index < lines.len();
        Ok((content, source_truncated, args.max_chars))
    }

    fn execute_grep(
        &self,
        arguments: &str,
    ) -> Result<(String, bool, usize), ProviderFileToolError> {
        let args: GrepFilesArgs = parse_args(GREP_FILES_TOOL_NAME, arguments)?;
        if args.query.is_empty()
            || args.query.chars().count() > 256
            || !(1..=500).contains(&args.max_matches)
        {
            return Err(invalid_args(
                GREP_FILES_TOOL_NAME,
                "query must contain 1..=256 characters and max_matches must be 1..=500",
            ));
        }
        let paths = match args.path.as_deref() {
            Some(path) => {
                // Explicit paths are unrestricted; omitted paths use the request source hints.
                read_review_file(path).map_err(|error| ProviderFileToolError::Execution {
                    tool: GREP_FILES_TOOL_NAME.to_string(),
                    message: error.to_string(),
                })?;
                vec![path.to_string()]
            }
            None => self.allowed_paths.clone(),
        };
        let needle = if args.case_sensitive {
            args.query.clone()
        } else {
            args.query.to_lowercase()
        };
        let mut results = Vec::new();
        let mut source_truncated = false;
        'files: for path in paths {
            let file =
                read_review_file(&path).map_err(|error| ProviderFileToolError::Execution {
                    tool: GREP_FILES_TOOL_NAME.to_string(),
                    message: error.to_string(),
                })?;
            source_truncated |= file.truncated;
            for (line_index, line) in file.content.lines().enumerate() {
                let haystack = if args.case_sensitive {
                    line.to_string()
                } else {
                    line.to_lowercase()
                };
                if haystack.contains(&needle) {
                    results.push(format!("{}:{}: {}", file.path, line_index + 1, line));
                    if results.len() == args.max_matches {
                        source_truncated = true;
                        break 'files;
                    }
                }
            }
        }
        let content = if results.is_empty() {
            "No matches in the selected source files.".to_string()
        } else {
            results.join("\n")
        };
        Ok((content, source_truncated, args.max_chars))
    }

    fn execute_find(
        &self,
        arguments: &str,
    ) -> Result<(String, bool, usize), ProviderFileToolError> {
        let args: FindFilesArgs = parse_args(FIND_FILES_TOOL_NAME, arguments)?;
        if args
            .pattern
            .as_ref()
            .is_some_and(|pattern| pattern.chars().count() > 256)
            || !(1..=500).contains(&args.max_results)
        {
            return Err(invalid_args(
                FIND_FILES_TOOL_NAME,
                "pattern must contain at most 256 characters and max_results must be 1..=500",
            ));
        }
        let pattern = args.pattern.as_deref().unwrap_or_default();
        let matching = self
            .allowed_paths
            .iter()
            .filter(|path| pattern.is_empty() || path.contains(pattern))
            .take(args.max_results.saturating_add(1))
            .cloned()
            .collect::<Vec<_>>();
        let source_truncated = matching.len() > args.max_results;
        let content = if matching.is_empty() {
            "No request source hints matched.".to_string()
        } else {
            matching
                .into_iter()
                .take(args.max_results)
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok((content, source_truncated, args.max_chars))
    }

    fn execute_write_review_file(
        &mut self,
        arguments: &str,
    ) -> Result<(String, bool, usize), ProviderFileToolError> {
        let args: WriteReviewFileArgs = parse_args(WRITE_REVIEW_FILE_TOOL_NAME, arguments)?;
        if !matches!(
            args.path.as_str(),
            "chain.md" | "nodes.json" | "exposure.json"
        ) {
            std::fs::write(&args.path, &args.content).map_err(|error| {
                ProviderFileToolError::Execution {
                    tool: WRITE_REVIEW_FILE_TOOL_NAME.into(),
                    message: error.to_string(),
                }
            })?;
            return Ok((format!("saved {}", args.path), false, MIN_TOOL_RESULT_CHARS));
        }
        let path = args.path;
        let content_length = args.content.chars().count();
        self.review_files.insert(path.clone(), args.content);
        Ok((
            format!("saved {path} ({content_length} characters)"),
            false,
            MIN_TOOL_RESULT_CHARS,
        ))
    }

    fn execute_validate_workspace(
        &self,
        arguments: &str,
    ) -> Result<(String, bool, usize), ProviderFileToolError> {
        let empty: serde_json::Value = parse_args(VALIDATE_REVIEW_WORKSPACE_TOOL_NAME, arguments)?;
        if empty.as_object().is_none_or(|object| !object.is_empty()) {
            return Err(invalid_args(
                VALIDATE_REVIEW_WORKSPACE_TOOL_NAME,
                "arguments must be an empty object",
            ));
        }
        for path in ["chain.md", "nodes.json", "exposure.json"] {
            if !self.review_files.contains_key(path) {
                return Err(ProviderFileToolError::Execution {
                    tool: VALIDATE_REVIEW_WORKSPACE_TOOL_NAME.to_string(),
                    message: format!("missing {path}"),
                });
            }
        }
        serde_json::from_str::<Vec<crate::CallChainNodeAssessment>>(
            &self.review_files["nodes.json"],
        )
        .map_err(|error| ProviderFileToolError::Execution {
            tool: VALIDATE_REVIEW_WORKSPACE_TOOL_NAME.to_string(),
            message: format!("nodes.json is invalid: {error}"),
        })?;
        let report = serde_json::from_str::<crate::CredentialExposureReport>(
            &self.review_files["exposure.json"],
        )
        .map_err(|error| ProviderFileToolError::Execution {
            tool: VALIDATE_REVIEW_WORKSPACE_TOOL_NAME.to_string(),
            message: format!("exposure.json is invalid: {error}"),
        })?;
        report
            .validate()
            .map_err(|message| ProviderFileToolError::Execution {
                tool: VALIDATE_REVIEW_WORKSPACE_TOOL_NAME.to_string(),
                message,
            })?;
        Ok((
            "review workspace validated; copy exposure.json into final exposure_report".to_string(),
            false,
            MIN_TOOL_RESULT_CHARS,
        ))
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(
    tool: &str,
    arguments: &str,
) -> Result<T, ProviderFileToolError> {
    serde_json::from_str(arguments).map_err(|error| invalid_args(tool, &error.to_string()))
}

fn invalid_args(tool: &str, message: &str) -> ProviderFileToolError {
    ProviderFileToolError::InvalidArguments {
        tool: tool.to_string(),
        message: message.to_string(),
    }
}

fn validate_max_chars(tool: &str, max_chars: usize) -> Result<(), ProviderFileToolError> {
    if !(MIN_TOOL_RESULT_CHARS..=MAX_TOOL_RESULT_CHARS).contains(&max_chars) {
        return Err(invalid_args(
            tool,
            &format!("max_chars must be {MIN_TOOL_RESULT_CHARS}..={MAX_TOOL_RESULT_CHARS}"),
        ));
    }
    Ok(())
}

fn bounded_tool_output(
    tool: &str,
    content: &str,
    source_truncated: bool,
    max_chars: usize,
) -> Result<String, ProviderFileToolError> {
    let serialize = |payload: &str, truncated: bool| {
        serde_json::to_string(&serde_json::json!({
            "tool": tool,
            "truncated": truncated,
            "content": payload,
        }))
        .map_err(|error| ProviderFileToolError::Execution {
            tool: tool.to_string(),
            message: error.to_string(),
        })
    };

    let full = serialize(content, source_truncated)?;
    if full.chars().count() <= max_chars {
        return Ok(full);
    }

    let characters = content.chars().collect::<Vec<_>>();
    let mut low = 0;
    let mut high = characters.len();
    let mut best = None;
    while low <= high {
        let middle = low + (high - low) / 2;
        let candidate = serialize(&characters[..middle].iter().collect::<String>(), true)?;
        if candidate.chars().count() <= max_chars {
            best = Some(candidate);
            low = middle.saturating_add(1);
        } else if middle == 0 {
            break;
        } else {
            high = middle - 1;
        }
    }
    best.ok_or_else(|| invalid_args(tool, "max_chars is too small for the tool result envelope"))
}

const fn default_start_line() -> usize {
    1
}

const fn default_max_lines() -> usize {
    200
}

const fn default_max_matches() -> usize {
    100
}

const fn default_max_results() -> usize {
    100
}

const fn default_result_chars() -> usize {
    DEFAULT_TOOL_RESULT_CHARS
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn arbitrary_commands_run_in_the_requested_directory() {
        let directory = tempdir().unwrap();
        let mut executor = ProviderFileToolExecutor::new(Vec::new());
        let output = executor
            .execute_async(
                RUN_COMMAND_TOOL_NAME,
                &serde_json::json!({
                    "command":"pwd", "cwd":directory.path(), "max_chars":1024
                })
                .to_string(),
            )
            .await
            .unwrap();
        assert!(output.contains(directory.path().file_name().unwrap().to_str().unwrap()));
        assert!(output.contains("exit_code: Some(0)"));
    }

    #[test]
    fn read_is_line_bounded_and_unicode_character_bounded() {
        let directory = tempdir().expect("temporary directory should exist");
        let script = directory.path().join("审批脚本.sh");
        fs::write(&script, "第一行\n第二行 token\n第三行\n")
            .expect("test script should be written");
        let path = script.to_string_lossy().into_owned();
        let mut executor = ProviderFileToolExecutor::new(vec![path.clone()]);

        let output = executor
            .execute(
                READ_FILE_TOOL_NAME,
                &serde_json::json!({
                    "path": path,
                    "start_line": 2,
                    "max_lines": 1,
                    "max_chars": 256
                })
                .to_string(),
            )
            .expect("allowlisted read should succeed");

        assert!(output.contains("第二行 token"));
        assert!(!output.contains("第一行"));
        assert!(output.chars().count() <= 256);
        assert!(output.contains("\"truncated\":true"));
    }

    #[test]
    fn request_hints_do_not_restrict_explicit_file_reads() {
        let directory = tempdir().expect("temporary directory should exist");
        let allowed = directory.path().join("allowed.sh");
        let denied = directory.path().join("denied.sh");
        fs::write(&allowed, "safe\nneedle here\n").expect("allowed file should be written");
        fs::write(&denied, "needle secret\n").expect("denied file should be written");
        let allowed_path = allowed.to_string_lossy().into_owned();
        let denied_path = denied.to_string_lossy().into_owned();
        let mut executor = ProviderFileToolExecutor::new(vec![allowed_path.clone()]);

        let found = executor
            .execute(
                FIND_FILES_TOOL_NAME,
                r#"{"pattern":".sh","max_results":10,"max_chars":512}"#,
            )
            .expect("find should succeed");
        assert!(found.contains(&allowed_path));
        assert!(!found.contains(&denied_path));

        let grep = executor
            .execute(
                GREP_FILES_TOOL_NAME,
                r#"{"query":"needle","max_matches":10,"max_chars":512}"#,
            )
            .expect("grep should succeed");
        assert!(grep.contains("needle here"));
        assert!(!grep.contains("needle secret"));

        let denied_result = executor.execute(
            READ_FILE_TOOL_NAME,
            &serde_json::json!({"path": denied_path, "max_chars": 512}).to_string(),
        );
        assert!(denied_result
            .expect("explicit paths are unrestricted")
            .contains("needle secret"));
    }

    #[test]
    fn per_call_and_total_character_limits_are_enforced() {
        let directory = tempdir().expect("temporary directory should exist");
        let script = directory.path().join("long.sh");
        fs::write(&script, "x".repeat(20_000)).expect("test script should be written");
        let path = script.to_string_lossy().into_owned();
        let mut executor = ProviderFileToolExecutor::new(vec![path.clone()]);

        let oversized = executor.execute(
            READ_FILE_TOOL_NAME,
            &serde_json::json!({"path": path, "max_chars": MAX_TOOL_RESULT_CHARS + 1}).to_string(),
        );
        assert!(matches!(
            oversized,
            Err(ProviderFileToolError::InvalidArguments { .. })
        ));

        let args = serde_json::json!({
            "path": script.to_string_lossy(),
            "max_chars": MAX_TOOL_RESULT_CHARS
        })
        .to_string();
        let first = executor
            .execute(READ_FILE_TOOL_NAME, &args)
            .expect("first bounded read should succeed");
        assert!(first.chars().count() <= MAX_TOOL_RESULT_CHARS);
        let second = executor
            .execute(READ_FILE_TOOL_NAME, &args)
            .expect("second bounded read should use remaining budget");
        assert!(second.chars().count() <= MAX_TOOL_RESULT_CHARS);
        assert!(matches!(
            executor.execute(READ_FILE_TOOL_NAME, &args),
            Err(ProviderFileToolError::TotalResultLimitExceeded)
        ));
    }
}
