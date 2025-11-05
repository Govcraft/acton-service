use clap::{Parser, Subcommand};
use colored::Colorize;

mod commands;
mod templates;
mod utils;
mod validator;

use commands::service::ServiceCommands;

/// acton - Production-ready Rust microservice CLI
#[derive(Parser)]
#[command(name = "acton")]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Service management commands
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
}

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let cli = Cli::parse();

    // Execute command
    let result = match cli.command {
        Commands::Service { command } => commands::service::execute(command).await,
    };

    // Handle result
    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("{} {}", "Error:".red().bold(), e);

            // Show context if available
            if let Some(source) = e.source() {
                eprintln!("\n{} {}", "Caused by:".yellow(), source);
            }

            std::process::exit(1);
        }
    }
}
