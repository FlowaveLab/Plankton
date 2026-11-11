//! A request-local result channel shared by all CLI adapters.
//! Conversation text remains audit evidence, never a substitute for a written result.
use std::{fs, io::Read, path::Path};

use crate::ProviderError;

const MAX_RESULT_BYTES: u64 = 1024 * 1024;

pub(super) struct ReviewOutput {
    directory: tempfile::TempDir,
}

impl ReviewOutput {
    pub(super) fn new(cwd: &Path) -> Result<Self, ProviderError> {
        let directory = tempfile::Builder::new()
            .prefix("review-result-")
            .tempdir_in(cwd)
            .map_err(|error| {
                ProviderError::Transport(format!("create review result directory: {error}"))
            })?;
        Ok(Self { directory })
    }

    pub(super) fn prompt(&self, prompt: &str) -> String {
        let path = self.directory.path().join("result.json");
        format!(
            "{prompt}\n\nResult delivery: use your normal file-writing tool to save the requested JSON to {}. \
             This file is the authoritative result for this request; ordinary progress messages and your final \
             acknowledgement are not parsed. Pretty-printed JSON is fine. For audit frames, write one JSON array \
             containing the requested frames, including the final complete frame. Do not copy the result back into \
             your response. Do not put credential values in this file. If file writing is unavailable, return only \
             the requested JSON as your final answer.",
            serde_json::to_string(&path.to_string_lossy()).expect("path serializes")
        )
    }

    pub(super) fn read(&self) -> Result<Option<String>, ProviderError> {
        let path = self.directory.path().join("result.json");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(ProviderError::Transport(format!(
                    "read review result: {error}"
                )))
            }
        };
        if !metadata.is_file() || metadata.len() > MAX_RESULT_BYTES {
            return Err(ProviderError::InvalidResponse(
                "review result must be a regular file of at most 1 MiB".into(),
            ));
        }
        let mut content = String::new();
        fs::File::open(path)
            .and_then(|file| file.take(MAX_RESULT_BYTES + 1).read_to_string(&mut content))
            .map_err(|error| {
                ProviderError::InvalidResponse(format!("read review result UTF-8: {error}"))
            })?;
        if content.len() as u64 > MAX_RESULT_BYTES {
            return Err(ProviderError::InvalidResponse(
                "review result exceeds 1 MiB".into(),
            ));
        }
        // Reject malformed files instead of falling back to an unrelated answer.
        serde_json::from_str::<serde_json::Value>(&content).map_err(|error| {
            ProviderError::InvalidResponse(format!("invalid result.json: {error}"))
        })?;
        Ok(Some(content))
    }

    pub(super) fn clear(&self) -> Result<(), ProviderError> {
        match fs::remove_file(self.directory.path().join("result.json")) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ProviderError::Transport(format!(
                "reset review result: {error}"
            ))),
        }
    }
}

pub(super) fn audit_frames(content: &str) -> Result<String, ProviderError> {
    let trimmed = content.trim();
    if !trimmed.starts_with('[') {
        return Ok(content.to_string());
    }
    let frames: Vec<serde_json::Value> = serde_json::from_str(trimmed).map_err(|error| {
        ProviderError::InvalidResponse(format!("invalid audit JSON array: {error}"))
    })?;
    let mut lines = String::new();
    for frame in frames {
        if !frame.is_object() {
            return Err(ProviderError::InvalidResponse(
                "audit frames must be objects".into(),
            ));
        }
        lines.push_str(&serde_json::to_string(&frame).expect("JSON value serializes"));
        lines.push('\n');
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_requests_have_independent_results_and_no_stale_retry() {
        let root = tempfile::tempdir().unwrap();
        let first = ReviewOutput::new(root.path()).unwrap();
        let second = ReviewOutput::new(root.path()).unwrap();
        fs::write(
            first.directory.path().join("result.json"),
            r#"{"suggested_decision":"deny"}"#,
        )
        .unwrap();
        assert!(first.read().unwrap().unwrap().contains("deny"));
        assert!(second.read().unwrap().is_none());
        first.clear().unwrap();
        assert!(first.read().unwrap().is_none());
    }

    #[test]
    fn malformed_written_result_is_not_replaced_by_conversation_text() {
        let root = tempfile::tempdir().unwrap();
        let output = ReviewOutput::new(root.path()).unwrap();
        fs::write(output.directory.path().join("result.json"), "incomplete {").unwrap();
        assert!(output.read().is_err());
    }

    #[test]
    fn pretty_printed_audit_preserves_frame_contents_and_order() {
        let input = r#"[
          {"type":"surface","surface":"network","annotations":[]},
          {"type":"complete"}
        ]"#;
        let lines = audit_frames(input).unwrap();
        let parsed: Vec<serde_json::Value> = lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            serde_json::Value::Array(parsed),
            serde_json::from_str::<serde_json::Value>(input).unwrap()
        );
        assert!(audit_frames("[1]").is_err());
    }
}
