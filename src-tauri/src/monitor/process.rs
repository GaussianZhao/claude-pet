//! Detects whether a Claude Code CLI process is currently running.

use sysinfo::System;

/// Returns true if at least one `claude` CLI process is alive.
///
/// Claude Code may run as a native `claude` binary or as a node script whose
/// argv contains a path ending in `/claude`, so we check both the process name
/// and its command line.
pub fn is_claude_running(sys: &System) -> bool {
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_ascii_lowercase();
        if name == "claude" {
            return true;
        }
        p.cmd().iter().any(|arg| {
            let a = arg.to_string_lossy();
            a.ends_with("/claude")
                || a.contains("/.claude/local/claude")
                || a.contains("claude-code")
        })
    })
}
