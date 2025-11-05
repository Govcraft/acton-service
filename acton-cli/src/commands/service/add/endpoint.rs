use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::templates::handlers::{generate_endpoint_handler, HandlerTemplate};
use crate::utils::{self, format as name_format};

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    method: String,
    path: String,
    version: String,
    handler: Option<String>,
    _auth: Option<String>,
    _rate_limit: Option<u32>,
    _model: Option<String>,
    _validate: bool,
    _response: String,
    _cache: bool,
    _event: Option<String>,
    _openapi: bool,
    dry_run: bool,
) -> Result<()> {
    // Validate HTTP method
    validate_http_method(&method)?;

    // Validate path
    validate_path(&path)?;

    // Find project root
    let project_root = utils::find_project_root()
        .context("Not in a service project directory. Run this command from within a service created with 'acton service new'")?;

    // Determine handler function name
    let function_name = if let Some(h) = handler {
        h
    } else {
        name_format::method_to_function_name(&method, &path)
    };

    // Determine if this endpoint needs request body (POST, PUT, PATCH)
    let has_request_body = matches!(method.to_uppercase().as_str(), "POST" | "PUT" | "PATCH");

    // Check if path has parameters
    let has_path_params = path.contains(':');

    let template = HandlerTemplate {
        function_name: function_name.clone(),
        method: method.to_uppercase(),
        path: path.clone(),
        has_request_body,
        has_path_params,
        with_auth: false,
        with_state: false,
    };

    if dry_run {
        show_dry_run(&template, &version);
        return Ok(());
    }

    // Create progress bar
    let pb = ProgressBar::new(4);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    // Generate handler code
    pb.set_message("Generating handler...");
    let handler_code = generate_endpoint_handler(&template);

    // Add handler to handlers.rs or create new module
    pb.set_message("Updating handlers.rs...");
    add_handler_to_file(&project_root, &handler_code)?;

    // Update main.rs to add route
    pb.set_message("Adding route to main.rs...");
    add_route_to_main(&project_root, &method, &path, &function_name, &version)?;

    // Format code
    if utils::cargo::is_available() {
        pb.set_message("Formatting code...");
        let _ = utils::cargo::fmt(&project_root);
    }

    pb.finish_and_clear();

    show_success(&template, &version, &project_root);

    Ok(())
}

fn validate_http_method(method: &str) -> Result<()> {
    let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];
    let method_upper = method.to_uppercase();

    if !valid_methods.contains(&method_upper.as_str()) {
        anyhow::bail!(
            "Invalid HTTP method '{}'\n\n\
            Valid methods:\n\
            • GET    - Retrieve resources\n\
            • POST   - Create new resources\n\
            • PUT    - Update/replace resources\n\
            • PATCH  - Partial update resources\n\
            • DELETE - Remove resources",
            method
        );
    }

    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    if !path.starts_with('/') {
        anyhow::bail!(
            "Path must start with '/'\n\n\
            Example: /users or /users/:id"
        );
    }

    Ok(())
}

fn add_handler_to_file(project_root: &Path, handler_code: &str) -> Result<()> {
    let handlers_path = project_root.join("src").join("handlers.rs");

    if !handlers_path.exists() {
        anyhow::bail!(
            "handlers.rs not found at {}\n\n\
            This command must be run from within a service created with 'acton service new'",
            handlers_path.display()
        );
    }

    let current_content = fs::read_to_string(&handlers_path)
        .context("Failed to read handlers.rs")?;

    // Append the new handler
    let new_content = if current_content.trim().is_empty() || current_content.trim() == "// Add your handler modules here\n// Example:\n// pub mod users;" {
        handler_code.to_string()
    } else {
        format!("{}\n\n{}", current_content.trim_end(), handler_code)
    };

    fs::write(&handlers_path, new_content)
        .context("Failed to write handlers.rs")?;

    Ok(())
}

