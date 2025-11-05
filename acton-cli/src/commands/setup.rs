use anyhow::{Context, Result};
use clap::{CommandFactory, Subcommand};
use clap_complete::{generate, Shell};
use colored::Colorize;
use std::env;
use std::fs;
use std::io;
use std::path::Path;

use crate::Cli;

#[derive(Subcommand)]
pub enum SetupCommands {
    /// Generate and install shell completions
    Completions {
        /// Shell to generate completions for (auto-detected if not specified)
        #[arg(short, long, value_name = "SHELL")]
        shell: Option<Shell>,

        /// Output to stdout instead of installing
        #[arg(long)]
        stdout: bool,

        /// Show installation instructions only
        #[arg(long)]
        show_instructions: bool,
    },
}

pub async fn execute(command: SetupCommands) -> Result<()> {
    match command {
        SetupCommands::Completions {
            shell,
            stdout,
            show_instructions,
        } => completions(shell, stdout, show_instructions).await,
    }
}

async fn completions(shell: Option<Shell>, stdout: bool, show_instructions: bool) -> Result<()> {
    // If show_instructions, just display help and exit
    if show_instructions {
        display_installation_instructions();
        return Ok(());
    }

    // Detect shell if not specified
    let shell = match shell {
        Some(s) => s,
        None => detect_shell()?,
    };

    if stdout {
        // Generate to stdout for manual installation
        generate_completions_to_stdout(shell)?;
    } else {
        // Generate and install
        install_completions(shell)?;
    }

    Ok(())
}

/// Detect the user's current shell from $SHELL environment variable
fn detect_shell() -> Result<Shell> {
    let shell_path = env::var("SHELL").context("Failed to detect shell. $SHELL not set. Use --shell to specify explicitly.")?;

    let shell_name = Path::new(&shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .context("Invalid shell path")?;

    match shell_name {
        "bash" => Ok(Shell::Bash),
        "zsh" => Ok(Shell::Zsh),
        "fish" => Ok(Shell::Fish),
        "pwsh" | "powershell" => Ok(Shell::PowerShell),
        "elvish" => Ok(Shell::Elvish),
        other => anyhow::bail!(
            "Unsupported shell: {}. Supported shells: bash, zsh, fish, powershell, elvish.\nUse --shell to specify explicitly.",
            other
        ),
    }
}

/// Generate completions to stdout
fn generate_completions_to_stdout(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "acton", &mut io::stdout());
    Ok(())
}

/// Install completions to the appropriate directory for the shell
fn install_completions(shell: Shell) -> Result<()> {
    let home_dir = dirs::home_dir().context("Failed to find home directory")?;

    let (completion_dir, filename, requires_setup) = match shell {
        Shell::Bash => {
            let dir = home_dir.join(".local/share/bash-completion/completions");
            (dir, "acton".to_string(), false)
        }
        Shell::Zsh => {
            let dir = home_dir.join(".zfunc");
            (dir, "_acton".to_string(), true)
        }
        Shell::Fish => {
            let dir = home_dir.join(".config/fish/completions");
            (dir, "acton.fish".to_string(), false)
        }
        Shell::PowerShell => {
            // For PowerShell, we'll use a Completions directory in the profile path
            let profile_dir = home_dir.join("Documents/PowerShell");
            let dir = profile_dir.join("Completions");
            (dir, "acton.ps1".to_string(), true)
        }
        Shell::Elvish => {
            let dir = home_dir.join(".config/elvish/lib");
            (dir, "acton.elv".to_string(), true)
        }
        _ => {
            anyhow::bail!("Unsupported shell: {:?}", shell);
        }
    };

    // Create directory if it doesn't exist
    fs::create_dir_all(&completion_dir)
        .with_context(|| format!("Failed to create directory: {}", completion_dir.display()))?;

    // Generate completions to file
    let completion_path = completion_dir.join(&filename);
    let mut file = fs::File::create(&completion_path)
        .with_context(|| format!("Failed to create file: {}", completion_path.display()))?;

    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "acton", &mut file);

    println!(
        "{} Completions installed for {}",
        "✓".green().bold(),
        format!("{:?}", shell).cyan()
    );
    println!();
    println!(
        "  {}",
        completion_path.to_string_lossy().dimmed()
    );
    println!();

    // Display post-installation instructions
    display_post_install_instructions(shell, &completion_path, requires_setup);

    Ok(())
}

