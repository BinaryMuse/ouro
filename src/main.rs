mod cli;
mod config;
mod error;
mod exec;
mod safety;

use clap::Parser;

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
            tracing::info!("Run command invoked");
            // Agent loop will be implemented in Phase 2
        }
        cli::Commands::Resume { .. } => {
            tracing::info!("Resume command invoked");
            // Resume logic will be implemented in Phase 2
        }
    }

    Ok(())
}
