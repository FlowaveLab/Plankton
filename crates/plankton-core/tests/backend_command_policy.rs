use std::ffi::OsString;

use plankton_core::resources::{bitwarden_command, onepassword_command, CommandPolicyError};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn onepassword_ai_proxy_allows_only_search_and_get_operations() {
    for allowed in [
        &["item", "list", "--format=json", "--tags", "prod"][..],
        &[
            "item",
            "get",
            "GitHub",
            "--vault",
            "Engineering",
            "--reveal",
        ],
        &["vault", "list", "--format", "json"],
        &["read", "op://Engineering/GitHub/token"],
        &["whoami"],
    ] {
        onepassword_command::validate_ai_read(&args(allowed)).expect("read command should pass");
    }

    for rejected in [
        &["item", "create", "--category", "login"][..],
        &["item", "edit", "GitHub"],
        &["item", "delete", "GitHub"],
        &["document", "get", "artifact", "--out-file", "/tmp/leak"],
        &["run", "--", "env"],
        &["item", "get", "GitHub", "--unknown"],
    ] {
        assert!(
            onepassword_command::validate_ai_read(&args(rejected)).is_err(),
            "{rejected:?}"
        );
    }
}

#[test]
fn bitwarden_ai_proxy_allows_only_list_and_get_operations() {
    for allowed in [
        &["status"][..],
        &["list", "items", "--search", "github"],
        &["list", "items", "--organizationid=org"],
        &["get", "item", "item-id"],
        &["get", "password", "item-id", "--raw"],
    ] {
        bitwarden_command::validate_ai_read(&args(allowed)).expect("read command should pass");
    }

    for rejected in [
        &["create", "item", "payload"][..],
        &["edit", "item", "id", "payload"],
        &["delete", "item", "id"],
        &["export", "--output", "/tmp/vault.json"],
        &["list", "items", "--unknown"],
    ] {
        assert!(
            bitwarden_command::validate_ai_read(&args(rejected)).is_err(),
            "{rejected:?}"
        );
    }
}

#[test]
fn write_rejection_is_explicit_not_silently_empty() {
    assert!(matches!(
        bitwarden_command::validate_ai_read(&args(&["create", "item", "payload"])),
        Err(CommandPolicyError::WriteCommand {
            backend: "Bitwarden"
        })
    ));
}
