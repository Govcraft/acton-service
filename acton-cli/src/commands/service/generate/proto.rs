use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::utils;

pub async fn execute(service: String, output: Option<String>, dry_run: bool) -> Result<()> {
    // Find project root
    let project_root = utils::find_project_root()
        .context("Not in a service project directory. Run this command from within a service created with 'acton service new'")?;

    // Convert service name to appropriate formats
    let package_name = service.to_lowercase().replace('_', ".");
    let service_name = utils::format::to_pascal_case(&service);

    // Generate proto content
    let proto_content = generate_proto_template(&package_name, &service_name);

    if dry_run {
        show_dry_run(&proto_content, &service, &output);
        return Ok(());
    }

    // Create proto directory if it doesn't exist
    let proto_dir = project_root.join("proto");
    utils::create_dir_all(&proto_dir)?;

    // Determine output file name
    let file_name = if let Some(path) = output {
        path
    } else {
        format!("{}.proto", service.to_lowercase().replace('_', "-"))
    };

    let output_path = proto_dir.join(&file_name);

    // Check if file already exists
    if output_path.exists() {
        utils::warning(&format!(
            "Proto file already exists at: {}",
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

    // Write proto file
    fs::write(&output_path, proto_content).context("Failed to write proto file")?;

    show_success(&output_path, &service_name, &project_root);

    Ok(())
}

fn generate_proto_template(package_name: &str, service_name: &str) -> String {
    format!(
        r#"syntax = "proto3";

package {package_name}.v1;

// {service_name} service
//
// This service provides gRPC endpoints for {service_name} operations.
// Add your RPC methods below.
service {service_name} {{
  // Example RPC method - replace with your actual methods
  rpc Get{service_name}(Get{service_name}Request) returns (Get{service_name}Response);
}}

// Request message for Get{service_name}
message Get{service_name}Request {{
  // Add your request fields here
  string id = 1;
}}

// Response message for Get{service_name}
message Get{service_name}Response {{
  // Add your response fields here
  string id = 1;
  string status = 2;
}}
"#,
        package_name = package_name,
        service_name = service_name
    )
}

fn show_dry_run(proto: &str, service: &str, output: &Option<String>) {
    println!("\n{}", "Dry run - would generate:".bold());

    let default_name = format!("{}.proto", service.to_lowercase().replace('_', "-"));
    let file_name = output.as_deref().unwrap_or(&default_name);

    println!("\n{}:", "Output".bold());
    println!("  File: proto/{}", file_name.cyan());
    println!("  Size: {} bytes", proto.len());

    println!("\n{}:", "Proto Content".bold());
    for (i, line) in proto.lines().enumerate() {
        println!("{:3} │ {}", i + 1, line);
    }
}

fn show_success(output_path: &Path, service_name: &str, project_root: &Path) {
    utils::success(&format!("Generated proto file: {}", output_path.display()));

    println!("\n{}:", "Generated".bold());
    println!("  {} Service: {}", "✓".green(), service_name);
    println!("  {} Proto file: {}", "✓".green(), output_path.display());

    println!("\n{}:", "Next steps".bold());
    println!("  1. Customize the RPC methods and messages");
    println!("  2. Add build.rs if not present:");
    println!("     ```rust");
    println!("     fn main() -> Result<(), Box<dyn std::error::Error>> {{");
    println!("         #[cfg(feature = \"grpc\")]");
    println!("         acton_service::build_utils::compile_service_protos()?;");
    println!("         Ok(())");
    println!("     }}");
    println!("     ```");
    println!("  3. Add acton-service build dependency in Cargo.toml:");
    println!("     [build-dependencies]");
    println!("     acton-service = {{ version = \"*\", features = [\"build\"] }}");
    println!("  4. Run: cargo build --features grpc");

    if let Ok(relative_path) = output_path.strip_prefix(std::env::current_dir().unwrap_or_default())
    {
        println!("\n{} Edit proto: {}", "→".blue(), relative_path.display());
    }

    // Check if build.rs exists
    let build_rs = project_root.join("build.rs");
    if !build_rs.exists() {
        println!(
            "\n{} build.rs not found. Create it to compile protos!",
            "⚠".yellow()
        );
    }
}
