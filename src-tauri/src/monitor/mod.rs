//! ClaudeMonitor — fuses process / session / hook signals into per-task cards
//! plus one overall state for the pet.

pub mod hooks;
pub mod process;
pub mod session;

use chrono::Local;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use hooks::HookStatus;
use session::SessionInfo;

/// Fresh transcript activity (seconds) that makes us *enter* RUNNING quickly.
const RUNNING_WINDOW: i64 = 8;
/// Hysteresis: once RUNNING, only fall back to IDLE after this much quiet, so a
/// long tool call / model "thinking" gap doesn't flicker us to IDLE.
const IDLE_AFTER: i64 = 45;
/// Hook events older than this are ignored as stale.
const HOOK_FRESH: i64 = 3600;
/// An IDLE task stops being shown once it's been idle this long (10 min).
const SHOW_WINDOW: i64 = 600;
/// A pending tool_use must sit quiet this long before we call it "waiting for
/// approval" (avoids flagging normal fast tool calls; a tool that genuinely
/// runs longer than this will briefly show as waiting too — there's no external
/// way to tell a permission wait from a long-running tool).
const WAITING_QUIET: i64 = 5;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Idle,
    Running,
    Waiting,
    Completed,
    Error,
}

impl TaskStatus {
    /// Attention priority — the pet shows the highest among all sessions.
    fn priority(&self) -> u8 {
        match self {
            TaskStatus::Waiting => 4,
            TaskStatus::Error => 3,
            TaskStatus::Completed => 2,
            TaskStatus::Running => 1,
            TaskStatus::Idle => 0,
        }
    }
}

/// One task card.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub session_id: String,
    pub project: String,
    pub task_name: String,
    pub status: TaskStatus,
    pub cwd: String,
    pub updated_at: String,
}

/// What the app pushes to the webview each tick.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetState {
    /// Is any `claude` process alive at all (F2).
    pub running: bool,
    /// The single state the pet animates (highest-priority session).
    pub status: TaskStatus,
    /// One entry per active task, highest-priority first.
    pub sessions: Vec<SessionState>,
}

impl Default for PetState {
    fn default() -> Self {
        Self {
            running: false,
            status: TaskStatus::Idle,
            sessions: Vec::new(),
        }
    }
}

/// Builds the full pet state from all signals.
///
/// - `acked[session_id]` = unix ts the user last acknowledged that session.
///   A COMPLETED session stays COMPLETED until its completion is acknowledged
///   (the user opens it) — it never auto-settles to idle on its own.
/// - `prev[session_id]` = that session's previous status (for RUNNING/IDLE
///   hysteresis without hooks).
pub fn compute(
    running: bool,
    sessions: &[SessionInfo],
    hooks: &HashMap<String, HookStatus>,
    acked: &HashMap<String, i64>,
    prev: &HashMap<String, TaskStatus>,
) -> PetState {
    let now = unix_now();

    let mut cards: Vec<(SessionState, i64)> = sessions
        .iter()
        .map(|s| {
            let modified_ts = systemtime_unix(s.modified);
            let jsonl_age = if modified_ts == 0 {
                i64::MAX
            } else {
                now - modified_ts
            };
            let hook = hooks.get(&s.session_id).filter(|h| now - h.ts <= HOOK_FRESH);
            let prev_status = prev.get(&s.session_id).cloned().unwrap_or(TaskStatus::Idle);
            let ack_ts = acked.get(&s.session_id).copied().unwrap_or(0);

            let status = compute_session(
                running,
                modified_ts,
                jsonl_age,
                hook,
                &prev_status,
                ack_ts,
                s.pending_tool_use,
            );

            (
                SessionState {
                    session_id: s.session_id.clone(),
                    project: s.project.clone(),
                    task_name: s.task_name.clone(),
                    status,
                    cwd: s.cwd.clone(),
                    updated_at: fmt_ts(modified_ts),
                },
                jsonl_age,
            )
        })
        // Only surface tasks that matter right now.
        .filter(|(c, age)| c.status != TaskStatus::Idle || *age < SHOW_WINDOW)
        .collect();

    // Highest attention first, then most recently active.
    cards.sort_by(|a, b| {
        b.0.status
            .priority()
            .cmp(&a.0.status.priority())
            .then(a.1.cmp(&b.1))
    });

    let pet_status = cards
        .iter()
        .map(|(c, _)| c.status.clone())
        .max_by_key(|s| s.priority())
        .unwrap_or(TaskStatus::Idle);

    PetState {
        running,
        status: pet_status,
        sessions: cards.into_iter().map(|(c, _)| c).collect(),
    }
}

