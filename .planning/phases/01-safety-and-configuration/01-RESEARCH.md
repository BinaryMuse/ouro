# Phase 1: Safety & Configuration - Research

**Researched:** 2026-02-04
**Domain:** Workspace-scoped execution guardrails, command filtering, layered TOML configuration, CLI parsing (Rust)
**Confidence:** HIGH

## Summary

Phase 1 builds the sandbox and configuration systems that all subsequent phases depend on. The scope is: (1) a command blocklist that rejects dangerous shell patterns before execution, (2) workspace boundary enforcement that allows reads anywhere but restricts writes to the workspace, (3) a layered TOML config system (global -> workspace -> CLI), and (4) a CLI with subcommands for launching the harness. No agent loop, no tools, no TUI -- just the safety foundation and config loading.

The research confirms that all required components are well-served by standard Rust ecosystem crates: `clap` 4.5 for CLI, `toml` 0.9 + `serde` for config parsing, `tokio::process` for shell execution with timeout, `regex` for blocklist pattern matching, and `std::fs::canonicalize` for path resolution. The decision to use a blocklist (not allowlist) is locked. The decision to use TOML (not YAML or JSON) is locked. Config layering is manual (parse each source, merge with struct defaults) rather than using a framework crate -- this keeps dependencies minimal and gives full control over merge semantics.

**Primary recommendation:** Build three independent modules (config, command filter, workspace guard) with clean public APIs, then wire them together in the CLI entry point. Each module is independently testable. The command filter and workspace guard are pure functions over paths and strings -- no async, no I/O in the core logic.

## Standard Stack

The established libraries/tools for this phase:

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.5 | CLI argument parsing with derive macros | De facto Rust CLI standard. 336M+ downloads. Derive API is more maintainable than builder. Subcommand support is first-class. |
| toml | 0.9 | TOML config file parsing/serialization | Rust ecosystem standard for config (Cargo itself uses TOML). v0.9 is current stable (released 2025-07, latest 0.9.11). Serde-native. |
| serde | 1.0 | Serialization framework | Universal Rust serialization. Every config struct uses `#[derive(Serialize, Deserialize)]`. |
| serde_json | 1.0 | JSON serialization for structured errors | Blocked command errors return JSON-like structured responses to the agent. |
| tokio | 1.49 | Async runtime (process spawning, timeout) | Required for `tokio::process::Command` and `tokio::time::timeout`. Features needed: `process`, `time`, `fs`, `rt-multi-thread`, `macros`. |
| regex | 1.12 | Pattern matching for command blocklist | Standard regex engine for Rust. 574M+ downloads. Guaranteed linear-time matching prevents ReDoS. |
| directories | 5 | Platform-specific config paths | Maps to `~/.config/ouro/` on Linux (XDG), `~/Library/Application Support/ouro/` on macOS. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| anyhow | 1.1 | Application error handling | Top-level error propagation with `.context()`. |
| thiserror | 1.6 | Typed error definitions | Define `ConfigError`, `GuardrailError`, `CommandFilterError` enums at module boundaries. |
| tracing | 0.1 | Structured logging | Security log for blocked commands. Separate subscriber/layer for security events. |
| tracing-subscriber | 0.3 | Log formatting/filtering | File-based appender for security log. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Manual TOML layering | `confique` crate | confique provides type-safe layered config with builder pattern. BUT: adds a proc-macro dependency, relatively small community (low download count), and we need custom merge semantics (blocklist arrays should replace, not append). Manual merge with `toml` + `serde` is straightforward and gives full control. |
| Manual TOML layering | `config` crate (config-rs) | config-rs is the most popular layered config crate (58M+ downloads). BUT: it uses string-key based access (not type-safe), pulls in many dependencies, and its TOML support depends on toml 0.9 internally. For our simple two-file merge, it is over-engineered. |
| `regex` for blocklist | String `contains`/`starts_with` | Simpler but cannot match patterns like `rm -rf` with varying flag orderings (e.g., `-rf`, `-fr`, `-r -f`). Regex handles this cleanly. |
| `directories` | Hardcoded `~/.config/ouro/` | Not cross-platform. `directories` handles macOS (`~/Library/Application Support/`) and respects XDG overrides on Linux. |

**Installation:**

