/** Mirrors the Rust types (serde camelCase). */
export type TaskStatus =
  | "idle"
  | "running"
  | "waiting"
  | "completed"
  | "error";

export interface SessionState {
  sessionId: string;
  project: string;
  taskName: string;
  status: TaskStatus;
  cwd: string;
  updatedAt: string;
}

/** One plan-usage window. `usedPercent` is 0–100; `resetsAt` is unix seconds. */
export interface UsageWindow {
  usedPercent: number;
  resetsAt: number;
}

export interface Usage {
  fiveHour: UsageWindow | null;
  sevenDay: UsageWindow | null;
  status: string;
}

export interface PetState {
  running: boolean;
  status: TaskStatus;
  sessions: SessionState[];
  usage?: Usage | null;
}

export const EMPTY_PET: PetState = {
  running: false,
  status: "idle",
  sessions: [],
  usage: null,
};

/** Per-status accent color used across the pet + cards. */
export const STATUS_COLOR: Record<TaskStatus, number> = {
  idle: 0x8b5cf6, // claude purple
  running: 0x22c55e, // green
  waiting: 0xf59e0b, // amber
  completed: 0xec4899, // pink
  error: 0xef4444, // red
};

export const STATUS_HEX: Record<TaskStatus, string> = {
  idle: "#8b5cf6",
  running: "#22c55e",
  waiting: "#f59e0b",
  completed: "#ec4899",
  error: "#ef4444",
};

export const STATUS_LABEL: Record<TaskStatus, string> = {
  idle: "Idle",
  running: "Running",
  waiting: "Waiting",
  completed: "Completed",
  error: "Error",
};
