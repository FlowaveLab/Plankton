use std::ffi::OsString;

use super::CommandPolicyError;

const READ_COMMANDS: &[&[&str]] = &[
    &["status"],
    &["list", "items"],
    &["list", "folders"],
    &["list", "organizations"],
    &["list", "collections"],
    &["get", "item"],
    &["get", "folder"],
    &["get", "organization"],
    &["get", "collection"],
    &["get", "username"],
    &["get", "password"],
    &["get", "totp"],
];
const VALUE_FLAGS: &[&str] = &[
    "--organizationid",
    "--collectionid",
    "--folderid",
    "--search",
    "--url",
];
const SWITCH_FLAGS: &[&str] = &["--pretty", "--raw", "--response"];
const FORBIDDEN_COMMANDS: &[&str] = &[
    "create", "edit", "delete", "restore", "move", "import", "export", "send", "config",
    "generate", "encode",
];
const FORBIDDEN_FLAGS: &[&str] = &["--output", "--file", "--cleanexit", "--session"];

pub fn validate_ai_read(args: &[OsString]) -> Result<(), CommandPolicyError> {
    let backend = "Bitwarden";
    let utf8 = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .ok_or(CommandPolicyError::NonUtf8Argument { backend })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if utf8
        .first()
        .is_some_and(|command| FORBIDDEN_COMMANDS.contains(command))
    {
        return Err(CommandPolicyError::WriteCommand { backend });
    }
    if utf8.iter().any(|arg| {
        FORBIDDEN_FLAGS
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    }) {
        return Err(CommandPolicyError::FileOrSessionFlag { backend });
    }
    let command = READ_COMMANDS
        .iter()
        .find(|command| utf8.starts_with(command))
        .ok_or_else(|| CommandPolicyError::UnsupportedCommand {
            backend,
            command: utf8.join(" "),
        })?;

    let mut index = command.len();
    while index < utf8.len() {
        let arg = utf8[index];
        if !arg.starts_with('-') {
            index += 1;
            continue;
        }
        if SWITCH_FLAGS.contains(&arg) {
            index += 1;
            continue;
        }
        if let Some((flag, _)) = arg.split_once('=') {
            if VALUE_FLAGS.contains(&flag) {
                index += 1;
                continue;
            }
        }
        if VALUE_FLAGS.contains(&arg) && index + 1 < utf8.len() && !utf8[index + 1].starts_with('-')
        {
            index += 2;
            continue;
        }
        return Err(CommandPolicyError::UnsupportedFlag {
            backend,
            flag: arg.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_session_tokens_even_for_read_commands() {
        let error = validate_ai_read(&[
            OsString::from("get"),
            OsString::from("password"),
            OsString::from("item-id"),
            OsString::from("--session"),
            OsString::from("sensitive-token"),
        ])
        .expect_err("session material must never enter the AI command surface");

        assert_eq!(
            error,
            CommandPolicyError::FileOrSessionFlag {
                backend: "Bitwarden"
            }
        );
    }

    #[test]
    fn accepts_search_filters_without_a_session_token() {
        validate_ai_read(&[
            OsString::from("list"),
            OsString::from("items"),
            OsString::from("--search"),
            OsString::from("production"),
        ])
        .expect("read-only search flags should remain available");
    }
}
