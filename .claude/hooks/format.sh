#!/bin/sh
# Claude Code PostToolUse hook: formats each file Claude edits, the way
# .githooks/pre-commit and CI check it. Never blocks the edit.
f=$(jq -r '.tool_input.file_path // empty')
root=$(cd "$(dirname "$0")/../.." && pwd)
case "$f" in
  *.rs) rustfmt --edition 2024 --config-path "$root/rustfmt.toml" "$f" ;;
  "$root"/apps/desktop/src/*.svelte | "$root"/apps/desktop/src/*.ts | "$root"/apps/desktop/src/*.js | "$root"/apps/desktop/src/*.css)
    cd "$root/apps/desktop" && pnpm exec prettier --write --log-level warn "$f" ;;
esac >&2 || true
