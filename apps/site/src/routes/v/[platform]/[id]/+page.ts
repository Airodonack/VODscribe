// A VOD's page is made in the browser from files fetched as it opens, so
// there's no page to prerender for it: Cloudflare serves /v's page for its
// address (static/_redirects), an empty shell whose router then shows this route.
export const prerender = false;
export const ssr = false;
