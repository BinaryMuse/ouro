use ouro::safety::command_filter::{BlockedCommand, CommandFilter};
use ouro::safety::defaults::default_blocklist;

// ============================================================
// Construction tests
// ============================================================

#[test]
fn test_new_with_valid_patterns() {
    let patterns = vec![
        (r"\bsudo\b".to_string(), "no sudo".to_string()),
    ];
    let filter = CommandFilter::new(&patterns);
    assert!(filter.is_ok());
}

#[test]
fn test_new_with_invalid_regex_returns_error() {
    let patterns = vec![
        (r"[invalid".to_string(), "bad regex".to_string()),
    ];
    let filter = CommandFilter::new(&patterns);
    assert!(filter.is_err());
}

#[test]
fn test_from_defaults_constructs_successfully() {
    let filter = CommandFilter::from_defaults();
    assert!(filter.is_ok());
}

#[test]
fn test_custom_blocklist_works_independently() {
    let custom = vec![
        (r"(?i)\bforbidden\b".to_string(), "custom block".to_string()),
    ];
    let filter = CommandFilter::new(&custom).unwrap();

    // Custom pattern should block
    let result = filter.check("run forbidden command");
    assert!(result.is_some());
    assert_eq!(result.unwrap().reason, "custom block");

    // Default patterns should NOT be present
    let result = filter.check("sudo apt install foo");
    assert!(result.is_none(), "custom filter should not include default sudo pattern");
}

// ============================================================
// BLOCKED commands -- privilege escalation
// ============================================================

#[test]
fn test_blocks_sudo() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("sudo apt install foo");
    assert!(result.is_some(), "sudo should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.blocked);
    assert!(blocked.reason.to_lowercase().contains("privilege") || blocked.reason.to_lowercase().contains("sudo"));
    assert_eq!(blocked.command, "sudo apt install foo");
}

#[test]
fn test_blocks_su() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("su root");
    assert!(result.is_some(), "su should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.reason.to_lowercase().contains("privilege") || blocked.reason.to_lowercase().contains("su"));
}

#[test]
fn test_blocks_doas() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("doas rm foo");
    assert!(result.is_some(), "doas should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.reason.to_lowercase().contains("privilege") || blocked.reason.to_lowercase().contains("doas"));
}

// ============================================================
// BLOCKED commands -- destructive root operations
// ============================================================

#[test]
fn test_blocks_rm_rf_root() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("rm -rf /");
    assert!(result.is_some(), "rm -rf / should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.reason.to_lowercase().contains("delet") || blocked.reason.to_lowercase().contains("root"));
}

#[test]
fn test_blocks_rm_rf_root_star() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("rm -rf /*");
    assert!(result.is_some(), "rm -rf /* should be blocked");
}

#[test]
fn test_blocks_rm_r_f_root_separated_flags() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("rm -r -f /");
    assert!(result.is_some(), "rm -r -f / should be blocked");
}

// ============================================================
// BLOCKED commands -- system directory writes
// ============================================================

#[test]
fn test_blocks_write_to_etc() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("> /etc/passwd");
    assert!(result.is_some(), "write to /etc should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.reason.to_lowercase().contains("/etc") || blocked.reason.to_lowercase().contains("write"));
}

#[test]
fn test_blocks_write_to_usr() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("> /usr/local/bin/foo");
    assert!(result.is_some(), "write to /usr should be blocked");
}

// ============================================================
// BLOCKED commands -- disk operations
// ============================================================

#[test]
fn test_blocks_mkfs() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("mkfs.ext4 /dev/sda1");
    assert!(result.is_some(), "mkfs should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.reason.to_lowercase().contains("format"));
}

#[test]
fn test_blocks_dd_device_write() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("dd if=/dev/zero of=/dev/sda");
    assert!(result.is_some(), "dd device write should be blocked");
    let blocked = result.unwrap();
    assert!(blocked.reason.to_lowercase().contains("device") || blocked.reason.to_lowercase().contains("dd"));
}

// ============================================================
// BLOCKED commands -- system control
// ============================================================

#[test]
fn test_blocks_shutdown() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("shutdown -h now");
    assert!(result.is_some(), "shutdown should be blocked");
}

#[test]
fn test_blocks_reboot() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("reboot");
    assert!(result.is_some(), "reboot should be blocked");
}

// ============================================================
// BLOCKED commands -- root permission changes
// ============================================================

#[test]
fn test_blocks_chmod_root() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("chmod 777 /etc");
    assert!(result.is_some(), "chmod at root level should be blocked");
}

// ============================================================
// ALLOWED commands -- must pass through
// ============================================================

#[test]
fn test_allows_ls() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("ls -la").is_none(), "ls should be allowed");
}

