import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import process from "node:process";
import { colorMix, colorMixStyles } from "./colorMix.js";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [
    // color-mix() fallbacks for macOS 12's web view (colorMix.js).
    colorMix(),
    sveltekit({
      preprocess: [vitePreprocess(), colorMixStyles],
      // Ship each component's styles inside its JS. With separate CSS modules, the
      // dev server sometimes serves a component's CSS before compiling it and the
      // styles go missing (seen with vite-plugin-svelte 7.3 on Vite 8).
      compilerOptions: { css: "injected" },
      // Tauri has no server for SSR, so the app is a single page with index.html
      // as its fallback: https://v2.tauri.app/start/frontend/sveltekit/
      adapter: adapter({ fallback: "index.html" }),
    }),
  ],

  // The profiling build (scripts/build-profiling.sh) keeps the page's code
  // readable, so a profile's JavaScript frames have their own names.
  ...(process.env.VODSCRIBE_PROFILING === "1" && { build: { minify: false } }),

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
