import { STATUS_HEX, STATUS_LABEL, type SessionState } from "../types";
import { isTauri } from "../hooks/useClaudeState";

/** The card stack shown above the pet — one card per active task. */
export function StatusPanel({ sessions }: { sessions: SessionState[] }) {
  return (
    <div className="panel" onPointerDown={(e) => e.stopPropagation()}>
      <div className="panel-head">
        <span className="panel-title">Running tasks</span>
        <span className="panel-count">
          {sessions.length} task{sessions.length === 1 ? "" : "s"}
        </span>
      </div>

      <div className="cards">
        {sessions.length === 0 ? (
          <div className="empty">No active tasks</div>
        ) : (
          sessions.map((s) => <TaskCard key={s.sessionId} s={s} />)
        )}
      </div>
    </div>
  );
}

/** Running shows when it started; other states show when they were entered. */
function timeLabel(s: SessionState): string {
  if (!s.updatedAt) return "";
  return s.status === "running" ? `Started ${s.updatedAt}` : s.updatedAt;
}

function TaskCard({ s }: { s: SessionState }) {
  const color = STATUS_HEX[s.status];

  const open = async () => {
    if (!isTauri) return;
    const { invoke } = await import("@tauri-apps/api/core");
    // Acknowledge first (lets the pet leave COMPLETED), then surface the host.
    await invoke("acknowledge_session", { sessionId: s.sessionId });
    await invoke("open_session", { sessionId: s.sessionId, cwd: s.cwd });
  };

  return (
    <button
      className={`card status-${s.status}`}
      style={{ ["--accent" as string]: color }}
      onClick={open}
      title={`${s.project} — open this task`}
    >
      <span className="card-bar" />
      <span className="card-body">
        <span className="card-top">
          <span className="card-project">{s.project || "—"}</span>
          <span className="card-badge" style={{ background: color }}>
            {STATUS_LABEL[s.status]}
          </span>
        </span>
        <span className="card-task">{s.taskName || "—"}</span>
        <span className="card-time">{timeLabel(s)}</span>
      </span>
    </button>
  );
}
