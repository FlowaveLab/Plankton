//! pi-acp 0.0.34 reports an upstream model error as an empty end_turn.
//! Recover a value-free diagnosis only from the matching Pi review session.
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use chrono::{DateTime, Utc};
use serde_json::Value;

pub(super) fn model_error(
    home: &Path,
    cwd: &Path,
    session_id: &str,
    started: DateTime<Utc>,
) -> Option<&'static str> {
    let map = home.join(".pi/pi-acp/session-map.json");
    let info = fs::symlink_metadata(&map).ok()?;
    if !info.is_file() || info.len() > 4 * 1024 * 1024 {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(map)
        .ok()?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let index: Value = serde_json::from_slice(&bytes).ok()?;
    let entry = index.get("sessions")?.get(session_id)?;
    if entry.get("sessionId")?.as_str()? != session_id
        || Path::new(entry.get("cwd")?.as_str()?) != cwd
    {
        return None;
    }
    let path = Path::new(entry.get("sessionFile")?.as_str()?);
    if !path
        .file_name()?
        .to_str()?
        .ends_with(&format!("_{session_id}.jsonl"))
        || !fs::symlink_metadata(path).ok()?.is_file()
    {
        return None;
    }
    let mut session = File::open(path).ok()?;
    let start = session.metadata().ok()?.len().saturating_sub(64 * 1024);
    session.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    session.take(64 * 1024).read_to_end(&mut tail).ok()?;
    // Only classify the final event. Older turns cannot explain a new failure.
    let line = tail.rsplit(|b| *b == b'\n').find(|line| !line.is_empty())?;
    let event: Value = serde_json::from_slice(line).ok()?;
    if event.get("type")?.as_str()? != "message"
        || event
            .get("timestamp")?
            .as_str()?
            .parse::<DateTime<Utc>>()
            .ok()?
            .timestamp_millis()
            < started.timestamp_millis()
    {
        return None;
    }
    let message = event.get("message")?;
    if message.get("role")?.as_str()? != "assistant"
        || message.get("stopReason")?.as_str()? != "error"
    {
        return None;
    }
    let error = message.get("errorMessage")?.as_str()?;
    Some(
        if error.starts_with("402:") && error.to_ascii_lowercase().contains("insufficient balance")
        {
            "Pi model provider returned HTTP 402 (insufficient balance)"
        } else {
            "Pi model provider reported an error before returning a decision"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_current_sessions_latest_assistant_error_can_explain_empty_output() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        let session = root.path().join("date_test.jsonl");
        let map = root.path().join(".pi/pi-acp");
        fs::create_dir_all(&map).unwrap();
        fs::write(
            map.join("session-map.json"),
            json!({"sessions":{"test":{"sessionId":"test","cwd":cwd,"sessionFile":session}}})
                .to_string(),
        )
        .unwrap();
        let now = Utc::now();
        let event = json!({"timestamp":now,"type":"message","message":{"role":"assistant","stopReason":"error","errorMessage":"402: Insufficient Balance (private upstream detail)"}});
        fs::write(&session, format!("{}\n", event)).unwrap();
        assert_eq!(
            model_error(root.path(), &cwd, "test", now),
            Some("Pi model provider returned HTTP 402 (insufficient balance)")
        );
        assert_eq!(model_error(root.path(), &cwd, "other", now), None);
        assert_eq!(
            model_error(
                root.path(),
                &cwd,
                "test",
                now + chrono::Duration::seconds(1)
            ),
            None
        );
        let success = json!({"type":"message","timestamp":now,"message":{"role":"assistant","stopReason":"stop"}});
        fs::write(&session, format!("{event}\n{success}\n")).unwrap();
        assert_eq!(model_error(root.path(), &cwd, "test", now), None);
    }
}
