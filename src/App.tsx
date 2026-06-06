import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Pet } from "./components/Pet";
import { StatusPanel } from "./components/StatusPanel";
import { useClaudeState, isTauri } from "./hooks/useClaudeState";
import "./App.css";

// Window geometry (logical px). Closed: tight around the pet (small click
// footprint). Open: wide enough for the cards. Keep CLOSED_W/BASE_H in sync with
// tauri.conf.json and the pet canvas size / `.pet-anchor` in App.css.
const BASE_H = 150;
const CLOSED_W = 150;
const OPEN_W = 280;

export default function App() {
  const state = useClaudeState();
  const [open, setOpen] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const [panelH, setPanelH] = useState(0);
  const prevOpen = useRef(false);

  // Ask for notification permission once.
  useEffect(() => {
    if (!isTauri) return;
    (async () => {
      const { isPermissionGranted, requestPermission } = await import(
        "@tauri-apps/plugin-notification"
      );
      if (!(await isPermissionGranted())) await requestPermission();
    })();
  }, []);

  // Measure the actual panel height (no guessing) so the window fits it exactly.
  useLayoutEffect(() => {
    if (open && panelRef.current) {
      setPanelH(Math.ceil(panelRef.current.offsetHeight));
    }
  }, [open, state.sessions]);

  // Grow the window upward to fit the panel (or shrink when closed). The pet
  // stays put because resize_window keeps the bottom edge fixed.
  useEffect(() => {
    if (!isTauri) return;
    // Capture the anchor only on the open transition; reuse it for panel-size
    // changes and for closing, so the pet returns to the exact same spot.
    const justOpened = open && !prevOpen.current;
    prevOpen.current = open;
    (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("resize_window", {
        width: open ? OPEN_W : CLOSED_W,
        height: open ? BASE_H + panelH : BASE_H,
        anchor: justOpened,
      });
    })();
  }, [open, panelH]);

  return (
    <div className="app">
      {open && (
        <div className="panel-wrap" ref={panelRef}>
          <StatusPanel sessions={state.sessions} />
        </div>
      )}
      <div className="pet-anchor">
        <Pet state={state} onClick={() => setOpen((v) => !v)} />
      </div>
    </div>
  );
}
