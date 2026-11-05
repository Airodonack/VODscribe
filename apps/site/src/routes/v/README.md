# /v

The VODs with VODscriptions. Cloudflare answers every address under `/v/` with this page, whose router shows the VOD.

- `+page.svelte`: the list of VODs on the download host, and a way to open a file from the computer.
- `+page.ts`: prerenders the page as an empty shell.
- [`[platform]/[id]`](%5Bplatform%5D/%5Bid%5D): a VOD's page.
