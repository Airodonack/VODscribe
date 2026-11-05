# Workflows

GitHub Actions for this repository.

- `check.yml`: Checks, on every push and pull request: the workflows, formatting, type checks, the website build, the third-party notices, and the core's and the player's lints and tests.
- `build.yml`: Build installers, in a fast or a full lane, then the end-to-end tests; a tag's run also publishes the release.
- `e2e.yml`: the end-to-end tests of a build's installers in shards, also of an earlier run's without building again.
- `notices.yml`: regenerates the third-party notices on a Dependabot pull request and pushes them to its branch.
- `pinned-downloads.yml`: checks weekly that every pinned download is still there at its size, with `scripts/check-pinned-downloads.py`.

| Event                                                 | Lane | Runs                                                                                                                               |
| ----------------------------------------------------- | ---- | ---------------------------------------------------------------------------------------------------------------------------------- |
| a push to `main` that touches the app, a pull request | fast | the Linux build and the Linux end-to-end tests in 2 shards. A newer push to the same ref cancels the run in progress.              |
| every night at 09:00 UTC, a manual run, a `v*` tag    | full | all four builds, Linux, Windows, macOS on Apple silicon and Intel, and every system's end-to-end tests. Never cancelled by a push. |

A last job merges the shards' results into one summary, and fails on a failed scenario or a shard that never reported.

```sh
gh workflow run build.yml                        # the full lane by hand; -f lane=fast for the fast one
gh workflow run e2e.yml -f build_run=<run id>    # the tests against an earlier run's installers
gh run watch
```

`build.yml`'s header lists every secret and variable the publish needs.