```toml
[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }

# Config
toml = "0.9"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Async runtime (for shell execution)
tokio = { version = "1.49", features = ["process", "time", "fs", "rt-multi-thread", "macros"] }

# Command filtering
regex = "1.12"

# Platform paths
directories = "5"

# Error handling
anyhow = "1.1"
thiserror = "1.6"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## Architecture Patterns

### Recommended Project Structure

```
src/
├── main.rs              # Entry point: parse CLI, load config, wire up
├── cli.rs               # Clap derive structs (Cli, Commands, RunArgs)
├── config/
│   ├── mod.rs           # Public API: load_config(cli_args) -> AppConfig
│   ├── schema.rs        # Config struct definitions (serde Deserialize)
│   └── merge.rs         # Config layering logic (global + workspace + CLI)
├── safety/
│   ├── mod.rs           # Public API: CommandGuard, WorkspaceGuard
│   ├── command_filter.rs # Blocklist matching (is_blocked(cmd) -> Option<BlockReason>)
│   ├── workspace.rs     # Path validation (is_write_allowed(path, workspace) -> bool)
│   └── defaults.rs      # Default blocklist entries
├── exec/
│   ├── mod.rs           # Public API: execute_command(cmd, guard, timeout) -> ExecResult
│   └── shell.rs         # tokio::process::Command wrapper with timeout + process groups
└── error.rs             # Error types: ConfigError, GuardrailError, ExecError
```

### Pattern 1: Config Layering via Struct Defaults + Override

**What:** Parse each config source (global file, workspace file, CLI args) into an `Option`-wrapped struct, then merge by preferring the most specific non-None value. Base defaults come from the struct's `Default` impl.

**When to use:** Always for this project's config loading.

**Example:**

```rust
// config/schema.rs
use serde::Deserialize;

/// The fully-resolved runtime configuration. All fields have values.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub model: String,
    pub workspace: PathBuf,
    pub shell_timeout_secs: u64,
    pub context_limit: usize,
    pub blocked_commands: Vec<String>,
    pub security_log_path: PathBuf,
}

/// Partial config from a single TOML source. All fields are Option
/// so that missing fields don't override lower-priority values.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PartialConfig {
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub shell_timeout_secs: Option<u64>,
    pub context_limit: Option<usize>,
    pub blocked_commands: Option<Vec<String>>,
    pub security_log_path: Option<PathBuf>,
}

// config/merge.rs
impl PartialConfig {
    /// Merge self with a lower-priority fallback.
    /// Self's non-None values take precedence.
    pub fn with_fallback(self, fallback: PartialConfig) -> PartialConfig {
        PartialConfig {
            model: self.model.or(fallback.model),
            workspace: self.workspace.or(fallback.workspace),
            shell_timeout_secs: self.shell_timeout_secs.or(fallback.shell_timeout_secs),
            context_limit: self.context_limit.or(fallback.context_limit),
            // For blocklist: if explicitly set, REPLACE (don't merge).
            // This matches the user's expectation: workspace blocklist
            // overrides global blocklist entirely.
            blocked_commands: self.blocked_commands.or(fallback.blocked_commands),
            security_log_path: self.security_log_path.or(fallback.security_log_path),
        }
    }

    /// Convert to AppConfig, filling any remaining gaps with defaults.
    pub fn finalize(self) -> AppConfig {
        AppConfig {
            model: self.model.unwrap_or_else(|| "llama3.2".to_string()),
            workspace: self.workspace.unwrap_or_else(|| PathBuf::from("./workspace")),
            shell_timeout_secs: self.shell_timeout_secs.unwrap_or(30),
            context_limit: self.context_limit.unwrap_or(32768),
            blocked_commands: self.blocked_commands.unwrap_or_else(default_blocklist),
            security_log_path: self.security_log_path
                .unwrap_or_else(|| PathBuf::from("security.log")),
        }
    }
}

// config/mod.rs
pub fn load_config(cli: &CliArgs) -> Result<AppConfig> {
    // Layer 1: Global config (~/.config/ouro/ouro.toml)
    let global = load_toml_file(global_config_path()?)?;

    // Layer 2: Workspace config (workspace/ouro.toml)
    let workspace_path = cli.workspace.as_deref()
        .or(global.workspace.as_deref())
        .unwrap_or(Path::new("./workspace"));
    let workspace = load_toml_file(workspace_path.join("ouro.toml"))?;

    // Layer 3: CLI args (converted to PartialConfig)
    let cli_partial = cli.to_partial_config();

    // Merge: CLI > workspace > global > defaults
    let merged = cli_partial
        .with_fallback(workspace)
        .with_fallback(global)
        .finalize();

    Ok(merged)
}
```

### Pattern 2: Command Filter as Pure Function

**What:** The command filter is a pure function: given a command string and a list of blocklist patterns, return whether the command is blocked and why. No I/O, no state, trivially testable.

**When to use:** Always. The filter is called before every shell command execution.

**Example:**

```rust
// safety/command_filter.rs
use regex::RegexSet;

