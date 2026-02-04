mod agent;
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
            let safety = SafetyLayer::new(&config)?;

            tracing::info!(
                model = %config.model,
                workspace = %safety.workspace_root().display(),
                timeout_secs = config.shell_timeout_secs,
                blocklist_patterns = config.blocked_patterns.len(),
                "Safety layer initialized"
            );

            // Run the agent loop -- this blocks until shutdown or context full.
            agent::agent_loop::run_agent_loop(&config, &safety).await?;
        }
        cli::Commands::Resume { .. } => {
            println!("Resume not yet implemented.");
        }
    }

    Ok(())
}