fn add_route_to_main(
    project_root: &Path,
    method: &str,
    path: &str,
    handler: &str,
    _version: &str,
) -> Result<()> {
    let main_path = project_root.join("src").join("main.rs");

    if !main_path.exists() {
        anyhow::bail!("main.rs not found");
    }

    let main_content = fs::read_to_string(&main_path)
        .context("Failed to read main.rs")?;

    // Generate the route method (get, post, etc.)
    let route_method = match method.to_uppercase().as_str() {
        "GET" => "get",
        "POST" => "post",
        "PUT" => "put",
        "DELETE" => "delete",
        "PATCH" => "patch",
        _ => "get",
    };

    // Check if route already exists
    let route_check = format!("{}(handlers::{})", route_method, handler);
    if main_content.contains(&route_check) {
        utils::warning(&format!("Route {} {} already exists in main.rs", method, path));
        return Ok(());
    }

    // Look for the routing setup pattern
    let new_content = if let Some(pos) = main_content.find("// TODO: Add your routes here") {
        // Replace the TODO comment with the actual route
        let before = &main_content[..pos];
        let after_comment_start = pos;

        // Find the end of the comment line
        let remaining = &main_content[after_comment_start..];
        let comment_end = if let Some(newline_pos) = remaining.find('\n') {
            after_comment_start + newline_pos + 1
        } else {
            main_content.len()
        };

        // Find the next line with "router" to insert before it
        let after = &main_content[comment_end..];
        let route_line = format!("            router.route(\"{}\", {}(handlers::{}))", path, route_method, handler);

        format!("{}{}\n{}", before, route_line, after)
    } else if let Some(pos) = main_content.find(".route(") {
        // Find the end of the last route and add after it
        let after = &main_content[pos..];
        if let Some(end_pos) = after.find(')') {
            let route_end = pos + end_pos + 1;
            let before = &main_content[..route_end];
            let after = &main_content[route_end..];
            let route_line = format!("\n            .route(\"{}\", {}(handlers::{}))", path, route_method, handler);
            format!("{}{}{}", before, route_line, after)
        } else {
            main_content
        }
    } else {
        let route_line = format!("router.route(\"{}\", {}(handlers::{}))", path, route_method, handler);
        utils::warning("Could not automatically add route to main.rs. Please add manually:");
        println!("\n{}", route_line);
        return Ok(());
    };

    fs::write(&main_path, new_content)
        .context("Failed to write main.rs")?;

    Ok(())
}

fn show_dry_run(template: &HandlerTemplate, version: &str) {
    println!("\n{}", "Dry run - would generate:".bold());

    println!("\n{}:", "Handler Function".bold());
    println!("  Function: {}", template.function_name.cyan());
    println!("  Method: {}", template.method.cyan());
    println!("  Path: /{}{}", version, template.path);

    if template.has_request_body {
        println!("  Request: {}Request", name_format::to_pascal_case(&template.function_name));
    }
    println!("  Response: {}Response", name_format::to_pascal_case(&template.function_name));

    println!("\n{}:", "Files Modified".bold());
    println!("  • src/handlers.rs (handler function added)");
    println!("  • src/main.rs (route registered)");
}

fn show_success(template: &HandlerTemplate, version: &str, project_root: &Path) {
    utils::success(&format!("Added endpoint {} /{}{}", template.method, version, template.path));

    println!("\n{}:", "Generated".bold());
    println!("  {} Handler function: {}", "✓".green(), template.function_name);
    if template.has_request_body {
        println!("  {} Request struct: {}Request", "✓".green(), name_format::to_pascal_case(&template.function_name));
    }
    println!("  {} Response struct: {}Response", "✓".green(), name_format::to_pascal_case(&template.function_name));

    println!("\n{}:", "Next steps".bold());
    println!("  1. Implement handler logic in src/handlers.rs:{}", template.function_name);
    println!("  2. Define request/response fields");
    println!("  3. Test: cargo run");
    println!("  4. Verify: curl -X {} http://localhost:8080/{}{}", template.method, version, template.path);

    if let Ok(relative_path) = project_root.join("src/handlers.rs").strip_prefix(std::env::current_dir().unwrap_or_default()) {
        println!("\n{} Edit handler: {}", "→".blue(), relative_path.display());
    }
}
