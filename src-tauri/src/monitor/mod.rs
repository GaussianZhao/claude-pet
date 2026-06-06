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
/// How far the transcript must advance past a "waiting" hook before we treat the
/// approval as granted and the turn resumed. Small: the triggering tool_use is
/// written before the hook, so any genuinely *newer* record means work moved on.
const RESUMED_MARGIN: i64 = 2;
/// How long after the prompt a metadata-only file touch counts as "the user
/// opened/engaged with this conversation" (so the pet stops nagging). Larger
/// than RESUMED_MARGIN to skip the title/mode writes that cluster around the
/// triggering tool call itself.
const ENGAGED_MARGIN: i64 = 4;
// NB: there is intentionally no fixed "waiting timeout". Waiting clears only
// when the user actually engages (opens the conversation) or work resumes, so a
// genuine approval prompt keeps showing while you're away and you never miss it.
// The HOOK_FRESH staleness window is the sole, far-off ceiling.

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
            // Liveness is driven by the last *conversational* record, not the
            // file mtime — so opening a window (a metadata-only write) or a task
            // stalling on a rate limit no longer reads as "running".
            let activity_ts = systemtime_unix(s.activity);
            let jsonl_age = if activity_ts == 0 {
                i64::MAX
            } else {
                now - activity_ts
            };
            let hook = hooks.get(&s.session_id).filter(|h| now - h.ts <= HOOK_FRESH);
            let prev_status = prev.get(&s.session_id).cloned().unwrap_or(TaskStatus::Idle);
            let ack_ts = acked.get(&s.session_id).copied().unwrap_or(0);

            let status = compute_session(
                running,
                now,
                modified_ts,
                activity_ts,
                hook,
                &prev_status,
                ack_ts,
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
    now: i64,
    modified_ts: i64,
    activity_ts: i64,
    hook: Option<&HookStatus>,
    prev: &TaskStatus,
    ack_ts: i64,
) -> TaskStatus {
    let jsonl_age = if activity_ts == 0 { i64::MAX } else { now - activity_ts };
    match hook.and_then(hook_status) {
        // RUNNING from UserPromptSubmit until a terminal event. We deliberately
        // do NOT decay on transcript quiet: a long "thinking" gap or a
        // long-running tool (e.g. a build) produces no records, but the turn is
        // still in progress — the bracketing Stop / StopFailure hook is what
        // ends it.
        Some(TaskStatus::Running) => TaskStatus::Running,
        // Authoritative approval wait (PermissionRequest / Notification). No
        // more guessing from a pending tool call, which mislabels slow tools.
        Some(TaskStatus::Waiting) => {
            let hook_ts = hook.map(|h| h.ts).unwrap_or(0);
            // Self-heal #1 (work resumed): a *conversational* record landed after
            // the prompt → approved and the turn moved on. The triggering
            // tool_use is written before the hook, so genuine waiting has
            // activity_ts <= hook_ts; a newer record crosses it.
            if activity_ts > hook_ts + RESUMED_MARGIN {
                TaskStatus::Running
            }
            // Self-heal #2 (user engaged): the conversation file was touched
            // after the prompt — opening/viewing a session writes metadata
            // (mode/title) even with no new message. Since you must be looking at
            // the conversation to approve, this also covers the approval itself.
            // While you're away and haven't looked, it keeps showing waiting.
            else if modified_ts > hook_ts + ENGAGED_MARGIN {
                TaskStatus::Running
            } else {
                TaskStatus::Waiting
            }
        }
        Some(TaskStatus::Error) => TaskStatus::Error,
        Some(TaskStatus::Completed) => {
            let hook_ts = hook.map(|h| h.ts).unwrap_or(0);
            if ack_ts >= hook_ts {
                TaskStatus::Idle // acknowledged (user opened it) → settle
            } else {
                TaskStatus::Completed // sticky until acknowledged
            }
        }
        Some(TaskStatus::Idle) => TaskStatus::Idle,
        // No hook for this session (hooks not installed, or it predates them):
        // fall back to transcript liveness. Uses conversational activity (not
        // file mtime), so merely opening a window doesn't read as running.
        None => {
            if !running {
                return TaskStatus::Idle;
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

/// Maps the latest hook event onto a task status (None = defer to liveness).
///
/// This is the heart of the state machine: Claude Code brackets a turn with
/// events, and the most recent one tells us the state directly. We trust it
/// rather than guessing from transcript timing.
pub fn hook_status(h: &HookStatus) -> Option<TaskStatus> {
    // An explicit status override wins (manual / forward-compat).
    if !h.status.is_empty() {
        return match h.status.as_str() {
            "waiting" => Some(TaskStatus::Waiting),
            "completed" => Some(TaskStatus::Completed),
            "error" => Some(TaskStatus::Error),
            "running" => Some(TaskStatus::Running),
            "idle" => Some(TaskStatus::Idle),
            _ => None,
        };
    }
    match h.event.as_str() {
        // Turn is in progress — running until a terminal event arrives. Covers
        // "thinking" gaps and long-running tools, which legitimately go quiet.
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
        | "PostToolBatch" | "PermissionDenied" => Some(TaskStatus::Running),
        // Authoritative "Claude needs you" signals.
        "PermissionRequest" => Some(TaskStatus::Waiting),
        "Notification" => match h.notification_type.as_str() {
            "permission_prompt" => Some(TaskStatus::Waiting),
            // Idle nudge ("waiting for your input") — the turn is already done.
            "idle_prompt" => Some(TaskStatus::Idle),
            // Pre-`notification_type` builds only sent Notification for approval.
            "" => Some(TaskStatus::Waiting),
            _ => None,
        },
        // Turn ended.
        "Stop" => Some(TaskStatus::Completed),
        "StopFailure" => Some(TaskStatus::Error),
        // Session boundaries — nothing is running yet / anymore.
        "SessionStart" | "SessionEnd" => Some(TaskStatus::Idle),
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

    fn notif(notification_type: &str, ts: i64) -> HashMap<String, HookStatus> {
        let mut m = HashMap::new();
        m.insert(
            "s1".to_string(),
            HookStatus {
                event: "Notification".into(),
                notification_type: notification_type.into(),
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
            activity: SystemTime::now(),
        }]
    }

    fn at(unix: i64) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_secs(unix as u64)
    }

    /// Age both the file mtime and the conversational-activity time of the only
    /// session to `unix`, mirroring a transcript whose last real turn was then.
    fn age_activity(sessions: &mut [SessionInfo], unix: i64) {
        sessions[0].modified = at(unix);
        sessions[0].activity = at(unix);
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
        age_activity(&mut sessions, stop);

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
    fn long_tool_or_thinking_stays_running() {
        // Mid-turn the transcript can go silent for a long time (a slow build,
        // or the model "thinking"). The latest hook is still a running event, so
        // we must stay RUNNING and never flip to waiting or idle.
        let old = unix_now() - 600; // 10 min of silence
        let mut sessions = one_session();
        age_activity(&mut sessions, old);

        for ev in ["PreToolUse", "PostToolUse", "UserPromptSubmit"] {
            let s = compute(
                true,
                &sessions,
                &hook(ev, old),
                &HashMap::new(),
                &HashMap::new(),
            );
            assert_eq!(s.status, TaskStatus::Running, "{ev} should stay running");
        }
    }

    #[test]
    fn permission_request_is_waiting() {
        let s = compute(
            true,
            &one_session(),
            &hook("PermissionRequest", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Waiting);
    }

    #[test]
    fn waiting_persists_while_untouched() {
        // No fixed timeout: an unapproved prompt the user hasn't looked at keeps
        // showing waiting (so they don't miss it while away), even minutes later.
        let prompt = unix_now() - 300;
        let mut sessions = one_session();
        age_activity(&mut sessions, prompt); // never touched since the prompt
        let s = compute(
            true,
            &sessions,
            &hook("PermissionRequest", prompt),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Waiting);
    }

    #[test]
    fn waiting_clears_when_user_opens_conversation() {
        // The prompt fired 10s ago; no new message, but the conversation file
        // was just touched (the user opened/viewed the session that needs
        // approval) → they're handling it, so stop showing waiting.
        let prompt = unix_now() - 10;
        let mut sessions = one_session();
        sessions[0].activity = at(prompt); // no new conversational record
        sessions[0].modified = at(unix_now() - 1); // but file touched (viewed)
        let s = compute(
            true,
            &sessions,
            &hook("PermissionRequest", prompt),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Running);
    }

    #[test]
    fn waiting_self_heals_when_work_resumes() {
        // PermissionRequest fired earlier; the transcript has since advanced
        // (approval granted, tool ran) → it must leave waiting even if no
        // follow-up hook overwrote the file.
        let approved_at = unix_now() - 30;
        let mut sessions = one_session();
        age_activity(&mut sessions, unix_now() - 2); // newer than the hook
        let s = compute(
            true,
            &sessions,
            &hook("PermissionRequest", approved_at),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Running);
    }

    #[test]
    fn notification_kinds_map_correctly() {
        // permission_prompt → waiting; idle_prompt → idle.
        let w = compute(
            true,
            &one_session(),
            &notif("permission_prompt", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(w.status, TaskStatus::Waiting);

        let i = compute(
            true,
            &one_session(),
            &notif("idle_prompt", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(i.status, TaskStatus::Idle);
    }

    #[test]
    fn stop_failure_is_error() {
        let s = compute(
            true,
            &one_session(),
            &hook("StopFailure", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Error);
    }

    #[test]
    fn session_start_is_idle() {
        // Opening/resuming a session fires SessionStart — nothing is running.
        let s = compute(
            true,
            &one_session(),
            &hook("SessionStart", unix_now()),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(s.status, TaskStatus::Idle);
    }

    #[test]
    fn opening_a_window_does_not_read_as_running() {
        // A window was just opened/focused: Claude wrote metadata (title, mode)
        // so the file mtime is fresh, but the last real turn (activity) is old
        // and no claude-pet hook has fired for this session yet. The pet must
        // stay idle, not flip to running.
        let mut sessions = one_session();
        sessions[0].modified = at(unix_now()); // fresh metadata write
        sessions[0].activity = at(unix_now() - 300); // last real turn 5 min ago
        let s = compute(true, &sessions, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(s.status, TaskStatus::Idle);
    }

    #[test]
    fn no_hook_falls_back_to_liveness() {
        // Without any hook (hooks not installed / pre-install session), recent
        // conversational activity reads as running; long silence reads as idle.
        let mut live = one_session();
        age_activity(&mut live, unix_now() - 2);
        let s = compute(true, &live, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(s.status, TaskStatus::Running);

        let mut quiet = one_session();
        age_activity(&mut quiet, unix_now() - (IDLE_AFTER + 60));
        let s2 = compute(true, &quiet, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(s2.status, TaskStatus::Idle);
    }
}
