# The site's pages

Every page is prerendered, except the VOD pages, which are made in the browser.

- `+layout.svelte`: the header and footer every page shares.
- `+layout.ts`: prerenders every page, without trailing slashes.
- `+page.svelte`: the home page, with the demo, downloads and questions.
- [`vodscriptions`](vodscriptions): the page for streamers.
- [`v`](v): the VODs with VODscriptions, and a page for each.
- [`releases`](releases): every version's changes.
- [`support`](support): ways to get help.
- [`privacy`](privacy): the privacy page.
- [`licenses`](licenses): credits and licences of what VODscribe is built on.
- [`third-party-notices.txt`](third-party-notices.txt): the notices the installers include.
