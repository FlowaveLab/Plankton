use std::ffi::OsString;

use super::CommandPolicyError;

const READ_COMMANDS: &[&[&str]] = &[
    &["account", "list"],
    &["account", "get"],
    &["vault", "list"],
    &["vault", "get"],
    &["item", "list"],
    &["item", "get"],
    &["read"],
    &["whoami"],
];

const VALUE_FLAGS: &[&str] = &["--account", "--vault", "--fields", "--format", "--encoding"];
const SWITCH_FLAGS: &[&str] = &[
    "--cache",
    "--no-color",
    "--no-newline",
    "--iso-timestamps",
    "--reveal",
    "--include-archive",
    "--categories",
    "--tags",
    "--favorite",
];
const FORBIDDEN_FLAGS: &[&str] = &[
    "--out-file",
    "--output",
    "--force",
    "--dry-run",
    "--session",
];

pub fn validate_ai_read(args: &[OsString]) -> Result<(), CommandPolicyError> {
    validate_argv("1Password", args, READ_COMMANDS, VALUE_FLAGS, SWITCH_FLAGS)
}

fn validate_argv(
    backend: &'static str,
    args: &[OsString],
    commands: &[&[&str]],
    value_flags: &[&str],
    switch_flags: &[&str],
) -> Result<(), CommandPolicyError> {
    let utf8 = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .ok_or(CommandPolicyError::NonUtf8Argument { backend })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if utf8.iter().any(|arg| {
        FORBIDDEN_FLAGS
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    }) {
        return Err(CommandPolicyError::FileOrSessionFlag { backend });
    }
    let command = commands
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
        if switch_flags.contains(&arg) {
            index += 1;
            continue;
        }
        if let Some((flag, _)) = arg.split_once('=') {
            if value_flags.contains(&flag) {
                index += 1;
                continue;
            }
        }
        if value_flags.contains(&arg) && index + 1 < utf8.len() && !utf8[index + 1].starts_with('-')
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
