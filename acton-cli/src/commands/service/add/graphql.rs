//! `acton service add graphql` — scaffold the GraphQL transport into an
//! existing service.

use anyhow::{Context as _, Result};
use colored::Colorize;
use std::path::Path;

use crate::templates::graphql;
use crate::utils;

pub async fn execute(version: String, cedar: bool, dry_run: bool) -> Result<()> {
    let version_upper = version.to_uppercase();
    let project_path = Path::new(".");
    let src_path = project_path.join("src");
    let graphql_path = src_path.join("graphql.rs");
    let cargo_path = project_path.join("Cargo.toml");

    if dry_run {
        show_dry_run(&version_upper, cedar, cargo_path.display());
        return Ok(());
    }

    if !src_path.exists() {
        anyhow::bail!("src/ directory not found. Run this command from a service project root.");
    }

    if graphql_path.exists() {
        anyhow::bail!(
            "{} already exists — refusing to overwrite. Remove or merge manually.",
            graphql_path.display()
        );
    }

    utils::write_file(&graphql_path, &graphql::generate_module_with_cedar(cedar))?;
    println!("{} created {}", "✓".green().bold(), graphql_path.display());

    if cargo_path.exists() {
        ensure_feature_in_cargo(&cargo_path, cedar)?;
    } else {
        println!(
            "{} Cargo.toml not found — add the `graphql{}` feature manually",
            "!".yellow().bold(),
            if cedar { ",graphql-cedar" } else { "" }
        );
    }

    println!();
    println!("{}", "Next steps".bold());
    println!("  1. Add `mod graphql;` to src/main.rs");
    println!(
        "  2. Wire the schema into ServiceBuilder:\n     .with_versioned_graphql(crate::graphql::build())"
    );
    println!("  3. cargo run, then visit http://localhost:8080/api/v1/graphql");

    Ok(())
}

fn ensure_feature_in_cargo(cargo_path: &Path, cedar: bool) -> Result<()> {
    let contents = std::fs::read_to_string(cargo_path)
        .with_context(|| format!("read {}", cargo_path.display()))?;

    let needed: Vec<&str> = if cedar {
        vec!["graphql", "graphql-cedar"]
    } else {
        vec!["graphql"]
    };

    let mut new_contents = contents.clone();
    for feat in &needed {
        if !contents.contains(&format!("\"{feat}\"")) {
            // Look for an `acton-service = { ... features = [...] }` line and
            // append. We do this with a regex-light, line-based pass so we
            // don't have to ship `toml_edit` for one feature.
            new_contents = inject_feature(&new_contents, feat);
        }
    }

    if new_contents != contents {
        std::fs::write(cargo_path, new_contents)
            .with_context(|| format!("write {}", cargo_path.display()))?;
        println!(
            "{} updated Cargo.toml acton-service features",
            "✓".green().bold()
        );
    }
    Ok(())
}

fn inject_feature(toml: &str, feat: &str) -> String {
    let mut out = String::with_capacity(toml.len() + 32);
    let mut modified = false;
    for line in toml.lines() {
        let trimmed = line.trim_start();
        if !modified
            && trimmed.starts_with("acton-service")
            && line.contains("features")
            && line.contains(']')
        {
            // Single-line table: insert before the closing bracket.
            if let Some(idx) = line.rfind(']') {
                let (head, tail) = line.split_at(idx);
                let sep = if head.trim_end().ends_with('[') {
                    ""
                } else {
                    ", "
                };
                out.push_str(&format!("{head}{sep}\"{feat}\"{tail}"));
                out.push('\n');
                modified = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !modified {
        // Fall back: append a comment for the user. We don't try to parse
        // multi-line tables here — the user will see the warning.
        out.push_str(&format!(
            "\n# TODO: add \"{feat}\" to the acton-service features list\n"
        ));
    }
    out
}

fn show_dry_run(version: &str, cedar: bool, cargo_path: std::path::Display<'_>) {
    println!("\n{}", "Dry run - would generate:".bold());
    println!("  • src/graphql.rs (Query root resolver for {})", version);
    println!(
        "  • {} (add `graphql{}` feature)",
        cargo_path,
        if cedar { ",graphql-cedar" } else { "" }
    );
}
