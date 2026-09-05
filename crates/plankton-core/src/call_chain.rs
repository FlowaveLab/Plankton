use std::{
    borrow::Cow,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sysinfo::{get_current_pid, Pid, System};

const MAX_PREVIEW_FILE_BYTES: u64 = 256 * 1024;
const MAX_PREVIEW_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallChainNodeSource {
    OsProbe,
    BestEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallChainPreviewStatus {
    PathOnly,
    PreviewReady,
    NotPreviewable,
    FileMissing,
    UnsupportedEncoding,
    BinaryFile,
    TooLarge,
    IoError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallChainNode {
    pub pid: Option<u32>,
    pub ppid: Option<u32>,
    pub process_name: Option<String>,
    pub executable_path: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    pub resolved_file_path: Option<String>,
    pub source: CallChainNodeSource,
    pub previewable: bool,
    pub preview_status: CallChainPreviewStatus,
    pub preview_text: Option<String>,
    pub preview_error: Option<String>,
}

impl CallChainNode {
    pub fn best_effort_path(path: impl Into<String>) -> Self {
        let path = path.into();
        let has_path = !path.trim().is_empty();
        Self {
            pid: None,
            ppid: None,
            process_name: None,
            executable_path: None,
            argv: Vec::new(),
            resolved_file_path: has_path.then_some(path),
            source: CallChainNodeSource::BestEffort,
            previewable: has_path,
            preview_status: if has_path {
                CallChainPreviewStatus::PathOnly
            } else {
                CallChainPreviewStatus::NotPreviewable
            },
            preview_text: None,
            preview_error: None,
        }
    }

    pub fn prompt_display_path(&self) -> Option<&str> {
        self.resolved_file_path
            .as_deref()
            .or(self.executable_path.as_deref())
            .or(self.process_name.as_deref())
    }

    pub fn previewable_path(&self) -> Option<&str> {
        self.resolved_file_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
    }

    pub fn clear_preview_content(&mut self) {
        self.preview_text = None;
        self.preview_error = None;
        if self.previewable {
            self.preview_status = CallChainPreviewStatus::PathOnly;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallChainReadFileResult {
    pub path: String,
    pub encoding: String,
    pub truncated: bool,
    pub bytes_returned: usize,
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CallChainError {
    #[error("failed to inspect current process: {0}")]
    CurrentProcess(String),
    #[error("requested path {path} is not on the collected call-chain allowlist")]
    PathNotAllowlisted { path: String },
    #[error("requested path {path} is not previewable")]
    PathNotPreviewable { path: String },
    #[error("requested path {path} does not exist")]
    FileMissing { path: String },
    #[error("requested path {path} exceeded the preview size limit")]
    TooLarge { path: String },
    #[error("requested path {path} appears to be binary")]
    BinaryFile { path: String },
    #[error("requested path {path} used an unsupported encoding")]
    UnsupportedEncoding { path: String },
    #[error("failed to read {path}: {message}")]
    Io { path: String, message: String },
}

pub fn collect_runtime_call_chain() -> Result<Vec<CallChainNode>, CallChainError> {
    let mut system = System::new_all();
    system.refresh_all();

    let current_pid =
        get_current_pid().map_err(|error| CallChainError::CurrentProcess(error.to_string()))?;
    let mut next_pid = Some(current_pid);
    let mut collected = Vec::new();

    while let Some(pid) = next_pid {
        let Some(process) = system.process(pid) else {
            break;
        };
        collected.push(CallChainNode {
            pid: Some(process.pid().as_u32()),
            ppid: process.parent().map(Pid::as_u32),
            process_name: os_to_string(process.name()),
            executable_path: process.exe().map(path_to_string),
            argv: process
                .cmd()
                .iter()
                .map(|value| os_to_string_lossy(value.as_os_str()))
                .collect(),
            resolved_file_path: resolve_file_path(process),
            source: CallChainNodeSource::OsProbe,
            previewable: false,
            preview_status: CallChainPreviewStatus::NotPreviewable,
            preview_text: None,
            preview_error: None,
        });
        next_pid = process.parent();
    }

    collected.reverse();
    for node in &mut collected {
        let previewable = node.previewable_path().is_some();
        node.previewable = previewable;
        node.preview_status = if previewable {
            CallChainPreviewStatus::PathOnly
        } else {
            CallChainPreviewStatus::NotPreviewable
        };
    }

    Ok(collected)
}

pub fn derive_script_path(call_chain: &[CallChainNode]) -> Option<String> {
    call_chain
        .iter()
        .rev()
        .find_map(|node| node.resolved_file_path.clone())
}

pub fn derive_embedded_script_reference(call_chain: &[CallChainNode]) -> Option<String> {
    call_chain.iter().rev().find_map(|node| {
        let interpreter = node
            .executable_path
            .as_deref()
            .or(node.process_name.as_deref())
            .and_then(|value| Path::new(value).file_name())
            .and_then(OsStr::to_str)
            .map(|value| value.to_ascii_lowercase());
        matches!(interpreter.as_deref(), Some("bash" | "sh" | "zsh" | "fish"))
            .then(|| embedded_shell_script_reference(&node.argv))
            .flatten()
    })
}

pub fn prompt_call_chain_paths(call_chain: &[CallChainNode]) -> Vec<String> {
    call_chain
        .iter()
        .filter_map(|node| node.prompt_display_path().map(ToOwned::to_owned))
        .collect()
}

pub fn preview_call_chain_for_desktop(call_chain: &mut [CallChainNode]) {
    for node in call_chain {
        node.preview_text = None;
        node.preview_error = None;

        let Some(path) = node.previewable_path().map(ToOwned::to_owned) else {
            node.previewable = false;
            node.preview_status = CallChainPreviewStatus::NotPreviewable;
            continue;
        };

        match read_preview_file(Path::new(&path)) {
            Ok(result) => {
                node.previewable = true;
                node.preview_status = CallChainPreviewStatus::PreviewReady;
                node.preview_text = Some(result.content);
                node.preview_error = None;
            }
            Err(CallChainError::FileMissing { .. }) => {
                node.previewable = true;
                node.preview_status = CallChainPreviewStatus::FileMissing;
                node.preview_error = Some("Preview unavailable: file no longer exists".to_string());
            }
            Err(CallChainError::TooLarge { .. }) => {
                node.previewable = true;
                node.preview_status = CallChainPreviewStatus::TooLarge;
                node.preview_error =
                    Some("Preview unavailable: file exceeded the preview size limit".to_string());
            }
            Err(CallChainError::BinaryFile { .. }) => {
                node.previewable = false;
                node.preview_status = CallChainPreviewStatus::BinaryFile;
                node.preview_error =
                    Some("Preview unavailable: file does not look like plain text".to_string());
            }
            Err(CallChainError::UnsupportedEncoding { .. }) => {
                node.previewable = true;
                node.preview_status = CallChainPreviewStatus::UnsupportedEncoding;
                node.preview_error =
                    Some("Preview unavailable: file encoding is not supported yet".to_string());
            }
            Err(CallChainError::Io { message, .. }) => {
                node.previewable = true;
                node.preview_status = CallChainPreviewStatus::IoError;
                node.preview_error = Some(format!("Preview unavailable: {message}"));
            }
            Err(CallChainError::PathNotAllowlisted { .. })
            | Err(CallChainError::PathNotPreviewable { .. })
            | Err(CallChainError::CurrentProcess(_)) => {
                node.previewable = false;
                node.preview_status = CallChainPreviewStatus::NotPreviewable;
                node.preview_error = Some("Preview unavailable".to_string());
            }
        }
    }
}

pub fn read_allowlisted_call_chain_file(
    call_chain: &[CallChainNode],
    requested_path: &str,
) -> Result<CallChainReadFileResult, CallChainError> {
    let allowed_paths = call_chain
        .iter()
        .filter_map(|node| node.previewable_path().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    read_allowlisted_paths_file(&allowed_paths, requested_path)
}

pub fn read_allowlisted_paths_file(
    allowed_paths: &[String],
    requested_path: &str,
) -> Result<CallChainReadFileResult, CallChainError> {
    let requested_path = requested_path.trim();
    if requested_path.is_empty() {
        return Err(CallChainError::PathNotAllowlisted {
            path: requested_path.to_string(),
        });
    }

    let canonical_requested = normalize_path_for_match(Path::new(requested_path));
    let allowed = allowed_paths
        .iter()
        .any(|path| normalize_path_for_match(Path::new(path)) == canonical_requested);

    if !allowed {
        return Err(CallChainError::PathNotAllowlisted {
            path: requested_path.to_string(),
        });
    }

    read_preview_file(Path::new(requested_path))
}

/// Unrestricted reviewer read; desktop preview still uses its separate allowlisted API.
pub(crate) fn read_review_file(path: &str) -> Result<CallChainReadFileResult, CallChainError> {
    let bytes = fs::read(path).map_err(|error| CallChainError::Io {
        path: path.to_string(),
        message: error.to_string(),
    })?;
    let (encoding, content) =
        decode_text(&bytes).ok_or_else(|| CallChainError::UnsupportedEncoding {
            path: path.to_string(),
        })?;
    Ok(CallChainReadFileResult {
        path: path.to_string(),
        encoding,
        truncated: false,
        bytes_returned: content.len(),
        content,
    })
}

fn resolve_file_path(process: &sysinfo::Process) -> Option<String> {
    let cwd = process.cwd();
    let argv = process.cmd();
    let executable_basename = process
        .exe()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .map(|value| value.to_ascii_lowercase());
    let argv_strings = argv
        .iter()
        .map(|value| os_to_string_lossy(value.as_os_str()))
        .collect::<Vec<_>>();

    infer_script_path(&argv_strings, cwd, executable_basename.as_deref())
}

fn infer_script_path(
    argv: &[String],
    cwd: Option<&Path>,
    executable_basename: Option<&str>,
) -> Option<String> {
    if argv.is_empty() {
        return None;
    }

    let inferred_interpreter = argv.first().and_then(|value| {
        Path::new(value.as_str())
            .file_name()
            .and_then(OsStr::to_str)
            .map(|value| value.to_ascii_lowercase())
    });
    let interpreter = executable_basename.or(inferred_interpreter.as_deref());

    match interpreter {
        Some("bash") | Some("sh") | Some("zsh") | Some("fish") => {
            return infer_shell_script_path(argv, cwd);
        }
        Some("python") | Some("python3") | Some("pythonw") => {
            return infer_python_script_path(argv, cwd);
        }
        Some("pwsh") | Some("powershell") | Some("powershell.exe") | Some("pwsh.exe") => {
            return find_flagged_script_path(argv, &["-file"], cwd)
                .or_else(|| find_first_positional_path(argv.iter().skip(1), cwd));
        }
        Some("cmd") | Some("cmd.exe") => {
            return find_flagged_script_path(argv, &["/c", "/k"], cwd)
                .or_else(|| find_first_positional_path(argv.iter().skip(1), cwd));
        }
        _ => {}
    }

    if let Some(first) = argv
        .first()
        .and_then(|value| resolve_candidate_path(cwd, value))
    {
        if is_probably_script_path(&first) {
            return Some(first);
        }
    }

    find_first_positional_path(argv.iter().skip(1), cwd)
}

fn infer_shell_script_path(argv: &[String], cwd: Option<&Path>) -> Option<String> {
    let mut positionals = argv.iter().skip(1).peekable();

    while let Some(value) = positionals.peek() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            positionals.next();
            continue;
        }

        if trimmed == "--" {
            positionals.next();
            break;
        }

        if !trimmed.starts_with('-') {
            break;
        }

        if matches!(trimmed, "-c" | "-lc" | "-xc" | "-xec") {
            return infer_embedded_shell_script_path(argv, cwd);
        }

        positionals.next();
    }

    find_first_positional_path(positionals, cwd)
}

fn infer_embedded_shell_script_path(argv: &[String], cwd: Option<&Path>) -> Option<String> {
    let candidate = embedded_shell_script_reference(argv)?;
    resolve_existing_candidate_path(cwd, &candidate)
}

fn embedded_shell_script_reference(argv: &[String]) -> Option<String> {
    argv.iter().skip(2).find_map(|command| {
        let words = shell_words::split(command).ok()?;
        words.into_iter().find_map(|word| {
            let candidate = word
                .trim_matches(|character: char| matches!(character, '(' | ')' | ';' | '|' | '&'));
            is_probably_script_path(candidate).then(|| candidate.to_string())
        })
    })
}

fn infer_python_script_path(argv: &[String], cwd: Option<&Path>) -> Option<String> {
    let mut positionals = argv.iter().skip(1).peekable();

    while let Some(value) = positionals.peek() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            positionals.next();
            continue;
        }

        if trimmed == "--" {
            positionals.next();
            break;
        }

        if !trimmed.starts_with('-') {
            break;
        }

        if matches!(trimmed, "-c" | "-m") {
            return None;
        }

        positionals.next();
    }

    find_first_positional_path(positionals, cwd)
}

fn find_flagged_script_path(argv: &[String], flags: &[&str], cwd: Option<&Path>) -> Option<String> {
    let flags = flags
        .iter()
        .map(|flag| flag.to_ascii_lowercase())
        .collect::<Vec<_>>();
    argv.iter().enumerate().find_map(|(index, value)| {
        let value = value.trim().to_ascii_lowercase();
        if flags.iter().any(|flag| flag == &value) {
            argv.get(index + 1)
                .and_then(|candidate| resolve_candidate_path(cwd, candidate))
        } else {
            None
        }
    })
}

fn find_first_positional_path<'a>(
    values: impl Iterator<Item = &'a String>,
    cwd: Option<&Path>,
) -> Option<String> {
    values
        .filter(|value| !value.trim().is_empty())
        .filter(|value| !value.starts_with('-'))
        .find_map(|value| resolve_existing_candidate_path(cwd, value))
}

fn resolve_candidate_path(cwd: Option<&Path>, candidate: &str) -> Option<String> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = cwd {
        cwd.join(path)
    } else {
        PathBuf::from(path)
    };

    Some(
        normalize_path_for_match(&resolved)
            .to_string_lossy()
            .into_owned(),
    )
}

fn resolve_existing_candidate_path(cwd: Option<&Path>, candidate: &str) -> Option<String> {
    let resolved = resolve_candidate_path(cwd, candidate)?;
    Path::new(&resolved).is_file().then_some(resolved)
}

fn normalize_path_for_match(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn is_probably_script_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(OsStr::to_str)
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bash"
                    | "sh"
                    | "zsh"
                    | "fish"
                    | "py"
                    | "ps1"
                    | "bat"
                    | "cmd"
                    | "js"
                    | "mjs"
                    | "cjs"
                    | "ts"
                    | "tsx"
                    | "rb"
                    | "pl"
                    | "lua"
            )
        })
        .unwrap_or(false)
}

