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

## How state detection works (hybrid)

The hard part is knowing Claude's state without an official API. Three signals
fused in `src-tauri/src/monitor/`:

1. **Process scan** (`process.rs`) — is a `claude` CLI / desktop process alive?
   → `OFFLINE` vs alive. Polled every ~5 s.
2. **Session transcript** (`session.rs`) — the newest
   `~/.claude/projects/<cwd>/<session>.jsonl`. Gives project name, task title,
   and liveness (how recently the file was appended).
3. **Hook push files** (`hooks.rs`) — `~/.claude/claude-pet/sessions/<id>.json`,
   written by a Claude Code hook on `Stop` / tool-use events. Provides
   `COMPLETED` instantly.
4. **Pending tool-use heuristic** — the transcript ends with an assistant
   `tool_use` that has no `tool_result` yet, and has been quiet ≥ 5 s → `WAITING`.
   This is the only way to catch the desktop app's permission prompt, which does
   **not** fire the `Notification` hook.

`monitor/mod.rs::compute()` fuses the signals. A fresh hook event wins, but
newer transcript activity can override it (e.g. work resumed after approval).

---

## Install the hooks (recommended)

Without hooks the pet still shows `offline / running / idle / waiting`
(via the pending-tool-use heuristic). With hooks you also get reliable
`completed` and can rely on hook timestamps for state times.

```bash
./hooks/install-hooks.sh   # merges into ~/.claude/settings.json (backs it up)
```

Restart any open Claude Code sessions afterwards.

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
| AC3 | Waiting → wave animation + notify           | pending-tool-use heuristic → `animateWaiting` |
| AC4 | Completed → dance animation + notify        | `Stop` hook → `animateCompleted`         |
| AC5 | Click pet → card panel (one card per task)  | `Pet.tsx` + `StatusPanel.tsx`            |
| AC6 | macOS `.dmg` installer                      | `npm run app:build`                      |

## Performance

Targets from the PRD (CPU < 3%, RAM < 150MB): monitor does a heavy scan only
every 5 s and a cheap file read each second; the pet animation runs at 10 fps
when idle, 30 fps when active; transcripts are only re-parsed when their mtime
changes.

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
