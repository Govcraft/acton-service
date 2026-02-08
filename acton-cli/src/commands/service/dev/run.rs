use anyhow::{Context, Result};
use colored::Colorize;
use std::process::{Command, Stdio};

use crate::utils;

pub async fn execute(watch: bool, port: Option<u16>) -> Result<()> {
    // Find project root
    let project_root = utils::find_project_root()
        .context("Not in a service project directory. Run this command from within a service created with 'acton service new'")?;

    // Prepare environment variables
    let mut env_vars = vec![];
    if let Some(p) = port {
        env_vars.push(("ACTON_SERVICE_PORT", p.to_string()));
    }

    if watch {
        run_with_watch(&project_root, &env_vars)
    } else {
        run_service(&project_root, &env_vars)
    }
}

fn run_with_watch(project_root: &std::path::Path, env_vars: &[(&str, String)]) -> Result<()> {
    // Check if cargo-watch is installed
    let check_watch = Command::new("cargo")
        .arg("watch")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if check_watch.is_err() || !check_watch.unwrap().success() {
        utils::warning("cargo-watch is not installed");
        println!();
        println!("Install it with: cargo install cargo-watch");
        println!("Or run without watch: acton service dev run");
        println!();
        println!("Falling back to regular run...");
        println!();
        return run_service(project_root, env_vars);
    }

    println!("{}", "Running service with hot reload...".bold());
    println!(
        "  {}",
        "Watching for changes. Press Ctrl+C to stop.".dimmed()
    );
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("watch")
        .arg("-x")
        .arg("run")
        .current_dir(project_root);

    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    let status = cmd.status().context("Failed to run cargo watch")?;

    if !status.success() {
        anyhow::bail!(
            "Service exited with error code: {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

fn run_service(project_root: &std::path::Path, env_vars: &[(&str, String)]) -> Result<()> {
    println!("{}", "Running service...".bold());
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("run").current_dir(project_root);

    for (key, value) in env_vars {
        cmd.env(key, value);
        println!("  {} = {}", key.cyan(), value);
    }

    if !env_vars.is_empty() {
        println!();
    }

    let status = cmd.status().context("Failed to run cargo")?;

    if !status.success() {
        anyhow::bail!(
            "Service exited with error code: {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}
