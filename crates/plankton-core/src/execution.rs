//! Daemon-owned command evidence. Inline/stdin source is executed from the captured
//! bytes; named files retain their original runtime semantics and are hash checked
//! immediately before launch. This is not a filesystem or process sandbox.
mod shell;

use crate::{CallChainNode, CallChainNodeSource, CallChainPreviewStatus};
use plankton_protocol::resources::ResourceExecutionRequest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_EXECUTABLE_BYTES: usize = 128 * 1024 * 1024;
const MAX_NODES: usize = 32;
pub const EXECUTION_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
pub const CONSUMER_ENVIRONMENT: [(&str, &str); 2] =
    [("PATH", EXECUTION_PATH), ("LANG", "en_US.UTF-8")];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSource {
    pub node_index: usize,
    pub source_id: String,
    pub path: Option<String>,
    pub language: String,
    pub kind: String,
    pub sha256: String,
    pub content: String,
    pub binding: String,
}

#[derive(Clone)]
struct BoundFile {
    path: PathBuf,
    target: PathBuf,
    sha256: String,
    maximum: usize,
}

#[derive(Clone)]
pub struct ExecutionPlan {
    /// Compatibility field: first file source, or the executable for an inline/native command.
    pub script_path: PathBuf,
    /// Absolute lexical launch path; do not replace a venv symlink with its canonical target.
    pub interpreter_path: PathBuf,
    pub cwd: PathBuf,
    pub arguments: Vec<String>,
    pub timeout_seconds: u16,
    pub credential_env: Option<String>,
    pub command_stdin: Option<String>,
    stdin_is_source: bool,
    files: Vec<BoundFile>,
    nodes: Vec<CallChainNode>,
    sources: Vec<ExecutionSource>,
    output_paths: Vec<PathBuf>,
}

impl std::fmt::Debug for ExecutionPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionPlan")
            .field("program", &self.interpreter_path)
            .field("source_count", &self.sources.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Python,
    Node,
    Shell,
    Native,
}

fn kind(program: &str) -> Option<Kind> {
    let name = Path::new(program).file_name()?.to_str()?;
    if name == "python"
        || name.strip_prefix("python").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        })
    {
        return Some(Kind::Python);
    }
    match name {
        "node" | "nodejs" => Some(Kind::Node),
        "sh" | "bash" | "zsh" => Some(Kind::Shell),
        "curl" | "wget" | "git" | "gh" | "aws" | "gcloud" | "kubectl" => Some(Kind::Native),
        _ => None,
    }
}

/// A pure CLI hint. The daemon independently repeats all admission checks.
/// Shell -c/heredoc source is already present in argv and must not consume CLI stdin.
pub fn requires_source_stdin(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let Some(language) = kind(program) else {
        return false;
    };
    if language == Kind::Native {
        return false;
    }
    let mut i = 1;
    while i < command.len() {
        let arg = &command[i];
        if arg == "--" || (language == Kind::Shell && arg == "-") {
            i += 1;
            return if language == Kind::Shell {
                command.get(i).is_none()
            } else {
                command.get(i).is_none_or(|value| value == "-")
            };
        }
        if arg == "-" {
            return true;
        }
        match language {
            Kind::Python => {
                if arg == "-c" || arg == "-m" {
                    return false;
                }
                if arg == "-W" || arg == "-X" || arg == "--check-hash-based-pycs" {
                    i += 2;
                    continue;
                }
            }
            Kind::Node => {
                if ["-e", "-p", "--eval", "--print"].contains(&arg.as_str())
                    || arg.starts_with("--eval=")
                    || arg.starts_with("--print=")
                {
                    return false;
                }
                if arg == "--input-type" {
                    i += 2;
                    continue;
                }
            }
            Kind::Shell => {
                if arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c') {
                    return false;
                }
                if arg.starts_with('-') && !arg.starts_with("--") && arg.contains('s') {
                    return true;
                }
            }
            Kind::Native => return false,
        }
        if !arg.starts_with('-') {
            return false;
        }
        i += 1;
    }
    language != Kind::Native
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_limited(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| format!("execution file is not readable: {}", path.display()))?
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "execution file read failed")?;
    if bytes.len() > maximum {
        return Err("execution file exceeds the supported size limit".into());
    }
    Ok(bytes)
}
fn text_source(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.len() > MAX_SOURCE_BYTES || bytes.contains(&0) {
        return Err("execution source/input must be UTF-8 without NUL and at most 64 KiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "execution source/input must use UTF-8".into())
}
fn absolute_file(path: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    if !path.is_file() {
        return Err(format!(
            "execution entrypoint must be a readable regular file: {}",
            path.display()
        ));
    }
    Ok(path)
}
fn resolve_program(program: &str, cwd: &Path) -> Result<PathBuf, String> {
    if program.contains('/') {
        return absolute_file(Path::new(program), cwd);
    }
    for directory in EXECUTION_PATH.split(':') {
        let candidate = Path::new(directory).join(program);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "program {program:?} was not found in the recorded execution PATH"
    ))
}
fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}
fn runtime_env(name: &str) -> bool {
    matches!(
        name,
        "PATH"
            | "HOME"
            | "SHELL"
            | "TMPDIR"
            | "LANG"
            | "BASH_ENV"
            | "ENV"
            | "SHELLOPTS"
            | "BASHOPTS"
            | "IFS"
            | "CDPATH"
            | "ZDOTDIR"
            | "NODE_OPTIONS"
            | "NODE_PATH"
            | "GCONV_PATH"
            | "GLIBC_TUNABLES"
            | "PERL5OPT"
            | "RUBYOPT"
            | "RUBYLIB"
            | "JAVA_TOOL_OPTIONS"
            | "JDK_JAVA_OPTIONS"
            | "_JAVA_OPTIONS"
    ) || name.starts_with("PYTHON")
        || name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || name.starts_with("LC_")
}

