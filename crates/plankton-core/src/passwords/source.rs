use std::{collections::BTreeMap, fmt, fs, path::Path};

use dotenvy::from_path_iter;
use plankton_protocol::passwords::{
    FileFormat, PasswordDraftInput, PasswordSourceDescriptor, SelectedPasswordEntry,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParsedPasswordEntry {
    pub key: String,
    pub value: String,
}

impl fmt::Debug for ParsedPasswordEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedPasswordEntry")
            .field("key", &self.key)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParsedPasswordSource {
    pub descriptor: PasswordSourceDescriptor,
    pub entries: Vec<ParsedPasswordEntry>,
    pub suggested_item_title: Option<String>,
    pub suggested_destination: Option<plankton_protocol::passwords::PasswordDestination>,
    pub suggested_layout: Option<plankton_protocol::passwords::PasswordDraftLayoutSuggestion>,
}

impl fmt::Debug for ParsedPasswordSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedPasswordSource")
            .field("descriptor", &self.descriptor)
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

pub fn parse_password_draft_input(
    input: PasswordDraftInput,
) -> Result<ParsedPasswordSource, PasswordSourceError> {
    let mut descriptor = input.descriptor();
    let suggested_item_title = input
        .suggested_item_title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty());
    let suggested_destination = input.suggested_destination;
    let suggested_layout = input.suggested_layout;
    let entries = match &descriptor {
        PasswordSourceDescriptor::Manual { keys } => manual_entries(keys, &input.entries)?,
        PasswordSourceDescriptor::OnePassword { account, fields } => {
            let expected = fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            let supplied = input
                .entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            if fields.is_empty()
                || fields.iter().any(|field| field.validate().is_err())
                || account.as_ref().is_some_and(|account| {
                    account.trim().is_empty() || account.chars().any(char::is_control)
                })
                || expected.len() != fields.len()
                || supplied.len() != input.entries.len()
                || expected != supplied
                || input.entries.iter().any(|entry| entry.value.is_empty())
            {
                return Err(PasswordSourceError::InvalidOnePasswordDraft);
            }
            input.entries.into_iter().map(selected_entry).collect()
        }
        PasswordSourceDescriptor::Environment { .. } | PasswordSourceDescriptor::File { .. } => {
            let entries = input
                .entries
                .into_iter()
                .map(selected_entry)
                .collect::<Vec<_>>();
            if entries.is_empty() {
                return Err(PasswordSourceError::NoValuesFound);
            }
            entries
        }
    };
    if matches!(&descriptor, PasswordSourceDescriptor::Manual { .. }) {
        descriptor = PasswordSourceDescriptor::Manual {
            keys: entries.iter().map(|entry| entry.key.clone()).collect(),
        };
    }
    Ok(ParsedPasswordSource {
        descriptor,
        entries,
        suggested_item_title,
        suggested_destination,
        suggested_layout,
    })
}

pub fn parse_password_source_descriptor(
    descriptor: PasswordSourceDescriptor,
) -> Result<ParsedPasswordSource, PasswordSourceError> {
    let entries = match &descriptor {
        PasswordSourceDescriptor::Manual { .. } => {
            return Err(PasswordSourceError::ManualValuesMustBeEnteredByHuman)
        }
        PasswordSourceDescriptor::File { path, format, keys } => parse_file(path, *format, keys)?,
        PasswordSourceDescriptor::Environment { .. } => {
            return Err(PasswordSourceError::EnvironmentValuesMustBeSupplied)
        }
        PasswordSourceDescriptor::OnePassword { .. } => {
            return Err(PasswordSourceError::OnePasswordValuesMustBeSupplied)
        }
    };
    if entries.is_empty() {
        return Err(PasswordSourceError::NoValuesFound);
    }
    Ok(ParsedPasswordSource {
        descriptor,
        entries,
        suggested_item_title: None,
        suggested_destination: None,
        suggested_layout: None,
    })
}

fn manual_entries(
    keys: &[String],
    supplied_entries: &[SelectedPasswordEntry],
) -> Result<Vec<ParsedPasswordEntry>, PasswordSourceError> {
    if !supplied_entries.is_empty() {
        return Err(PasswordSourceError::ManualDraftCannotSupplyValues);
    }
    let mut seen = std::collections::BTreeSet::new();
    let normalized = keys
        .iter()
        .map(|key| key.trim().to_string())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return Err(PasswordSourceError::NoKeysRequested);
    }
    for key in &normalized {
        if key.is_empty() {
            return Err(PasswordSourceError::EmptyKey);
        }
        if !seen.insert(key.clone()) {
            return Err(PasswordSourceError::DuplicateKey(key.clone()));
        }
    }
    Ok(normalized
        .into_iter()
        .map(|key| ParsedPasswordEntry {
            key,
            value: String::new(),
        })
        .collect())
}

fn selected_entry(entry: SelectedPasswordEntry) -> ParsedPasswordEntry {
    ParsedPasswordEntry {
        key: entry.key,
        value: entry.value,
    }
}

