use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Run cargo fmt on a project
pub fn fmt(path: &Path) -> Result<()> {
    let output = Command::new("cargo")
        .args(["fmt"])
        .current_dir(path)
        .output()
        .context("Failed to run cargo fmt")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("cargo fmt failed: {}", stderr);
    }

    Ok(())
}

/// Check if cargo is available
pub fn is_available() -> bool {
    Command::new("cargo")
        .args(["--version"])
        .output()
        .is_ok()
}

/// Run cargo check to verify the project compiles
#[allow(dead_code)]
pub fn check(path: &Path) -> Result<()> {
    let output = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(path)
        .output()
        .context("Failed to run cargo check")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("cargo check failed: {}", stderr);
    }

    Ok(())
}
