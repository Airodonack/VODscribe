# Scripts

Builds, release packaging and checks, run by hand and by CI.

- `prepare-sidecars.sh`: builds the transcription workers into `apps/desktop/src-tauri/binaries/`, and fetches sherpa-onnx's libraries into `lib/`.
- `sherpa-onnx-prebuilt.sha256`: the pinned hashes of those libraries.
- `build-profiling.sh`: builds the app with symbols and frame pointers, for profiling.
- `reset-setup.sh`: removes the speech model and yt-dlp, so first-run setup shows again.
- `fetch-ffmpeg.py`: puts the ffmpeg the app ships in a folder, for CI's player tests.
- `third-party-notices.py`: writes `THIRD-PARTY-NOTICES.txt`; `--check` fails when it's out of date.
- `fetch-cargo-about.sh`: downloads the pinned cargo-about for the notices.
- `check-pinned-downloads.py`: checks every pinned download is still there, at its size.
- `check-nsis-template.py`: fails when Tauri's installer template moved on from our copy.
- `check-workflows.mjs`: checks every workflow file's shape.
- `export-demo-chat.py`: exports a VOD's chat for the website's demo, with made-up names.
- `ready-to-release.py`: checks a release branch is ready to tag, and prints the commands.
- `release.py`: names the build's files, writes `latest.json` and the winget manifests.
- `verify-release.py`: checks a published release as the app and the website see it.
- `test_release.py`: tests of the three release scripts.

## Releasing

Between releases, the root `Cargo.toml`'s version is the next one with `+dev`, such as `0.4.1+dev`, so a development build never passes for a release. It is build metadata, not `-dev`, because Linux packages can't have a hyphen in their version. Semver ranks it level with 0.4.1, so a development build isn't offered 0.4.1 by the updater.

1. Write each change's bullet in `CHANGELOG.md` as it lands, under the version's "(unreleased)" heading.
2. Cut `release/0.4.1` from `main` for the release round. Fixes land on it and are merged into `main` at once; `main` moves on meanwhile.
3. When it's clean, set the version without `+dev` on the branch, date the heading, commit "Version 0.4.1" with the entry as its body, and push the branch.
4. Run `python3 scripts/ready-to-release.py`, then `git tag -s v0.4.1 -m "Version 0.4.1"` and push the tag; its run builds every system, runs every end-to-end test and publishes.
5. If that run needs a fix, revert the version commit on the branch, add the fix and merge it into `main`, then make a fresh version commit. The unpublished tag is deleted, and the new version commit gets the same tag.
6. Once it's out, do what `ready-to-release.py` prints: move `site` to the tag, merge the release branch into `main`, set `main` to `0.4.2+dev` with an empty entry, and delete the release branches.

The publish signs the update bundles, uploads them to R2 under `releases/<version>/` and `releases/latest/`, writes `releases/latest.json` last, then runs `verify-release.py`. The app installs an update only if it's signed with the key whose public half is in `tauri.conf.json`; back up the private key, since installed copies can't be updated without it.

The winget job then submits the version to `microsoft/winget-pkgs` as `Airodonack.VODscribe`, once `WINGET_TOKEN` is set. Set the repository variable `WINGET_SUBMIT` to `false` while a version waits in winget's review, since a newer submission starts that review over.
