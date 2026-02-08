use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::templates::worker::{generate_worker, WorkerTemplate};
use crate::utils;

pub async fn execute(
    name: String,
    source: String,
    stream: String,
    subject: Option<String>,
    dry_run: bool,
) -> Result<()> {
    // Validate worker name
    validate_worker_name(&name)?;

    // Validate source
    validate_source(&source)?;

    // Find project root
    let project_root = utils::find_project_root()
        .context("Not in a service project directory. Run this command from within a service created with 'acton service new'")?;

    let template = WorkerTemplate {
        name: name.clone(),
        source: source.clone(),
        stream: stream.clone(),
        subject: subject.clone(),
    };

    if dry_run {
        show_dry_run(&template);
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

    // Generate worker code
    pb.set_message("Generating worker module...");
    let worker_code = generate_worker(&template);

    // Create workers directory if it doesn't exist
    pb.set_message("Creating workers directory...");
    let workers_dir = project_root.join("src").join("workers");
    utils::create_dir_all(&workers_dir)?;

    // Write worker file
    pb.set_message("Writing worker file...");
    let worker_file = workers_dir.join(format!("{}.rs", name.replace('-', "_")));
    fs::write(&worker_file, worker_code).context("Failed to write worker file")?;

    // Update workers/mod.rs
    pb.set_message("Updating workers module...");
    update_workers_mod(&workers_dir, &name)?;

    // Format code
    if utils::cargo::is_available() {
        pb.set_message("Formatting code...");
        let _ = utils::cargo::fmt(&project_root);
    }

    pb.finish_and_clear();

    show_success(&template, &project_root);

    Ok(())
}

fn validate_worker_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Worker name cannot be empty");
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "Worker name must be lowercase with hyphens or underscores\n\n\
            Valid examples:\n\
            • email-worker\n\
            • notification_worker\n\
            • data-processor"
        );
    }

    Ok(())
}

fn validate_source(source: &str) -> Result<()> {
    let valid_sources = ["nats", "redis", "redis-stream"];

    if !valid_sources.contains(&source) {
        utils::warning(&format!(
            "Source '{}' is not a known type. Valid types: nats, redis, redis-stream",
            source
        ));
    }

    Ok(())
}

fn update_workers_mod(workers_dir: &Path, name: &str) -> Result<()> {
    let mod_file = workers_dir.join("mod.rs");
    let module_name = name.replace('-', "_");

    let content = if mod_file.exists() {
        let current = fs::read_to_string(&mod_file)?;
        if current.contains(&format!("pub mod {};", module_name)) {
            utils::warning(&format!(
                "Worker '{}' already exists in workers/mod.rs",
                name
            ));
            return Ok(());
        }
        format!("{}\npub mod {};", current.trim_end(), module_name)
    } else {
        format!("pub mod {};", module_name)
    };

    fs::write(&mod_file, content).context("Failed to write workers/mod.rs")?;

    Ok(())
}

fn show_dry_run(template: &WorkerTemplate) {
    println!("\n{}", "Dry run - would generate:".bold());

    println!("\n{}:", "Worker".bold());
    println!("  Name: {}", template.name.cyan());
    println!("  Source: {}", template.source.cyan());
    println!("  Stream: {}", template.stream.cyan());
    if let Some(subject) = &template.subject {
        println!("  Subject: {}", subject.cyan());
    }

    println!("\n{}:", "Files Created".bold());
    println!("  • src/workers/{}.rs", template.name.replace('-', "_"));
    println!("  • src/workers/mod.rs (updated)");
}

fn show_success(template: &WorkerTemplate, project_root: &Path) {
    utils::success(&format!("Added worker '{}'", template.name));

    println!("\n{}:", "Generated".bold());
    println!(
        "  {} Worker module: src/workers/{}.rs",
        "✓".green(),
        template.name.replace('-', "_")
    );
    println!("  {} Source: {}", "✓".green(), template.source);
    println!("  {} Stream: {}", "✓".green(), template.stream);

    println!("\n{}:", "Next steps".bold());
    println!(
        "  1. Implement worker logic in src/workers/{}.rs",
        template.name.replace('-', "_")
    );
    println!("  2. Add worker to main.rs:");
    println!("     ```rust");
    println!("     mod workers;");
    println!("     ");
    println!("     // In main() or service setup:");
    println!(
        "     let worker = workers::{}::{}Worker::new(/* deps */);",
        template.name.replace('-', "_"),
        to_pascal_case(&template.name)
    );
    println!("     tokio::spawn(async move {{");
    println!("         worker.run().await");
    println!("     }});");
    println!("     ```");
    println!("  3. Ensure dependencies are in Cargo.toml");

    if template.source == "nats" {
        println!("\n{} NATS dependencies needed:", "→".blue());
        println!("  async-nats = \"*\"");
        println!("  futures = \"*\"");
    } else if template.source == "redis" || template.source == "redis-stream" {
        println!("\n{} Redis dependencies needed:", "→".blue());
        println!("  redis = {{ version = \"*\", features = [\"tokio-comp\", \"streams\"] }}");
    }

    if let Ok(relative_path) = project_root
        .join("src/workers")
        .join(format!("{}.rs", template.name.replace('-', "_")))
        .strip_prefix(std::env::current_dir().unwrap_or_default())
    {
        println!("\n{} Edit worker: {}", "→".blue(), relative_path.display());
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split(&['-', '_'][..])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}
