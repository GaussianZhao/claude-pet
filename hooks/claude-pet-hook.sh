#!/usr/bin/env bash
#
# Claude Pet status hook.
#
# Claude Code invokes this on lifecycle events with a JSON object on stdin
# (session_id, cwd, hook_event_name, message, ...). We write one file PER
# SESSION to ~/.claude/claude-pet/sessions/<session_id>.json so the pet can
# show every concurrent task as its own card. This is the push channel that
# makes WAITING (Notification) and COMPLETED (Stop) instant.
#
# Hooks must never block Claude, so this always exits 0.

set -uo pipefail

DIR="$HOME/.claude/claude-pet/sessions"
mkdir -p "$DIR"

input="$(cat 2>/dev/null || true)"

read_field() {
  printf '%s' "$input" | jq -r "$1 // empty" 2>/dev/null || true
}

event="$(read_field '.hook_event_name')"
[ -z "$event" ] && event="${1:-}"
session="$(read_field '.session_id')"
cwd="$(read_field '.cwd')"
message="$(read_field '.message')"
project=""
[ -n "$cwd" ] && project="$(basename "$cwd")"
ts="$(date +%s)"

# A session id is required to key the per-session file.
[ -z "$session" ] && exit 0

OUT="$DIR/$session.json"
tmp="$(mktemp "${DIR}/.status.XXXXXX")"
jq -n \
  --arg event "$event" \
  --arg session "$session" \
  --arg project "$project" \
  --arg cwd "$cwd" \
  --arg message "$message" \
  --argjson ts "$ts" \
  '{event:$event, sessionId:$session, project:$project, cwd:$cwd, message:$message, ts:$ts}' \
  >"$tmp" 2>/dev/null && mv -f "$tmp" "$OUT" || rm -f "$tmp"

# --- DEBUG PROBE: log every event (compact) so we can see the real sequence
# around a permission prompt. Bounded to the last 400 lines.
LOG="$HOME/.claude/claude-pet/events.log"
tool="$(read_field '.tool_name')"
printf '%s\n' "$(date '+%H:%M:%S') ${event} tool=${tool:-} sess=$(printf '%s' "$session" | cut -c1-8) msg=${message}" >>"$LOG" 2>/dev/null || true
tail -n 400 "$LOG" 2>/dev/null >"${LOG}.tmp" && mv -f "${LOG}.tmp" "$LOG" 2>/dev/null || true

# Prune session status files older than a day so the dir doesn't grow forever.
find "$DIR" -name '*.json' -type f -mtime +1 -delete 2>/dev/null || true

exit 0