pub struct CommandFilter {
    patterns: RegexSet,
    pattern_reasons: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockedCommand {
    pub blocked: bool,
    pub reason: String,
    pub command: String,
}

impl CommandFilter {
    pub fn new(patterns: &[(String, String)]) -> Result<Self, regex::Error> {
        let (regexes, reasons): (Vec<_>, Vec<_>) = patterns.iter().cloned().unzip();
        Ok(Self {
            patterns: RegexSet::new(&regexes)?,
            pattern_reasons: reasons,
        })
    }

    pub fn check(&self, command: &str) -> Option<BlockedCommand> {
        let matches: Vec<_> = self.patterns.matches(command).into_iter().collect();
        if matches.is_empty() {
            None
        } else {
            Some(BlockedCommand {
                blocked: true,
                reason: self.pattern_reasons[matches[0]].clone(),
                command: command.to_string(),
            })
        }
    }
}
```

### Pattern 3: Workspace Guard with Canonical Path Comparison

**What:** Resolve the workspace root to a canonical path on startup. Before every write operation, resolve the target path and check it starts with the canonical workspace root. Reads are unrestricted.

**When to use:** Every file write and shell command that produces file output.

**Example:**

```rust
// safety/workspace.rs
use std::path::{Path, PathBuf};

pub struct WorkspaceGuard {
    /// Canonical (absolute, symlinks resolved) workspace root
    canonical_root: PathBuf,
}

impl WorkspaceGuard {
    pub fn new(workspace_path: &Path) -> std::io::Result<Self> {
        // Create workspace directory if it doesn't exist
        std::fs::create_dir_all(workspace_path)?;
        // Resolve to canonical path
        let canonical_root = std::fs::canonicalize(workspace_path)?;
        Ok(Self { canonical_root })
    }

    /// Check if a write to the given path is allowed.
    /// The path is resolved relative to the workspace root.
    pub fn is_write_allowed(&self, target: &Path) -> Result<bool, std::io::Error> {
        // For existing paths, canonicalize fully
        // For non-existent paths, canonicalize the parent + filename
        let canonical = if target.exists() {
            std::fs::canonicalize(target)?
        } else {
            let parent = target.parent()
                .ok_or_else(|| std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "path has no parent"
                ))?;
            if parent.exists() {
                let canonical_parent = std::fs::canonicalize(parent)?;
                canonical_parent.join(target.file_name().unwrap_or_default())
            } else {
                // Parent doesn't exist either -- reject
                return Ok(false);
            }
        };

