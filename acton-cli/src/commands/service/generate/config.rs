use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::templates::{config, ServiceTemplate};
use crate::utils;

pub async fn execute(output: Option<String>, examples: bool, dry_run: bool) -> Result<()> {
    // Find project root
    let project_root = utils::find_project_root()
        .context("Not in a service project directory. Run this command from within a service created with 'acton service new'")?;

    // Read Cargo.toml to determine service configuration
    let cargo_toml_path = project_root.join("Cargo.toml");
    let cargo_content =
        fs::read_to_string(&cargo_toml_path).context("Failed to read Cargo.toml")?;

    // Parse service template from Cargo.toml
    let template = parse_service_config(&cargo_content)?;

    // Generate config
    let config_content = if examples {
        add_examples(&config::generate(&template))
    } else {
        config::generate(&template)
    };

    if dry_run {
        show_dry_run(&config_content, &output);
        return Ok(());
    }

    // Determine output path
    let output_path = if let Some(path) = output {
        project_root.join(path)
    } else {
        project_root.join("config.toml")
    };

    // Check if file already exists
    if output_path.exists() {
        utils::warning(&format!(
            "Config file already exists at: {}",
            output_path.display()
        ));
        println!("\nOverwrite? (y/N): ");

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled");
            return Ok(());
        }
    }

    // Write config file
    fs::write(&output_path, config_content).context("Failed to write config file")?;

    show_success(&output_path);

    Ok(())
}

fn parse_service_config(cargo_toml: &str) -> Result<ServiceTemplate> {
    let mut name = String::new();
    let mut http = false;
    let mut grpc = false;
    let mut database = None;
    let mut cache = None;
    let mut events = None;
    let mut auth = None;
    let mut observability = false;
    let mut resilience = false;
    let mut rate_limit = false;
    let mut openapi = false;
    let mut audit = false;

    // Parse package name
    for line in cargo_toml.lines() {
        if line.trim().starts_with("name") {
            if let Some(n) = line.split('=').nth(1) {
                name = n.trim().trim_matches('"').to_string();
            }
        }
    }

    // Parse features
    let in_features_section = false;
    for line in cargo_toml.lines() {
        if line.contains("[features]") || in_features_section {
            if line.contains("http") {
                http = true;
            }
            if line.contains("grpc") {
                grpc = true;
            }
            if line.contains("surrealdb") {
                database = Some("surrealdb".to_string());
            } else if line.contains("database") {
                database = Some("postgres".to_string());
            }
            if line.contains("cache") {
                cache = Some("redis".to_string());
            }
            if line.contains("events") {
                events = Some("nats".to_string());
            }
            if line.contains("auth") || line.contains("jwt") {
                auth = Some("jwt".to_string());
            }
            if line.contains("observability") {
                observability = true;
            }
            if line.contains("resilience") {
                resilience = true;
            }
            if line.contains("rate-limit") || line.contains("rate_limit") {
                rate_limit = true;
            }
            if line.contains("openapi") {
                openapi = true;
            }
            if line.contains("audit") {
                audit = true;
            }
        }
    }

    let pascal_name = utils::format::to_pascal_case(&name);
    let snake_name = name.replace('-', "_");

    Ok(ServiceTemplate {
        name,
        pascal_name,
        snake_name,
        http,
        grpc,
        database,
        cache,
        events,
        auth,
        observability,
        resilience,
        rate_limit,
        openapi,
        audit,
    })
}

fn add_examples(config: &str) -> String {
    format!(
        r#"# acton-service Configuration
#
# This file demonstrates all available configuration options.
# Remove sections you don't need.
#
# Environment variables can override any setting:
# Format: ACTON_SECTION_KEY=value
# Example: ACTON_SERVICE_PORT=3000

{}

# Additional Examples:
#
# [middleware.jwt]
# secret = "your-secret-key-here"  # Or use ACTON_MIDDLEWARE_JWT_SECRET
# algorithm = "HS256"
# issuer = "your-service"
# audience = "your-api"
#
# [middleware.custom]
# timeout_secs = 60
# max_retries = 3
"#,
        config
    )
}

fn show_dry_run(config: &str, output: &Option<String>) {
    println!("\n{}", "Dry run - would generate:".bold());

    let path = output.as_deref().unwrap_or("config.toml");
    println!("\n{}:", "Output".bold());
    println!("  File: {}", path.cyan());
    println!("  Size: {} bytes", config.len());

    println!("\n{}:", "Preview (first 20 lines)".bold());
    for (i, line) in config.lines().take(20).enumerate() {
        println!("{:3} │ {}", i + 1, line);
    }

    if config.lines().count() > 20 {
        println!("    │ ... ({} more lines)", config.lines().count() - 20);
    }
}

fn show_success(output_path: &Path) {
    utils::success(&format!("Generated config file: {}", output_path.display()));

    println!("\n{}:", "Next steps".bold());
    println!("  1. Review and customize the configuration");
    println!("  2. Set environment-specific values");
    println!("  3. Use environment variables for secrets:");
    println!("     export ACTON_DATABASE_URL=postgres://...");
    println!("     export ACTON_CACHE_URL=redis://...");

    if let Ok(relative_path) = output_path.strip_prefix(std::env::current_dir().unwrap_or_default())
    {
        println!("\n{} Edit config: {}", "→".blue(), relative_path.display());
    }
}
