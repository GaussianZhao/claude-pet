//! Reads the per-session status files written by the Claude Code hook script.
//!
//! The hook (see `hooks/claude-pet-hook.sh`) writes one file per session to
//! `~/.claude/claude-pet/sessions/<session_id>.json` on Notification / Stop /
//! tool-use events. This is the push channel that lets us catch
//! WAITING_APPROVAL and COMPLETED instantly, per session.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // some fields mirror the full payload for forthcoming use
pub struct HookStatus {
    /// The Claude Code hook event name, e.g. "Notification", "Stop".
    #[serde(default)]
    pub event: String,
    /// Optional explicit status override ("waiting" | "completed" | ...).
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub cwd: String,
    /// Unix epoch seconds when the event fired.
    #[serde(default)]
    pub ts: i64,
    #[serde(default)]
    pub message: String,
}

fn sessions_dir() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".claude")
            .join("claude-pet")
            .join("sessions"),
    )
}

/// Reads every per-session hook file, keyed by session id.
pub fn read_all() -> HashMap<String, HookStatus> {
    let mut out = HashMap::new();
    let Some(dir) = sessions_dir() else {
        return out;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(h) = serde_json::from_str::<HookStatus>(&raw) {
                if !h.session_id.is_empty() {
                    out.insert(h.session_id.clone(), h);
                }
            }
        }
    }
    out
}
