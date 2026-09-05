use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

const FRONTEND_INPUTS: &[&str] = &[
    "../index.html",
    "../package-lock.json",
    "../package.json",
    "../postcss.config.cjs",
    "../src",
    "../tailwind.config.cjs",
    "../vite.config.ts",
];

fn main() {
    let revision =
        frontend_revision(Path::new(".")).expect("failed to calculate frontend cache revision");
    println!("cargo:rustc-env=PLANKTON_FRONTEND_REVISION={revision}");
    for input in FRONTEND_INPUTS {
        println!("cargo:rerun-if-changed={input}");
    }
    tauri_build::build()
}

fn frontend_revision(root: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    for input in FRONTEND_INPUTS {
        collect_files(&root.join(input), &mut files)?;
    }
    files.sort();

    let mut hasher = Sha256::new();
    for path in files {
        let normalized = path.to_string_lossy().replace('\\', "/");
        hasher.update(normalized.as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(path)?);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }

    let mut children = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    children.sort();
    for child in children {
        if child.is_dir() {
            collect_files(&child, files)?;
        } else if child.is_file() {
            files.push(child);
        }
    }
    Ok(())
}
