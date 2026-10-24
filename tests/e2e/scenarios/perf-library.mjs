// A large library, timed against its budgets (budgets.json, lib/budgets.mjs):
// search-large's 60 VODs of two hours' talk (about 108,000 lines,
// fixtures/large-library.mjs). Searches typed, each from the search sent
// to its results drawn: the box's 120 ms wait for more typing (App.svelte)
// left out, so what's timed is the search and the drawing. Then one of the
// VODs' transcripts (1,800 lines) scrolled by script, every gap between
// animation frames taken meanwhile: the 95th percentile and the worst, the
// budget on the first (one late frame in a few hundred is noise; many are
// a stutter). Each is taken several times and judged on the median; a miss
// fails with the numbers.

import { existsSync } from "node:fs";
import path from "node:path";

import { COMMON, RARE, grow } from "../fixtures/large-library.mjs";
import { MINUTE, until } from "../lib/app.mjs";
import { Measures, percentile } from "../lib/budgets.mjs";
import { dataDirs } from "../lib/data.mjs";

const SEARCH = 'input[placeholder="Search for a phrase…"]';
const VODS = 60;
/** How many times each search is timed. */
const TIMES = 5;
/** The scroll: this many frames, this many px a frame (about 2,400 px a second, a quick
 *  read down), from each of these places in the transcript (fractions of its height). */
const FRAMES = 180;
const STEP = 40;
const FROM = [0, 0.35, 0.7];

export default {
  name: "perf-library",
  async run(app, ctx) {
    const measures = new Measures(ctx, "perf-library");
    await app.toLibrary();
    await app.addAndTranscribe(ctx.fixtures.landed);
    await ctx.quit();
    const begun = Date.now();
    const library = dataDirs()
      .map((d) => path.join(d, "library.sqlite"))
      .find(existsSync);
    if (!library) throw new Error("no library.sqlite to grow");
    const { lines } = grow(library, { vods: VODS, hours: 2 });
    ctx.log(`the library grown by ${VODS} VODs, ${lines} lines, in ${((Date.now() - begun) / 1000).toFixed(1)} s`);
    app = await ctx.start();
    await app.toLibrary();
    await until("the large library listed", async () => (await app.invoke("list_vods")).length === VODS + 1, MINUTE);

    // Each search call as it's sent (the page's own, through fetch, as Tauri's invoke goes).
    await app.js(`
      const sent = (window.__perfSearches = []);
      const fetch = window.fetch;
      window.fetch = (url, init) => {
        if (decodeURIComponent(String(url)).split("/").pop() === "search") sent.push(performance.now());
        return fetch(url, init);
      };`);
    for (const [what, words, found] of [
      ["many lines", COMMON, true],
      ["one line a VOD", RARE, true],
      ["none", "zyzzyva", false],
    ]) {
      const step = `search sent to drawn, ${what}`;
      for (let i = 0; i < TIMES; i++) {
        const seen = await app.js(
          `const [selector, words] = arguments;
           const input = document.querySelector(selector);
           if (!input) throw new Error("no search box");
           // Out of the last search's results first, so they aren't taken for this one's.
           input.value = "";
           input.dispatchEvent(new Event("input", { bubbles: true }));
           await new Promise((r) => setTimeout(r, 500));
           const sent = window.__perfSearches;
           const before = sent.length;
           const shown = () => {
             const rows = document.querySelectorAll(".results button.row").length;
             const none = document.querySelector(".none")?.textContent.trim() ?? "";
             return rows || none ? { rows, none, summary: document.querySelector(".results .summary")?.textContent.trim() ?? "" } : null;
           };
           const typed = performance.now();
           input.value = words;
           input.dispatchEvent(new Event("input", { bubbles: true }));
           while (performance.now() - typed < 60000) {
             await new Promise((r) => requestAnimationFrame(r));
             const s = shown();
             if (s && sent.length > before) {
               const now = performance.now();
               return { ...s, ms: Math.round(now - sent[before]), typed: Math.round(now - typed), searches: sent.length - before };
             }
           }
           return { error: "nothing shown in a minute" };`,
          SEARCH,
          words,
        );
        if (seen.error) throw new Error(`searching "${words}": ${seen.error}`);
        if (found !== seen.rows > 0)
          throw new Error(`searching "${words}": ${seen.rows} rows (${seen.summary || seen.none})`);
        const to = ctx.clock();
        measures.add(step, seen.ms, to - seen.ms / 1000, to);
        ctx.log(
          `"${words}": ${seen.summary || seen.none} in ${seen.ms} ms from the search sent (${seen.typed} ms from typed)`,
        );
      }
    }

    // Out of the search, into one long VOD's transcript.
    await app.js(
      `const input = document.querySelector(arguments[0]);
       input.value = "";
       input.dispatchEvent(new Event("input", { bubbles: true }));`,
      SEARCH,
    );
    const vod = (await app.invoke("list_vods")).find((v) => v.title === "Long stream 1");
    if (!vod) throw new Error("no Long stream 1 in the library");
    await app.openVod(vod.id);
    await until(
      "its transcript",
      () => app.js(`return document.querySelectorAll(".transcript button.row").length > 20`),
      MINUTE,
    );
    for (const start of FROM) {
      const from = ctx.clock();
      const seen = await app.js(
        `const [start, frames, step] = arguments;
         const s = document.querySelector(".transcript").closest(".body");
         if (!s) throw new Error("no transcript scroller");
         const frame = () => new Promise((r) => requestAnimationFrame(r));
         s.scrollTop = Math.round(start * (s.scrollHeight - s.clientHeight));
         // Settled there first: its rows read and laid out.
         await new Promise((r) => setTimeout(r, 500));
         await frame();
         const top = s.scrollTop;
         const gaps = [];
         let last = await frame();
         for (let i = 0; i < frames; i++) {
           s.scrollTop += step;
           const now = await frame();
           gaps.push(now - last);
           last = now;
         }
         return { gaps, moved: s.scrollTop - top, height: s.scrollHeight, rows: document.querySelectorAll(".transcript button.row").length };`,
        start,
        FRAMES,
        STEP,
      );
      // Most of the way, or the scroll didn't happen (or hit the end: the transcript shorter than meant).
      if (seen.moved < FRAMES * STEP * 0.9)
        throw new Error(`the transcript scrolled ${seen.moved} px of ${FRAMES * STEP} (${seen.height} px tall)`);
      const p95 = Math.round(percentile(seen.gaps, 95));
      const worst = Math.round(Math.max(...seen.gaps));
      measures.add("transcript scroll, frame gap p95", p95, from, ctx.clock());
      measures.add("transcript scroll, worst frame gap", worst, from, ctx.clock());
      ctx.log(
        `scrolled ${seen.moved} px from ${Math.round(start * 100)}% of ${seen.height} px (${seen.rows} rows drawn): ` +
          `frame gaps median ${Math.round(percentile(seen.gaps, 50))} ms, p95 ${p95} ms, worst ${worst} ms`,
      );
    }
    measures.check();
  },
};