#[test]
fn test_allows_cat_etc_hosts() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("cat /etc/hosts").is_none(), "reading /etc/hosts should be allowed");
}

#[test]
fn test_allows_pip_install() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("pip install requests").is_none(), "pip install should be allowed");
}

#[test]
fn test_allows_cargo_build() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("cargo build").is_none(), "cargo build should be allowed");
}

#[test]
fn test_allows_npm_install() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("npm install express").is_none(), "npm install should be allowed");
}

#[test]
fn test_allows_curl() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("curl https://example.com").is_none(), "curl should be allowed");
}

#[test]
fn test_allows_python() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("python3 script.py").is_none(), "python3 should be allowed");
}

#[test]
fn test_allows_rm_relative_path() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("rm -rf ./temp").is_none(), "rm on relative path should be allowed");
}

#[test]
fn test_allows_rm_file() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("rm my_file.txt").is_none(), "rm of a file should be allowed");
}

#[test]
fn test_allows_echo_redirect_relative() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("echo hello > output.txt").is_none(), "redirect to relative path should be allowed");
}

// ============================================================
// Edge cases
// ============================================================

#[test]
fn test_blocks_sudo_case_insensitive() {
    let filter = CommandFilter::from_defaults().unwrap();
    let result = filter.check("SUDO apt install foo");
    assert!(result.is_some(), "SUDO (uppercase) should be blocked");
}

#[test]
fn test_allows_empty_string() {
    let filter = CommandFilter::from_defaults().unwrap();
    assert!(filter.check("").is_none(), "empty string should be allowed");
}

#[test]
fn test_handles_very_long_command() {
    let filter = CommandFilter::from_defaults().unwrap();
    let long_command = "echo ".to_string() + &"a".repeat(2000);
    // Should not hang or crash -- just return None (allowed)
    assert!(filter.check(&long_command).is_none(), "long safe command should be allowed");
}

#[test]
fn test_long_command_with_blocked_pattern() {
    let filter = CommandFilter::from_defaults().unwrap();
    let long_command = "a".repeat(1000) + " sudo do_something";
    assert!(filter.check(&long_command).is_some(), "long command with sudo should still be blocked");
}

// ============================================================
// JSON serialization
// ============================================================

#[test]
fn test_blocked_command_json_serialization() {
    let filter = CommandFilter::from_defaults().unwrap();
    let blocked = filter.check("sudo rm -rf /").unwrap();
    let json = blocked.to_json();

    // Parse it back to verify it is valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should be valid JSON");
    assert_eq!(parsed["blocked"], true);
    assert!(parsed["reason"].is_string());
    assert_eq!(parsed["command"], "sudo rm -rf /");
}

#[test]
fn test_blocked_command_json_has_all_fields() {
    let blocked = BlockedCommand {
        blocked: true,
        reason: "test reason".to_string(),
        command: "test command".to_string(),
    };
    let json = blocked.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should be valid JSON");

    assert_eq!(parsed["blocked"], true);
    assert_eq!(parsed["reason"], "test reason");
    assert_eq!(parsed["command"], "test command");
}

// ============================================================
// Default blocklist coverage
// ============================================================

#[test]
fn test_default_blocklist_is_nonempty() {
    let blocklist = default_blocklist();
    assert!(!blocklist.is_empty(), "default blocklist should have entries");
    assert!(blocklist.len() >= 15, "default blocklist should cover multiple categories");
}

#[test]
fn test_default_blocklist_patterns_are_valid_regex() {
    let blocklist = default_blocklist();
    for (pattern, reason) in &blocklist {
        assert!(
            regex::Regex::new(pattern).is_ok(),
            "Pattern '{}' (reason: '{}') should be valid regex",
            pattern, reason
        );
    }
}

#[test]
fn test_default_blocklist_covers_all_categories() {
    let blocklist = default_blocklist();
    let reasons: Vec<&str> = blocklist.iter().map(|(_, r)| r.as_str()).collect();

    // Check that each major category is represented
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("sudo") || r.to_lowercase().contains("privilege")),
        "blocklist should cover sudo/privilege escalation");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("su") || r.to_lowercase().contains("privilege")),
        "blocklist should cover su");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("doas") || r.to_lowercase().contains("privilege")),
        "blocklist should cover doas");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("delet") || r.to_lowercase().contains("root")),
        "blocklist should cover destructive root operations");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("/etc") || r.to_lowercase().contains("write")),
        "blocklist should cover system directory writes");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("format") || r.to_lowercase().contains("mkfs")),
        "blocklist should cover disk formatting");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("device") || r.to_lowercase().contains("dd")),
        "blocklist should cover device writes");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("shutdown")),
        "blocklist should cover system shutdown");
    assert!(reasons.iter().any(|r| r.to_lowercase().contains("reboot")),
        "blocklist should cover system reboot");
}