impl ExecutionPlan {
    pub fn prepare(request: &ResourceExecutionRequest) -> Result<Self, String> {
        if request.resource_id.trim().is_empty()
            || request.reason.trim().is_empty()
            || request.requested_by.trim().is_empty()
        {
            return Err("resource_id, reason and requested_by are required".into());
        }
        if !(1..=300).contains(&request.timeout_seconds) {
            return Err("timeout must be 1..=300 seconds".into());
        }
        let cwd = Path::new(&request.cwd);
        if !cwd.is_absolute() {
            return Err("cwd must be an absolute directory".into());
        }
        let cwd = fs::canonicalize(cwd).map_err(|_| "cwd does not exist")?;
        if !cwd.is_dir() {
            return Err("cwd must be a directory".into());
        }
        if let Some(name) = &request.credential_env {
            if !valid_env_name(name) || runtime_env(name) {
                return Err("credential environment variable must be a POSIX name and cannot control command lookup or runtime startup".into());
            }
        }
        if request.command_stdin.is_some() && request.credential_env.is_none() {
            return Err("source or business stdin requires --env; credential stdin cannot also carry program/input bytes".into());
        }
        if let Some(input) = &request.command_stdin {
            text_source(input.as_bytes().to_vec())?;
        }
        let command = if request.command.is_empty() {
            if request.script_path.is_empty() || request.interpreter_path.is_empty() {
                return Err("provide a command, or a script and interpreter".into());
            }
            let mut command = vec![
                request.interpreter_path.clone(),
                absolute_file(Path::new(&request.script_path), &cwd)?
                    .display()
                    .to_string(),
            ];
            command.extend(request.arguments.clone());
            command
        } else {
            if !request.script_path.is_empty()
                || !request.interpreter_path.is_empty()
                || !request.arguments.is_empty()
            {
                return Err(
                    "command cannot be combined with legacy script/interpreter/arguments fields"
                        .into(),
                );
            }
            request.command.clone()
        };
        if command.len() > 128
            || command.iter().any(|arg| arg.contains('\0'))
            || command.iter().map(String::len).sum::<usize>() > 128 * 1024
        {
            return Err("command arguments exceed supported limits".into());
        }
        let mut plan = Self {
            script_path: PathBuf::new(),
            interpreter_path: PathBuf::new(),
            cwd: cwd.clone(),
            arguments: Vec::new(),
            timeout_seconds: request.timeout_seconds,
            credential_env: request.credential_env.clone(),
            command_stdin: request.command_stdin.clone(),
            stdin_is_source: requires_source_stdin(&command),
            files: Vec::new(),
            nodes: Vec::new(),
            sources: Vec::new(),
            output_paths: Vec::new(),
        };
        plan.analyze(&command, &cwd, request.command_stdin.as_deref(), 0)?;
        for output in &plan.output_paths {
            if plan.files.iter().any(|file| &file.target == output) {
                return Err("shell output would replace a collected script or executable before it runs; use a separate prepared entrypoint".into());
            }
        }
        let node = &plan.nodes[0];
        plan.interpreter_path =
            PathBuf::from(node.executable_path.as_ref().expect("analyzed executable"));
        plan.arguments = command[1..].to_vec();
        plan.script_path = plan
            .sources
            .iter()
            .find_map(|source| source.path.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| plan.interpreter_path.clone());
        Ok(plan)
    }

