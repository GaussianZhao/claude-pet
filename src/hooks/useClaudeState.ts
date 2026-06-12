import { useEffect, useState } from "react";
import { EMPTY_PET, type PetState } from "../types";

export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const now = () => new Date().toISOString().slice(0, 16).replace("T", " ");

/**
 * Subscribes to the Rust `claude-state` event (a PetState with one entry per
 * task). In a plain browser it falls back to a demo with several cards so the
 * card stack can be exercised without Claude attached.
 */
export function useClaudeState(): PetState {
  const [state, setState] = useState<PetState>(EMPTY_PET);

  useEffect(() => {
    if (!isTauri) return startMock(setState);

    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      const { listen } = await import("@tauri-apps/api/event");
      try {
        const initial = await invoke<PetState>("get_state");
        if (!cancelled) setState(initial);
      } catch {
        /* command not ready yet — the event will populate it */
      }
      const stop = await listen<PetState>("claude-state", (e) => {
        setState(e.payload);
      });
      if (cancelled) stop();
      else unlisten = stop;
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return state;
}

function startMock(set: (s: PetState) => void) {
  const sessions = [
    { sessionId: "a", project: "auth-uar-intelli-mind", taskName: "检查项目，清理无用代码，添加决策逻辑查看入口", status: "waiting", cwd: "/x", updatedAt: now() },
    { sessionId: "b", project: "applog-ingestion-agent", taskName: "Generating dashboard components", status: "running", cwd: "/y", updatedAt: now() },
    { sessionId: "c", project: "data-intake-manager", taskName: "Refactor pipeline retries", status: "completed", cwd: "/z", updatedAt: now() },
    { sessionId: "d", project: "ai-coding-pet", taskName: "npm run build failed", status: "error", cwd: "/w", updatedAt: now() },
  ] as const;
  const usage = {
    fiveHour: { usedPercent: 22, resetsAt: Math.floor(Date.now() / 1000) + 3 * 3600 },
    sevenDay: { usedPercent: 48, resetsAt: Math.floor(Date.now() / 1000) + 26 * 3600 },
    status: "allowed",
  };
  let n = 4;
  // Expose a global so the puppeteer screenshot script can inject a specific
  // state and stop the auto-cycle so it doesn't override the injected state.
  let intervalId: ReturnType<typeof setInterval> | undefined;
  (window as unknown as Record<string, unknown>).__setMockState = (s: PetState) => {
    if (intervalId !== undefined) clearInterval(intervalId);
    set(s);
  };
  const tick = () => {
    const visible = sessions.slice(0, n).map((s) => ({ ...s, updatedAt: now() }));
    const order = { waiting: 4, error: 3, completed: 2, running: 1, idle: 0 } as const;
    const status = visible.reduce<PetState["status"]>(
      (acc, s) => (order[s.status] > order[acc] ? s.status : acc),
      "idle",
    );
    set({ running: true, status, sessions: visible, usage });
    n = n === 1 ? 4 : n - 1;
  };
  tick();
  intervalId = window.setInterval(tick, 3500);
  return () => { if (intervalId !== undefined) clearInterval(intervalId); };
}
