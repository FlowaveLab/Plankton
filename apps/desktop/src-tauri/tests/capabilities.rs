use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;

fn capability(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("capabilities")
        .join(name);
    let bytes = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read capability {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("capability {} is invalid JSON: {error}", path.display()))
}

#[test]
fn approval_window_has_only_event_subscription_and_hide_permissions() {
    let approval = capability("approval.json");
    let windows = approval["windows"]
        .as_array()
        .expect("approval capability windows must be an array");
    assert_eq!(windows, &[Value::String("approval".to_string())]);

    let permissions = approval["permissions"]
        .as_array()
        .expect("approval capability permissions must be an array")
        .iter()
        .map(|permission| {
            permission
                .as_str()
                .expect("approval permissions must be strings")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        permissions,
        BTreeSet::from([
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "core:window:allow-hide",
        ])
    );
}

#[test]
fn password_change_window_can_only_hide_itself() {
    let confirmation = capability("password-change.json");
    let windows = confirmation["windows"]
        .as_array()
        .expect("password change capability windows must be an array");
    assert_eq!(windows, &[Value::String("password-change".to_string())]);

    let permissions = confirmation["permissions"]
        .as_array()
        .expect("password change permissions must be an array")
        .iter()
        .map(|permission| {
            permission
                .as_str()
                .expect("password change permissions must be strings")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(permissions, BTreeSet::from(["core:window:allow-hide"]));
}
