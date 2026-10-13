# Changelog

What changed in each release, for people who use VODscribe. The entry for the version being released is also the release notes in `latest.json` (`scripts/release.py` reads it), so write it for them, not for developers. The newest release is first; a version still being worked on is marked "(unreleased)".

## 0.1.4 (unreleased)

- Downloaded installers carry the version in their file name, like VODscribe-0.1.4-windows-x64-setup.exe.

## 0.1.3 — 2026-10-13

- Setup and the GPU add-on downloads have a Cancel button. Tools and model files that finished stay; the GPU add-on's own pack starts over.
- The GPU add-on is smaller: it now contains only ONNX Runtime and NVIDIA's libraries, since the speech-recognition library ships with the app. It downloads again once after updating.
- Settings has a Help section with where to report problems, and the website answers the same question.
- Under the hood: a weekly check that every file setup downloads is still available where it's expected.
- A YouTube download that the site refused is tried again from a fresh lookup before it's reported, since YouTube sometimes refuses the first.
- VODscribe's code is public, under the Functional Source License (source-available; Apache 2.0 two years after each release).

## 0.1.2 — 2026-10-12

- Downloads with several files show which one is under way, like "(3 of 5)".
- Settings no longer scrolls sideways.
- Download sizes are exact.
- Under the hood: SvelteKit 3 and more tests.

## 0.1.1 — 2026-10-12

- Fixes YouTube and Twitch playback in the installed app.
- Linux downloads are .deb and .rpm only.
- Opening VODscribe twice brings up the open window.
- Downloads work on Windows for users with non-ASCII names.
- Several smaller fixes.

## 0.1.0 — 2026-10-12

First release.
