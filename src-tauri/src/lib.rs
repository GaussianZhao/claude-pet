//! Claude Pet — Tauri application entry point.
//!
//! Spawns the ClaudeMonitor on a background thread, pushes per-task state to the
//! webview via the `claude-state` event, drives a menubar tray, and fires
//! native macOS notifications when a task starts waiting or finishes.

mod monitor;

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use monitor::session::SessionInfo;
use monitor::{PetState, TaskStatus};
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder,
    PredefinedMenuItem,
};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

#[derive(Default)]
struct Tray {
    status: Option<MenuItem<Wry>>,
    ontop: Option<CheckMenuItem<Wry>>,
}

struct AppState {
    last: Mutex<PetState>,
    /// session_id → unix ts the user acknowledged that session's completion.
    acked: Mutex<HashMap<String, i64>>,
    /// session_id → (current status, unix ts it entered that status).
    since: Mutex<HashMap<String, (TaskStatus, i64)>>,
    /// The pet's fixed bottom-center (logical screen px) while the panel is
    /// open, captured once on open so repeated resizes don't drift.
    anchor: Mutex<Option<(f64, f64)>>,
    tray: Mutex<Tray>,
}

#[tauri::command]
fn get_state(state: tauri::State<AppState>) -> PetState {
    state.last.lock().unwrap().clone()
}

/// Marks a task's completion as seen so the pet may leave COMPLETED.
#[tauri::command]
fn acknowledge_session(session_id: String, state: tauri::State<AppState>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    state.acked.lock().unwrap().insert(session_id, now);
}

/// Best-effort: surface where a task is running. There's no public API to
/// deep-link to a specific Claude session, so we bring the host to the front.
///
/// Uses `open` (LaunchServices) rather than AppleScript so it works without
/// Automation permission (which an unsigned app can't reliably obtain). Prefers
/// the Claude desktop app; falls back to a running terminal.
#[tauri::command]
fn open_session(_session_id: String, _cwd: String) -> Result<(), String> {
    // Claude desktop app (where agent sessions live for most users).
    let claude_ok = std::process::Command::new("open")
        .args(["-b", "com.anthropic.claudefordesktop"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if claude_ok {
        return Ok(());
    }

    // Fallback: activate a *running* terminal (open -a NAME, detect via procname
    // so we never launch a terminal that wasn't already open).
    const TERMINALS: &[(&str, &str)] = &[
        ("iTerm", "iTerm2"),
        ("Terminal", "Terminal"),
        ("Warp", "stable"),
        ("Ghostty", "ghostty"),
        ("WezTerm", "wezterm-gui"),
        ("Alacritty", "alacritty"),
        ("kitty", "kitty"),
    ];
    for (app, proc) in TERMINALS {
        let running = std::process::Command::new("pgrep")
            .args(["-x", proc])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if running {
            let _ = std::process::Command::new("open").args(["-a", app]).status();
            break;
        }
    }
    Ok(())
}

#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, value: bool) -> Result<(), String> {
    window.set_always_on_top(value).map_err(|e| e.to_string())
}

/// Resizes the window to `width`×`height` logical px while keeping its
/// BOTTOM-CENTER fixed: the panel expands up/out over the desktop and the pet
/// doesn't move. The window stays small (just the pet) when the panel is closed,
/// so the surrounding transparent area doesn't swallow desktop clicks.
#[tauri::command]
fn resize_window(
    window: tauri::WebviewWindow,
    state: tauri::State<AppState>,
    width: f64,
    height: f64,
    anchor: bool,
) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let mut stored = state.anchor.lock().unwrap();

    // Recompute the bottom-center from live geometry only when asked (on open)
    // or if we don't have one yet; otherwise reuse it so repeated resizes can't
    // drift from stale reads.
    let (center_x, bottom_y) = if anchor || stored.is_none() {
        let pos = window
            .outer_position()
            .map_err(|e| e.to_string())?
            .to_logical::<f64>(scale);
        let size = window
            .inner_size()
            .map_err(|e| e.to_string())?
            .to_logical::<f64>(scale);
        let v = (pos.x + size.width / 2.0, pos.y + size.height);
        *stored = Some(v);
        v
    } else {
        stored.unwrap()
    };

    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(LogicalPosition::new(center_x - width / 2.0, bottom_y - height))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn status_label(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Idle => "Idle",
        TaskStatus::Running => "Running",
        TaskStatus::Waiting => "Waiting Approval",
        TaskStatus::Completed => "Completed",
        TaskStatus::Error => "Error",
    }
}