    fn bind_file(&mut self, path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
        let target = fs::canonicalize(path)
            .map_err(|_| format!("execution file is unavailable: {}", path.display()))?;
        if !target.is_file() {
            return Err("execution files must be regular files".into());
        }
        let bytes = read_limited(path, maximum)?;
        if !self.files.iter().any(|file| file.path == path) {
            self.files.push(BoundFile {
                path: path.to_path_buf(),
                target,
                sha256: digest(&bytes),
                maximum,
            });
        }
        Ok(bytes)
    }

    fn verify_program(&mut self, program: &Path, language: Kind) -> Result<(), String> {
        let bytes = self.bind_file(program, MAX_EXECUTABLE_BYTES)?;
        let fingerprint = digest(&bytes);
        let name = program
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("program name is not UTF-8")?;
        let mut names = vec![name];
        match language {
            Kind::Python => names.extend(["python3", "python"]),
            Kind::Node => names.extend(["node", "nodejs"]),
            _ => {}
        }
        let trusted = EXECUTION_PATH.split(':').any(|directory| {
            names.iter().any(|name| {
                read_limited(&Path::new(directory).join(name), MAX_EXECUTABLE_BYTES)
                    .is_ok_and(|candidate| digest(&candidate) == fingerprint)
            })
        });
        if !trusted {
            return Err(format!("{} does not match an installed recognized runtime/tool; renamed or unknown executables cannot be approved",program.display()));
        }
        Ok(())
    }

    fn add_source(
        &mut self,
        node: usize,
        language: Kind,
        source_kind: &str,
        path: Option<PathBuf>,
        content: String,
    ) -> Result<(), String> {
        text_source(content.as_bytes().to_vec())?;
        let source_id = path
            .as_ref()
            .map(|path| format!("file:{}", path.display()))
            .unwrap_or_else(|| format!("execution:{node}:{source_kind}:{}", self.sources.len()));
        let source = ExecutionSource {
            node_index: node,
            source_id,
            path: path.as_ref().map(|path| path.display().to_string()),
            language: match language {
                Kind::Python => "python",
                Kind::Node => "javascript",
                Kind::Shell => "shell",
                Kind::Native => "text",
            }
            .into(),
            kind: source_kind.into(),
            sha256: digest(content.as_bytes()),
            content: content.clone(),
            binding: if path.is_some() {
                "hash_checked"
            } else {
                "frozen"
            }
            .into(),
        };
        let entry = &mut self.nodes[node];
        if entry.preview_text.is_none() {
            entry.resolved_file_path = source.path.clone();
            entry.previewable = true;
            entry.preview_status = CallChainPreviewStatus::PreviewReady;
            entry.preview_text = Some(content);
        }
        self.sources.push(source);
        Ok(())
    }

    fn file_source(
        &mut self,
        node: usize,
        language: Kind,
        path: &str,
        cwd: &Path,
    ) -> Result<String, String> {
        let path = absolute_file(Path::new(path), cwd)?;
        let content = text_source(self.bind_file(&path, MAX_SOURCE_BYTES)?)?;
        let canonical = fs::canonicalize(path).map_err(|_| "script could not be resolved")?;
        self.add_source(node, language, "file", Some(canonical), content.clone())?;
        Ok(content)
    }