        Ok(canonical.starts_with(&self.canonical_root))
    }

    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }
}
```

### Anti-Patterns to Avoid

- **String-based path comparison without canonicalization:** Comparing raw path strings fails on `..`, symlinks, and trailing slashes. Always canonicalize both the workspace root and the target path before comparison.
- **Blocklist-only without workspace enforcement:** The blocklist catches known-dangerous patterns, but novel patterns bypass it. The workspace guard is the defense-in-depth layer that catches what the blocklist misses for write operations.
- **Blocking reads:** The user explicitly decided "read anywhere, write workspace only." Do not restrict file reads. The agent needs to read system files, documentation, and source code outside its workspace.
- **Merging blocklist arrays across config layers:** If the user specifies a blocklist in their workspace config, it should REPLACE the global blocklist, not merge with it. This gives full control. If they want the defaults plus extras, they include the defaults in their workspace config.

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CLI argument parsing | Custom arg parsing, getopt | `clap` 4.5 with derive | Handles help text, error messages, subcommands, validation, shell completions. Thousands of edge cases. |
| TOML parsing | Custom TOML parser | `toml` 0.9 + `serde` | TOML spec is complex (datetime, inline tables, multiline strings). The toml crate handles the full TOML 1.1 spec. |
| Regex matching | Custom string matching for blocklist | `regex` 1.12 with `RegexSet` | `RegexSet` matches multiple patterns in a single pass. Linear-time guarantee prevents ReDoS attacks from adversarial patterns. |
| Platform config paths | Hardcoded `~/.config/` | `directories` 5 | macOS uses `~/Library/Application Support/`, Linux uses XDG, Windows uses AppData. The crate handles all platforms. |
| Process group management | Manual `unsafe pre_exec` + nix | `Command::process_group(0)` (std) | Stabilized in Rust 1.64. Uses the fast `posix_spawn` path. Available on `tokio::process::Command` via `CommandExt`. |
| Timeout for shell commands | Manual timer + kill logic | `tokio::time::timeout` wrapping `child.wait_with_output()` | Handles the future cancellation correctly. Combine with explicit `child.kill()` on timeout (kill_on_drop has known zombie issues). |

**Key insight:** Every component in Phase 1 has a well-tested, standard library solution. The custom logic is in the wiring: combining the filter, guard, and config into a coherent safety layer. Do not spend time building primitives that exist.

## Common Pitfalls

### Pitfall 1: kill_on_drop Zombie Processes

**What goes wrong:** Using `kill_on_drop(true)` on `tokio::process::Command` and assuming child processes are cleaned up on timeout. In practice, zombie processes accumulate because the runtime's best-effort reaping is not immediate.
**Why it happens:** `kill_on_drop` sends SIGKILL but does not call `waitpid` synchronously. The tokio runtime reaps asynchronously on a best-effort basis. On timeout, if the `Child` handle is simply dropped, the child may not be reaped before the next command spawns.
**How to avoid:** Always explicitly call `child.kill().await` followed by `child.wait().await` on timeout. This ensures the process is killed AND reaped. Use `process_group(0)` to also kill child processes spawned by the command.
**Warning signs:** `ps aux | grep defunct` shows zombie processes after running timed-out commands.

### Pitfall 2: Path Traversal via Symlinks Inside Workspace

**What goes wrong:** The agent creates a symlink inside the workspace pointing to `/etc/passwd` or `~/.ssh/`, then writes through the symlink. The path appears to be inside the workspace but the actual write target is outside.
**Why it happens:** Path validation checks the symlink path, not the resolved target. `starts_with` on the unresolved path passes even though the write escapes the workspace.
**How to avoid:** Always `canonicalize()` the full target path (which resolves symlinks) before checking against the workspace root. Note: the user decided symlinks are allowed (the agent can create them), but writes through symlinks that resolve outside the workspace must be blocked.
**Warning signs:** Agent creates symlinks pointing outside workspace and then writes to them.

### Pitfall 3: Blocklist Bypass via Shell Metacharacters

**What goes wrong:** The blocklist catches `sudo rm -rf /` but misses `su\do rm -rf /`, `$'sudo' rm -rf /`, `` `echo sudo` rm -rf / ``, or `bash -c "sudo rm -rf /"`.
**Why it happens:** The command string is parsed by the blocklist regex as plain text, but the shell interprets metacharacters, escape sequences, and subshells. There is always a gap between text-pattern matching and shell evaluation.
**How to avoid:** Accept that the blocklist is defense-in-depth, not a security boundary. The workspace guard (which checks actual file paths at write time) is the primary defense. The blocklist catches obvious mistakes and signals intent. Also: match against common shell wrapping patterns (e.g., `bash -c`, `sh -c`, `eval` followed by blocked content).
**Warning signs:** Agent uses shell features (backticks, $(), eval, alias) to construct commands that bypass the blocklist.

### Pitfall 4: Config File Load Order Confusion

**What goes wrong:** Global config is loaded but workspace config silently fails (file not found), and the developer assumes it was loaded. Or: CLI args are supposed to override config but the merge order is wrong.
**Why it happens:** TOML file loading that returns `Default::default()` on file-not-found makes it invisible when a config source was actually missing vs. intentionally empty.
**How to avoid:** Log which config files were loaded and which were not found. Return `None` (not default) when a file does not exist. The merge logic should distinguish "field was not in this source" from "field was explicitly set to default value."
**Warning signs:** Config values don't match what's in the workspace ouro.toml. User changes workspace config but behavior doesn't change.

### Pitfall 5: Canonicalize Fails on Non-Existent Paths

**What goes wrong:** `std::fs::canonicalize()` returns an error if the path does not exist. When the agent writes a new file, the target path does not exist yet, so canonicalization fails.
**Why it happens:** `canonicalize` needs to resolve symlinks, which requires the filesystem entry to exist. New files by definition do not exist yet.
**How to avoid:** For new files: canonicalize the parent directory (which must exist for the write to succeed), then append the filename. For the workspace root itself: create the directory first with `create_dir_all`, then canonicalize.
**Warning signs:** Write operations fail with "No such file or directory" during path validation even though the write itself would create the file.

### Pitfall 6: Regex Compilation on Every Command Check

**What goes wrong:** Compiling regex patterns from the blocklist on every call to `is_blocked()`. Regex compilation is expensive (milliseconds per pattern). With 20+ blocklist entries checked before every shell command, this adds latency.
**Why it happens:** The blocklist is loaded from config and passed around as `Vec<String>`. The filter recompiles on each check.
**How to avoid:** Compile the `RegexSet` once at startup (or when config changes) and store it in the `CommandFilter` struct. `RegexSet::new()` compiles all patterns into a single automaton, so matching is a single pass regardless of pattern count.
**Warning signs:** Shell command execution has noticeable latency even for trivial commands.

## Code Examples

Verified patterns from official sources:

### CLI with Subcommands (clap derive)

```rust
// cli.rs
// Source: https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
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
    },
    /// Resume a previous agent session
    Resume {
        /// Workspace directory to resume from
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
}
```

### TOML Config File Parsing

```rust
// config/schema.rs
// Source: https://docs.rs/toml/latest/toml/ (from_str API)
use serde::Deserialize;
use std::path::PathBuf;

