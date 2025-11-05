use clap::{Parser, Subcommand};
use colored::Colorize;

mod commands;
mod templates;
mod utils;
mod validator;

use commands::service::ServiceCommands;
use commands::setup::SetupCommands;

/// acton - Production-ready Rust microservice CLI
#[derive(Parser)]
#[command(name = "acton")]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Service management commands
    Service {
        #[command(subcommand)]
        command: Box<ServiceCommands>,
    },
    /// Setup and configuration commands
    Setup {
        #[command(subcommand)]
        command: SetupCommands,
    },
}

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let cli = Cli::parse();

    // Execute command
    let result = match cli.command {
        Commands::Service { command } => commands::service::execute(*command).await,
        Commands::Setup { command } => commands::setup::execute(command).await,
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
