use anyhow::{bail, Result};
use colored::Colorize;
use std::path::Path;

use crate::validator::{validate_service, ValidationResult};

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    path: String,
    check: Option<String>,
    all: bool,
    deployment: bool,
    security: bool,
    format: String,
    verbose: bool,
    quiet: bool,
    _ci: bool,
    min_score: f32,
    strict: bool,
    fix: bool,
    report: Option<String>,
) -> Result<()> {
    // Verify path exists
    let service_path = Path::new(&path);
    if !service_path.exists() {
        bail!("Path does not exist: {}", path);
    }

    if !quiet {
        println!("{}", "Validating service...".bold());
        println!();
    }

    // Determine what to validate
    let checks = determine_checks(&check, all, deployment, security);

    // Run validation
    let result = if fix {
        if !quiet {
            println!("{}", "Running validation with auto-fix...".yellow());
            println!();
        }
        run_validation_with_fix(&path, &checks, verbose)?
    } else {
        run_validation(&path, &checks, verbose)?
    };

    // Output results based on format
    match format.as_str() {
        "json" => output_json(&result)?,
        "text" => output_text(&result, verbose, quiet)?,
        "ci" | "github" => output_ci(&result)?,
        _ => {
            if !quiet {
                eprintln!("{} Unknown format: {}", "Warning:".yellow(), format);
            }
            output_text(&result, verbose, quiet)?;
        }
    }

    // Write report if requested
    if let Some(report_path) = report {
        write_report(&result, &report_path)?;
        if !quiet {
            println!();
            println!("{} {}", "Report written to:".green(), report_path.cyan());
        }
    }

    // Check if validation passed
    if result.score < min_score {
        if !quiet {
            eprintln!();
            eprintln!(
                "{} Score {:.1} is below minimum {:.1}",
                "FAILED:".red().bold(),
                result.score,
                min_score
            );
        }
        bail!("Validation failed: score below minimum");
    }

    if strict && !result.warnings.is_empty() {
        if !quiet {
            eprintln!();
            eprintln!(
                "{} {} warnings found (strict mode)",
                "FAILED:".red().bold(),
                result.warnings.len()
            );
        }
        bail!("Validation failed: warnings in strict mode");
    }

    if !result.errors.is_empty() {
        if !quiet {
            eprintln!();
            eprintln!(
                "{} {} errors found",
                "FAILED:".red().bold(),
                result.errors.len()
            );
        }
        bail!("Validation failed: errors found");
    }

    if !quiet {
        println!();
        println!("{} Validation passed!", "✓".green().bold());
    }

    Ok(())
}

fn determine_checks(
    check: &Option<String>,
    all: bool,
    deployment: bool,
    security: bool,
) -> Vec<String> {
    if all {
        return vec![
            "structure".to_string(),
            "dependencies".to_string(),
            "config".to_string(),
            "security".to_string(),
            "deployment".to_string(),
            "tests".to_string(),
            "documentation".to_string(),
        ];
    }

    let mut checks = vec!["structure".to_string(), "dependencies".to_string()];

    if deployment {
        checks.push("deployment".to_string());
        checks.push("config".to_string());
    }

    if security {
        checks.push("security".to_string());
    }

    if let Some(specific) = check {
        if !checks.contains(specific) {
            checks.push(specific.clone());
        }
    }

    checks
}

fn run_validation(path: &str, checks: &[String], verbose: bool) -> Result<ValidationResult> {
    let mut result = validate_service(path)?;

    // Perform specific checks
    for check_type in checks {
        match check_type.as_str() {
            "structure" => validate_structure(&mut result, path, verbose),
            "dependencies" => validate_dependencies(&mut result, path, verbose),
            "config" => validate_config(&mut result, path, verbose),
            "security" => validate_security(&mut result, path, verbose),
            "deployment" => validate_deployment(&mut result, path, verbose),
            "tests" => validate_tests(&mut result, path, verbose),
            "documentation" => validate_documentation(&mut result, path, verbose),
            _ => {
                result
                    .warnings
                    .push(format!("Unknown check type: {}", check_type));
            }
        }
    }

    Ok(result)
}

fn run_validation_with_fix(
    path: &str,
    checks: &[String],
    verbose: bool,
) -> Result<ValidationResult> {
    let result = run_validation(path, checks, verbose)?;

    // Auto-fix would be implemented here
    // For now, just return the validation result
    Ok(result)
}