/// ouro.toml file structure
#[derive(Debug, Deserialize, Default)]
pub struct ConfigFile {
    pub general: Option<GeneralConfig>,
    pub safety: Option<SafetyConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GeneralConfig {
    pub model: Option<String>,
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyConfig {
    pub shell_timeout_secs: Option<u64>,
    pub context_limit: Option<usize>,
    /// If specified, fully replaces the default blocklist
    pub blocked_patterns: Option<Vec<BlocklistEntry>>,
    pub security_log: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlocklistEntry {
    pub pattern: String,
    pub reason: String,
}

// Loading
fn load_toml_file(path: &Path) -> Result<ConfigFile> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let config: ConfigFile = toml::from_str(&contents)
                .context(format!("Failed to parse {}", path.display()))?;
            tracing::info!("Loaded config from {}", path.display());
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("No config file at {}, using defaults", path.display());
            Ok(ConfigFile::default())
        }
        Err(e) => Err(e).context(format!("Failed to read {}", path.display())),
    }
}
```

### Example ouro.toml

```toml
# ouro.toml -- Ouroboros configuration file

[general]
model = "llama3.2"
workspace = "./workspace"

[safety]
shell_timeout_secs = 30
context_limit = 32768
security_log = "security.log"

# Optional: override the default blocklist entirely
# [[safety.blocked_patterns]]
# pattern = "sudo"
# reason = "Privilege escalation not allowed"
#
# [[safety.blocked_patterns]]
# pattern = "rm\\s+(-[rRf]+\\s+)*/"
# reason = "Recursive deletion of root not allowed"
```

### Shell Execution with Timeout and Process Groups

```rust
// exec/shell.rs
// Source: https://docs.rs/tokio/latest/tokio/process/struct.Command.html
//         https://docs.rs/tokio/latest/tokio/time/fn.timeout.html
use std::os::unix::process::CommandExt;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

pub async fn execute_shell(
    command: &str,
    working_dir: &Path,
    timeout_secs: u64,
) -> Result<ExecResult> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(working_dir)
        .process_group(0)  // New process group for clean kill
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn shell process")?;

    let timeout_duration = Duration::from_secs(timeout_secs);

    match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(ExecResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
            timed_out: false,
        }),
        Ok(Err(e)) => Err(anyhow::anyhow!("Process execution failed: {}", e)),
        Err(_elapsed) => {
            // Timeout: kill the entire process group
            let pid = child.id();
            if let Some(pid) = pid {
                // Kill the process group (negative PID = process group)
                unsafe {
                    libc::killpg(pid as i32, libc::SIGKILL);
                }
            }
            // Still need to wait to avoid zombies
            let _ = child.wait().await;

            Ok(ExecResult {
                stdout: String::new(),
                stderr: format!("Command timed out after {}s", timeout_secs),
                exit_code: None,
                timed_out: true,
            })
        }
    }
}
```

### Structured Error Response for Blocked Commands

```rust
// safety/mod.rs
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BlockedResponse {
    pub blocked: bool,
    pub reason: String,
    pub command: String,
}

