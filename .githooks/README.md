# Git hooks

Checks run before a commit. Turn them on once per clone with `git config core.hooksPath .githooks`.

- `pre-commit`: refuses unformatted Rust or page code, a workflow file that doesn't parse, and a staged `CHANGELOG.md` that fails `scripts/changelog-check.py`.
- `commit-msg`: refuses a commit message containing names kept out of the history.