fn spawn_monitor(app: tauri::AppHandle) {
    thread::spawn(move || {
        use std::path::PathBuf;
        use std::time::SystemTime;
        use sysinfo::{ProcessesToUpdate, System};

        let mut sys = System::new();
        let mut tick: u64 = 0;
        let mut running = false;
        let mut sessions: Vec<SessionInfo> = Vec::new();
        // Parse cache: path → (mtime, parsed). Avoids re-reading big transcripts.
        let mut cache: HashMap<PathBuf, (SystemTime, SessionInfo)> = HashMap::new();

        loop {
            if tick % 5 == 0 {
                sys.refresh_processes(ProcessesToUpdate::All, true);
                running = monitor::process::is_claude_running(&sys);

                let paths = monitor::session::recent_paths();
                let mut next = Vec::with_capacity(paths.len());
                let mut fresh_cache = HashMap::with_capacity(paths.len());
                for (path, mtime) in paths {
                    let info = match cache.get(&path) {
                        Some((m, info)) if *m == mtime => info.clone(),
                        _ => match monitor::session::parse_session(&path, mtime) {
                            Some(i) => i,
                            None => continue,
                        },
                    };
                    fresh_cache.insert(path, (mtime, info.clone()));
                    next.push(info);
                }
                cache = fresh_cache;
                sessions = next;
            }

            let hooks = monitor::hooks::read_all();

            let state = app.state::<AppState>();
            let prev_map: HashMap<String, TaskStatus> = {
                let last = state.last.lock().unwrap();
                last.sessions
                    .iter()
                    .map(|s| (s.session_id.clone(), s.status.clone()))
                    .collect()
            };
            let acked = state.acked.lock().unwrap().clone();

            let mut next = monitor::compute(running, &sessions, &hooks, &acked, &prev_map);
            stamp_since(&state, &mut next, &hooks);

            let mut last = state.last.lock().unwrap();
            if *last != next {
                let prev_status = last.status.clone();
                *last = next.clone();
                drop(last);

                let _ = app.emit("claude-state", &next);
                maybe_notify(&app, &prev_status, &next);
                update_tray(&app, &next);
            }

            tick = tick.wrapping_add(1);
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Stamps each card with the time it entered its current status, so the UI can
/// show "Started …" for running tasks and the transition time for the rest.
fn stamp_since(
    state: &tauri::State<AppState>,
    next: &mut PetState,
    hooks: &HashMap<String, monitor::hooks::HookStatus>,
) {
    use std::collections::hash_map::Entry;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut since = state.since.lock().unwrap();
    let mut seen = std::collections::HashSet::new();
    for s in next.sessions.iter_mut() {
        seen.insert(s.session_id.clone());
        let ts = match since.entry(s.session_id.clone()) {
            Entry::Occupied(mut o) => {
                if o.get().0 != s.status {
                    let t = transition_ts(hooks, &s.session_id, &s.status, now);
                    o.insert((s.status.clone(), t));
                    t
                } else {
                    o.get().1
                }
            }
            Entry::Vacant(v) => {
                let t = transition_ts(hooks, &s.session_id, &s.status, now);
                v.insert((s.status.clone(), t));
                t
            }
        };
        s.updated_at = monitor::fmt_time_short(ts);
    }
    since.retain(|k, _| seen.contains(k));
}

/// Prefer the hook event time when it matches the new status (accurate + stable
/// across restarts); otherwise the moment we observed the transition.
fn transition_ts(
    hooks: &HashMap<String, monitor::hooks::HookStatus>,
    id: &str,
    status: &TaskStatus,
    now: i64,
) -> i64 {
    if let Some(h) = hooks.get(id) {
        if monitor::hook_status(h).as_ref() == Some(status) && h.ts > 0 && h.ts <= now {
            return h.ts;
        }
    }
    now
}

fn update_tray(app: &tauri::AppHandle, s: &PetState) {
    let state = app.state::<AppState>();
    let tray = state.tray.lock().unwrap();
    if let Some(item) = &tray.status {
        let dot = if s.running { "🟢" } else { "⚪" };
        let n = s.sessions.len();
        let _ = item.set_text(format!(
            "{dot} {} · {} task{}",
            status_label(&s.status),
            n,
            if n == 1 { "" } else { "s" }
        ));
    }
    if let Some(icon) = app.tray_by_id("main") {
        let _ = icon.set_tooltip(Some(&format!("Claude Pet — {}", status_label(&s.status))));
    }
}

/// Fires a notification when the pet enters WAITING or COMPLETED (F4 / F5).
fn maybe_notify(app: &tauri::AppHandle, prev: &TaskStatus, next: &PetState) {
    let body = match &next.status {
        TaskStatus::Waiting if *prev != TaskStatus::Waiting => "Claude 正在等待你的确认",
        TaskStatus::Completed if *prev != TaskStatus::Completed => "Claude 任务已完成",
        _ => return,
    };
    let _ = app
        .notification()
        .builder()
        .title("Claude Pet")
        .body(body)
        .show();
}

fn handle_menu_event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "quit" => app.exit(0),
        "show" => {
            if let Some(w) = app.get_webview_window("pet") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        "hide" => {
            if let Some(w) = app.get_webview_window("pet") {
                let _ = w.hide();
            }
        }
        "ontop" => {
            let state = app.state::<AppState>();
            let checked = state
                .tray
                .lock()
                .unwrap()
                .ontop
                .as_ref()
                .and_then(|i| i.is_checked().ok())
                .unwrap_or(true);
            if let Some(w) = app.get_webview_window("pet") {
                let _ = w.set_always_on_top(checked);
            }
        }
        "autostart" => {
            let mgr = app.autolaunch();
            match mgr.is_enabled() {
                Ok(true) => {
                    let _ = mgr.disable();
                }
                _ => {
                    let _ = mgr.enable();
                }
            }
        }
        _ => {}
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let status = MenuItemBuilder::with_id("status", "⚪ Idle")
        .enabled(false)
        .build(app)?;
    let show = MenuItemBuilder::with_id("show", "Show Pet").build(app)?;
    let hide = MenuItemBuilder::with_id("hide", "Hide Pet").build(app)?;
    let ontop = CheckMenuItemBuilder::with_id("ontop", "Always on Top")
        .checked(true)
        .build(app)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItemBuilder::with_id("autostart", "Launch at Login")
        .checked(autostart_on)
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Claude Pet").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &hide,
            &ontop,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ])
        .build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Claude Pet")
        .menu(&menu)
        .on_menu_event(handle_menu_event)
        .build(app)?;

    let state = app.state::<AppState>();
    let mut tray = state.tray.lock().unwrap();
    tray.status = Some(status);
    tray.ontop = Some(ontop);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            last: Mutex::new(PetState::default()),
            acked: Mutex::new(HashMap::new()),
            since: Mutex::new(HashMap::new()),
            anchor: Mutex::new(None),
            tray: Mutex::new(Tray::default()),
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            acknowledge_session,
            open_session,
            set_always_on_top,
            resize_window
        ])
        .setup(|app| {
            // The borderless transparent pet window must be shown + raised.
            if let Some(win) = app.get_webview_window("pet") {
                let _ = win.show();
                let _ = win.set_always_on_top(true);
                let _ = win.set_visible_on_all_workspaces(true);
            }
            setup_tray(app)?;
            spawn_monitor(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Claude Pet");
}