fn parse_file(
    path: &Path,
    requested_format: FileFormat,
    keys: &[String],
) -> Result<Vec<ParsedPasswordEntry>, PasswordSourceError> {
    let format = resolve_format(path, requested_format)?;
    let values = match format {
        FileFormat::Dotenv => from_path_iter(path)
            .map_err(|_| PasswordSourceError::Dotenv)?
            .map(|entry| entry.map_err(|_| PasswordSourceError::Dotenv))
            .collect::<Result<BTreeMap<_, _>, _>>()?,
        FileFormat::Json => {
            let source = fs::read_to_string(path).map_err(PasswordSourceError::Read)?;
            let value = serde_json::from_str(&source).map_err(PasswordSourceError::Json)?;
            flatten_value(&value)?
        }
        FileFormat::Yaml => {
            let source = fs::read_to_string(path).map_err(PasswordSourceError::Read)?;
            let value: Value =
                serde_yaml_ng::from_str(&source).map_err(PasswordSourceError::Yaml)?;
            flatten_value(&value)?
        }
        FileFormat::Auto => unreachable!("auto format is resolved before parsing"),
    };
    select_keys(values, keys)
}

fn resolve_format(path: &Path, requested: FileFormat) -> Result<FileFormat, PasswordSourceError> {
    if requested != FileFormat::Auto {
        return Ok(requested);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if name == ".env" || name.ends_with(".env") {
        return Ok(FileFormat::Dotenv);
    }
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("json") => Ok(FileFormat::Json),
        Some("yaml" | "yml") => Ok(FileFormat::Yaml),
        _ => Err(PasswordSourceError::UnknownFormat(path.to_path_buf())),
    }
}

fn flatten_value(value: &Value) -> Result<BTreeMap<String, String>, PasswordSourceError> {
    let mut flattened = BTreeMap::new();
    flatten_at("", value, &mut flattened)?;
    Ok(flattened)
}

fn flatten_at(
    path: &str,
    value: &Value,
    flattened: &mut BTreeMap<String, String>,
) -> Result<(), PasswordSourceError> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten_at(&child, value, flattened)?;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                flatten_at(&format!("{path}[{index}]"), value, flattened)?;
            }
        }
        Value::String(value) => {
            flattened.insert(path.to_string(), value.clone());
        }
        Value::Number(value) => {
            flattened.insert(path.to_string(), value.to_string());
        }
        Value::Bool(value) => {
            flattened.insert(path.to_string(), value.to_string());
        }
        Value::Null => return Err(PasswordSourceError::UnsupportedNull(path.to_string())),
    }
    Ok(())
}

