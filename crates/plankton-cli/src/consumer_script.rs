use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{bail, Context, Result};
use plankton_core::read_allowlisted_paths_file;

/// Add a declared consumer without pretending it was found in the process ancestry.
/// Only the path crosses the request boundary; source is read by the existing review tools.
pub fn prepare_script_context(
    collected_script_path: Option<String>,
    consumer_script: Option<&Path>,
    metadata: &mut BTreeMap<String, String>,
) -> Result<Option<String>> {
    metadata.remove("script_path_source");
    metadata.remove("collected_script_path");
    let Some(consumer_script) = consumer_script else {
        return Ok(collected_script_path);
    };

    let canonical = fs::canonicalize(consumer_script)
        .context("--consumer-script must name an existing readable script file")?;
    if !canonical.is_file() {
        bail!("--consumer-script must name a regular script file");
    }
    let path = canonical
        .to_str()
        .context("--consumer-script path must be valid UTF-8")?
        .to_string();
    let preview = read_allowlisted_paths_file(std::slice::from_ref(&path), &path)
        .context("--consumer-script must be a readable text script within the preview limit")?;
    if preview.truncated {
        bail!("--consumer-script exceeds the complete source preview limit (64 KiB)");
    }

    metadata.insert("script_path_source".into(), "declared_consumer".into());
    if let Some(collected) = collected_script_path {
        metadata.insert("collected_script_path".into(), collected);
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_only_the_canonical_path_and_preserves_the_collected_script() {
        let dir = tempfile::tempdir().expect("temp directory");
        let script = dir.path().join("consumer.py");
        let contents = "# fixture source marker\nimport sys\nvalue = sys.stdin.read()\n";
        fs::write(&script, contents).expect("write fixture");
        let mut metadata = BTreeMap::from([("purpose".into(), "call the API".into())]);

        let path = prepare_script_context(
            Some("/workspace/wrapper.sh".into()),
            Some(&script),
            &mut metadata,
        )
        .expect("declare consumer");

        assert_eq!(path.as_deref(), script.canonicalize().unwrap().to_str());
        assert_eq!(metadata["script_path_source"], "declared_consumer");
        assert_eq!(metadata["collected_script_path"], "/workspace/wrapper.sh");
        assert_eq!(metadata["purpose"], "call the API");
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("fixture source marker"));
        assert_eq!(fs::read_to_string(&script).unwrap(), contents);
    }

    #[test]
    fn keeps_automatic_context_when_no_consumer_was_declared() {
        let mut metadata = BTreeMap::from([
            ("script_path_source".into(), "declared_consumer".into()),
            ("collected_script_path".into(), "/incorrect/path".into()),
        ]);
        let path = prepare_script_context(Some("/workspace/run.py".into()), None, &mut metadata)
            .expect("automatic context");
        assert_eq!(path.as_deref(), Some("/workspace/run.py"));
        assert!(metadata.is_empty());
    }

    #[test]
    fn rejects_missing_directories_binary_and_truncated_sources() {
        let dir = tempfile::tempdir().expect("temp directory");
        let missing = dir.path().join("missing.py");
        let binary = dir.path().join("binary.py");
        let oversized = dir.path().join("oversized.py");
        fs::write(&binary, b"\0binary").unwrap();
        fs::write(&oversized, "x".repeat(64 * 1024 + 1)).unwrap();
        for path in [
            missing.as_path(),
            dir.path(),
            binary.as_path(),
            oversized.as_path(),
        ] {
            let mut metadata = BTreeMap::new();
            assert!(prepare_script_context(None, Some(path), &mut metadata).is_err());
            assert!(metadata.is_empty());
        }
    }
}