    fn analyze(
        &mut self,
        command: &[String],
        cwd: &Path,
        stdin: Option<&str>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 8 || self.nodes.len() >= MAX_NODES {
            return Err("consumer command nesting exceeds supported limits".into());
        }
        let name = command
            .first()
            .filter(|name| !name.is_empty())
            .ok_or("consumer command is empty")?;
        let language=kind(name).ok_or_else(||format!("program {name:?} has no execution-evidence recognizer yet; use a supported runtime or tool"))?;
        let program = resolve_program(name, cwd)?;
        self.verify_program(&program, language)?;
        if language == Kind::Shell
            && program.file_name().is_some_and(|name| name == "zsh")
            && !command
                .iter()
                .skip(1)
                .take_while(|arg| arg.starts_with('-'))
                .any(|arg| !arg.starts_with("--") && arg.contains('f'))
        {
            return Err("zsh execution requires -f to avoid uncollected user startup files".into());
        }
        let node = self.nodes.len();
        let mut argv = command.to_vec();
        argv[0] = program.display().to_string();
        self.nodes.push(CallChainNode {
            pid: None,
            ppid: None,
            process_name: Some("Daemon-bound command plan".into()),
            executable_path: Some(program.display().to_string()),
            argv,
            resolved_file_path: None,
            source: CallChainNodeSource::ControlledExecution,
            previewable: false,
            preview_status: CallChainPreviewStatus::NotPreviewable,
            preview_text: None,
            preview_error: None,
        });
        if language == Kind::Native {
            // Standard CLIs may themselves be installed script entrypoints (aws/gcloud).
            let prefix = read_limited(&program, MAX_EXECUTABLE_BYTES)?;
            if prefix.starts_with(b"#!") {
                let content = text_source(prefix)?;
                let first = content
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim_start_matches("#!")
                    .trim();
                let mut interpreter = shell_words::split(first)
                    .map_err(|_| "native tool shebang is not recognizable")?;
                if interpreter.first().is_some_and(|value| {
                    Path::new(value)
                        .file_name()
                        .is_some_and(|name| name == "env")
                }) {
                    interpreter.remove(0);
                    if interpreter
                        .first()
                        .is_some_and(|value| value.starts_with('-'))
                    {
                        return Err(
                            "env shebang options are not supported by the entrypoint recognizer"
                                .into(),
                        );
                    }
                }
                let interpreter_kind = interpreter
                    .first()
                    .and_then(|name| kind(name))
                    .filter(|kind| *kind != Kind::Native)
                    .ok_or("native tool's script runtime has no recognizer")?;
                let runtime = resolve_program(&interpreter[0], cwd)?;
                self.verify_program(&runtime, interpreter_kind)?;
                self.add_source(
                    node,
                    interpreter_kind,
                    "file",
                    Some(fs::canonicalize(&program).map_err(|_| "tool script is unavailable")?),
                    content,
                )?;
            }
            return Ok(());
        }
        let mut index = 1;
        let mut source = None;
        let mut source_kind = "file";
        let mut file = None;
        while index < command.len() {
            let arg = &command[index];
            if arg == "--" || (language == Kind::Shell && arg == "-") {
                index += 1;
                if let Some(path) = command.get(index) {
                    if path != "-" || language == Kind::Shell {
                        file = Some(path.as_str());
                    }
                }
                break;
            }
            if arg == "-" {
                source_kind = "stdin";
                break;
            }
            match language {
                Kind::Python => {
                    if arg == "-c" {
                        source_kind = "inline";
                        source = Some(
                            command
                                .get(index + 1)
                                .ok_or("python -c requires source")?
                                .clone(),
                        );
                        break;
                    }
                    if arg == "-m" {
                        return Err("Python module entrypoints are not statically collected yet; use a readable file or inline source".into());
                    }
                    if ["-W", "-X", "--check-hash-based-pycs"].contains(&arg.as_str()) {
                        index += 2;
                        continue;
                    }
                    if arg.starts_with('-') {
                        if [
                            "-B", "-E", "-I", "-O", "-OO", "-P", "-q", "-s", "-S", "-u", "-v",
                        ]
                        .contains(&arg.as_str())
                            || arg.starts_with("-W")
                            || arg.starts_with("-X")
                        {
                            index += 1;
                            continue;
                        }
                        return Err(format!(
                            "Python option {arg} is not recognized for execution evidence"
                        ));
                    }
                }
                Kind::Node => {
                    if ["-e", "--eval", "-p", "--print"].contains(&arg.as_str()) {
                        source_kind = "inline";
                        source = Some(
                            command
                                .get(index + 1)
                                .ok_or("Node eval requires source")?
                                .clone(),
                        );
                        break;
                    }
                    if let Some(code) = arg
                        .strip_prefix("--eval=")
                        .or_else(|| arg.strip_prefix("--print="))
                    {
                        source_kind = "inline";
                        source = Some(code.into());
                        break;
                    }
                    if arg == "--input-type" {
                        index += 2;
                        continue;
                    }
                    if arg.starts_with('-') {
                        if arg.starts_with("--input-type=")
                            || [
                                "--no-warnings",
                                "--enable-source-maps",
                                "--trace-warnings",
                                "--experimental-strip-types",
                            ]
                            .contains(&arg.as_str())
                        {
                            index += 1;
                            continue;
                        }
                        return Err(format!(
                            "Node option {arg} is not recognized for execution evidence"
                        ));
                    }
                }
                Kind::Shell => {
                    if arg.starts_with('-')
                        && !arg.starts_with("--")
                        && arg.chars().skip(1).any(|flag| !"euxvfcso".contains(flag))
                    {
                        return Err("shell login, interactive and unrecognized startup options cannot be bound".into());
                    }
                    if arg.starts_with('-') && !arg.starts_with("--") && arg.contains('o') {
                        let option = command
                            .get(index + 1)
                            .ok_or("shell -o requires an option")?;
                        if ![
                            "pipefail", "errexit", "nounset", "xtrace", "verbose", "noglob",
                        ]
                        .contains(&option.as_str())
                            || arg.contains('c')
                            || arg.contains('s')
                        {
                            return Err("shell option combination cannot be bound".into());
                        }
                        index += 2;
                        continue;
                    }
                    if arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c') {
                        source_kind = "inline";
                        source = Some(
                            command
                                .get(index + 1)
                                .ok_or("shell -c requires source")?
                                .clone(),
                        );
                        break;
                    }
                    if arg.starts_with('-') && !arg.starts_with("--") && arg.contains('s') {
                        if command
                            .get(index + 1)
                            .is_some_and(|next| next.starts_with('-') && next != "--")
                        {
                            return Err("place shell startup flags before -s; following options are not accepted".into());
                        }
                        source_kind = "stdin";
                        break;
                    }
                    if arg.starts_with('-') {
                        if ["--noprofile", "--norc"].contains(&arg.as_str())
                            || (!arg.starts_with("--") && arg.len() > 1)
                        {
                            index += 1;
                            continue;
                        }
                        return Err(format!(
                            "shell option {arg} is not recognized for execution evidence"
                        ));
                    }
                }
                Kind::Native => unreachable!(),
            }
            file = Some(arg.as_str());
            break;
        }
        let content = if let Some(path) = file {
            self.file_source(node, language, path, cwd)?
        } else {
            let content = source.unwrap_or_else(|| {
                source_kind = "stdin";
                stdin.unwrap_or_default().into()
            });
            if source_kind == "stdin" && stdin.is_none() {
                return Err("this consumer reads program source from stdin; use --env with captured source input".into());
            }
            self.add_source(node, language, source_kind, None, content.clone())?;
            content
        };
        if language == Kind::Shell {
            self.analyze_shell(&content, cwd, depth + 1)?;
        }
        Ok(())
    }

    fn analyze_shell(&mut self, source: &str, cwd: &Path, depth: usize) -> Result<(), String> {
        for command in shell::commands(source)? {
            for output in &command.output_paths {
                if output.dynamic {
                    return Err("dynamic shell output paths cannot be checked against collected execution files".into());
                }
                let lexical = cwd.join(&output.text);
                let normalized = fs::canonicalize(&lexical)
                    .or_else(|_| {
                        let parent = lexical
                            .parent()
                            .ok_or_else(|| std::io::Error::other("output parent missing"))?;
                        Ok::<_, std::io::Error>(
                            fs::canonicalize(parent)?.join(
                                lexical
                                    .file_name()
                                    .ok_or_else(|| std::io::Error::other("output name missing"))?,
                            ),
                        )
                    })
                    .map_err(|_| "shell output directory cannot be resolved")?;
                self.output_paths.push(normalized);
            }
            let mut words = command.words;
            while words.first().is_some_and(|word| {
                word.text
                    .split_once('=')
                    .is_some_and(|(name, _)| valid_env_name(name))
            }) {
                let word = words.remove(0);
                if word
                    .text
                    .split_once('=')
                    .is_some_and(|(name, _)| runtime_env(name))
                {
                    return Err("shell assignments cannot change runtime control variables during evidence-bound execution".into());
                }
            }
            if words.is_empty() {
                continue;
            }
            if words[0].dynamic {
                return Err(
                    "dynamic shell command names cannot be bound; use a literal executable".into(),
                );
            }
            if words[0].text == "exec" || words[0].text == "env" {
                let wrapper = words.remove(0);
                if words.first().is_some_and(|word| word.text.starts_with('-')) {
                    return Err(format!(
                        "{} options have no wrapper recognizer yet",
                        wrapper.text
                    ));
                }
                while words.first().is_some_and(|word| {
                    word.text
                        .split_once('=')
                        .is_some_and(|(name, _)| valid_env_name(name))
                }) {
                    let word = words.remove(0);
                    if word
                        .text
                        .split_once('=')
                        .is_some_and(|(name, _)| runtime_env(name))
                    {
                        return Err("runtime-changing env wrappers cannot be bound".into());
                    }
                }
                if words.is_empty() {
                    return Err("command wrapper needs an executable".into());
                }
            }
            if words[0].dynamic {
                return Err("dynamic shell command names cannot be bound".into());
            }
            let program = words[0].text.as_str();
            if program == "cd" {
                return Err(
                    "shell cd is not collected yet; use --cwd to bind the working directory".into(),
                );
            }
            if ["echo", "true", "false", ":", "pwd", "test", "[", "exit"].contains(&program) {
                continue;
            }
            if program == "read" || program == "unset" || program == "export" {
                for word in words.iter().skip(1) {
                    if (program == "read" && word.text == "-r")
                        || (program == "unset" && word.text == "-v")
                    {
                        continue;
                    }
                    let name = word
                        .text
                        .split_once('=')
                        .map_or(word.text.as_str(), |(name, _)| name);
                    if (word.dynamic && !word.text.contains('='))
                        || !valid_env_name(name)
                        || runtime_env(name)
                    {
                        return Err("shell builtin variable targets must be literal non-runtime variable names".into());
                    }
                }
                continue;
            }
            if program == "printf" {
                let mut format_index = 1;
                if words.get(1).is_some_and(|word| word.text == "-v") {
                    let target = words.get(2).ok_or("printf -v requires a variable")?;
                    if target.dynamic || !valid_env_name(&target.text) || runtime_env(&target.text)
                    {
                        return Err(
                            "printf variable target cannot change runtime configuration".into()
                        );
                    }
                    format_index = 3;
                }
                if words
                    .get(format_index)
                    .is_some_and(|word| word.text == "--")
                {
                    format_index += 1;
                }
                let format = words
                    .get(format_index)
                    .ok_or("printf requires a literal format")?;
                if format.dynamic || format.text.contains("%n") || format.text.starts_with('-') {
                    return Err("dynamic or variable-writing printf formats cannot be bound".into());
                }
                continue;
            }
            if program == "set" {
                if words.iter().skip(1).any(|word| {
                    word.text
                        .split_once('=')
                        .is_some_and(|(name, _)| runtime_env(name))
                }) {
                    return Err("shell runtime configuration changes cannot be bound".into());
                }
                continue;
            }
            if [
                "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case",
                "esac", "eval", "source", ".",
            ]
            .contains(&program)
            {
                return Err(format!("shell {program} needs a more advanced recognizer; use a directly collected consumer command"));
            }
            let language = kind(program).ok_or_else(|| {
                format!("shell command {program:?} has no execution-evidence recognizer yet")
            })?;
            // Interpreter source/file selection must be literal. Native argument
            // variable expansion is represented by the captured outer shell source.
            if language != Kind::Native && words.iter().skip(1).any(|word| word.dynamic) {
                return Err("dynamic interpreter arguments cannot be bound yet; use literal source/file arguments".into());
            }
            let mut input = command.stdin;
            if let Some(path) = command.stdin_path {
                if path.dynamic {
                    return Err("dynamic input redirection cannot be collected".into());
                }
                let path = absolute_file(Path::new(&path.text), cwd)?;
                input = Some(text_source(self.bind_file(&path, MAX_SOURCE_BYTES)?)?);
            }
            self.analyze(
                &words
                    .iter()
                    .map(|word| word.text.clone())
                    .collect::<Vec<_>>(),
                cwd,
                input.as_deref(),
                depth,
            )?;
        }
        Ok(())
    }

    pub fn verify_unchanged(&self) -> Result<(), String> {
        for file in &self.files {
            if fs::canonicalize(&file.path).ok().as_ref() != Some(&file.target) {
                return Err("reviewed executable or script symlink target changed".into());
            }
            if digest(&read_limited(&file.path, file.maximum)?) != file.sha256 {
                return Err(
                    "reviewed executable or script changed; submit a new execution request".into(),
                );
            }
        }
        if fs::canonicalize(&self.cwd).ok().as_ref() != Some(&self.cwd) {
            return Err("reviewed working directory changed".into());
        }
        Ok(())
    }
    pub fn source_path(&self) -> Option<&str> {
        self.sources
            .iter()
            .find_map(|source| source.path.as_deref())
    }
    pub fn call_chain_node(&self) -> CallChainNode {
        self.nodes[0].clone()
    }
    pub fn call_chain_nodes(&self) -> Vec<CallChainNode> {
        self.nodes.clone()
    }
    pub fn evidence(&self, plan_id: &str) -> BTreeMap<String, String> {
        let mut result = BTreeMap::from([
            ("execution_admission".into(), "daemon_command_v1".into()),
            ("execution_plan_id".into(), plan_id.into()),
            (
                "execution_command".into(),
                serde_json::to_string(&self.nodes[0].argv).expect("argv serializes"),
            ),
            ("execution_cwd".into(), self.cwd.display().to_string()),
            (
                "execution_timeout_seconds".into(),
                self.timeout_seconds.to_string(),
            ),
            (
                "execution_environment".into(),
                serde_json::to_string(&BTreeMap::from(CONSUMER_ENVIRONMENT))
                    .expect("environment serializes"),
            ),
            (
                "execution_sources".into(),
                serde_json::to_string(&self.sources).expect("sources serialize"),
            ),
            (
                "execution_credential_transport".into(),
                if self.credential_env.is_some() {
                    "env"
                } else {
                    "stdin"
                }
                .into(),
            ),
            (
                "execution_interpreter_path".into(),
                self.interpreter_path.display().to_string(),
            ),
        ]);
        if let Some(source) = self.sources.first() {
            result.insert("execution_source".into(), source.content.clone());
            result.insert("execution_script_sha256".into(), source.sha256.clone());
        }
        if let Some(path) = self.source_path() {
            result.insert("execution_script_path".into(), path.into());
        }
        if let Some(file) = self.files.first() {
            result.insert("execution_interpreter_sha256".into(), file.sha256.clone());
        }
        if let Some(name) = &self.credential_env {
            result.insert("execution_credential_env".into(), name.clone());
        }
        if let Some(input) = &self.command_stdin {
            result.insert(
                "execution_stdin_kind".into(),
                if self.stdin_is_source {
                    "source"
                } else {
                    "data"
                }
                .into(),
            );
            result.insert("execution_stdin_sha256".into(), digest(input.as_bytes()));
            if !self.stdin_is_source {
                result.insert("execution_stdin_content".into(), input.clone());
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(directory: &Path, command: Vec<String>) -> ResourceExecutionRequest {
        ResourceExecutionRequest {
            resource_id: "secret/test".into(),
            reason: "test".into(),
            requested_by: "test".into(),
            cwd: directory.display().to_string(),
            command,
            ..Default::default()
        }
    }
    #[test]
    #[cfg(unix)]
    fn captures_files_without_changing_python_runtime_semantics() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("consumer.py");
        let original = "import sys\nprint(len(sys.stdin.read()))\n";
        fs::write(&script, original).unwrap();
        let plan = ExecutionPlan::prepare(&request(
            directory.path(),
            vec!["python3".into(), script.display().to_string()],
        ))
        .unwrap();
        plan.verify_unchanged().unwrap();
        assert_eq!(plan.arguments, [script.display().to_string()]);
        fs::write(&script, "print('changed')").unwrap();
        assert!(plan.verify_unchanged().is_err());
        let mut nodes = plan.call_chain_nodes();
        crate::preview_call_chain_for_desktop(&mut nodes);
        assert_eq!(nodes[0].preview_text.as_deref(), Some(original));
        assert_eq!(
            crate::read_allowlisted_call_chain_file(&nodes, script.to_str().unwrap())
                .unwrap()
                .content,
            original
        );
    }
    #[test]
    #[cfg(unix)]
    fn inline_stdin_env_and_disguised_program_admission() {
        let directory = tempfile::tempdir().unwrap();
        let mut input = request(directory.path(), vec!["python3".into(), "-".into()]);
        assert!(ExecutionPlan::prepare(&input).is_err());
        input.credential_env = Some("API_TOKEN".into());
        input.command_stdin = Some("import os\nassert os.environ['API_TOKEN']\n".into());
        let plan = ExecutionPlan::prepare(&input).unwrap();
        assert_eq!(plan.sources[0].kind, "stdin");
        assert_eq!(plan.sources[0].binding, "frozen");
        for name in [
            "PATH",
            "BASH_ENV",
            "PYTHONPATH",
            "NODE_OPTIONS",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "bad-name",
        ] {
            input.credential_env = Some(name.into());
            assert!(ExecutionPlan::prepare(&input).is_err());
        }
        let fake = directory.path().join("curl");
        fs::write(&fake, "#!/bin/sh\ncat\n").unwrap();
        assert!(ExecutionPlan::prepare(&request(
            directory.path(),
            vec![fake.display().to_string()]
        ))
        .unwrap_err()
        .contains("does not match"));
        let fake = directory.path().join("node");
        fs::copy("/bin/sh", &fake).unwrap();
        assert!(ExecutionPlan::prepare(&request(
            directory.path(),
            vec![fake.display().to_string(), "-e".into(), "x".into()]
        ))
        .is_err());
        assert!(
            ExecutionPlan::prepare(&request(directory.path(), vec!["unknown-tool".into()]))
                .unwrap_err()
                .contains("no execution-evidence recognizer")
        );
    }
    #[test]
    #[cfg(unix)]
    fn preserves_venv_launch_path_and_rejects_target_changes() {
        let directory = tempfile::tempdir().unwrap();
        let python = directory.path().join("python");
        std::os::unix::fs::symlink("/usr/bin/python3", &python).unwrap();
        let plan = ExecutionPlan::prepare(&request(
            directory.path(),
            vec![
                python.display().to_string(),
                "-c".into(),
                "print('ok')".into(),
            ],
        ))
        .unwrap();
        assert_eq!(plan.interpreter_path, python);
        plan.verify_unchanged().unwrap();
        fs::remove_file(&python).unwrap();
        std::os::unix::fs::symlink("/bin/sh", &python).unwrap();
        assert!(plan.verify_unchanged().is_err());
    }

    #[test]
    #[cfg(unix)]
    fn source_symlink_and_shell_runtime_mutations_cannot_swap_collected_entrypoints() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original.py");
        let replacement = directory.path().join("replacement.py");
        let link = directory.path().join("consumer.py");
        fs::write(&original, "print('original')\n").unwrap();
        fs::write(&replacement, "print('replacement')\n").unwrap();
        std::os::unix::fs::symlink(&original, &link).unwrap();
        let plan = ExecutionPlan::prepare(&request(
            directory.path(),
            vec!["python3".into(), "consumer.py".into()],
        ))
        .unwrap();
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&replacement, &link).unwrap();
        assert!(plan.verify_unchanged().is_err());
        for source in [
            "read PATH; curl https://example.test",
            "unset PATH; curl https://example.test",
            "printf -v PATH '%s' /tmp; curl https://example.test",
            "printf '%n' PATH; curl https://example.test",
            "read $NAME; curl https://example.test",
            "cd /tmp | python3 consumer.py",
        ] {
            assert!(
                ExecutionPlan::prepare(&request(
                    directory.path(),
                    vec!["/bin/sh".into(), "-c".into(), source.into()]
                ))
                .is_err(),
                "{source}"
            );
        }
    }
    #[test]
    #[cfg(unix)]
    fn binds_shell_inline_pipeline_heredoc_and_rejects_dynamic_entrypoints() {
        let directory = tempfile::tempdir().unwrap();
        let command = vec![
            "/bin/sh".into(),
            "-c".into(),
            "python3 - <<'PY' | curl --data-binary @- https://example.test\nprint('fixed')\nPY\n"
                .into(),
        ];
        let plan = ExecutionPlan::prepare(&request(directory.path(), command)).unwrap();
        assert_eq!(plan.nodes.len(), 3);
        assert_eq!(plan.sources.len(), 2);
        assert_eq!(plan.sources[1].kind, "stdin");
        for source in [
            "$RUNNER script.py",
            "env -S 'python3 x.py'",
            "python3 $SCRIPT",
            "curl $(cat destination)",
        ] {
            assert!(
                ExecutionPlan::prepare(&request(
                    directory.path(),
                    vec!["/bin/sh".into(), "-c".into(), source.into()]
                ))
                .is_err(),
                "{source}"
            );
        }
        assert!(!requires_source_stdin(&[
            "sh".into(),
            "-c".into(),
            "python - <<'PY'\npass\nPY".into()
        ]));
        assert!(requires_source_stdin(&["python3".into(), "-".into()]));
    }
}
