#!/bin/sh
# Claude Code PostToolUse hook: formats each file Claude edits, the way
# .githooks/pre-commit and CI check it. Never blocks the edit.
f=$(jq -r '.tool_input.file_path // empty')
root=$(cd "$(dirname "$0")/../.." && pwd)
case "$f" in
  *.rs) rustfmt --edition 2024 --config-path "$root/rustfmt.toml" "$f" ;;
  "$root"/apps/*/src/*.svelte | "$root"/apps/*/src/*.ts | "$root"/apps/*/src/*.js | "$root"/apps/*/src/*.css)
    # Prettier runs from the file's app (apps/<name>), which has its config.
    app="${f#"$root"/apps/}"
    cd "$root/apps/${app%%/*}" && pnpm exec prettier --write --log-level warn "$f" ;;
esac >&2 || true
