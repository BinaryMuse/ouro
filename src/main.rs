mod cli;
mod config;
mod error;
mod exec;
mod safety;

use clap::Parser;

use safety::SafetyLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = cli::Cli::parse();
    tracing::info!("Ouroboros starting");

    let config = config::load_config(&cli)?;
    tracing::info!(model = %config.model, workspace = %config.workspace.display(), "Config loaded");

    match cli.command {
        cli::Commands::Run { .. } => {
            // Build the safety layer from config.
            let safety = SafetyLayer::new(&config)?;

            tracing::info!(
                model = %config.model,
                workspace = %safety.workspace_root().display(),
                timeout_secs = config.shell_timeout_secs,
                blocklist_patterns = config.blocked_patterns.len(),
                "Safety layer initialized"
            );

            println!(
                "Ouroboros initialized. Safety layer active.\n  Model: {}\n  Workspace: {}\n  Timeout: {}s\n  Blocklist patterns: {}\nWaiting for agent loop (Phase 2).",
                config.model,
                safety.workspace_root().display(),
                config.shell_timeout_secs,
                config.blocked_patterns.len(),
            );

            // Agent loop will be implemented in Phase 2.
        }
        cli::Commands::Resume { .. } => {
            println!("Resume not yet implemented.");
        }
    }

    Ok(())
}
