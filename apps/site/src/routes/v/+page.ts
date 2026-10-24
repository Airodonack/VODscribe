// The VOD index, prerendered as an empty shell (no server rendering): it
// lists what the download host has as it opens, and it's also the page
// Cloudflare serves for every VOD's address (static/_redirects: /v/* is
// answered with this page, status 200, so a shared link isn't a 404). A
// shell without server-rendered content starts its router at the address in
// the bar, which shows that VOD's route.
export const prerender = true;
export const ssr = false;
