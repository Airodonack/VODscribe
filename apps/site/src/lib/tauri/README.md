# Stand-ins for Tauri

`vite.config.ts` puts these in place of the Tauri modules the app's page imports. Commands the demo can answer go to the backend; the rest only work in the app.

- `core.ts`: `@tauri-apps/api/core`, sending commands to the backend.
- `backend.ts`: the app's commands, answered from `static/demo` or a VOD page's annotations.
- `search.ts`: a port of the app's phrase search.
- `query.ts`: a port of the app's query parser; keep it in step with `crates/core/src/query.rs`.
- `annotations.ts`: the VOD a VOD page has open, for the backend to answer from.
- `annotated.ts`: a VOD's annotations turned into the demo's shapes.
- `event.ts`: `@tauri-apps/api/event`; on a VOD page it says when the chat arrives.
- `app.ts`, `window.ts`, `webview.ts`: the app, window and web view APIs.
- `clipboard.ts`, `dialog.ts`, `opener.ts`, `process.ts`, `updater.ts`: the plugins; links open in a new tab.
- `*.test.mjs`: the ports checked against the Rust's own cases.
- [`fixtures`](fixtures): a recorded VODscription for the tests.
