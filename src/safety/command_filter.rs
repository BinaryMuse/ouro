use regex::RegexSet;

use super::defaults::default_blocklist;

/// Checks commands against a set of blocked patterns.
///
/// Patterns are compiled into a [`RegexSet`] once at construction time,
/// enabling efficient single-pass matching against all patterns simultaneously.
pub struct CommandFilter {
    patterns: RegexSet,
    pattern_reasons: Vec<String>,
}

/// Information about a blocked command.
///
/// Serializes to JSON for structured agent consumption:
/// ```json
/// { "blocked": true, "reason": "...", "command": "..." }
/// ```
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockedCommand {
    pub blocked: bool,
    pub reason: String,
    pub command: String,
}

impl CommandFilter {
    /// Create a new filter from a list of (pattern, reason) tuples.
    ///
    /// The RegexSet is compiled once for efficient multi-pattern matching.
    /// Returns an error if any pattern is not a valid regex.
    pub fn new(patterns: &[(String, String)]) -> Result<Self, regex::Error> {
        let (regexes, reasons): (Vec<_>, Vec<_>) = patterns.iter().cloned().unzip();
        Ok(Self {
            patterns: RegexSet::new(&regexes)?,
            pattern_reasons: reasons,
        })
    }

    /// Create a filter using the default blocklist from [`default_blocklist()`].
    ///
    /// This is a convenience constructor covering privilege escalation,
    /// destructive root operations, system directory writes, disk operations,
    /// fork bombs, system control, and root permission changes.
    pub fn from_defaults() -> Result<Self, regex::Error> {
        Self::new(&default_blocklist())
    }

    /// Check if a command is blocked.
    ///
    /// Returns `Some(BlockedCommand)` with the reason if the command matches
    /// any blocked pattern, or `None` if the command is allowed.
    ///
    /// Uses [`RegexSet::matches`] for single-pass matching against all patterns.
    pub fn check(&self, command: &str) -> Option<BlockedCommand> {
        self.patterns
            .matches(command)
            .into_iter()
            .next()
            .map(|idx| BlockedCommand {
                blocked: true,
                reason: self.pattern_reasons[idx].clone(),
                command: command.to_string(),
            })
    }
}

impl BlockedCommand {
    /// Serialize this blocked command to a JSON string.
    ///
    /// Returns a JSON object with `blocked`, `reason`, and `command` fields.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("BlockedCommand serialization should never fail")
    }
}