fn compute_session(
    running: bool,
    modified_ts: i64,
    jsonl_age: i64,
    hook: Option<&HookStatus>,
    prev: &TaskStatus,
    ack_ts: i64,
    pending_tool_use: bool,
) -> TaskStatus {
    let hook_ts = hook.map(|h| h.ts).unwrap_or(0);
    let resumed_after_hook = modified_ts > hook_ts + 1;
    // Claude emitted a tool call (assistant tool_use, no tool_result yet) and
    // has gone quiet → it's blocked, almost always on the user's approval. This
    // is how we catch the desktop app's stdio permission prompt, which fires
    // NO Notification hook and leaves PreToolUse as the latest event.
    let awaiting_approval = pending_tool_use && jsonl_age >= WAITING_QUIET;

    match hook.and_then(hook_status) {
        Some(TaskStatus::Waiting) => {
            if resumed_after_hook && jsonl_age < RUNNING_WINDOW {
                TaskStatus::Running
            } else {
                TaskStatus::Waiting
            }
        }
        Some(TaskStatus::Completed) => {
            if resumed_after_hook && jsonl_age < RUNNING_WINDOW {
                TaskStatus::Running
            } else if ack_ts >= hook_ts {
                // Acknowledged (user opened it) → it may settle to idle.
                TaskStatus::Idle
            } else {
                // Sticky: stays completed until acknowledged.
                TaskStatus::Completed
            }
        }
        Some(TaskStatus::Error) => TaskStatus::Error,
        Some(TaskStatus::Running) => {
            if awaiting_approval {
                TaskStatus::Waiting
            } else {
                TaskStatus::Running
            }
        }
        Some(TaskStatus::Idle) => TaskStatus::Idle,
        None => {
            if !running {
                return TaskStatus::Idle;
            }
            if awaiting_approval {
                return TaskStatus::Waiting;
            }
            let quiet_limit = if *prev == TaskStatus::Running {
                IDLE_AFTER
            } else {
                RUNNING_WINDOW
            };
            if jsonl_age < quiet_limit {
                TaskStatus::Running
            } else {
                TaskStatus::Idle
            }
        }
    }
}

/// Maps a hook event/status string onto a task status (None = defer to liveness).
pub fn hook_status(h: &HookStatus) -> Option<TaskStatus> {
    let key = if !h.status.is_empty() {
        h.status.as_str()
    } else {
        h.event.as_str()
    };
    match key {
        "waiting" | "Notification" => Some(TaskStatus::Waiting),
        "completed" | "Stop" => Some(TaskStatus::Completed),
        "error" | "Error" => Some(TaskStatus::Error),
        "running" | "PreToolUse" | "PostToolUse" | "UserPromptSubmit" => Some(TaskStatus::Running),
        "idle" => Some(TaskStatus::Idle),
        _ => None,
    }
}

fn fmt_ts(unix: i64) -> String {
    if unix == 0 {
        return String::new();
    }
    use chrono::TimeZone;
    Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// "HH:MM" for today, else "MM-DD HH:MM". Used for the per-card transition time.
pub fn fmt_time_short(unix: i64) -> String {
    if unix == 0 {
        return String::new();
    }
    use chrono::TimeZone;
    let Some(t) = Local.timestamp_opt(unix, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    if t.date_naive() == today {
        t.format("%H:%M").to_string()
    } else {
        t.format("%m-%d %H:%M").to_string()
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn systemtime_unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(event: &str, ts: i64) -> HashMap<String, HookStatus> {
        let mut m = HashMap::new();
        m.insert(
            "s1".to_string(),
            HookStatus {
                event: event.into(),
                session_id: "s1".into(),
                ts,
                ..Default::default()
            },
        );
        m
    }

    fn one_session() -> Vec<SessionInfo> {
        vec![SessionInfo {
            project: "proj".into(),
            task_name: "t".into(),
            session_id: "s1".into(),
            cwd: "/tmp/proj".into(),
            modified: SystemTime::now(),
            pending_tool_use: false,
        }]
    }

    #[test]
    fn waiting_from_hook() {
        let s = compute(
            true,
            &one_session(),
            &hook("Notification", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Waiting);
        assert_eq!(s.sessions.len(), 1);
    }

    #[test]
    fn completed_is_sticky_until_acknowledged() {
        // Stop fired a minute ago; transcript hasn't moved since.
        let stop = unix_now() - 60;
        let mut sessions = one_session();
        sessions[0].modified = UNIX_EPOCH + std::time::Duration::from_secs(stop as u64);

        let unacked = compute(
            true,
            &sessions,
            &hook("Stop", stop),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(unacked.status, TaskStatus::Completed, "stays completed");

        let mut acked = HashMap::new();
        acked.insert("s1".to_string(), unix_now());
        let done = compute(true, &sessions, &hook("Stop", stop), &acked, &HashMap::new());
        assert_eq!(done.status, TaskStatus::Idle, "settles after acknowledge");
    }

    #[test]
    fn working_hook_stays_running() {
        let s = compute(
            true,
            &one_session(),
            &hook("PreToolUse", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Running);
    }

    #[test]
    fn pending_tool_use_without_pretool_is_waiting() {
        // Tool call outstanding for >WAITING_QUIET, latest hook is PostToolUse
        // (not PreToolUse) → the user is being asked to approve it.
        let mut sessions = one_session();
        sessions[0].pending_tool_use = true;
        let old = unix_now() - 10;
        sessions[0].modified = UNIX_EPOCH + std::time::Duration::from_secs(old as u64);

        let s = compute(
            true,
            &sessions,
            &hook("PostToolUse", old),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Waiting);
    }

    #[test]
    fn pending_tool_use_with_pretool_is_also_waiting() {
        // PreToolUse fires BEFORE the permission prompt, so a pending tool_use
        // that's gone quiet is waiting even when PreToolUse is the latest event.
        let mut sessions = one_session();
        sessions[0].pending_tool_use = true;
        let old = unix_now() - 10;
        sessions[0].modified = UNIX_EPOCH + std::time::Duration::from_secs(old as u64);

        let s = compute(
            true,
            &sessions,
            &hook("PreToolUse", old),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Waiting);
    }

    #[test]
    fn fresh_tool_call_is_not_yet_waiting() {
        // A pending tool_use that's still fresh (< WAITING_QUIET) is running,
        // so normal fast tool calls don't flicker to waiting.
        let mut sessions = one_session();
        sessions[0].pending_tool_use = true; // modified = now (fresh)
        let s = compute(
            true,
            &sessions,
            &hook("PreToolUse", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Running);
    }
}