fn validate_structure(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);

    // Check for Cargo.toml
    if service_path.join("Cargo.toml").exists() {
        result.passed.push("✓ Cargo.toml exists".to_string());
    } else {
        result.errors.push("✗ Cargo.toml not found".to_string());
    }

    // Check for src directory
    if service_path.join("src").exists() {
        result.passed.push("✓ src/ directory exists".to_string());
    } else {
        result.errors.push("✗ src/ directory not found".to_string());
    }

    // Check for main.rs or lib.rs
    if service_path.join("src/main.rs").exists() || service_path.join("src/lib.rs").exists() {
        result
            .passed
            .push("✓ Entry point exists (main.rs or lib.rs)".to_string());
    } else {
        result
            .errors
            .push("✗ No entry point found (main.rs or lib.rs)".to_string());
    }

    // Check for config.toml
    if service_path.join("config.toml").exists() {
        result.passed.push("✓ config.toml exists".to_string());
    } else {
        result
            .warnings
            .push("⚠ config.toml not found (recommended)".to_string());
    }
}

fn validate_dependencies(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);
    let cargo_toml = service_path.join("Cargo.toml");

    if !cargo_toml.exists() {
        return;
    }

    // Read Cargo.toml and check for acton-service dependency
    if let Ok(content) = std::fs::read_to_string(cargo_toml) {
        if content.contains("acton-service") {
            result
                .passed
                .push("✓ acton-service dependency found".to_string());
        } else {
            result
                .warnings
                .push("⚠ acton-service dependency not found".to_string());
        }

        // Check for common production dependencies
        if content.contains("tokio") {
            result.passed.push("✓ tokio runtime configured".to_string());
        }

        if content.contains("tracing") {
            result.passed.push("✓ tracing configured".to_string());
        }
    }
}

fn validate_config(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);
    let config_file = service_path.join("config.toml");

    if !config_file.exists() {
        result.warnings.push("⚠ config.toml not found".to_string());
        return;
    }

    if let Ok(content) = std::fs::read_to_string(config_file) {
        // Check for service configuration
        if content.contains("[service]") {
            result
                .passed
                .push("✓ Service configuration section found".to_string());
        } else {
            result
                .warnings
                .push("⚠ [service] section missing in config.toml".to_string());
        }

        // Check for middleware configuration
        if content.contains("[middleware]") {
            result
                .passed
                .push("✓ Middleware configuration found".to_string());
        }

        // Check for environment-specific settings
        if content.contains("environment") {
            result
                .passed
                .push("✓ Environment configuration found".to_string());
        }
    }
}

fn validate_security(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);

    // Check for .env file (should not be committed)
    if service_path.join(".env").exists() {
        if service_path.join(".gitignore").exists() {
            if let Ok(content) = std::fs::read_to_string(service_path.join(".gitignore")) {
                if content.contains(".env") {
                    result
                        .passed
                        .push("✓ .env file properly ignored".to_string());
                } else {
                    result
                        .errors
                        .push("✗ .env file exists but not in .gitignore".to_string());
                }
            }
        } else {
            result
                .warnings
                .push("⚠ .env file exists, ensure it's not committed".to_string());
        }
    }

    // Check for JWT configuration
    if let Ok(content) = std::fs::read_to_string(service_path.join("config.toml")) {
        if content.contains("[jwt]") {
            result.passed.push("✓ JWT configuration found".to_string());

            // Check for secure algorithm
            if content.contains("RS256") || content.contains("ES256") {
                result
                    .passed
                    .push("✓ Secure JWT algorithm configured".to_string());
            } else if content.contains("HS256") {
                result
                    .warnings
                    .push("⚠ HS256 JWT algorithm (consider RS256/ES256)".to_string());
            }
        }
    }

    // Check for HTTPS/TLS configuration
    if let Ok(content) = std::fs::read_to_string(service_path.join("config.toml")) {
        if content.contains("tls") || content.contains("https") {
            result
                .passed
                .push("✓ TLS/HTTPS configuration found".to_string());
        } else {
            result
                .warnings
                .push("⚠ No TLS/HTTPS configuration (required for production)".to_string());
        }
    }
}

fn validate_deployment(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);

    // Check for Dockerfile
    if service_path.join("Dockerfile").exists() {
        result.passed.push("✓ Dockerfile exists".to_string());
    } else {
        result
            .warnings
            .push("⚠ Dockerfile not found (required for containerized deployment)".to_string());
    }

    // Check for Kubernetes manifests
    let k8s_paths = [
        service_path.join("k8s"),
        service_path.join("kubernetes"),
        service_path.join("deployment"),
    ];

    if k8s_paths.iter().any(|p| p.exists()) {
        result
            .passed
            .push("✓ Kubernetes manifests directory found".to_string());
    } else {
        result
            .warnings
            .push("⚠ No Kubernetes manifests found".to_string());
    }

    // Check for health check endpoints
    if let Ok(entries) = std::fs::read_dir(service_path.join("src")) {
        let has_health = entries.flatten().any(|entry| {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                content.contains("/health") || content.contains("/ready")
            } else {
                false
            }
        });

        if has_health {
            result
                .passed
                .push("✓ Health check endpoints configured".to_string());
        } else {
            result
                .warnings
                .push("⚠ No health check endpoints found".to_string());
        }
    }
}

