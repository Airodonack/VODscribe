// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  // Ship each component's styles inside its JS. With separate CSS modules, the
  // dev server sometimes serves a component's CSS before compiling it and the
  // styles go missing (seen with vite-plugin-svelte 7.3 on Vite 8).
  compilerOptions: { css: "injected" },
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
