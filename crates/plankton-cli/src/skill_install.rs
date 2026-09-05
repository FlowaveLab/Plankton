use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};

const BUNDLED_SKILL_NAME: &str = "secret-access";
const BUNDLED_SKILL_MD: &str = include_str!("../../../.codex/skills/secret-access/SKILL.md");
const SKILLS_CLI_PACKAGE: &str = "skills@1.5.18";

pub fn install_bundled_skill(agents: &[String]) -> Result<()> {
    let staging = tempfile::Builder::new()
        .prefix("plankton-skill-install-")
        .tempdir()
        .context("failed to create bundled skill staging directory")?;
    let skill_dir = write_bundled_skill(staging.path())?;

    let status = Command::new("npx")
        .args(skills_cli_arguments(&skill_dir, agents))
        .env("AI_AGENT", "plankton")
        .env("DISABLE_TELEMETRY", "1")
        .status()
        .context("failed to launch npx; install Node.js 18 or newer before installing the skill")?;

    if !status.success() {
        bail!("Vercel Skills CLI failed with status {status}");
    }

    Ok(())
}

pub fn bundled_skill_name() -> &'static str {
    BUNDLED_SKILL_NAME
}

pub fn bundled_skill_markdown() -> &'static str {
    BUNDLED_SKILL_MD
}

fn write_bundled_skill(staging_dir: &Path) -> Result<PathBuf> {
    let skill_dir = staging_dir.join(BUNDLED_SKILL_NAME);
    fs::create_dir(&skill_dir).context("failed to create bundled skill directory")?;
    fs::write(skill_dir.join("SKILL.md"), BUNDLED_SKILL_MD)
        .context("failed to stage the bundled secret-access skill")?;
    Ok(skill_dir)
}

fn skills_cli_arguments(skill_dir: &Path, agents: &[String]) -> Vec<OsString> {
    let mut arguments = Vec::from([
        OsString::from("--yes"),
        OsString::from(SKILLS_CLI_PACKAGE),
        OsString::from("add"),
        skill_dir.as_os_str().to_owned(),
        OsString::from("--skill"),
        OsString::from(BUNDLED_SKILL_NAME),
        OsString::from("--global"),
        OsString::from("--yes"),
        OsString::from("--agent"),
    ]);
    arguments.extend(agents.iter().map(OsString::from));
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_is_the_repository_skill() {
        assert!(BUNDLED_SKILL_MD.starts_with("---\nname: secret-access\n"));
        assert!(BUNDLED_SKILL_MD.contains("# Secret Access"));
    }

    #[test]
    fn stages_the_embedded_skill_as_a_valid_skill_directory() {
        let staging = tempfile::tempdir().expect("temp dir");

        let skill_dir = write_bundled_skill(staging.path()).expect("stage skill");

        assert_eq!(
            skill_dir.file_name().and_then(|name| name.to_str()),
            Some("secret-access")
        );
        assert_eq!(
            fs::read_to_string(skill_dir.join("SKILL.md")).expect("read staged skill"),
            BUNDLED_SKILL_MD
        );
    }

    #[test]
    fn invokes_the_pinned_vercel_installer_for_the_bundled_skill() {
        let skill_dir = Path::new("/tmp/plankton-skill-install/secret-access");

        let args = skills_cli_arguments(skill_dir, &["codex".into(), "claude-code".into()]);

        assert_eq!(
            args,
            [
                "--yes",
                "skills@1.5.18",
                "add",
                "/tmp/plankton-skill-install/secret-access",
                "--skill",
                "secret-access",
                "--global",
                "--yes",
                "--agent",
                "codex",
                "claude-code",
            ]
            .map(OsString::from)
        );
    }
}
