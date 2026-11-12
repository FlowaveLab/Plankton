//! A request-local result channel shared by all CLI adapters.
//! Direct JSON and explicit file references share strict result validation.
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
        format!(
            "{prompt}\n\nResult delivery: return only the requested JSON as your final answer. \
             If your work already produced the result in a file, return only \
             {{\"result_file\":\"/absolute/path/to/your/result.json\"}} instead; the host reads it. \
             Use a request-unique filename. Do not read back or copy the result into chat. \
             Files and tools are optional, not a prerequisite for returning JSON. For audit frames, \
             a referenced file may contain a JSON array including the complete frame. \
             Keep explanations in the requested fields; no analysis narration or completion recap. \
             Never include credential values."
        )
    }

    pub(super) fn read(&self, response: &str) -> Result<Option<String>, ProviderError> {
        let Some(content) = self.read_content(response)? else {
            return Ok(None);
        };
        // Decision files must be one JSON value, never several competing answers.
        serde_json::from_str::<serde_json::Value>(&content).map_err(|error| {
            ProviderError::InvalidResponse(format!("invalid result.json: {error}"))
        })?;
        Ok(Some(content))
    }

    pub(super) fn read_audit(&self, response: &str) -> Result<Option<String>, ProviderError> {
        let Some(content) = self.read_content(response)? else {
            return Ok(None);
        };
        let frames = audit_frames(&content)?;
        if frames.trim().is_empty() {
            return Err(ProviderError::InvalidResponse("empty audit result".into()));
        }
        for line in frames.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
                ProviderError::InvalidResponse(format!("invalid audit result: {error}"))
            })?;
            if !value.is_object() {
                return Err(ProviderError::InvalidResponse(
                    "audit frames must be objects".into(),
                ));
            }
        }
        Ok(Some(frames))
    }

    fn read_content(&self, response: &str) -> Result<Option<String>, ProviderError> {
        let legacy_path = self.directory.path().join("result.json");
        let reference = serde_json::from_str::<serde_json::Value>(response.trim()).ok();
        let reference = reference
            .as_ref()
            .and_then(|value| value.as_object())
            .filter(|object| object.len() == 1)
            .and_then(|object| object.get("result_file"));
        let path = if fs::symlink_metadata(&legacy_path).is_ok() {
            legacy_path.as_path()
        } else if let Some(reference) = reference {
            let path = reference
                .as_str()
                .map(Path::new)
                .filter(|path| path.is_absolute())
                .ok_or_else(|| {
                    ProviderError::InvalidResponse("result_file must be an absolute path".into())
                })?;
            path
        } else {
            return Ok(None);
        };
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
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
        assert!(first.read("").unwrap().unwrap().contains("deny"));
        assert!(second.read("").unwrap().is_none());
        first.clear().unwrap();
        assert!(first.read("").unwrap().is_none());
    }

    #[test]
    fn explicit_file_reference_is_optional_and_validated_without_fallback() {
        let root = tempfile::tempdir().unwrap();
        let output = ReviewOutput::new(root.path()).unwrap();
        assert!(output.read(r#"{"reviews":[]}"#).unwrap().is_none());
        let path = root.path().join("chosen-result.json");
        let reference = serde_json::json!({"result_file":path}).to_string();
        assert!(output.read(&reference).is_err());
        fs::write(&path, r#"{"reviews":[]}"#).unwrap();
        assert_eq!(
            output.read(&reference).unwrap().unwrap(),
            r#"{"reviews":[]}"#
        );
        fs::write(&path, "not JSON").unwrap();
        assert!(output.read(&reference).is_err());
        assert!(output.read(r#"{"result_file":"relative.json"}"#).is_err());
        #[cfg(unix)]
        {
            let link = root.path().join("link.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(output
                .read(&serde_json::json!({"result_file":link}).to_string())
                .is_err());
        }
    }

    #[test]
    fn malformed_written_result_is_not_replaced_by_conversation_text() {
        let root = tempfile::tempdir().unwrap();
        let output = ReviewOutput::new(root.path()).unwrap();
        fs::write(output.directory.path().join("result.json"), "incomplete {").unwrap();
        assert!(output.read("").is_err());
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

    #[test]
    fn audit_file_accepts_ndjson_without_relaxing_decision_file_validation() {
        let root = tempfile::tempdir().unwrap();
        let output = ReviewOutput::new(root.path()).unwrap();
        let path = output.directory.path().join("result.json");
        let frames = "{\"type\":\"node\"}\n{\"type\":\"complete\"}\n";
        fs::write(&path, frames).unwrap();
        assert_eq!(output.read_audit("").unwrap().unwrap(), frames);
        assert!(output.read("").is_err());
        fs::write(&path, "{\"type\":\"node\"}\nmalformed").unwrap();
        assert!(output.read_audit("").is_err());
        fs::write(&path, "[1]").unwrap();
        assert!(output.read_audit("").is_err());
    }
}
