use std::{fs, time::Duration};

use plankton_core::passwords::{
    parse_password_source_descriptor, ConfirmationError, ConfirmationLedger, FileFormat,
    PasswordDestination, PasswordSourceDescriptor,
};
use tempfile::tempdir;

#[test]
fn parses_dotenv_json_and_yaml_with_explicit_key_selection() {
    let temp = tempdir().expect("temp");
    let fixtures = [
        (".env", "TOKEN=dotenv-secret\nOTHER=skip\n", "TOKEN"),
        (
            "secrets.json",
            r#"{"service":{"token":"json-secret"},"skip":"x"}"#,
            "service.token",
        ),
        (
            "secrets.yml",
            "service:\n  token: yaml-secret\nskip: x\n",
            "service.token",
        ),
    ];

    for (name, contents, key) in fixtures {
        let path = temp.path().join(name);
        fs::write(&path, contents).expect("fixture");
        let parsed = parse_password_source_descriptor(PasswordSourceDescriptor::File {
            path,
            format: FileFormat::Auto,
            keys: vec![key.into()],
        })
        .expect("source parses");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].key, key);
        assert!(parsed.entries[0].value.ends_with("-secret"));
    }
}

#[test]
fn confirmation_is_single_use_and_bound_to_destination_and_content() {
    let temp = tempdir().expect("temp");
    let path = temp.path().join(".env");
    fs::write(&path, "TOKEN=first\n").expect("fixture");
    let descriptor = PasswordSourceDescriptor::File {
        path: path.clone(),
        format: FileFormat::Dotenv,
        keys: vec!["TOKEN".into()],
    };
    let parsed = parse_password_source_descriptor(descriptor.clone()).expect("parse");
    let mut ledger = ConfirmationLedger::new(Duration::from_secs(60));
    let draft_id = ledger.create_draft(parsed);
    let destination = PasswordDestination::Plankton {
        vault_id: "personal".into(),
    };
    let grant = ledger
        .confirm(draft_id, destination.clone())
        .expect("confirm");

    let wrong_destination = PasswordDestination::External {
        binding_id: "op".into(),
        vault_id: "work".into(),
    };
    assert!(matches!(
        ledger.consume(&grant.token, draft_id, &wrong_destination),
        Err(ConfirmationError::BindingMismatch)
    ));
    assert!(matches!(
        ledger.consume(&grant.token, draft_id, &destination),
        Err(ConfirmationError::InvalidOrConsumed)
    ));

    let new_grant = ledger
        .confirm(draft_id, destination.clone())
        .expect("reconfirm");
    fs::write(&path, "TOKEN=second\n").expect("change fixture");
    let changed = parse_password_source_descriptor(descriptor).expect("reparse");
    ledger
        .replace_draft(draft_id, changed)
        .expect("replace draft");
    assert!(matches!(
        ledger.consume(&new_grant.token, draft_id, &destination),
        Err(ConfirmationError::ContentChanged)
    ));
}