fn select_keys(
    values: BTreeMap<String, String>,
    keys: &[String],
) -> Result<Vec<ParsedPasswordEntry>, PasswordSourceError> {
    if keys.is_empty() {
        return Ok(values
            .into_iter()
            .map(|(key, value)| ParsedPasswordEntry { key, value })
            .collect());
    }
    keys.iter()
        .map(|key| {
            values
                .get(key)
                .cloned()
                .map(|value| ParsedPasswordEntry {
                    key: key.clone(),
                    value,
                })
                .ok_or_else(|| PasswordSourceError::KeyMissing(key.clone()))
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum PasswordSourceError {
    #[error("1Password values must be resolved and supplied by the client")]
    OnePasswordValuesMustBeSupplied,
    #[error("1Password draft entries must exactly match valid selected field references and contain non-empty text values")]
    InvalidOnePasswordDraft,
    #[error("manual password values must be entered in the desktop confirmation window")]
    ManualValuesMustBeEnteredByHuman,
    #[error("manual password drafts cannot supply values through the CLI or daemon API")]
    ManualDraftCannotSupplyValues,
    #[error("manual password draft contains no requested keys")]
    NoKeysRequested,
    #[error("manual password key cannot be empty")]
    EmptyKey,
    #[error("manual password key {0} is duplicated")]
    DuplicateKey(String),
    #[error("environment values must be resolved and supplied by the client")]
    EnvironmentValuesMustBeSupplied,
    #[error("failed to read password source: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to parse dotenv source")]
    Dotenv,
    #[error("failed to parse JSON source: {0}")]
    Json(#[source] serde_json::Error),
    #[error("failed to parse YAML source: {0}")]
    Yaml(#[source] serde_yaml_ng::Error),
    #[error("cannot infer password source format from {0}")]
    UnknownFormat(std::path::PathBuf),
    #[error("password source key {0} was not found")]
    KeyMissing(String),
    #[error("null password value at {0} is unsupported")]
    UnsupportedNull(String),
    #[error("password source contains no scalar values")]
    NoValuesFound,
}

#[cfg(test)]
mod tests {
    use super::{parse_password_draft_input, parse_password_source_descriptor};
    use plankton_protocol::passwords::{
        FileFormat, PasswordDraftInput, PasswordSourceDescriptor, SelectedPasswordEntry,
    };
    use tempfile::tempdir;

    #[test]
    fn onepassword_drafts_accept_only_the_exact_selected_fields_without_resolving_references() {
        let descriptor = PasswordSourceDescriptor::OnePassword {
            account: Some("team".into()),
            fields: vec![plankton_protocol::passwords::OnePasswordFieldReference {
                key: "TOKEN".into(),
                reference: "op://Work/Service/password".into(),
            }],
        };
        assert!(parse_password_source_descriptor(descriptor.clone()).is_err());
        let input = PasswordDraftInput {
            descriptor,
            entries: vec![SelectedPasswordEntry {
                key: "TOKEN".into(),
                value: "OP_SECRET_SENTINEL".into(),
            }],
            suggested_item_title: None,
            suggested_destination: None,
            suggested_layout: None,
        };
        let parsed = parse_password_draft_input(input.clone()).unwrap();
        assert_eq!(parsed.entries[0].value, "OP_SECRET_SENTINEL");
        assert!(!format!("{parsed:?}").contains("OP_SECRET_SENTINEL"));
        assert!(!serde_json::to_string(&parsed.descriptor)
            .unwrap()
            .contains("OP_SECRET_SENTINEL"));
        for entries in [
            vec![],
            vec![SelectedPasswordEntry {
                key: "OTHER".into(),
                value: "OP_SECRET_SENTINEL".into(),
            }],
            vec![input.entries[0].clone(), input.entries[0].clone()],
            vec![SelectedPasswordEntry {
                key: "TOKEN".into(),
                value: String::new(),
            }],
        ] {
            let error = parse_password_draft_input(PasswordDraftInput {
                entries,
                ..input.clone()
            })
            .unwrap_err();
            assert!(!error.to_string().contains("OP_SECRET_SENTINEL"));
        }
    }

    #[test]
    fn supplied_environment_entries_keep_values_out_of_debug_and_metadata() {
        let parsed = parse_password_draft_input(PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::Environment {
                names: vec!["CLIENT_ONLY_TOKEN".into()],
            },
            entries: vec![SelectedPasswordEntry {
                key: "CLIENT_ONLY_TOKEN".into(),
                value: "client-only-secret".into(),
            }],
            suggested_item_title: Some("  Client token  ".into()),
            suggested_destination: None,
            suggested_layout: None,
        })
        .expect("client-supplied entry should parse without daemon environment access");

        assert_eq!(parsed.entries[0].value, "client-only-secret");
        assert_eq!(parsed.suggested_item_title.as_deref(), Some("Client token"));
        let debug = format!("{parsed:?}");
        assert!(debug.contains("CLIENT_ONLY_TOKEN"));
        assert!(!debug.contains("client-only-secret"));
        let metadata = serde_json::to_string(&parsed.descriptor).expect("metadata serializes");
        assert!(!metadata.contains("client-only-secret"));
    }

    #[test]
    fn manual_draft_creates_empty_fields_without_accepting_cli_values() {
        let input = PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::Manual {
                keys: vec![" CLIENT_ID ".into(), "CLIENT_SECRET".into()],
            },
            entries: Vec::new(),
            suggested_item_title: Some("Example credentials".into()),
            suggested_destination: None,
            suggested_layout: None,
        };
        let serialized = serde_json::to_string(&input).expect("manual draft serializes");
        assert!(!serialized.contains("value"));

        let parsed = parse_password_draft_input(input).expect("manual draft parses");
        assert_eq!(
            parsed.descriptor,
            PasswordSourceDescriptor::Manual {
                keys: vec!["CLIENT_ID".into(), "CLIENT_SECRET".into()],
            }
        );
        assert_eq!(
            parsed
                .entries
                .iter()
                .map(|entry| (entry.key.as_str(), entry.value.as_str()))
                .collect::<Vec<_>>(),
            vec![("CLIENT_ID", ""), ("CLIENT_SECRET", "")]
        );

        let error = parse_password_draft_input(PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::Manual {
                keys: vec!["CLIENT_ID".into()],
            },
            entries: vec![SelectedPasswordEntry {
                key: "CLIENT_ID".into(),
                value: "must-not-enter-through-cli".into(),
            }],
            suggested_item_title: None,
            suggested_destination: None,
            suggested_layout: None,
        })
        .expect_err("manual CLI values must be rejected");
        assert!(matches!(
            error,
            super::PasswordSourceError::ManualDraftCannotSupplyValues
        ));
    }

    #[test]
    fn malformed_dotenv_error_omits_the_source_line_from_display_and_debug() {
        let temp = tempdir().expect("temp dir");
        let path = temp.path().join(".env");
        let sentinel = "DOTENV_LINE_SECRET_SENTINEL";
        std::fs::write(&path, format!("INVALID LINE {sentinel}\n")).expect("fixture");

        let error = parse_password_source_descriptor(PasswordSourceDescriptor::File {
            path,
            format: FileFormat::Dotenv,
            keys: Vec::new(),
        })
        .expect_err("malformed dotenv must fail");

        assert!(!error.to_string().contains(sentinel));
        assert!(!format!("{error:?}").contains(sentinel));
    }
}
