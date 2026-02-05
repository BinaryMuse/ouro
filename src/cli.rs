use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "ouro", version, about = "Autonomous AI research harness")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start a new agent session
    Run {
        /// Ollama model name (e.g., "llama3.2", "qwen2.5:7b")
        #[arg(short, long)]
        model: Option<String>,

        /// Workspace directory path
        #[arg(short, long)]
        workspace: Option<PathBuf>,

        /// Shell command timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,

        /// Path to config file (overrides default search)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Run without TUI (headless mode, original behavior)
        #[arg(long)]
        headless: bool,
    },
    /// Resume a previous agent session
    Resume {
        /// Workspace directory to resume from
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
}
