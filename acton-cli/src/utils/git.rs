use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Initialize git repository
pub fn init(path: &Path) -> Result<()> {
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .context("Failed to initialize git repository")?;

    Ok(())
}

/// Check if git is available
pub fn is_available() -> bool {
    Command::new("git")
        .args(["--version"])
        .output()
        .is_ok()
}
