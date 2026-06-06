# Claude Pet 🤖

**[中文文档](README_zh.md)**

A pixel-art desktop pet for macOS that shows **Claude Code's live status** and
nudges you when Claude needs your attention — so you can keep an eye on a
session without staring at the terminal.

<img src="docs/states/running.png" width="160" alt="Claude Pet – running state">

---

## What it does

A small, always-on-top, transparent-background pet sits on your desktop.
Its animation reflects what Claude Code is doing right now.
**Click the pet** to open a card panel above it — one card per active task.
Drag the pet anywhere on screen.

### State animations

| Claude state       | Pet animation                     | Screenshot                                          |
| ------------------ | --------------------------------- | --------------------------------------------------- |
| `idle`             | gentle standing bob               | <img src="docs/states/idle.png" width="90">         |
| `running`          | arms hammering (typing)           | <img src="docs/states/running.png" width="90">      |
| `waiting`          | right arm waving + "!" bubble     | <img src="docs/states/waiting.png" width="90">      |
| `completed`        | dancing sway + floating notes     | <img src="docs/states/completed.png" width="90">    |
| `error`            | slumped posture + falling tears   | <img src="docs/states/error.png" width="90">        |
| *Claude offline*   | dimmed / faded                    | <img src="docs/states/offline.png" width="90">      |

> Screenshots live in `docs/states/`. Run `npm run dev` in a browser to see
> the mock cycle and capture each state (the mock rotates every 3.5 s).

When Claude starts **waiting for approval** or **completes a task**, a native
macOS notification fires (even if the pet is hidden behind windows).

### Card panel

The panel opens *above* the pet (the window grows upward; the pet stays put).
One horizontal strip card per active task — project · status badge · task name · time.
Cards scroll when there are many tasks; idle tasks disappear after 10 minutes.

**Sticky completion:** a finished card pulses and the pet keeps dancing until
you **click that card** to acknowledge it, so you never miss a finished task.
Clicking a card also brings the Claude app or terminal to the front.

### Menubar tray

The app shows a tray icon (menubar). The menu shows live status and offers
**Show/Hide Pet**, **Always on Top**, **Launch at Login**, and **Quit**.

### Diagnostics

```bash
"Claude Pet.app/Contents/MacOS/claude-pet" --probe
```

Runs one scan against your real environment and prints the fused `ClaudeState`
as JSON — handy for verifying detection without opening the UI.

---

## How state detection works (hook-driven state machine)

There's no official "what is Claude doing" API, so the state is driven by
**Claude Code's lifecycle hooks**, which bracket every turn. The most recent
hook event for a session decides its state directly — we trust the events
rather than guessing from file timing.

`hooks/claude-pet-hook.sh` writes one file per session to
`~/.claude/claude-pet/sessions/<id>.json` on each event; `monitor/hooks.rs`
reads them and `monitor/mod.rs::hook_status()` maps the latest event:

| Latest hook event | State |
| --- | --- |
| `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolBatch`, `PermissionDenied` | **running** |
| `PermissionRequest`, `Notification[permission_prompt]` | **waiting** |
| `Notification[idle_prompt]` | idle |
| `Stop` | **completed** (sticky until acknowledged) |
| `StopFailure` (API/rate-limit error) | **error** |
| `SessionStart`, `SessionEnd` | idle |

Key property: a turn is **running from `UserPromptSubmit` until a terminal
event** — it is *not* timed out while "thinking" or while a long tool (e.g. a
build) runs silently. Two supporting signals:

- **Process scan** (`process.rs`) — is a `claude` process alive at all? →
  `OFFLINE` vs alive. Polled every ~5 s.
- **Session transcript** (`session.rs`) — the newest
  `~/.claude/projects/<cwd>/<session>.jsonl` provides project name and task
  title. Its last *conversational* record (not the file mtime, which moves on
  metadata writes) is a liveness **fallback** used only when a session has no
  hook (hooks not installed yet).

> **Note on `waiting` and the desktop app:** "waiting for approval" relies on
> `PermissionRequest` / `Notification`. Terminal Claude Code emits these; some
> desktop-app builds may not, in which case a real permission prompt shows as
> `running` rather than `waiting`. This is deliberate — the previous
> pending-tool-use heuristic was removed because it mislabelled long-running
> tools as "waiting".