fn read_preview_file(path: &Path) -> Result<CallChainReadFileResult, CallChainError> {
    let path_label = path.to_string_lossy().into_owned();
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CallChainError::FileMissing {
                path: path_label.clone(),
            }
        } else {
            CallChainError::Io {
                path: path_label.clone(),
                message: error.to_string(),
            }
        }
    })?;

    if !metadata.is_file() {
        return Err(CallChainError::PathNotPreviewable { path: path_label });
    }

    if metadata.len() > MAX_PREVIEW_FILE_BYTES {
        return Err(CallChainError::TooLarge { path: path_label });
    }

    let bytes = fs::read(path).map_err(|error| CallChainError::Io {
        path: path_label.clone(),
        message: error.to_string(),
    })?;

    if looks_binary(&bytes) {
        return Err(CallChainError::BinaryFile { path: path_label });
    }

    let (encoding, mut content) =
        decode_text(&bytes).ok_or_else(|| CallChainError::UnsupportedEncoding {
            path: path_label.clone(),
        })?;
    let truncated = truncate_utf8(&mut content, MAX_PREVIEW_TEXT_BYTES);
    let bytes_returned = content.len();

    Ok(CallChainReadFileResult {
        path: path_label,
        encoding,
        truncated,
        bytes_returned,
        content,
    })
}

fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return true;
    }

    let control_count = bytes
        .iter()
        .filter(|byte| matches!(byte, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F))
        .count();
    !bytes.is_empty() && (control_count as f64 / bytes.len() as f64) > 0.2
}

fn decode_text(bytes: &[u8]) -> Option<(String, String)> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec())
            .ok()
            .map(|content| ("utf-8-bom".to_string(), content));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], true).map(|content| ("utf-16le".to_string(), content));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], false).map(|content| ("utf-16be".to_string(), content));
    }

    String::from_utf8(bytes.to_vec())
        .ok()
        .map(|content| ("utf-8".to_string(), content))
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }

    let units = bytes
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();

    String::from_utf16(&units).ok()
}

fn truncate_utf8(content: &mut String, limit: usize) -> bool {
    if content.len() <= limit {
        return false;
    }

    let mut boundary = limit;
    while !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    content.truncate(boundary);
    true
}

fn os_to_string(value: &OsStr) -> Option<String> {
    let text = value.to_string_lossy().trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn os_to_string_lossy(value: &OsStr) -> String {
    match value.to_string_lossy() {
        Cow::Borrowed(value) => value.to_string(),
        Cow::Owned(value) => value,
    }
}

fn path_to_string(path: &Path) -> String {
    normalize_path_for_match(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{
        collect_runtime_call_chain, decode_text, derive_embedded_script_reference,
        infer_script_path, normalize_path_for_match, prompt_call_chain_paths,
        read_allowlisted_call_chain_file, truncate_utf8, CallChainNode, CallChainPreviewStatus,
    };

    #[test]
    fn runtime_chain_includes_the_requesting_process_as_its_final_node() {
        let current_pid = sysinfo::get_current_pid().expect("current pid should resolve");
        let call_chain =
            collect_runtime_call_chain().expect("runtime call chain should be collected");

        assert_eq!(
            call_chain.last().and_then(|node| node.pid),
            Some(current_pid.as_u32())
        );
    }

    #[test]
    fn derives_prompt_paths_from_structured_nodes() {
        let paths = prompt_call_chain_paths(&[
            CallChainNode::best_effort_path("/tmp/outer.sh"),
            CallChainNode {
                pid: Some(123),
                ppid: Some(1),
                process_name: Some("bash".to_string()),
                executable_path: Some("/bin/bash".to_string()),
                argv: vec!["bash".to_string()],
                resolved_file_path: None,
                source: super::CallChainNodeSource::OsProbe,
                previewable: false,
                preview_status: CallChainPreviewStatus::NotPreviewable,
                preview_text: None,
                preview_error: None,
            },
        ]);

        assert_eq!(
            paths,
            vec!["/tmp/outer.sh".to_string(), "/bin/bash".to_string()]
        );
    }

    #[test]
    fn reads_allowlisted_utf8_preview_and_truncates_large_text() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("script.sh");
        let content = "echo test\n".repeat(10_000);
        fs::write(&path, content).expect("script file should be written");
        let call_chain = vec![CallChainNode::best_effort_path(path.display().to_string())];

        let result = read_allowlisted_call_chain_file(&call_chain, &path.display().to_string())
            .expect("allowlisted preview should load");

        assert_eq!(result.encoding, "utf-8");
        assert!(result.truncated);
        assert!(!result.content.is_empty());
        assert!(result.bytes_returned <= 64 * 1024);
    }

    #[test]
    fn rejects_non_allowlisted_paths() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("script.sh");
        fs::write(&path, "echo test\n").expect("script file should be written");
        let call_chain = vec![CallChainNode::best_effort_path("/tmp/other.sh")];

        let error = read_allowlisted_call_chain_file(&call_chain, &path.display().to_string())
            .expect_err("non-allowlisted path should fail");

        assert!(error.to_string().contains("allowlist"));
    }

    #[test]
    fn decodes_utf16_text() {
        let bytes = vec![0xFF, 0xFE, b'a', 0, b'b', 0];
        let (encoding, content) = decode_text(&bytes).expect("utf16 text should decode");

        assert_eq!(encoding, "utf-16le");
        assert_eq!(content, "ab");
    }

    #[test]
    fn truncates_utf8_on_character_boundary() {
        let mut value = "你好hello".repeat(20_000);
        let truncated = truncate_utf8(&mut value, 1024);

        assert!(truncated);
        assert!(value.len() <= 1024);
        assert!(value.is_char_boundary(value.len()));
    }

    #[test]
    fn infers_shell_script_from_absolute_path_argument() {
        let temp = tempdir().expect("temp directory should be created");
        let script = temp.path().join("test.sh");
        fs::write(&script, "#!/usr/bin/env bash\necho test\n")
            .expect("script file should be written");

        let inferred = infer_script_path(
            &["/bin/bash".to_string(), script.display().to_string()],
            None,
            Some("bash"),
        );

        assert_eq!(
            inferred
                .as_deref()
                .map(|path| normalize_path_for_match(Path::new(path))),
            Some(normalize_path_for_match(script.as_path()))
        );
    }

    #[test]
    fn does_not_treat_shell_command_string_as_script_path() {
        let inferred = infer_script_path(
            &[
                "/bin/bash".to_string(),
                "-lc".to_string(),
                "echo hello".to_string(),
            ],
            None,
            Some("bash"),
        );

        assert_eq!(inferred, None);
    }

    #[test]
    fn infers_python_script_embedded_in_shell_wrapper_argument() {
        let temp = tempdir().expect("temp directory should be created");
        let scripts = temp.path().join("scripts");
        fs::create_dir(&scripts).expect("scripts directory should be created");
        let script = scripts.join("replay.py");
        fs::write(&script, "print('ok')\n").expect("python file should be written");

        let inferred = infer_script_path(
            &[
                "/bin/zsh".to_string(),
                "-c".to_string(),
                "builtin eval \"$1\"".to_string(),
                "--".to_string(),
                "set +x; token_source | env SERVICE_TOKEN=\"$token\" uv run python scripts/replay.py --dry-run"
                    .to_string(),
            ],
            Some(temp.path()),
            Some("zsh"),
        );

        assert_eq!(
            inferred
                .as_deref()
                .map(|path| normalize_path_for_match(Path::new(path))),
            Some(normalize_path_for_match(script.as_path()))
        );

        let mut shell = CallChainNode::best_effort_path("zsh");
        shell.process_name = Some("zsh".to_string());
        shell.executable_path = Some("/bin/zsh".to_string());
        shell.argv = vec![
            "/bin/zsh".to_string(),
            "-c".to_string(),
            "builtin eval \"$1\"".to_string(),
            "--".to_string(),
            "uv run python scripts/replay.py --dry-run".to_string(),
        ];
        assert_eq!(
            derive_embedded_script_reference(&[shell]).as_deref(),
            Some("scripts/replay.py")
        );
    }

    #[test]
    fn infers_python_script_from_explicit_script_argument() {
        let temp = tempdir().expect("temp directory should be created");
        let script = temp.path().join("tool.py");
        fs::write(&script, "print('ok')\n").expect("python file should be written");

        let inferred = infer_script_path(
            &[
                "/usr/bin/python3".to_string(),
                script.display().to_string(),
                "--flag".to_string(),
            ],
            None,
            Some("python3"),
        );

        assert_eq!(
            inferred
                .as_deref()
                .map(|path| normalize_path_for_match(Path::new(path))),
            Some(normalize_path_for_match(script.as_path()))
        );
    }
}
