# The site's components

- [`tauri`](tauri): stand-ins for the Tauri modules, so the app's page runs in a browser.
- `Demo.svelte`: the live demo, the app's own interface on two transcribed VODs.
- `VodView.svelte`: a VOD with its VODscription, in the app's view, for the VOD pages.
- `DownloadNote.svelte`: what to press when Windows or macOS warns about the download.
- `downloads.ts`: the installers' addresses on the download host.
- `changelog.ts`: reads `CHANGELOG.md` for the releases page.
- `links.ts`: the site's addresses, mail links and issue forms.
- `review.svelte.ts`, `ReviewControls.svelte`: reviewing a VOD page's chapters and highlights.
- `verdicts.ts`: the review's verdicts, kept and exported.
- `siteBar.svelte.ts`: what a VOD page puts in the site's bar.
- `*.test.mjs`: unit tests of the module of the same name, run with `node --test`.
