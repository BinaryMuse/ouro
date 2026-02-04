use regex::RegexSet;

/// Checks commands against a set of blocked patterns.
pub struct CommandFilter {
    patterns: RegexSet,
    pattern_reasons: Vec<String>,
}

/// Information about a blocked command.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockedCommand {
    pub blocked: bool,
    pub reason: String,
    pub command: String,
}

impl CommandFilter {
    /// Create a new filter from a list of (pattern, reason) tuples.
    /// The RegexSet is compiled once for efficient multi-pattern matching.
    pub fn new(patterns: &[(String, String)]) -> Result<Self, regex::Error> {
        let (regexes, reasons): (Vec<_>, Vec<_>) = patterns.iter().cloned().unzip();
        Ok(Self {
            patterns: RegexSet::new(&regexes)?,
            pattern_reasons: reasons,
        })
    }

    /// Check if a command is blocked. Returns Some(BlockedCommand) if blocked, None if allowed.
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