fn validate_tests(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);

    // Check for tests directory
    if service_path.join("tests").exists() {
        result.passed.push("✓ tests/ directory exists".to_string());
    } else {
        result
            .warnings
            .push("⚠ tests/ directory not found".to_string());
    }

    // Check for test modules in src
    if let Ok(entries) = std::fs::read_dir(service_path.join("src")) {
        let has_tests = entries.flatten().any(|entry| {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                content.contains("#[cfg(test)]") || content.contains("#[test]")
            } else {
                false
            }
        });

        if has_tests {
            result
                .passed
                .push("✓ Test modules found in src/".to_string());
        } else {
            result
                .warnings
                .push("⚠ No test modules found in src/".to_string());
        }
    }
}

fn validate_documentation(result: &mut ValidationResult, path: &str, _verbose: bool) {
    let service_path = Path::new(path);

    // Check for README
    if service_path.join("README.md").exists() {
        result.passed.push("✓ README.md exists".to_string());
    } else {
        result.warnings.push("⚠ README.md not found".to_string());
    }

    // Check for API documentation
    if service_path.join("docs").exists() || service_path.join("api-docs").exists() {
        result
            .passed
            .push("✓ Documentation directory found".to_string());
    }
}

fn output_text(result: &ValidationResult, verbose: bool, quiet: bool) -> Result<()> {
    if quiet {
        println!("{:.1}", result.score);
        return Ok(());
    }

    println!("{}", "Validation Results".bold());
    println!("{}", "=".repeat(50));
    println!();

    // Show passed checks
    if !result.passed.is_empty() && verbose {
        println!("{}", "Passed:".green().bold());
        for item in &result.passed {
            println!("  {}", item.green());
        }
        println!();
    }

    // Show warnings
    if !result.warnings.is_empty() {
        println!("{}", "Warnings:".yellow().bold());
        for item in &result.warnings {
            println!("  {}", item.yellow());
        }
        println!();
    }

    // Show errors
    if !result.errors.is_empty() {
        println!("{}", "Errors:".red().bold());
        for item in &result.errors {
            println!("  {}", item.red());
        }
        println!();
    }

    // Show score
    println!("{}", "Score:".bold());
    let score_color = if result.score >= 8.0 {
        result.score.to_string().green()
    } else if result.score >= 6.0 {
        result.score.to_string().yellow()
    } else {
        result.score.to_string().red()
    };
    println!("  {:.1}/10.0", score_color);

    Ok(())
}

fn output_json(result: &ValidationResult) -> Result<()> {
    let json = serde_json::json!({
        "score": result.score,
        "passed": result.passed,
        "warnings": result.warnings,
        "errors": result.errors,
    });
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn output_ci(result: &ValidationResult) -> Result<()> {
    // GitHub Actions format
    println!("::group::Validation Results");

    for item in &result.errors {
        println!("::error::{}", item);
    }

    for item in &result.warnings {
        println!("::warning::{}", item);
    }

    for item in &result.passed {
        println!("::notice::{}", item);
    }

    println!("::endgroup::");
    println!();
    println!("Score: {:.1}/10.0", result.score);

    Ok(())
}

fn write_report(result: &ValidationResult, path: &str) -> Result<()> {
    let report = format!(
        "# Service Validation Report\n\n\
        ## Score: {:.1}/10.0\n\n\
        ## Passed Checks\n{}\n\n\
        ## Warnings\n{}\n\n\
        ## Errors\n{}\n",
        result.score,
        if result.passed.is_empty() {
            "None\n".to_string()
        } else {
            result
                .passed
                .iter()
                .map(|s| format!("- {}\n", s))
                .collect::<String>()
        },
        if result.warnings.is_empty() {
            "None\n".to_string()
        } else {
            result
                .warnings
                .iter()
                .map(|s| format!("- {}\n", s))
                .collect::<String>()
        },
        if result.errors.is_empty() {
            "None\n".to_string()
        } else {
            result
                .errors
                .iter()
                .map(|s| format!("- {}\n", s))
                .collect::<String>()
        }
    );

    std::fs::write(path, report)?;
    Ok(())
}