/// Display installation instructions for all shells
fn display_installation_instructions() {
    println!("{}", "Shell Completion Installation Guide".bold());
    println!();

    println!("{}", "Bash:".cyan().bold());
    println!("  Location: ~/.local/share/bash-completion/completions/acton");
    println!("  Install:  acton setup completions --shell bash");
    println!("  Manual:   acton setup completions --shell bash --stdout > ~/.local/share/bash-completion/completions/acton");
    println!();

    println!("{}", "Zsh:".cyan().bold());
    println!("  Location: ~/.zfunc/_acton");
    println!("  Install:  acton setup completions --shell zsh");
    println!("  Setup:    Add to ~/.zshrc:");
    println!("            fpath=(~/.zfunc $fpath)");
    println!("            autoload -Uz compinit && compinit");
    println!();

    println!("{}", "Fish:".cyan().bold());
    println!("  Location: ~/.config/fish/completions/acton.fish");
    println!("  Install:  acton setup completions --shell fish");
    println!();

    println!("{}", "PowerShell:".cyan().bold());
    println!("  Location: ~/Documents/PowerShell/Completions/acton.ps1");
    println!("  Install:  acton setup completions --shell powershell");
    println!("  Setup:    Add to PowerShell profile ($PROFILE):");
    println!("            . $HOME/Documents/PowerShell/Completions/acton.ps1");
    println!();

    println!("{}", "Elvish:".cyan().bold());
    println!("  Location: ~/.config/elvish/lib/acton.elv");
    println!("  Install:  acton setup completions --shell elvish");
    println!("  Setup:    Add to ~/.config/elvish/rc.elv:");
    println!("            use acton");
    println!();
}

/// Display post-installation instructions for a specific shell
fn display_post_install_instructions(shell: Shell, path: &Path, requires_setup: bool) {
    println!("{}", "To enable completions:".bold());
    println!();

    match shell {
        Shell::Bash => {
            println!("  {}:", "Automatic".green());
            println!("    Completions will be loaded automatically on next shell start.");
            println!();
            println!("  {}:", "Immediate use".yellow());
            println!("    source {}", path.display());
        }
        Shell::Zsh => {
            if requires_setup {
                println!("  {}:", "Required setup".yellow().bold());
                println!("    Add to ~/.zshrc:");
                println!("      {}", "fpath=(~/.zfunc $fpath)".cyan());
                println!("      {}", "autoload -Uz compinit && compinit".cyan());
                println!();
            }
            println!("  {}:", "Activate now".green());
            println!("    exec zsh");
        }
        Shell::Fish => {
            println!("  {}:", "Automatic".green());
            println!("    Completions will be loaded automatically on next shell start.");
            println!();
            println!("  {}:", "Immediate use".yellow());
            println!("    source {}", path.display());
        }
        Shell::PowerShell => {
            if requires_setup {
                println!("  {}:", "Required setup".yellow().bold());
                println!("    Add to your PowerShell profile ($PROFILE):");
                println!("      {}", format!(". {}", path.display()).cyan());
                println!();
            }
            println!("  {}:", "Activate now".green());
            println!("    . {}", path.display());
        }
        Shell::Elvish => {
            if requires_setup {
                println!("  {}:", "Required setup".yellow().bold());
                println!("    Add to ~/.config/elvish/rc.elv:");
                println!("      {}", "use acton".cyan());
                println!();
            }
            println!("  {}:", "Activate now".green());
            println!("    use {}", path.display());
        }
        _ => {
            // For unknown shells, provide generic instructions
            println!("  {}:", "Manual installation required".yellow().bold());
            println!("    Completion file: {}", path.display());
        }
    }
    println!();
}