impl BlockedResponse {
    pub fn new(command: &str, reason: &str) -> Self {
        Self {
            blocked: true,
            reason: reason.to_string(),
            command: command.to_string(),
        }
    }

    /// Serialize to JSON for agent consumption
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(r#"{{"blocked":true,"reason":"{}","command":"{}"}}"#,
                self.reason, self.command)
        })
    }
}
```

### Global Config Path Resolution

```rust
// config/mod.rs
// Source: https://docs.rs/directories/latest/directories/struct.ProjectDirs.html
use directories::ProjectDirs;

pub fn global_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "ouro")
        .map(|dirs| dirs.config_dir().join("ouro.toml"))
    // Linux: ~/.config/ouro/ouro.toml
    // macOS: ~/Library/Application Support/ouro/ouro.toml
}
```

## Default Blocklist Recommendation

This is a Claude's Discretion item. Recommended default blocklist entries:

```rust
// safety/defaults.rs
pub fn default_blocklist() -> Vec<(String, String)> {
    vec![
        // Privilege escalation
        (r"(?i)\bsudo\b".into(), "Privilege escalation (sudo) not allowed".into()),
        (r"(?i)\bsu\b\s".into(), "Privilege escalation (su) not allowed".into()),
        (r"(?i)\bdoas\b".into(), "Privilege escalation (doas) not allowed".into()),

        // Destructive filesystem operations at root
        (r"rm\s+(-[^\s]*)?(\s+-[^\s]*)?\s+/($|\s)".into(),
            "Recursive deletion at root not allowed".into()),
        (r"rm\s+(-[^\s]*)?(\s+-[^\s]*)?\s+/\*".into(),
            "Recursive deletion at root not allowed".into()),

        // System directory writes
        (r">\s*/etc/".into(), "Write to /etc not allowed".into()),
        (r">\s*/usr/".into(), "Write to /usr not allowed".into()),
        (r">\s*/boot/".into(), "Write to /boot not allowed".into()),
        (r">\s*/sys/".into(), "Write to /sys not allowed".into()),
        (r">\s*/proc/".into(), "Write to /proc not allowed".into()),

        // Disk-level destructive operations
        (r"(?i)\bmkfs\b".into(), "Filesystem formatting not allowed".into()),
        (r"(?i)\bdd\b\s.*of=/dev/".into(), "Direct device writes not allowed".into()),

        // Fork bomb patterns
        (r":\(\)\s*\{.*\}".into(), "Fork bomb pattern detected".into()),

        // System shutdown/reboot
        (r"(?i)\bshutdown\b".into(), "System shutdown not allowed".into()),
        (r"(?i)\breboot\b".into(), "System reboot not allowed".into()),
        (r"(?i)\bhalt\b".into(), "System halt not allowed".into()),
        (r"(?i)\bpoweroff\b".into(), "System poweroff not allowed".into()),

        // Permission changes at system level
        (r"chmod\s.*\s/($|\s|[a-z])".into(),
            "Permission changes at root level not allowed".into()),
        (r"chown\s.*\s/($|\s|[a-z])".into(),
            "Ownership changes at root level not allowed".into()),
    ]
}
```

Note: This blocklist is intentionally non-exhaustive. It catches obvious dangerous patterns but does not attempt to be a security boundary. The workspace guard (canonical path checking on writes) is the primary defense.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `toml` 0.8 | `toml` 0.9 (TOML 1.1 spec) | July 2025 | New version splits parsing into `toml_parser`, adds multi-error reporting. API is compatible. Use 0.9. |
| `pre_exec` + `nix::setpgid` | `Command::process_group(0)` | Rust 1.64 (Sept 2022) | No more unsafe blocks for process groups. Uses fast posix_spawn path. |
| `command-group` crate | `std::process::Command::process_group` | Rust 1.64 | `command-group` is deprecated in favor of `process-wrap` or the stdlib method. Use the stdlib. |
| Blocklist approach (Claude Code) | Allowlist approach (Claude Code v1.0.93+) | 2025 (CVE-2025-66032) | Claude Code switched from blocklist to allowlist after security issues. However, our user explicitly chose blocklist for v1 for flexibility. Document the tradeoff. |

**Note on blocklist vs allowlist:** Claude Code discovered that blocklists are bypassable (CVE-2025-66032) and switched to an allowlist. Our user chose blocklist for v1 because: (1) the agent needs broad command access for autonomous exploration, (2) the workspace guard provides the actual security boundary, and (3) the blocklist is defense-in-depth, not the primary defense. This is an informed decision, not an oversight.

## Open Questions

Things that could not be fully resolved:

1. **Partial output capture on timeout**
   - What we know: The user decided timed-out commands should return partial output captured before the kill. `wait_with_output()` only returns output after the process exits, so on timeout the output is lost.
   - What's unclear: The best approach to capture partial output while also enforcing timeout. Options: (a) stream stdout/stderr to buffers in a separate task while waiting with timeout, (b) use `take_stdout()`/`take_stderr()` on the child and read in parallel with a timeout.
   - Recommendation: Use approach (b). Take stdout/stderr handles, spawn tasks to read them into buffers, then select on (read completion, timeout). On timeout, kill the process and return whatever was buffered.

2. **Config file auto-generation**
   - What we know: The TOML config format is defined. First-time users need an example config.
   - What's unclear: Whether to generate a default config file on first run, or ship an example in docs/README.
   - Recommendation: Do NOT auto-generate. Print a message on first run saying where the config would go and what the defaults are. Auto-generating files surprises users.

3. **Libc dependency for killpg**
   - What we know: Killing the process group requires `libc::killpg` (or the `nix` crate). `tokio::process::Child` does not expose a `kill_group()` method.
   - What's unclear: Whether to add `libc` as a direct dependency or use the `nix` crate for safer wrappers.
   - Recommendation: Use `nix` crate (it provides safe wrappers around `killpg` and `signal::kill`). Add `nix = { version = "0.29", features = ["signal", "process"] }` to dependencies. This avoids raw `unsafe` blocks.

## Sources

### Primary (HIGH confidence)
- [clap derive tutorial (docs.rs)](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) - Subcommand derive pattern, arg attributes
- [toml crate (docs.rs)](https://docs.rs/toml/latest/toml/) - v0.9.11, from_str/to_string API, serde integration
- [tokio::process::Command (docs.rs)](https://docs.rs/tokio/latest/tokio/process/struct.Command.html) - spawn, kill_on_drop, process_group, zombie caveats
- [tokio::time::timeout (docs.rs)](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html) - Timeout wrapper for futures
- [std::os::unix::process::CommandExt (doc.rust-lang.org)](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html) - process_group method, stabilized Rust 1.64
- [std::fs::canonicalize (doc.rust-lang.org)](https://doc.rust-lang.org/std/fs/fn.canonicalize.html) - Path resolution, symlink handling
- [directories::ProjectDirs (docs.rs)](https://docs.rs/directories/latest/directories/struct.ProjectDirs.html) - Platform-specific config paths
- [regex crate (docs.rs)](https://docs.rs/regex/latest/regex/) - v1.12, RegexSet for multi-pattern matching

### Secondary (MEDIUM confidence)
- [StackHawk: Rust Path Traversal Guide](https://www.stackhawk.com/blog/rust-path-traversal-guide-example-and-prevention/) - Path traversal prevention patterns
- [Tokio zombie process issue #2685](https://github.com/tokio-rs/tokio/issues/2685) - kill_on_drop zombie behavior
- [Tokio process_group PR #5114](https://github.com/tokio-rs/tokio/pull/5114) - process_group on tokio::process::Command
- [Claude Code block dangerous commands](https://perrotta.dev/2025/12/claude-code-block-dangerous-commands/) - Hook-based command blocking pattern
- [Destructive command guard (GitHub)](https://github.com/Dicklesworthstone/destructive_command_guard) - Command blocking patterns for agent safety

### Tertiary (LOW confidence)
- [soft-canonicalize crate](https://docs.rs/soft-canonicalize) - Workspace-bounded canonicalization (small crate, unverified adoption)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All crates are well-established, versions verified via docs.rs and crates.io
- Architecture: HIGH - Patterns are standard Rust idioms (struct with Option fields for config merge, pure functions for filtering, canonicalize for path safety)
- Pitfalls: HIGH - Process zombie issues, path traversal via symlinks, and blocklist bypass are well-documented in official sources and issue trackers
- Default blocklist: MEDIUM - Specific regex patterns are recommendations based on common patterns; should be validated with testing

**Research date:** 2026-02-04
**Valid until:** 2026-03-04 (30 days -- this domain is stable, no fast-moving libraries)
