# The website

vodscribe.com: the home page with its downloads, the VODscriptions page, VOD pages, the releases, support, privacy and licenses pages. Its live demo is the desktop app's own page, imported as the `desktop` package, with stand-ins for the shell that answer from transcribed VODs.

- [`src`](src): the pages and the stand-ins.
- `static/`: files served as they are: the demo's VODs in `demo/`, the screenshot and social images, favicons, `robots.txt`, `sitemap.xml`, and Cloudflare's `_headers` and `_redirects`.
- [`scripts`](scripts): exports the demo's VODs.
- `vite.config.ts`: swaps the Tauri modules for the stand-ins, and sets the content security policy and the app's version.
- `wrangler.jsonc`: Cloudflare Workers' settings: serve `build/`, with `404.html` for missing pages.
- `package.json`, `tsconfig.json`, `.prettierrc`, `.nvmrc`: the `site` package and its settings.

## Building

`pnpm site` serves it at localhost:5173. `pnpm --filter site build` prerenders every page into `build/`. Controls that only work in the app grey out in the demo; mark new ones with `demoOff` from `apps/desktop/src/lib/demo.ts`.

## The demo's VODs

Transcribe them with the CLI into a fresh library, then export:

```sh
export VODSCRIBE_DATA=/tmp/demo
vodscribe-cli add <url>          # for each VOD, after vodscribe-cli setup
python3 apps/site/scripts/export-demo.py /tmp/demo/library.sqlite
```

`scripts/export-demo-chat.py` exports a VOD's chat, with everyone in it given a made-up name.

## Deploying

Cloudflare builds the site from this repository with `pnpm install --frozen-lockfile && pnpm --filter site build`, serving `apps/site/build`. The production site builds from the `site` branch, which each release moves to its tag, so the demo never shows unreleased code. A change to the site alone between releases goes on `site` too; every other branch gets a preview.

The download buttons link to `dl.vodscribe.com/releases/latest/`, so each release is uploaded there under stable names.
