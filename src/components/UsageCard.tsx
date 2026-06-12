import type { Usage, UsageWindow } from "../types";

/** The plan-usage flyout — 5-hour + weekly limit bars, like Claude's /usage. */
export function UsageCard({ usage }: { usage: Usage }) {
  const rows: Array<{ label: string; w: UsageWindow | null }> = [
    { label: "5-hour limit", w: usage.fiveHour },
    { label: "Weekly · all models", w: usage.sevenDay },
  ];
  return (
    <div className="usage" onPointerDown={(e) => e.stopPropagation()}>
      <div className="usage-title">Plan usage</div>
      {rows.map(({ label, w }) => (
        <UsageRow key={label} label={label} w={w} />
      ))}
    </div>
  );
}

function UsageRow({ label, w }: { label: string; w: UsageWindow | null }) {
  const pct = w ? Math.min(100, Math.max(0, w.usedPercent)) : null;
  return (
    <div className="usage-row">
      <div className="usage-line">
        <span className="usage-label">{label}</span>
        <span className="usage-meta">
          {pct === null ? "—" : `${pct}%`}
          {w && w.resetsAt > 0 ? ` · resets ${resetsIn(w.resetsAt)}` : ""}
        </span>
      </div>
      <div className="usage-track">
        <div
          className="usage-fill"
          style={{ width: `${pct ?? 0}%`, background: barColor(pct ?? 0) }}
        />
      </div>
    </div>
  );
}

/** Amber past 80%, red past 95% — otherwise the Claude blue/purple. */
function barColor(pct: number): string {
  if (pct >= 95) return "#ef4444";
  if (pct >= 80) return "#f59e0b";
  return "#6366f1";
}

/** Compact "resets in" label: 1d / 3h / 12m, like the /usage panel. */
function resetsIn(unixSec: number): string {
  const s = Math.max(0, unixSec - Date.now() / 1000);
  if (s >= 86400) return `${Math.round(s / 86400)}d`;
  if (s >= 3600) return `${Math.round(s / 3600)}h`;
  if (s >= 60) return `${Math.round(s / 60)}m`;
  return "<1m";
}
