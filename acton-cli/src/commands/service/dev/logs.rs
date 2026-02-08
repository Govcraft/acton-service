use anyhow::Result;
use colored::Colorize;

pub async fn execute(follow: bool, level: Option<String>, filter: Option<String>) -> Result<()> {
    println!("{}", "Service Logs".bold());
    println!();

    println!(
        "{}",
        "Log viewing depends on how you're running the service:".bold()
    );
    println!();

    // cargo run
    println!("{}:", "If running locally with cargo run".green().bold());
    println!("  Logs are printed to stdout/stderr");
    println!(
        "  Set log level with: RUST_LOG={} cargo run",
        level.as_deref().unwrap_or("info")
    );
    println!();

    // docker
    println!("{}:", "If running in Docker".green().bold());
    println!("  View logs: docker logs <container-name>");
    if follow {
        println!("  Follow logs: docker logs -f <container-name>");
    }
    if let Some(lvl) = &level {
        println!(
            "  Filter by level: docker logs <container-name> | grep {}",
            lvl.to_uppercase()
        );
    }
    if let Some(pattern) = &filter {
        println!(
            "  Filter pattern: docker logs <container-name> | grep '{}'",
            pattern
        );
    }
    println!();

    // kubernetes
    println!("{}:", "If running in Kubernetes".green().bold());
    println!("  View logs: kubectl logs <pod-name>");
    if follow {
        println!("  Follow logs: kubectl logs -f <pod-name>");
    }
    if let Some(lvl) = &level {
        println!(
            "  Filter by level: kubectl logs <pod-name> | grep {}",
            lvl.to_uppercase()
        );
    }
    if let Some(pattern) = &filter {
        println!(
            "  Filter pattern: kubectl logs <pod-name> | grep '{}'",
            pattern
        );
    }
    println!();

    // systemd
    println!("{}:", "If running as systemd service".green().bold());
    println!("  View logs: journalctl -u <service-name>");
    if follow {
        println!("  Follow logs: journalctl -u <service-name> -f");
    }
    if let Some(lvl) = &level {
        println!("  Filter by level: journalctl -u <service-name> -p {}", lvl);
    }
    if let Some(pattern) = &filter {
        println!(
            "  Filter pattern: journalctl -u <service-name> | grep '{}'",
            pattern
        );
    }
    println!();

    // helpful tip
    println!("{}:", "Tip".yellow().bold());
    println!("  acton-service uses the tracing crate for structured logging");
    println!("  Configure log level via RUST_LOG environment variable:");
    println!("    RUST_LOG=debug        # All debug logs");
    println!("    RUST_LOG=info         # Info and above (default)");
    println!("    RUST_LOG=my_service=debug,tower=info  # Per-module levels");
    println!();

    println!("{}:", "JSON logs".cyan().bold());
    println!("  Enable JSON output: ACTON_SERVICE_LOG_FORMAT=json");
    println!("  Then pipe to jq for filtering:");
    println!("    docker logs <container> | jq 'select(.level == \"ERROR\")'");

    Ok(())
}
