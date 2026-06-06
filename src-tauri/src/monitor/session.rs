//! Reads recent Claude Code sessions from `~/.claude/projects`.
//!
//! Each session is a JSONL transcript at
//! `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`. Liveness comes from
//! the timestamp of the last *conversational* record (a real user/assistant
//! turn), not the file mtime — Claude rewrites metadata (titles, mode) on every
//! window open, which would otherwise look like activity. We also parse a few
//! fields out of the transcript for display (project, task title, cwd).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many of the most-recently-touched transcripts we parse per poll.
const MAX_SESSIONS: usize = 12;
/// Ignore transcripts older than this (seconds) — not a "current" task.
const MAX_AGE_SECS: u64 = 2 * 60 * 60;

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub project: String,
    pub task_name: String,
    pub session_id: String,
    /// Launch directory of the session (for "open").
    pub cwd: String,
    /// File mtime. Moves on any write, including metadata-only writes Claude
    /// makes when a window is merely opened or focused (custom-title, ai-title,
    /// mode, queue-operation), so it is NOT a reliable "task running" signal.
    /// Kept for cache invalidation / fallback only.
    pub modified: SystemTime,
    /// Timestamp of the last *conversational* record (a real `user` prompt or
    /// `assistant` turn). This is our RUNNING/IDLE signal: unlike `modified` it
    /// does not move when a window is just opened (metadata-only writes) and it
    /// stops advancing the moment a task is interrupted (rate limit, error,
    /// Ctrl-C), so a stalled task goes quiet and settles to idle. Falls back to
    /// `modified` when no conversational record carries a parseable timestamp.
    pub activity: SystemTime,
}

/// The most-recently-active transcript paths (newest first), capped and
/// age-filtered. Cheap (stat only) so the caller can cache parses by mtime.
pub fn recent_paths() -> Vec<(PathBuf, SystemTime)> {
    let mut paths = collect_transcripts();
    paths.sort_by(|a, b| b.1.cmp(&a.1)); // newest mtime first

    let now = SystemTime::now();
    paths.retain(|(_, m)| {
        now.duration_since(*m)
            .map(|d| d.as_secs() <= MAX_AGE_SECS)
            .unwrap_or(true)
    });
    paths.truncate(MAX_SESSIONS);
    paths
}

/// Parses the most-recently-active sessions (newest first).
#[cfg(test)]
pub fn recent_sessions() -> Vec<SessionInfo> {
    recent_paths()
        .into_iter()
        .filter_map(|(path, modified)| parse_session(&path, modified))
        .collect()
}

fn collect_transcripts() -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    let Some(base) = dirs::home_dir().map(|h| h.join(".claude").join("projects")) else {
        return out;
    };
    let Ok(projects) = fs::read_dir(&base) else {
        return out;
    };
    for proj in projects.flatten() {
        let pdir = proj.path();
        if !pdir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&pdir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(m) = entry.metadata().ok().and_then(|md| md.modified().ok()) {
                out.push((path, m));
            }
        }
    }
    out
}

pub fn parse_session(path: &Path, modified: SystemTime) -> Option<SessionInfo> {
    let content = fs::read_to_string(path).ok()?;

    // The project root is the directory Claude was launched from. cwd can move
    // into subdirectories during a session, so the *shortest* observed cwd is
    // the most reliable root.
    let mut root_cwd: Option<String> = None;
    let mut title = String::new();
    let mut session_id = String::new();
    let mut last_user_prompt = String::new();
    let mut last_activity: Option<SystemTime> = None;

    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        if session_id.is_empty() {
            if let Some(s) = v.get("sessionId").and_then(|x| x.as_str()) {
                session_id = s.to_string();
            }
        }

        if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
            if root_cwd.as_ref().map_or(true, |r| c.len() < r.len()) {
                root_cwd = Some(c.to_string());
            }
        }

        match v.get("type").and_then(|x| x.as_str()) {
            Some("custom-title") => {
                if let Some(t) = v.get("content").and_then(|x| x.as_str()) {
                    title = t.to_string();
                }
            }
            Some("ai-title") => {
                if title.is_empty() {
                    if let Some(t) = v.get("content").and_then(|x| x.as_str()) {
                        title = t.to_string();
                    }
                }
            }
            Some("assistant") => {
                if let Some(t) = record_time(&v) {
                    last_activity = Some(t);
                }
            }
            Some("user") => {
                if let Some(text) = extract_user_text(&v) {
                    last_user_prompt = text;
                }
                if let Some(t) = record_time(&v) {
                    last_activity = Some(t);
                }
            }
            _ => {}
        }
    }

    // Session id falls back to the file stem if the transcript had none yet.
    if session_id.is_empty() {
        session_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
    }

    let cwd = root_cwd.unwrap_or_default();
    let project = Path::new(&cwd)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let task_name = if !title.is_empty() {
        title
    } else if !last_user_prompt.is_empty() {
        truncate(&last_user_prompt, 80)
    } else {
        String::new()
    };

    Some(SessionInfo {
        project,
        task_name,
        session_id,
        cwd,
        modified,
        // No parseable conversational timestamp (e.g. an empty or freshly
        // opened session) → fall back to file mtime so behaviour degrades
        // gracefully rather than treating the session as ancient.
        activity: last_activity.unwrap_or(modified),
    })
}

/// Parses a transcript record's RFC3339 `timestamp` into a `SystemTime`.
/// Returns `None` for missing / unparseable / pre-epoch values.
fn record_time(v: &serde_json::Value) -> Option<SystemTime> {
    let s = v.get("timestamp").and_then(|t| t.as_str())?;
    let secs = chrono::DateTime::parse_from_rfc3339(s).ok()?.timestamp();
    (secs >= 0).then(|| UNIX_EPOCH + Duration::from_secs(secs as u64))
}

fn extract_user_text(v: &serde_json::Value) -> Option<String> {
    let content = v.get("message")?.get("content")?;
    let text = match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    };
    let first_line = text.lines().next().unwrap_or("").trim().to_string();
    if first_line.is_empty() || first_line.starts_with('<') || first_line.starts_with('/') {
        None
    } else {
        Some(first_line)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
