#!/usr/bin/env bash
#
# Installs the Claude Pet hooks into ~/.claude/settings.json.
#
# Copies the hook script to a stable path (~/.claude/claude-pet/) and merges
# (does not clobber) the lifecycle hooks needed for live status:
#   SessionStart / UserPromptSubmit / PreToolUse / PostToolUse /
#   PermissionRequest / Notification / Stop / StopFailure / SessionEnd
# Safe to re-run; it replaces any previous claude-pet hook entries.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$SCRIPT_DIR/claude-pet-hook.sh"
SETTINGS="$HOME/.claude/settings.json"

# Install the hook into a STABLE location, independent of this repo. Pointing
# settings.json at the repo checkout means every Claude event breaks the moment
# the repo is moved or deleted — so copy it under ~/.claude and reference that.
INSTALL_DIR="$HOME/.claude/claude-pet"
HOOK="$INSTALL_DIR/claude-pet-hook.sh"
mkdir -p "$INSTALL_DIR"
cp "$SRC" "$HOOK"
chmod +x "$HOOK"

mkdir -p "$HOME/.claude"
[ -f "$SETTINGS" ] || echo '{}' >"$SETTINGS"

# One hook entry pointing at our script (optionally with a matcher).
entry() {
  local matcher="${1:-}"
  if [ -n "$matcher" ]; then
    jq -n --arg cmd "$HOOK" --arg m "$matcher" \
      '{matcher:$m, hooks:[{type:"command", command:$cmd}]}'
  else
    jq -n --arg cmd "$HOOK" \
      '{hooks:[{type:"command", command:$cmd}]}'
  fi
}

backup="$SETTINGS.claude-pet.bak"
cp "$SETTINGS" "$backup"

tmp="$(mktemp)"
jq \
  --argjson notif "$(entry)" \
  --argjson stop "$(entry)" \
  --argjson stopfail "$(entry)" \
  --argjson prompt "$(entry)" \
  --argjson pre "$(entry '*')" \
  --argjson post "$(entry '*')" \
  --argjson perm "$(entry)" \
  --argjson sstart "$(entry)" \
  --argjson send "$(entry)" \
  '
  # Drop any existing claude-pet entry for this event, then add exactly one.
  # NB: collect the nested commands into an array before `any` — `(.. | .command?)`
  # is a *stream*, and feeding a stream to `select` mis-filters and duplicates
  # entries (which silently piled up across re-runs).
  def add($evt; $e):
    .hooks[$evt] = ((.hooks[$evt] // [])
      | map(select([.. | .command? // ""] | any(contains("claude-pet-hook.sh")) | not))
      + [$e]);
  .hooks = (.hooks // {})
  | add("Notification"; $notif)
  | add("Stop"; $stop)
  | add("StopFailure"; $stopfail)
  | add("UserPromptSubmit"; $prompt)
  | add("PreToolUse"; $pre)
  | add("PostToolUse"; $post)
  | add("PermissionRequest"; $perm)
  | add("SessionStart"; $sstart)
  | add("SessionEnd"; $send)
  ' "$SETTINGS" >"$tmp"

mv -f "$tmp" "$SETTINGS"

echo "✓ Installed Claude Pet hooks into $SETTINGS"
echo "  (backup saved to $backup)"
echo "  Restart any running Claude Code sessions to pick them up."
