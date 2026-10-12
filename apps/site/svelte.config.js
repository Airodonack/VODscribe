import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  // As in the app (see its svelte.config.js), and the demo's components come from there.
  compilerOptions: { css: "injected" },
  kit: {
    // Every page is prerendered to plain HTML for Cloudflare.
    // 404.html is Cloudflare's not-found page: the site's shell, which shows SvelteKit's error page.
    adapter: adapter({ fallback: "404.html", strict: true }),
    alias: { $desktop: "../desktop/src/lib" },
    // SvelteKit hashes its own inline startup script into this policy. Headers
    // a <meta> policy can't set (frame-ancestors) are in static/_headers.
    csp: {
      mode: "hash",
      directives: {
        "default-src": ["self"],
        "script-src": ["self"],
        "style-src": ["self", "unsafe-inline"],
        "img-src": ["self", "data:", "blob:"],
        "connect-src": ["self"],
        "frame-src": ["https://www.youtube-nocookie.com", "https://player.twitch.tv"],
        "object-src": ["none"],
        "base-uri": ["none"],
        "form-action": ["none"],
      },
    },
  },
};

export default config;
