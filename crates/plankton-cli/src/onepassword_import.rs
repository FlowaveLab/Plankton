use std::{collections::BTreeSet, ffi::OsString, path::Path, process::Stdio, time::Duration};

use anyhow::{bail, Result};
use plankton_protocol::passwords::{OnePasswordFieldReference, SelectedPasswordEntry};
use tokio::{process::Command, time::timeout};

pub fn parse_selection(input: &str) -> Result<OnePasswordFieldReference, String> {
    let (key, reference) = if input.starts_with("op://") {
        let path = input.split('?').next().unwrap_or(input);
        (path.rsplit('/').next().unwrap_or_default(), input)
    } else {
        input
            .split_once('=')
            .ok_or("Use op://VAULT/ITEM/FIELD or KEY=op://VAULT/ITEM/FIELD")?
    };
    let selection = OnePasswordFieldReference {
        key: key.trim().into(),
        reference: reference.into(),
    };
    selection.validate().map_err(str::to_string)?;
    Ok(selection)
}

pub fn validate_selection(
    fields: &[OnePasswordFieldReference],
    account: Option<&str>,
) -> Result<()> {
    if fields.is_empty() {
        bail!("At least one 1Password field reference is required");
    }
    if account
        .is_some_and(|account| account.trim().is_empty() || account.chars().any(char::is_control))
    {
        bail!("--onepassword-account must be a non-empty account selector");
    }
    let mut keys = BTreeSet::new();
    for field in fields {
        field.validate().map_err(anyhow::Error::msg)?;
        if !keys.insert(&field.key) {
            bail!("Duplicate 1Password field key; assign distinct keys with KEY=op://VAULT/ITEM/FIELD");
        }
    }
    Ok(())
}

pub fn suggested_title(fields: &[OnePasswordFieldReference]) -> String {
    let item = |field: &OnePasswordFieldReference| {
        field
            .reference
            .strip_prefix("op://")
            .and_then(|path| path.split('/').nth(1))
            .unwrap_or_default()
            .to_string()
    };
    let first = fields.first().map(item).unwrap_or_default();
    if !first.is_empty() && fields.iter().all(|field| item(field) == first) {
        first
    } else {
        "1Password import".into()
    }
}

pub async fn read_fields(
    fields: &[OnePasswordFieldReference],
    account: Option<&str>,
) -> Result<Vec<SelectedPasswordEntry>> {
    let program =
        std::env::var_os("PLANKTON_1PASSWORD_CLI_BIN").unwrap_or_else(|| OsString::from("op"));
    read_fields_with_program(
        Path::new(&program),
        fields,
        account,
        Duration::from_secs(120),
    )
    .await
}

async fn read_fields_with_program(
    program: &Path,
    fields: &[OnePasswordFieldReference],
    account: Option<&str>,
    deadline: Duration,
) -> Result<Vec<SelectedPasswordEntry>> {
    validate_selection(fields, account)?;
    let mut entries = Vec::with_capacity(fields.len());
    for field in fields {
        let mut args = vec![
            OsString::from("read"),
            OsString::from(&field.reference),
            OsString::from("--no-newline"),
        ];
        if let Some(account) = account {
            args.extend([OsString::from("--account"), OsString::from(account)]);
        }
        plankton_core::resources::onepassword_command::validate_ai_read(&args)?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = timeout(deadline, command.output()).await
            .map_err(|_| anyhow::anyhow!("1Password read timed out. Unlock 1Password and retry."))?
            .map_err(|_| anyhow::anyhow!("Could not start 1Password CLI. Install op and enable its desktop integration or sign in, then retry."))?;
        if !output.status.success() {
            // Backend stdout/stderr may contain values; neither belongs in CLI diagnostics.
            bail!("1Password could not read field {} (exit {:?}). Unlock or sign in to 1Password and check the account, vault, item, and field reference.", field.key, output.status.code());
        }
        let value = String::from_utf8(output.stdout).map_err(|_| {
            anyhow::anyhow!("1Password returned a non-text value; choose a text password field.")
        })?;
        if value.is_empty() {
            bail!(
                "1Password field {} is empty; no draft was created.",
                field.key
            );
        }
        entries.push(SelectedPasswordEntry {
            key: field.key.clone(),
            value,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_references_and_aliases_are_validated_before_reading() {
        assert_eq!(
            parse_selection("op://Work/GitHub/password").unwrap().key,
            "password"
        );
        assert_eq!(
            parse_selection("TOKEN=op://Work/Service/Section/token")
                .unwrap()
                .key,
            "TOKEN"
        );
        assert_eq!(
            parse_selection("op://Work/OTP/one-time password?attribute=otp")
                .unwrap()
                .key,
            "one-time password"
        );
        for invalid in [
            "",
            "plain-text",
            "op://vault/item",
            "op://vault//password",
            "=op://v/i/f",
            "op://v/i/f\n",
        ] {
            assert!(parse_selection(invalid).is_err());
        }
        let duplicate = [
            parse_selection("op://v/a/password").unwrap(),
            parse_selection("op://v/b/password").unwrap(),
        ];
        assert!(validate_selection(&duplicate, None).is_err());
        assert!(validate_selection(&duplicate[..1], Some(" ")).is_err());
    }

    #[cfg(unix)]
    fn fixture(script: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("op");
        std::fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        (directory, program)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reads_only_selected_fields_with_literal_arguments_and_preserves_whitespace() {
        let (_directory, program) = fixture(
            r#"
[ "$1" = read ] && [ "$3" = --no-newline ] && [ "$4" = --account ] && [ "$5" = 'account name' ] || exit 7
case "$2" in
  'op://Work vault/Service/password') printf '  test-password\n' ;;
  'op://Work vault/Service/user name') printf 'test-user' ;;
  *) exit 8 ;;
esac
"#,
        );
        let fields = [
            parse_selection("PASSWORD=op://Work vault/Service/password").unwrap(),
            parse_selection("USERNAME=op://Work vault/Service/user name").unwrap(),
        ];
        let entries = read_fields_with_program(
            &program,
            &fields,
            Some("account name"),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(entries[0].value, "  test-password\n");
        assert_eq!(entries[1].value, "test-user");
        assert!(!format!("{entries:?}").contains("test-password"));
        assert_eq!(suggested_title(&fields), "Service");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failures_invalid_text_empty_values_and_timeouts_never_include_backend_output() {
        let fields = [parse_selection("op://v/i/password").unwrap()];
        for script in [
            "printf SECRET_SENTINEL; printf SECRET_SENTINEL >&2; exit 1",
            r"printf '\377SECRET_SENTINEL'",
            "exit 0",
            "exec sleep 2",
        ] {
            let (_directory, program) = fixture(script);
            let error =
                read_fields_with_program(&program, &fields, None, Duration::from_millis(100))
                    .await
                    .unwrap_err();
            assert!(!format!("{error:#}").contains("SECRET_SENTINEL"));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_second_field_returns_no_partial_draft_values() {
        let (_directory, program) = fixture("case \"$2\" in 'op://v/i/first') printf FIRST_SECRET_SENTINEL;; *) printf SECOND_SECRET_SENTINEL >&2; exit 2;; esac");
        let fields = [
            parse_selection("op://v/i/first").unwrap(),
            parse_selection("op://v/i/second").unwrap(),
        ];
        let error = read_fields_with_program(&program, &fields, None, Duration::from_secs(2))
            .await
            .unwrap_err();
        assert!(!format!("{error:#}").contains("SECRET_SENTINEL"));
    }
}
