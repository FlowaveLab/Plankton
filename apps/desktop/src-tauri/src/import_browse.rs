use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, Context, Result};
use dotenvy::from_path_iter;
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, error, info};
use url::Url;

const ALL_CONTAINERS_ID: &str = "all";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportPickerOption {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportFieldOption {
    pub selector: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BitwardenContainerOption {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DotenvGroupOption {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    pub key_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DotenvKeyOption {
    pub group_id: String,
    pub label: String,
    pub full_key: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DotenvInspection {
    pub file_path: String,
    pub groups: Vec<DotenvGroupOption>,
    pub keys: Vec<DotenvKeyOption>,
}

fn onepassword_program() -> PathBuf {
    env::var_os("PLANKTON_1PASSWORD_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("op"))
}

fn bitwarden_program() -> PathBuf {
    env::var_os("PLANKTON_BITWARDEN_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bw"))
}

pub fn list_onepassword_accounts() -> Result<Vec<ImportPickerOption>> {
    list_onepassword_accounts_with_program(onepassword_program().as_path())
}

pub fn list_onepassword_vaults(account_id: &str) -> Result<Vec<ImportPickerOption>> {
    list_onepassword_vaults_with_program(onepassword_program().as_path(), account_id)
}

pub fn list_onepassword_items(account_id: &str, vault_id: &str) -> Result<Vec<ImportPickerOption>> {
    list_onepassword_items_with_program(onepassword_program().as_path(), account_id, vault_id)
}

pub fn list_onepassword_fields(
    account_id: &str,
    vault_id: &str,
    item_id: &str,
) -> Result<Vec<ImportFieldOption>> {
    list_onepassword_fields_with_program(
        onepassword_program().as_path(),
        account_id,
        vault_id,
        item_id,
    )
}

pub fn list_bitwarden_accounts() -> Result<Vec<ImportPickerOption>> {
    list_bitwarden_accounts_with_program(bitwarden_program().as_path())
}

pub fn list_bitwarden_containers() -> Result<Vec<BitwardenContainerOption>> {
    list_bitwarden_containers_with_program(bitwarden_program().as_path())
}

pub fn list_bitwarden_items(
    container_kind: Option<&str>,
    container_id: Option<&str>,
    organization_id: Option<&str>,
) -> Result<Vec<ImportPickerOption>> {
    list_bitwarden_items_with_program(
        bitwarden_program().as_path(),
        container_kind,
        container_id,
        organization_id,
    )
}

pub fn list_bitwarden_fields(item_id: &str) -> Result<Vec<ImportFieldOption>> {
    list_bitwarden_fields_with_program(bitwarden_program().as_path(), item_id)
}

pub fn pick_dotenv_files() -> Result<Vec<String>> {
    Ok(rfd::FileDialog::new()
        .add_filter("dotenv", &["env"])
        .pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.display().to_string())
        .collect())
}

pub fn inspect_dotenv_file(file_path: &str) -> Result<DotenvInspection> {
    inspect_dotenv_file_at(Path::new(file_path))
}

fn list_onepassword_accounts_with_program(program: &Path) -> Result<Vec<ImportPickerOption>> {
    let response = run_json_command(program, &["account", "list", "--format", "json"])?;
    let accounts = response
        .as_array()
        .ok_or_else(|| anyhow!("1Password account list did not return a JSON array"))?;

    Ok(accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let email = string_field(account, "email");
            let url = string_field(account, "url");
            let normalized_url = url
                .as_deref()
                .and_then(normalize_onepassword_account_selector);
            let label = email
                .clone()
                .or(normalized_url.clone())
                .unwrap_or_else(|| format!("Account {}", index + 1));
            let id = normalized_url
                .clone()
                .or(email.clone())
                .or(string_field(account, "account_uuid"))
                .unwrap_or_else(|| label.clone());
            let subtitle = match (email, normalized_url) {
                (Some(email), Some(_url)) if label != email => Some(email),
                (Some(_email), Some(url)) if label != url => Some(url),
                (Some(email), None) if label != email => Some(email),
                (None, Some(url)) if label != url => Some(url),
                _ => None,
            };

            ImportPickerOption {
                id,
                label,
                subtitle,
            }
        })
        .collect())
}

fn normalize_onepassword_account_selector(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    Url::parse(trimmed)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .or_else(|| Some(trimmed.to_string()))
}

fn list_onepassword_vaults_with_program(
    program: &Path,
    account_id: &str,
) -> Result<Vec<ImportPickerOption>> {
    let response = run_json_command(
        program,
        &["vault", "list", "--account", account_id, "--format", "json"],
    )?;
    let vaults = response
        .as_array()
        .ok_or_else(|| anyhow!("1Password vault list did not return a JSON array"))?;

    vaults
        .iter()
        .map(|vault| {
            Ok(ImportPickerOption {
                id: required_string_field(vault, "id", "1Password vault")?,
                label: required_string_field(vault, "name", "1Password vault")?,
                subtitle: None,
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn list_onepassword_items_with_program(
    program: &Path,
    account_id: &str,
    vault_id: &str,
) -> Result<Vec<ImportPickerOption>> {
    let response = run_json_command(
        program,
        &[
            "item",
            "list",
            "--account",
            account_id,
            "--vault",
            vault_id,
            "--format",
            "json",
        ],
    )?;
    let items = response
        .as_array()
        .ok_or_else(|| anyhow!("1Password item list did not return a JSON array"))?;

    items
        .iter()
        .map(|item| {
            let title = required_string_field(item, "title", "1Password item")?;
            let subtitle = string_field(item, "additional_information").or_else(|| {
                item.get("vault")
                    .and_then(|vault| string_field(vault, "name"))
            });

            Ok(ImportPickerOption {
                id: required_string_field(item, "id", "1Password item")?,
                label: title,
                subtitle,
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn list_onepassword_fields_with_program(
    program: &Path,
    account_id: &str,
    vault_id: &str,
    item_id: &str,
) -> Result<Vec<ImportFieldOption>> {
    let response = run_json_command(
        program,
        &[
            "item",
            "get",
            item_id,
            "--account",
            account_id,
            "--vault",
            vault_id,
            "--format",
            "json",
        ],
    )?;
    let item_fields = response
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("1Password item response omitted its fields array"))?;
    let mut fields = Vec::new();
    let notes_reference = item_fields.iter().find_map(|field| {
        let is_notes_field = string_field(field, "id")
            .map(|value| value.eq_ignore_ascii_case("notesPlain"))
            .unwrap_or(false)
            || string_field(field, "purpose")
                .map(|value| value.eq_ignore_ascii_case("NOTES"))
                .unwrap_or(false);
        if !is_notes_field {
            return None;
        }

        string_field(field, "reference")
    });
    let referenced_notes = notes_reference
        .as_deref()
        .map(|reference| onepassword_reference_value(program, account_id, reference))
        .transpose()?;
    let has_notes_field = string_field(&response, "notesPlain")
        .map(|notes| !notes.trim().is_empty())
        .unwrap_or(false)
        || referenced_notes
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);

    if has_notes_field {
        fields.push(ImportFieldOption {
            selector: "notes".to_string(),
            label: "notes".to_string(),
            subtitle: Some("Secure Note".to_string()),
            field_id: Some("notesPlain".to_string()),
        });
    }

    for field in item_fields {
        let id = string_field(&field, "id");
        let is_notes_field = id
            .as_deref()
            .map(|value| value.eq_ignore_ascii_case("notesPlain"))
            .unwrap_or(false)
            || string_field(&field, "purpose")
                .map(|value| value.eq_ignore_ascii_case("NOTES"))
                .unwrap_or(false);
        if is_notes_field {
            continue;
        }
        let label = string_field(&field, "label")
            .or_else(|| string_field(&field, "purpose"))
            .or_else(|| id.clone())
            .unwrap_or_else(|| "field".to_string());
        let subtitle = string_field(&field, "purpose").filter(|value| value != &label);

        fields.push(ImportFieldOption {
            selector: label.clone(),
            label,
            subtitle,
            field_id: id,
        });
    }

    Ok(fields)
}

fn onepassword_reference_value(
    program: &Path,
    account_id: &str,
    reference: &str,
) -> Result<String> {
    let stdout =
        run_command_capture_stdout(program, &["read", reference, "--account", account_id])?;
    String::from_utf8(stdout)
        .map(|value| value.trim_end_matches('\n').to_string())
        .map_err(|error| anyhow!("1Password returned non-UTF8 output: {error}"))
}

fn list_bitwarden_accounts_with_program(program: &Path) -> Result<Vec<ImportPickerOption>> {
    let response = run_json_command(program, &["status"])?;
    let label = string_field(&response, "userEmail")
        .or_else(|| string_field(&response, "serverUrl"))
        .unwrap_or_else(|| "Bitwarden".to_string());
    let id = string_field(&response, "userId")
        .or_else(|| string_field(&response, "serverUrl"))
        .unwrap_or_else(|| label.clone());

    Ok(vec![ImportPickerOption {
        id,
        label,
        subtitle: string_field(&response, "status"),
    }])
}

fn list_bitwarden_containers_with_program(program: &Path) -> Result<Vec<BitwardenContainerOption>> {
    let mut containers = vec![BitwardenContainerOption {
        id: ALL_CONTAINERS_ID.to_string(),
        kind: "all".to_string(),
        label: "All Items".to_string(),
        subtitle: None,
        organization_id: None,
        organization_label: None,
    }];

    let organizations = run_json_command(program, &["list", "organizations"])?;
    let organizations = organizations
        .as_array()
        .ok_or_else(|| anyhow!("Bitwarden organization list did not return a JSON array"))?;
    for organization in organizations {
        let organization_id = required_string_field(organization, "id", "Bitwarden organization")?;
        let organization_label =
            required_string_field(organization, "name", "Bitwarden organization")?;

        containers.push(BitwardenContainerOption {
            id: organization_id.clone(),
            kind: "organization".to_string(),
            label: organization_label.clone(),
            subtitle: Some("Organization".to_string()),
            organization_id: Some(organization_id.clone()),
            organization_label: Some(organization_label.clone()),
        });

        let collections = list_bitwarden_collections_with_program(program, &organization_id)?;
        containers.extend(
            collections
                .into_iter()
                .map(|collection| BitwardenContainerOption {
                    organization_id: Some(organization_id.clone()),
                    organization_label: Some(organization_label.clone()),
                    ..collection
                }),
        );
    }

    let folders = run_json_command(program, &["list", "folders"])?;
    let folders = folders
        .as_array()
        .ok_or_else(|| anyhow!("Bitwarden folder list did not return a JSON array"))?;
    for folder in folders {
        containers.push(BitwardenContainerOption {
            id: required_string_field(folder, "id", "Bitwarden folder")?,
            kind: "folder".to_string(),
            label: required_string_field(folder, "name", "Bitwarden folder")?,
            subtitle: Some("Folder".to_string()),
            organization_id: None,
            organization_label: None,
        });
    }

    Ok(containers)
}

fn list_bitwarden_collections_with_program(
    program: &Path,
    organization_id: &str,
) -> Result<Vec<BitwardenContainerOption>> {
    let direct = run_json_command(
        program,
        &[
            "list",
            "org-collections",
            "--organizationid",
            organization_id,
        ],
    )
    .or_else(|_| {
        run_json_command(
            program,
            &["list", "collections", "--organizationid", organization_id],
        )
    })?;

    let collections = direct
        .as_array()
        .ok_or_else(|| anyhow!("Bitwarden collections did not return a JSON array"))?;

    collections
        .iter()
        .map(|collection| {
            Ok(BitwardenContainerOption {
                id: required_string_field(collection, "id", "Bitwarden collection")?,
                kind: "collection".to_string(),
                label: required_string_field(collection, "name", "Bitwarden collection")?,
                subtitle: Some("Collection".to_string()),
                organization_id: None,
                organization_label: None,
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn list_bitwarden_items_with_program(
    program: &Path,
    container_kind: Option<&str>,
    container_id: Option<&str>,
    organization_id: Option<&str>,
) -> Result<Vec<ImportPickerOption>> {
    let mut args = vec!["list", "items"];

    match (container_kind, container_id) {
        (Some("folder"), Some(container_id)) => {
            args.extend(["--folderid", container_id]);
        }
        (Some("organization"), Some(container_id)) => {
            args.extend(["--organizationid", container_id]);
        }
        (Some("collection"), Some(container_id)) => {
            args.extend(["--collectionid", container_id]);
            if let Some(organization_id) = organization_id {
                args.extend(["--organizationid", organization_id]);
            }
        }
        _ => {}
    }

    let response = run_json_command(program, &args)?;
    let items = response
        .as_array()
        .ok_or_else(|| anyhow!("Bitwarden item list did not return a JSON array"))?;

    items
        .iter()
        .map(|item| {
            let label = string_field(item, "name")
                .or_else(|| string_field(item, "title"))
                .or_else(|| {
                    item.get("login")
                        .and_then(|login| string_field(login, "username"))
                })
                .unwrap_or_else(|| "Item".to_string());

            let subtitle = string_field(item, "notes")
                .or_else(|| string_field(item, "folderId"))
                .or_else(|| string_field(item, "organizationId"));

            Ok(ImportPickerOption {
                id: required_string_field(item, "id", "Bitwarden item")?,
                label,
                subtitle,
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn list_bitwarden_fields_with_program(
    program: &Path,
    item_id: &str,
) -> Result<Vec<ImportFieldOption>> {
    let response = run_json_command(program, &["get", "item", item_id])?;
    let mut fields = Vec::new();

    if let Some(username) = response
        .get("login")
        .and_then(|login| string_field(login, "username"))
        .filter(|value| !value.trim().is_empty())
    {
        fields.push(ImportFieldOption {
            selector: "username".to_string(),
            label: "username".to_string(),
            subtitle: Some(format!("{} chars", username.len())),
            field_id: None,
        });
    }

    if let Some(password) = response
        .get("login")
        .and_then(|login| string_field(login, "password"))
        .filter(|value| !value.trim().is_empty())
    {
        fields.push(ImportFieldOption {
            selector: "password".to_string(),
            label: "password".to_string(),
            subtitle: Some(format!("{} chars", password.len())),
            field_id: None,
        });
    }

    if let Some(notes) = string_field(&response, "notes").filter(|value| !value.trim().is_empty()) {
        fields.push(ImportFieldOption {
            selector: "notes".to_string(),
            label: "notes".to_string(),
            subtitle: Some(format!("{} chars", notes.len())),
            field_id: None,
        });
    }

    let custom_fields = response
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("Bitwarden item response omitted its fields array"))?;
    for field in custom_fields {
        let label = required_string_field(&field, "name", "Bitwarden field")?;
        fields.push(ImportFieldOption {
            selector: label.clone(),
            label,
            subtitle: None,
            field_id: None,
        });
    }

    Ok(fields)
}

fn inspect_dotenv_file_at(path: &Path) -> Result<DotenvInspection> {
    let file_path = path.display().to_string();
    let iter = from_path_iter(path).with_context(|| format!("failed to open {file_path}"))?;
    let mut keys = Vec::new();

    for entry in iter {
        let (key, _value) = entry.with_context(|| format!("failed to parse {file_path}"))?;
        keys.push(key);
    }

    keys.sort();
    let mut groups = vec![DotenvGroupOption {
        id: ALL_CONTAINERS_ID.to_string(),
        label: "All Keys".to_string(),
        namespace: None,
        prefix: None,
        key_count: keys.len(),
    }];
    let mut group_counts: BTreeMap<String, (String, String, usize)> = BTreeMap::new();
    let mut key_options = Vec::new();

    for key in keys {
        key_options.push(DotenvKeyOption {
            group_id: ALL_CONTAINERS_ID.to_string(),
            label: key.clone(),
            full_key: key.clone(),
        });

        if let Some((namespace, prefix, short_key)) = infer_prefix_group(&key) {
            let entry = group_counts.entry(prefix.clone()).or_insert((
                namespace.clone(),
                prefix.clone(),
                0,
            ));
            entry.2 += 1;
            key_options.push(DotenvKeyOption {
                group_id: prefix.clone(),
                label: short_key,
                full_key: key,
            });
        }
    }

    groups.extend(
        group_counts
            .into_iter()
            .map(|(group_id, (namespace, prefix, count))| DotenvGroupOption {
                id: group_id.clone(),
                label: namespace.clone(),
                namespace: Some(namespace),
                prefix: Some(prefix),
                key_count: count,
            }),
    );

    Ok(DotenvInspection {
        file_path,
        groups,
        keys: key_options,
    })
}

fn infer_prefix_group(full_key: &str) -> Option<(String, String, String)> {
    let underscore_index = full_key.find('_')?;
    let namespace = full_key[..underscore_index].trim();
    let short_key = full_key[underscore_index + 1..].trim();

    if namespace.is_empty() || short_key.is_empty() {
        return None;
    }

    Some((
        namespace.to_string(),
        format!("{namespace}_"),
        short_key.to_string(),
    ))
}

fn run_json_command(program: &Path, args: &[&str]) -> Result<Value> {
    debug!(
        program = %program.display(),
        args = %args.join(" "),
        "import browse JSON command start"
    );
    let stdout = run_command_capture_stdout(program, args)?;
    let parsed = serde_json::from_slice(&stdout).with_context(|| {
        format!(
            "{} {} did not return valid JSON",
            program.display(),
            args.join(" ")
        )
    })?;
    debug!(
        program = %program.display(),
        args = %args.join(" "),
        "import browse JSON command success"
    );
    Ok(parsed)
}

fn run_command_capture_stdout(program: &Path, args: &[&str]) -> Result<Vec<u8>> {
    info!(
        program = %program.display(),
        args = %args.join(" "),
        "import browse command start"
    );
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {}", program.display()))?;

    if !output.status.success() {
        let message = compact_process_message(&output);
        error!(
            program = %program.display(),
            args = %args.join(" "),
            status = %output.status,
            error = %message,
            "import browse command failed"
        );
        return Err(anyhow!(message));
    }

    debug!(
        program = %program.display(),
        args = %args.join(" "),
        status = %output.status,
        stdout_len = output.stdout.len(),
        "import browse command success"
    );
    Ok(output.stdout)
}

fn compact_process_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("process exited with status {}", output.status))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn required_string_field(value: &Value, field: &str, label: &str) -> Result<String> {
    string_field(value, field).ok_or_else(|| anyhow!("{label} is missing {field}"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{
        infer_prefix_group, inspect_dotenv_file_at, list_bitwarden_accounts_with_program,
        list_bitwarden_containers_with_program, list_bitwarden_fields_with_program,
        list_bitwarden_items_with_program, list_onepassword_accounts_with_program,
        list_onepassword_fields_with_program, list_onepassword_items_with_program,
        list_onepassword_vaults_with_program,
    };

    #[cfg(unix)]
    #[test]
    fn browses_onepassword_hierarchy_via_mock_cli() {
        let temp = tempdir().expect("temp directory should be created");
        let op_path = temp.path().join("op");
        write_executable(
            op_path.as_path(),
            r#"#!/bin/sh
if [ "$1" = "account" ] && [ "$2" = "list" ]; then
  cat <<'JSON'
[{"url":"https://example.1password.com","email":"demo@example.com","user_uuid":"user-1","account_uuid":"acct-1"}]
JSON
  exit 0
fi
if [ "$1" = "vault" ] && [ "$2" = "list" ]; then
  if [ "$4" != "example.1password.com" ]; then
    echo "unexpected vault account selector: $4" >&2
    exit 1
  fi
  cat <<'JSON'
[{"id":"vault-1","name":"Engineering"}]
JSON
  exit 0
fi
if [ "$1" = "item" ] && [ "$2" = "list" ]; then
  if [ "$4" != "example.1password.com" ]; then
    echo "unexpected item list account selector: $4" >&2
    exit 1
  fi
  cat <<'JSON'
[{"id":"item-1","title":"API Token","vault":{"id":"vault-1","name":"Engineering"},"additional_information":"service credentials"}]
JSON
  exit 0
fi
if [ "$1" = "item" ] && [ "$2" = "get" ]; then
  if [ "$5" != "example.1password.com" ]; then
    echo "unexpected item get account selector: $5" >&2
    exit 1
  fi
  cat <<'JSON'
{"notesPlain":"note body","fields":[{"id":"field-1","type":"CONCEALED","purpose":"PASSWORD","label":"password","value":"secret","reference":"op://vault/item/password"}]}
JSON
  exit 0
fi
echo "unexpected op invocation" >&2
exit 1
"#,
        );

        let accounts =
            list_onepassword_accounts_with_program(op_path.as_path()).expect("account list works");
        let vaults =
            list_onepassword_vaults_with_program(op_path.as_path(), "example.1password.com")
                .expect("vault list works");
        let items = list_onepassword_items_with_program(
            op_path.as_path(),
            "example.1password.com",
            "vault-1",
        )
        .expect("item list works");
        let fields = list_onepassword_fields_with_program(
            op_path.as_path(),
            "example.1password.com",
            "vault-1",
            "item-1",
        )
        .expect("field list works");

        assert_eq!(accounts[0].label, "demo@example.com");
        assert_eq!(accounts[0].id, "example.1password.com");
        assert_eq!(
            accounts[0].subtitle.as_deref(),
            Some("example.1password.com")
        );
        assert_eq!(vaults[0].label, "Engineering");
        assert_eq!(items[0].label, "API Token");
        assert_eq!(fields[0].selector, "notes");
        assert_eq!(fields[0].field_id.as_deref(), Some("notesPlain"));
        assert_eq!(fields[1].field_id.as_deref(), Some("field-1"));
    }

    #[cfg(unix)]
    #[test]
    fn browses_onepassword_reference_only_notes_field() {
        let temp = tempdir().expect("temp directory should be created");
        let op_path = temp.path().join("op");
        write_executable(
            op_path.as_path(),
            r#"#!/bin/sh
if [ "$1" = "item" ] && [ "$2" = "get" ]; then
  cat <<'JSON'
{"fields":[{"id":"notesPlain","type":"STRING","purpose":"NOTES","label":"notesPlain","reference":"op://vault/item/notesPlain"},{"id":"password","type":"CONCEALED","purpose":"PASSWORD","label":"password","value":"secret","reference":"op://vault/item/password"}]}
JSON
  exit 0
fi
if [ "$1" = "read" ]; then
  printf 'note body'
  exit 0
fi
echo "unexpected op invocation" >&2
exit 1
"#,
        );

        let fields = list_onepassword_fields_with_program(
            op_path.as_path(),
            "example.1password.com",
            "vault-1",
            "item-1",
        )
        .expect("field list works");

        assert_eq!(fields[0].selector, "notes");
        assert_eq!(fields[0].field_id.as_deref(), Some("notesPlain"));
        assert_eq!(fields[1].selector, "password");
        assert_eq!(fields.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn hides_empty_onepassword_reference_only_notes_field() {
        let temp = tempdir().expect("temp directory should be created");
        let op_path = temp.path().join("op");
        write_executable(
            op_path.as_path(),
            r#"#!/bin/sh
if [ "$1" = "item" ] && [ "$2" = "get" ]; then
  cat <<'JSON'
{"fields":[{"id":"notesPlain","type":"STRING","purpose":"NOTES","label":"notesPlain","reference":"op://vault/item/notesPlain"},{"id":"password","type":"CONCEALED","purpose":"PASSWORD","label":"password","value":"secret","reference":"op://vault/item/password"}]}
JSON
  exit 0
fi
if [ "$1" = "read" ]; then
  exit 0
fi
echo "unexpected op invocation" >&2
exit 1
"#,
        );

        let fields = list_onepassword_fields_with_program(
            op_path.as_path(),
            "example.1password.com",
            "vault-1",
            "item-1",
        )
        .expect("field list works");

        assert_eq!(fields[0].selector, "password");
        assert_eq!(fields.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn browses_bitwarden_contract_via_mock_cli() {
        let temp = tempdir().expect("temp directory should be created");
        let bw_path = temp.path().join("bw");
        write_executable(
            bw_path.as_path(),
            r#"#!/bin/sh
if [ "$1" = "status" ]; then
  cat <<'JSON'
{"status":"unlocked","userEmail":"demo@example.com","userId":"user-1","serverUrl":"https://vault.example.test"}
JSON
  exit 0
fi
if [ "$1" = "list" ] && [ "$2" = "organizations" ]; then
  cat <<'JSON'
[{"id":"org-1","name":"Engineering"}]
JSON
  exit 0
fi
if [ "$1" = "list" ] && [ "$2" = "org-collections" ]; then
  cat <<'JSON'
[{"id":"collection-1","name":"Prod"}]
JSON
  exit 0
fi
if [ "$1" = "list" ] && [ "$2" = "folders" ]; then
  cat <<'JSON'
[{"id":"folder-1","name":"Local"}]
JSON
  exit 0
fi
if [ "$1" = "list" ] && [ "$2" = "items" ]; then
  cat <<'JSON'
[{"id":"item-1","name":"API Token"}]
JSON
  exit 0
fi
if [ "$1" = "get" ] && [ "$2" = "item" ]; then
  cat <<'JSON'
{"login":{"username":"demo-user","password":"demo-pass"},"notes":"bw notes","fields":[{"name":"custom_token","value":"custom-secret"}]}
JSON
  exit 0
fi
echo "unexpected bw invocation" >&2
exit 1
"#,
        );

        let accounts =
            list_bitwarden_accounts_with_program(bw_path.as_path()).expect("status works");
        let containers =
            list_bitwarden_containers_with_program(bw_path.as_path()).expect("containers work");
        let items = list_bitwarden_items_with_program(
            bw_path.as_path(),
            Some("collection"),
            Some("collection-1"),
            Some("org-1"),
        )
        .expect("item list works");
        let fields = list_bitwarden_fields_with_program(bw_path.as_path(), "item-1")
            .expect("field list works");

        assert_eq!(accounts[0].label, "demo@example.com");
        assert!(containers
            .iter()
            .any(|container| container.kind == "collection"));
        assert_eq!(items[0].label, "API Token");
        assert!(fields.iter().any(|field| field.selector == "password"));
        assert!(fields.iter().any(|field| field.selector == "custom_token"));
    }

    #[test]
    fn inspects_dotenv_file_into_prefix_groups() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join(".env");
        fs::write(
            path.as_path(),
            "APP_TOKEN=one\nAPP_ENDPOINT=two\nROOT_SECRET=three\nPLAIN=four\n",
        )
        .expect("dotenv file should be written");

        let inspection = inspect_dotenv_file_at(path.as_path()).expect("inspection should succeed");

        assert_eq!(inspection.groups[0].id, "all");
        assert!(inspection.groups.iter().any(|group| group.id == "APP_"));
        assert!(inspection
            .keys
            .iter()
            .any(|key| key.group_id == "APP_" && key.label == "TOKEN"));
        assert!(inspection
            .keys
            .iter()
            .any(|key| key.group_id == "all" && key.label == "PLAIN"));
    }

    #[test]
    fn infers_prefix_groups_from_env_keys() {
        assert_eq!(
            infer_prefix_group("APP_TOKEN"),
            Some(("APP".to_string(), "APP_".to_string(), "TOKEN".to_string()))
        );
        assert_eq!(infer_prefix_group("PLAIN"), None);
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, content).expect("script should be written");
        let mut permissions = fs::metadata(path)
            .expect("metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("permissions should be updated");
    }
}
