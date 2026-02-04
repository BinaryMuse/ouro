/// Returns the default blocklist of (pattern, reason) tuples.
/// This list catches obvious dangerous patterns but is not a security boundary.
/// The workspace guard (canonical path checking on writes) is the primary defense.
pub fn default_blocklist() -> Vec<(String, String)> {
    vec![
        // Privilege escalation
        (r"(?i)\bsudo\b".into(), "Privilege escalation (sudo) not allowed".into()),
        (r"(?i)\bsu\b\s".into(), "Privilege escalation (su) not allowed".into()),
        (r"(?i)\bdoas\b".into(), "Privilege escalation (doas) not allowed".into()),
        // Destructive filesystem operations at root
        (r"rm\s+(-[^\s]*)?(\s+-[^\s]*)?\s+/($|\s)".into(), "Recursive deletion at root not allowed".into()),
        (r"rm\s+(-[^\s]*)?(\s+-[^\s]*)?\s+/\*".into(), "Recursive deletion at root not allowed".into()),
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
        (r"chmod\s.*\s/($|\s|[a-z])".into(), "Permission changes at root level not allowed".into()),
        (r"chown\s.*\s/($|\s|[a-z])".into(), "Ownership changes at root level not allowed".into()),
    ]
}