---

## Install the hooks (recommended)

The installer **copies** the hook script to a stable location
(`~/.claude/claude-pet/claude-pet-hook.sh`) — independent of this repo, so the
hooks keep working even if you move or delete the checkout — and merges the
event registrations into `~/.claude/settings.json`:

```bash
./hooks/install-hooks.sh   # safe + idempotent; backs up settings.json first
```

Restart any open Claude Code / desktop sessions afterwards so they pick up the
newly registered events.

Without hooks the pet still shows `offline / running / idle` from the transcript
fallback, but `waiting` / `completed` / `error` need the hooks.

> Set `CLAUDE_PET_DEBUG=1` in the environment to log every hook event to
> `~/.claude/claude-pet/events.log` (off by default; capped at 400 lines).

---

## Develop

```bash
npm install
npm run app:dev      # tauri dev — launches the pet with hot reload
```

`npm run dev` alone opens the web UI in a browser; with no Tauri runtime it
runs a **demo cycle** through all states so you can preview animations.

## Build the installer (.dmg)

```bash
# one-time: generate icon set from the source PNG
cargo tauri icon src-tauri/icons/source.png

npm run app:build    # .dmg lands in src-tauri/target/release/bundle/dmg/
```

> **Note:** `tauri build` embeds the frontend. Running `cargo build --release`
> alone produces a dev-mode binary that loads `localhost:1420` and shows blank —
> always test via the bundled `.app`.

---

## Acceptance criteria

| ID  | Criterion                                   | Where                                    |
| --- | ------------------------------------------- | ---------------------------------------- |
| AC1 | Pet shows on launch                         | `tauri.conf.json` window + `App.tsx`     |
| AC2 | Running → arm-hammering animation           | `process.rs` + `PixelPet.animateRunning` |
| AC3 | Waiting → wave animation + notify           | `PermissionRequest`/`Notification` hook → `animateWaiting` |
| AC4 | Completed → dance animation + notify        | `Stop` hook → `animateCompleted`         |
| AC5 | Click pet → card panel (one card per task)  | `Pet.tsx` + `StatusPanel.tsx`            |
| AC6 | macOS `.dmg` installer                      | `npm run app:build`                      |

## Performance

Targets from the PRD (CPU < 3%, RAM < 150MB): monitor does a heavy scan only
every 5 s and a cheap file read each second; the pet animation runs at 10 fps
when idle, 30 fps when active; transcripts are only re-parsed when their mtime
changes.

## Running long-term on your machine

Notes for leaving it installed and running every day:

- **Hook script lives in `~/.claude/`, not this repo.** `install-hooks.sh` copies
  it to `~/.claude/claude-pet/`, so deleting/moving the checkout won't break your
  Claude sessions. Re-running the installer is idempotent (no duplicate entries).
- **Bounded disk usage.** Per-session status files in
  `~/.claude/claude-pet/sessions/` are pruned after 1 day. The debug
  `events.log` is **off by default** (opt-in via `CLAUDE_PET_DEBUG=1`) and capped
  at 400 lines.
- **Large transcripts.** A session's `.jsonl` is parsed in full whenever its
  mtime changes (~every 5 s while active). Very long sessions (tens of MB) make
  that parse heavier; it's cached by mtime so idle sessions cost nothing.
- **Updating the hook.** If you change `hooks/claude-pet-hook.sh`, re-run
  `./hooks/install-hooks.sh` to copy the new version into `~/.claude/`.
- **Upgrading Claude Code.** Hooks live in your user `settings.json` and persist
  across upgrades. Newer events (`PermissionRequest`, `StopFailure`, …) are
  ignored by versions that don't emit them — harmless.

---

## Tech stack

| Layer   | Choice                        |
|---------|-------------------------------|
| UI      | React + TypeScript + Vite     |
| Renderer| PixiJS v8 (WebGL, procedural) |
| Desktop | Tauri v2                      |
| Backend | Rust                          |
| Notify  | macOS Notification Center     |

---

## Roadmap

V1.1 token/quota stats · V1.2 GitHub Copilot · V1.3 Cursor · V2.0 multi-agent center
