import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Pet } from "./components/Pet";
import { StatusPanel } from "./components/StatusPanel";
import { UsageCard } from "./components/UsageCard";
import { useClaudeState, isTauri } from "./hooks/useClaudeState";
import "./App.css";

// Window geometry (logical px). Closed: tight around the pet (small click
// footprint). Open: wide enough for the cards. Keep CLOSED_W/BASE_H in sync with
// tauri.conf.json and the pet canvas size / `.pet-anchor` in App.css.
const BASE_H = 150;
const CLOSED_W = 150;
// Width of the side panel (running tasks + plan usage, stacked). Keep in sync
// with `.panel` / `.usage` in App.css.
const OPEN_W = 240;

// Where the pet and the panel sit inside the window (logical px). The backend
// computes this so the pet stays put on screen while the panel shifts to stay
// fully on-monitor near an edge/corner. `panelSide` is the chosen side.
type Layout = {
  petLeft: number;
  petBottom: number;
  panelLeft: number;
  panelTop: number;
  panelSide: "left" | "right";
};
const CLOSED_LAYOUT: Layout = {
  petLeft: 0,
  petBottom: 0,
  panelLeft: 0,
  panelTop: 0,
  panelSide: "right",
};

export default function App() {
  const state = useClaudeState();
  const [open, setOpen] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const [panelH, setPanelH] = useState(0);
  const [layout, setLayout] = useState<Layout>(CLOSED_LAYOUT);
  const prevOpen = useRef(false);

  const usage = state.usage ?? null;
  const showUsage = !!(usage && (usage.fiveHour || usage.sevenDay));

  // Refresh plan usage each time the panel opens, so the bars are current.
  useEffect(() => {
    if (!isTauri || !open) return;
    (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      invoke("refresh_usage").catch(() => {});
    })();
  }, [open]);

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
  }, [open, showUsage, state.sessions, usage]);

  // Resize/position the window to fit the card (or shrink when closed). The pet
  // keeps its screen position; the backend returns where to place the pet and
  // card inside the (possibly shifted) window.
  useEffect(() => {
    if (!isTauri) return;
    // Capture the anchor only on the open transition; reuse it for panel-size
    // changes and for closing, so the pet returns to the exact same spot.
    const justOpened = open && !prevOpen.current;
    prevOpen.current = open;
    (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      const next = await invoke<Layout>("resize_window", {
        open,
        panelH,
        closedW: CLOSED_W,
        openW: OPEN_W,
        baseH: BASE_H,
        anchor: justOpened,
      });
      setLayout(open ? next : CLOSED_LAYOUT);
    })();
  }, [open, panelH]);

  return (
    <div className="app">
      {open && (
        <div
          className={`panel-wrap panel-${layout.panelSide}`}
          ref={panelRef}
          style={{ left: layout.panelLeft, top: layout.panelTop, width: OPEN_W }}
        >
          <StatusPanel sessions={state.sessions} />
          {showUsage && usage && <UsageCard usage={usage} />}
        </div>
      )}
      <div
        className="pet-anchor"
        style={{ left: layout.petLeft, bottom: layout.petBottom }}
      >
        <Pet state={state} onClick={() => setOpen((v) => !v)} />
      </div>
    </div>
  );
}
