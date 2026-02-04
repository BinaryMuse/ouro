pub mod merge;
pub mod schema;

pub use merge::*;
pub use schema::*;

use crate::cli::Cli;

/// Load configuration by merging global, workspace, and CLI sources.
/// Precedence: CLI > workspace config > global config > defaults.
pub fn load_config(_cli: &Cli) -> anyhow::Result<AppConfig> {
    // Stub: returns defaults. Full implementation in Task 2.
    Ok(PartialConfig::default().finalize())
}
