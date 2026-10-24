import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";
import { colorMix, colorMixStyles } from "../desktop/colorMix.js";

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));
// The app's version, from the Cargo workspace, for the download button and the demo.
const version = readFileSync(here("../../Cargo.toml"), "utf8").match(/^version = "(.+)"/m)?.[1];

export default defineConfig({
  plugins: [
    // As in the app: color-mix() fallbacks for older Safari (apps/desktop/colorMix.js).
    colorMix(),
    sveltekit({
      preprocess: [vitePreprocess(), colorMixStyles],
      // As in the app (see its vite.config.js), and the demo's components come from there.
      compilerOptions: { css: "injected" },

      // Every page is prerendered to plain HTML for Cloudflare.
      // 404.html is Cloudflare's not-found page: the site's shell, which shows SvelteKit's error page.
      adapter: adapter({ fallback: "404.html", strict: true }),
      // SvelteKit hashes its own inline startup script into this policy. Headers
      // a <meta> policy can't set (frame-ancestors) are in static/_headers.
      csp: {
        mode: "hash",
        directives: {
          "default-src": ["self"],
          "script-src": ["self"],
          "style-src": ["self", "unsafe-inline"],
          "img-src": ["self", "data:", "blob:"],
          // The download host: a VOD page's annotations (routes/v).
          "connect-src": ["self", "https://dl.vodscribe.com"],
          // The platforms' players, driven over postMessage (the app's embed.ts):
          // no script of theirs runs in the page.
          "frame-src": ["https://www.youtube-nocookie.com", "https://player.twitch.tv"],
          "object-src": ["none"],
          "base-uri": ["none"],
          "form-action": ["none"],
        },
      },
    }),
  ],
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
      "@tauri-apps/api/window": here("src/lib/tauri/window.ts"),
      "@tauri-apps/plugin-opener": here("src/lib/tauri/opener.ts"),
      "@tauri-apps/plugin-clipboard-manager": here("src/lib/tauri/clipboard.ts"),
      "@tauri-apps/plugin-dialog": here("src/lib/tauri/dialog.ts"),
      "@tauri-apps/plugin-updater": here("src/lib/tauri/updater.ts"),
      "@tauri-apps/plugin-process": here("src/lib/tauri/process.ts"),
    },
    // The app's components import Svelte from apps/desktop; there must be one copy.
    dedupe: ["svelte"],
  },
  // The releases page reads the changelog at the repository's root.
  server: { fs: { allow: [here("../desktop"), here("../../CHANGELOG.md")] } },
});
