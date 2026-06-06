#!/usr/bin/env bash
#
# Installs the Claude Pet hooks into ~/.claude/settings.json.
#
# Merges (does not clobber) the Notification / Stop / UserPromptSubmit /
# PreToolUse / PostToolUse hooks needed for live status. Safe to re-run.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOOK="$SCRIPT_DIR/claude-pet-hook.sh"
SETTINGS="$HOME/.claude/settings.json"

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
  --argjson prompt "$(entry)" \
  --argjson pre "$(entry '*')" \
  --argjson post "$(entry '*')" \
  '
  def add($evt; $e):
    .hooks[$evt] = ((.hooks[$evt] // [])
      | map(select((.. | .command? // "") | contains("claude-pet-hook.sh") | not))
      + [$e]);
  .hooks = (.hooks // {})
  | add("Notification"; $notif)
  | add("Stop"; $stop)
  | add("UserPromptSubmit"; $prompt)
  | add("PreToolUse"; $pre)
  | add("PostToolUse"; $post)
  ' "$SETTINGS" >"$tmp"

mv -f "$tmp" "$SETTINGS"

echo "✓ Installed Claude Pet hooks into $SETTINGS"
echo "  (backup saved to $backup)"
echo "  Restart any running Claude Code sessions to pick them up."
