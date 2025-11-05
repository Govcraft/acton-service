use anyhow::{Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::fs;

pub mod format;
pub mod git;
pub mod cargo;

/// Validate service name is kebab-case
pub fn validate_service_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Service name cannot be empty");
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!(
            "Service name must be kebab-case (lowercase letters, numbers, hyphens only)\n\n\
            Valid examples:\n\
            • my-service\n\
            • user-api\n\
            • auth-service-v2"
        );
    }

    if name.starts_with('-') || name.ends_with('-') {
        anyhow::bail!("Service name cannot start or end with a hyphen");
    }

    if name.len() > 64 {
        anyhow::bail!("Service name must be 64 characters or less");
    }

    Ok(())
}

/// Create directory and all parent directories
pub fn create_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("Failed to create directory: {}", path.display()))
}

/// Write file with content
pub fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("Failed to write file: {}", path.display()))
}

/// Check if directory exists and is empty
#[allow(dead_code)]
pub fn is_dir_empty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }

    if !path.is_dir() {
        anyhow::bail!("Path exists but is not a directory: {}", path.display());
    }

    let entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory: {}", path.display()))?;

    Ok(entries.count() == 0)
}

/// Get project root directory (where Cargo.toml is)
pub fn find_project_root() -> Result<PathBuf> {
    let current_dir = std::env::current_dir()
        .context("Failed to get current directory")?;

    let mut dir = current_dir.as_path();

    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            return Ok(dir.to_path_buf());
        }

        match dir.parent() {
            Some(parent) => dir = parent,
            None => anyhow::bail!("Could not find Cargo.toml in current directory or any parent directory"),
        }
    }
}

/// Success message with checkmark
pub fn success(message: &str) {
    println!("{} {}", "✓".green().bold(), message);
}

/// Info message
#[allow(dead_code)]
pub fn info(message: &str) {
    println!("{} {}", "→".blue().bold(), message);
}

/// Warning message
pub fn warning(message: &str) {
    println!("{} {}", "⚠".yellow().bold(), message);
}

/// Error message
#[allow(dead_code)]
pub fn error(message: &str) {
    eprintln!("{} {}", "✗".red().bold(), message);
}

/// Section header
#[allow(dead_code)]
pub fn section(title: &str) {
    println!("\n{}", title.bold().underline());
}
