import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));
// The app's version, from the Cargo workspace, for the download button and the demo.
const version = readFileSync(here("../../Cargo.toml"), "utf8").match(/^version = "(.+)"/m)?.[1];

export default defineConfig({
  plugins: [sveltekit()],
  define: {
    // Turns on the demo behavior in the app's components (apps/desktop/src/lib/demo.ts).
    __DEMO__: true,
    __APP_VERSION__: JSON.stringify(version),
  },
  resolve: {
    // The app's components call Tauri; in the demo these stand-ins answer from static/demo.
    alias: {
      "@tauri-apps/api/core": here("src/lib/tauri/core.ts"),
      "@tauri-apps/api/event": here("src/lib/tauri/event.ts"),
      "@tauri-apps/api/app": here("src/lib/tauri/app.ts"),
      "@tauri-apps/api/webview": here("src/lib/tauri/webview.ts"),
      "@tauri-apps/plugin-opener": here("src/lib/tauri/opener.ts"),
      "@tauri-apps/plugin-clipboard-manager": here("src/lib/tauri/clipboard.ts"),
      "@tauri-apps/plugin-dialog": here("src/lib/tauri/dialog.ts"),
    },
    // The app's components import Svelte from apps/desktop; there must be one copy.
    dedupe: ["svelte"],
  },
  server: { fs: { allow: [here("../desktop")] } },
});
