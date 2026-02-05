mod agent;
mod cli;
mod config;
mod error;
mod exec;
mod safety;
mod tui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use genai::chat::ChatMessage;

use agent::agent_loop::ShutdownReason;
use safety::SafetyLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    // Determine if we're in TUI mode (TUI owns the terminal, so suppress stderr tracing).
    let is_tui_mode = matches!(&cli.command, cli::Commands::Run { headless, .. } if !headless);

    // Initialize tracing -- suppress stderr in TUI mode to avoid corrupting the terminal.
    if is_tui_mode {
        // In TUI mode, only log if RUST_LOG is explicitly set (developer debugging).
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_env("RUST_LOG"),
            )
            .with_writer(std::io::sink)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::INFO.into()),
            )
            .init();
    }

    tracing::info!("Ouroboros starting");

    let config = config::load_config(&cli)?;
    tracing::info!(model = %config.model, workspace = %config.workspace.display(), "Config loaded");

    match cli.command {
        cli::Commands::Run { headless, .. } => {
            let safety = SafetyLayer::new(&config)?;

            tracing::info!(
                model = %config.model,
                workspace = %safety.workspace_root().display(),
                timeout_secs = config.shell_timeout_secs,
                blocklist_patterns = config.blocked_patterns.len(),
                "Safety layer initialized"
            );

            // -- Set up two-phase Ctrl+C shutdown (once, shared across sessions)
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_clone = shutdown.clone();

            tokio::spawn(async move {
                // First Ctrl+C: set graceful shutdown flag.
                tokio::signal::ctrl_c().await.ok();
                shutdown_clone.store(true, Ordering::SeqCst);
                // In headless mode, print message. In TUI mode, the TUI
                // handles its own quit flow via the 'q' key.
                if headless {
                    eprintln!(
                        "\nShutting down after current turn... (Ctrl+C again to force quit)"
                    );
                }

                // Second Ctrl+C: force exit.
                tokio::signal::ctrl_c().await.ok();
                std::process::exit(1);
            });

            if headless {
                // ---- Headless mode: original behavior (no TUI) ----
                let mut session_number: u32 = 1;
                let mut carryover_messages: Vec<ChatMessage> = Vec::new();

                loop {
                    let result = agent::agent_loop::run_agent_session(
                        &config,
                        &safety,
                        session_number,
                        &carryover_messages,
                        shutdown.clone(),
                        None, // event_tx: no TUI in headless mode
                        None, // pause_flag: no pause in headless mode
                    )
                    .await?;

                    match result.shutdown_reason {
                        ShutdownReason::ContextFull {
                            carryover_messages: carry,
                        } => {
                            // Check max_restarts
                            if let Some(max) = config.max_restarts {
                                if session_number >= max {
                                    eprintln!(
                                        "Max restarts ({max}) reached. Exiting."
                                    );
                                    break;
                                }
                            }

                            // Check auto_restart
                            if !config.auto_restart {
                                eprintln!(
                                    "Session context full. Auto-restart disabled. \
                                     Press Enter to continue or Ctrl+C to exit."
                                );
                                let mut input = String::new();
                                std::io::stdin().read_line(&mut input)?;
                                if shutdown.load(Ordering::SeqCst) {
                                    break;
                                }
                            }

                            // Check if user triggered shutdown during the session
                            if shutdown.load(Ordering::SeqCst) {
                                break;
                            }

                            session_number += 1;
                            carryover_messages = carry;
                            eprintln!(
                                "\n--- Starting session #{session_number} ---\n"
                            );
                        }
                        ShutdownReason::UserShutdown => {
                            eprintln!(
                                "User shutdown. {session_number} session(s) completed."
                            );
                            break;
                        }
                        ShutdownReason::MaxTurnsOrError(msg) => {
                            eprintln!("Session ended: {msg}");
                            break;
                        }
                    }
                }
            } else {
                // ---- TUI mode (default): full dashboard ----
                tui::runner::run_tui(&config, &safety, shutdown).await?;
            }
        }
        cli::Commands::Resume { .. } => {
            eprintln!(
                "Resume not yet implemented. \
                 The agent auto-restarts sessions when context fills."
            );
        }
    }

    Ok(())
}
