pub mod merge;
pub mod schema;

pub use schema::*;

use crate::cli::{Cli, Commands};
use anyhow::Context;
use std::path::Path;

/// Load configuration by merging global, workspace, and CLI sources.
/// Precedence: CLI > workspace config > global config > defaults.
///
/// Missing config files are handled gracefully (defaults apply).
pub fn load_config(cli: &Cli) -> anyhow::Result<AppConfig> {
    // Layer 1: Global config (~/.config/ouro/ouro.toml or platform equivalent)
    let global = load_global_config();

    // Determine workspace path from CLI or global config, for loading workspace config.
    let workspace_path = cli_workspace(cli)
        .or_else(|| global.workspace.clone())
        .unwrap_or_else(|| std::path::PathBuf::from("./workspace"));

    // Layer 2: Workspace config (workspace/ouro.toml)
    let workspace = load_workspace_config(&workspace_path);

    // Layer 3: CLI args (converted to PartialConfig)
    let cli_partial = cli_to_partial(cli);

    // Merge: CLI > workspace > global > defaults
    let config = cli_partial
        .with_fallback(workspace)
        .with_fallback(global)
        .finalize();

    Ok(config)
}

/// Load global config from the platform-specific config directory.
/// Returns empty PartialConfig if file not found.
fn load_global_config() -> PartialConfig {
    let path = global_config_path();
    match path {
        Some(p) => load_toml_file(&p).unwrap_or_default(),
        None => {
            tracing::debug!("Could not determine global config directory");
            PartialConfig::default()
        }
    }
}

/// Load workspace config from workspace/ouro.toml.
/// Returns empty PartialConfig if file not found.
fn load_workspace_config(workspace_path: &Path) -> PartialConfig {
    let config_path = workspace_path.join("ouro.toml");
    load_toml_file(&config_path).unwrap_or_default()
}

/// Load and parse a TOML config file into a PartialConfig.
/// Returns None-equivalent PartialConfig on file-not-found; propagates parse errors to log.
fn load_toml_file(path: &Path) -> Option<PartialConfig> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            match toml::from_str::<ConfigFile>(&contents)
                .context(format!("Failed to parse {}", path.display()))
            {
                Ok(config_file) => {
                    tracing::info!("Loaded config from {}", path.display());
                    Some(config_file.to_partial())
                }
                Err(e) => {
                    tracing::warn!("Config parse error: {:#}", e);
                    None
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("No config file at {}, using defaults", path.display());
            None
        }
        Err(e) => {
            tracing::warn!("Failed to read config at {}: {}", path.display(), e);
            None
        }
    }
}

/// Resolve the platform-specific global config path.
/// Linux: ~/.config/ouro/ouro.toml
/// macOS: ~/Library/Application Support/ouro/ouro.toml
fn global_config_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "ouro")
        .map(|dirs| dirs.config_dir().join("ouro.toml"))
}

/// Extract workspace path from CLI args.
fn cli_workspace(cli: &Cli) -> Option<std::path::PathBuf> {
    match &cli.command {
        Commands::Run { workspace, .. } => workspace.clone(),
        Commands::Resume { workspace } => workspace.clone(),
    }
}

/// Convert CLI arguments to a PartialConfig for merging.
fn cli_to_partial(cli: &Cli) -> PartialConfig {
    match &cli.command {
        Commands::Run {
            model,
            workspace,
            timeout,
            config: _,
            ..
        } => PartialConfig {
            model: model.clone(),
            workspace: workspace.clone(),
            shell_timeout_secs: *timeout,
            ..Default::default()
        },
        Commands::Resume { workspace } => PartialConfig {
            workspace: workspace.clone(),
            ..Default::default()
        },
    }
}
