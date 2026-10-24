// A VOD's page is made in the browser from files fetched as it opens, so
// there's no page to prerender for it: Cloudflare answers its address with
// 404.html (the site's shell: the adapter's fallback, vite.config.ts), whose router then
// shows this route.
export const prerender = false;
export const ssr = false;
