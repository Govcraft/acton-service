use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::templates::{self, ServiceTemplate};
use crate::utils::{self, format};

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    name: String,
    http: bool,
    grpc: bool,
    full: bool,
    database: Option<String>,
    cache: Option<String>,
    events: Option<String>,
    auth: Option<String>,
    observability: bool,
    resilience: bool,
    rate_limit: bool,
    openapi: bool,
    _template: Option<String>,
    path: Option<String>,
    no_git: bool,
    interactive: bool,
    yes: bool,
    dry_run: bool,
) -> Result<()> {
    // Validate service name
    utils::validate_service_name(&name)?;

    // Determine if we should use interactive mode
    let use_interactive = interactive
        || (!yes
            && database.is_none()
            && cache.is_none()
            && events.is_none()
            && !grpc
            && !full);

    // Collect configuration
    let config = if use_interactive && !yes {
        collect_interactive_config(&name)?
    } else {
        ServiceConfig {
            name: name.clone(),
            http: if full { true } else { http },
            grpc: grpc || full,
            database,
            cache,
            events,
            auth,
            observability,
            resilience,
            rate_limit,
            openapi,
        }
    };

    // Determine project path
    let project_path = if let Some(p) = path {
        PathBuf::from(p).join(&name)
    } else {
        PathBuf::from(&name)
    };

    // Check if directory exists
    if project_path.exists() {
        anyhow::bail!(
            "Directory '{}' already exists\n\n\
            Suggestions:\n\
            • Use a different name: acton service new {}-v2\n\
            • Remove existing: rm -rf {}\n\
            • Update existing: cd {} && acton service add <feature>",
            project_path.display(),
            name,
            name,
            name
        );
    }

    // Show what will be generated
    if dry_run {
        show_dry_run(&config, &project_path);
        return Ok(());
    }

    // Create the project
    create_project(&config, &project_path, no_git).await?;

    // Show success message and next steps
    show_success(&config, &project_path);

    Ok(())
}

struct ServiceConfig {
    name: String,
    http: bool,
    grpc: bool,
    database: Option<String>,
    cache: Option<String>,
    events: Option<String>,
    auth: Option<String>,
    observability: bool,
    resilience: bool,
    rate_limit: bool,
    openapi: bool,
}

fn collect_interactive_config(name: &str) -> Result<ServiceConfig> {
    println!("\n{}", "Welcome to acton-service!".bold().cyan());
    println!("Let's create your microservice.\n");

    let theme = ColorfulTheme::default();

    // Service type
    let service_types = vec![
        "HTTP REST API (simple, recommended for beginners)",
        "gRPC Service (internal services, high performance)",
        "HTTP + gRPC (dual protocol, maximum flexibility)",
    ];

    let service_type = Select::with_theme(&theme)
        .with_prompt("Service type")
        .items(&service_types)
        .default(0)
        .interact()?;

    let (http, grpc) = match service_type {
        0 => (true, false),
        1 => (false, true),
        2 => (true, true),
        _ => (true, false),
    };

    // Database
    let enable_database = Confirm::with_theme(&theme)
        .with_prompt("Enable database?")
        .default(false)
        .interact()?;

    let database = if enable_database {
        let db_types = vec!["PostgreSQL"];
        let db_idx = Select::with_theme(&theme)
            .with_prompt("Database type")
            .items(&db_types)
            .default(0)
            .interact()?;

        Some(match db_idx {
            0 => "postgres",
            _ => "postgres",
        }
        .to_string())
    } else {
        None
    };

    // Cache
    let enable_cache = Confirm::with_theme(&theme)
        .with_prompt("Enable caching?")
        .default(false)
        .interact()?;

    let cache = if enable_cache {
        Some("redis".to_string())
    } else {
        None
    };

    // Events
    let enable_events = Confirm::with_theme(&theme)
        .with_prompt("Enable event streaming?")
        .default(false)
        .interact()?;

    let events = if enable_events {
        Some("nats".to_string())
    } else {
        None
    };

    // Observability
    let observability = Confirm::with_theme(&theme)
        .with_prompt("Enable observability (OpenTelemetry)?")
        .default(true)
        .interact()?;

    Ok(ServiceConfig {
        name: name.to_string(),
        http,
        grpc,
        database,
        cache,
        events,
        auth: None,
        observability,
        resilience: false,
        rate_limit: false,
        openapi: false,
    })
}

