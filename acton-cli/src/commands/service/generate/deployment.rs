use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::templates::deployment::{
    generate_k8s_deployment, generate_k8s_service, generate_k8s_hpa,
    generate_k8s_ingress, generate_service_monitor, DeploymentConfig,
};
use crate::utils;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    _platform: Option<String>,
    _all: bool,
    replicas: u32,
    hpa: bool,
    memory: String,
    cpu: String,
    namespace: Option<String>,
    monitoring: bool,
    _alerts: bool,
    ingress: bool,
    tls: bool,
    env: Option<String>,
    registry: Option<String>,
    image_tag: String,
    dry_run: bool,
    output: String,
) -> Result<()> {
    // Find project root
    let project_root = utils::find_project_root()
        .context("Not in a service project directory")?;

    // Get service name from Cargo.toml
    let cargo_toml_path = project_root.join("Cargo.toml");
    let cargo_toml_content = fs::read_to_string(&cargo_toml_path)
        .context("Failed to read Cargo.toml")?;

    let service_name = extract_package_name(&cargo_toml_content)?;

    // Determine image name
    let image = registry
        .map(|r| format!("{}/{}", r, service_name))
        .unwrap_or_else(|| service_name.clone());

    let config = DeploymentConfig {
        service_name: service_name.clone(),
        namespace: namespace.clone(),
        replicas,
        image,
        image_tag,
        memory_limit: memory,
        cpu_limit: cpu,
        enable_hpa: hpa,
        enable_monitoring: monitoring,
        enable_ingress: ingress,
        enable_tls: tls,
        environment: env,
    };

    if dry_run {
        show_dry_run(&config);
        return Ok(());
    }

    // Create output directory
    let output_path = project_root.join(&output);
    utils::create_dir_all(&output_path)?;

    // Generate files
    utils::success("Generating Kubernetes manifests");

    let deployment_yaml = generate_k8s_deployment(&config);
    fs::write(output_path.join("deployment.yaml"), deployment_yaml)?;
    println!("  {} deployment.yaml", "✓".green());

    let service_yaml = generate_k8s_service(&config);
    fs::write(output_path.join("service.yaml"), service_yaml)?;
    println!("  {} service.yaml", "✓".green());

    if config.enable_hpa {
        let hpa_yaml = generate_k8s_hpa(&config);
        fs::write(output_path.join("hpa.yaml"), hpa_yaml)?;
        println!("  {} hpa.yaml", "✓".green());
    }

    if config.enable_ingress {
        let ingress_yaml = generate_k8s_ingress(&config);
        fs::write(output_path.join("ingress.yaml"), ingress_yaml)?;
        println!("  {} ingress.yaml", "✓".green());
    }

    if config.enable_monitoring {
        let monitor_yaml = generate_service_monitor(&config);
        fs::write(output_path.join("servicemonitor.yaml"), monitor_yaml)?;
        println!("  {} servicemonitor.yaml", "✓".green());
    }

    show_success(&config, &output_path);

    Ok(())
}

fn extract_package_name(cargo_toml: &str) -> Result<String> {
    for line in cargo_toml.lines() {
        if line.trim().starts_with("name") {
            if let Some(name) = line.split('=').nth(1) {
                return Ok(name.trim().trim_matches('"').to_string());
            }
        }
    }
    anyhow::bail!("Could not find package name in Cargo.toml");
}

fn show_dry_run(config: &DeploymentConfig) {
    println!("\n{}", "Dry run - would generate:".bold());

    println!("\n{}:", "Configuration".bold());
    println!("  Service: {}", config.service_name.cyan());
    println!("  Namespace: {}", config.namespace.as_ref().unwrap_or(&"default".to_string()).cyan());
    println!("  Replicas: {}", config.replicas.to_string().cyan());
    println!("  Image: {}:{}", config.image.cyan(), config.image_tag.cyan());

    println!("\n{}:", "Manifests".bold());
    println!("  • deployment.yaml");
    println!("  • service.yaml");
    if config.enable_hpa {
        println!("  • hpa.yaml");
    }
    if config.enable_ingress {
        println!("  • ingress.yaml");
    }
    if config.enable_monitoring {
        println!("  • servicemonitor.yaml");
    }
}

fn show_success(config: &DeploymentConfig, output_path: &Path) {
    println!("\n{}", "Deployment manifests generated!".green().bold());

    println!("\n{}:", "Next steps".bold());
    println!("  1. Review manifests in: {}", output_path.display());
    println!("  2. Apply to cluster:");
    println!("     kubectl apply -f {}", output_path.display());

    if config.enable_ingress && config.enable_tls {
        println!("\n{} TLS certificate needed:", "⚠".yellow());
        println!("  Create TLS secret:");
        println!("  kubectl create secret tls {}-tls \\", config.service_name);
        println!("    --cert=path/to/cert.pem \\");
        println!("    --key=path/to/key.pem \\");
        if let Some(ns) = &config.namespace {
            println!("    -n {}", ns);
        }
    }
}