async fn create_project(config: &ServiceConfig, project_path: &Path, no_git: bool) -> Result<()> {
    let pb = ProgressBar::new(10);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    // Create template data
    let template = ServiceTemplate {
        name: config.name.clone(),
        pascal_name: format::to_pascal_case(&config.name),
        snake_name: format::to_snake_case(&config.name),
        http: config.http,
        grpc: config.grpc,
        database: config.database.clone(),
        cache: config.cache.clone(),
        events: config.events.clone(),
        auth: config.auth.clone(),
        observability: config.observability,
        resilience: config.resilience,
        rate_limit: config.rate_limit,
        openapi: config.openapi,
    };

    pb.set_message("Creating project structure...");
    utils::create_dir_all(project_path)?;

    // Create src directory
    let src_dir = project_path.join("src");
    utils::create_dir_all(&src_dir)?;

    pb.set_message("Generating Cargo.toml...");
    utils::write_file(
        &project_path.join("Cargo.toml"),
        &templates::cargo_toml::generate(&template),
    )?;

    pb.set_message("Generating configuration...");
    utils::write_file(
        &project_path.join("config.toml"),
        &templates::config::generate(&template),
    )?;

    pb.set_message("Generating main.rs...");
    utils::write_file(
        &src_dir.join("main.rs"),
        &templates::service::generate_main_rs(&template),
    )?;

    // Generate handlers if HTTP enabled
    if config.http {
        pb.set_message("Generating handlers...");
        utils::write_file(
            &src_dir.join("handlers.rs"),
            &templates::handlers::generate_handlers_mod(),
        )?;
    }

    // Generate build.rs if gRPC enabled
    if config.grpc {
        if let Some(build_rs) = templates::service::generate_build_rs(&template) {
            pb.set_message("Generating build.rs...");
            utils::write_file(&project_path.join("build.rs"), &build_rs)?;
        }

        // Create proto directory
        let proto_dir = project_path.join("proto");
        utils::create_dir_all(&proto_dir)?;
    }

    pb.set_message("Generating .gitignore...");
    utils::write_file(
        &project_path.join(".gitignore"),
        &templates::service::generate_gitignore(),
    )?;

    pb.set_message("Generating README.md...");
    utils::write_file(
        &project_path.join("README.md"),
        &templates::service::generate_readme(&template),
    )?;

    pb.set_message("Generating Dockerfile...");
    utils::write_file(
        &project_path.join("Dockerfile"),
        &templates::deployment::generate_dockerfile(&config.name),
    )?;

    utils::write_file(
        &project_path.join(".dockerignore"),
        &templates::deployment::generate_dockerignore(),
    )?;

    // Initialize git
    if !no_git && utils::git::is_available() {
        pb.set_message("Initializing git repository...");
        utils::git::init(project_path)?;
    }

    // Format generated code
    if utils::cargo::is_available() {
        pb.set_message("Formatting code...");
        let _ = utils::cargo::fmt(project_path); // Ignore errors
    }

    pb.finish_and_clear();

    Ok(())
}

fn show_dry_run(config: &ServiceConfig, project_path: &Path) {
    println!("\n{}", "Dry run - would generate:".bold());
    println!("\n{}", format!("Project: {}", project_path.display()).cyan());

    println!("\n{}:", "Files".bold());
    println!("  • Cargo.toml");
    println!("  • config.toml");
    println!("  • src/main.rs");
    if config.http {
        println!("  • src/handlers.rs");
    }
    if config.grpc {
        println!("  • build.rs");
        println!("  • proto/ (directory)");
    }
    println!("  • .gitignore");
    println!("  • README.md");
    println!("  • Dockerfile");
    println!("  • .dockerignore");

    println!("\n{}:", "Features".bold());
    if config.http {
        println!("  ✓ HTTP REST API");
    }
    if config.grpc {
        println!("  ✓ gRPC Service");
    }
    if config.database.is_some() {
        println!("  ✓ Database ({})", config.database.as_ref().unwrap());
    }
    if config.cache.is_some() {
        println!("  ✓ Cache ({})", config.cache.as_ref().unwrap());
    }
    if config.events.is_some() {
        println!("  ✓ Events ({})", config.events.as_ref().unwrap());
    }
    if config.observability {
        println!("  ✓ Observability");
    }
    if config.resilience {
        println!("  ✓ Resilience patterns");
    }
    if config.rate_limit {
        println!("  ✓ Rate limiting");
    }
}

fn show_success(config: &ServiceConfig, project_path: &Path) {
    println!("\n{} {}", "✓".green().bold(), format!("Created {} service", config.name).bold());

    if config.http || config.grpc || config.database.is_some() {
        println!("\n{}:", "Features enabled".bold());
        if config.http {
            println!("  {} HTTP REST API with versioning", "✓".green());
        }
        if config.grpc {
            println!("  {} gRPC service", "✓".green());
        }
        if let Some(db) = &config.database {
            println!("  {} {} database with connection pooling", "✓".green(), db);
        }
        if let Some(cache) = &config.cache {
            println!("  {} {} caching", "✓".green(), cache);
        }
        if let Some(events) = &config.events {
            println!("  {} {} event streaming", "✓".green(), events);
        }
        if config.observability {
            println!("  {} OpenTelemetry observability", "✓".green());
        }
    }

    println!("\n{}:", "Next steps".bold());
    println!("  cd {}", project_path.display());

    if config.http {
        println!("  acton service add endpoint GET /users");
    }

    println!("  cargo run");

    println!("\n{} Learn more: https://docs.acton-service.dev/getting-started", "📚".cyan());
}
