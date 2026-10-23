// Searching a large library, timed: 60 VODs of two hours' talk each (about
// 108,000 lines, fixtures/large-library.mjs, made from one VOD added and
// transcribed), searched for a word in thousands of lines, a phrase in one
// line a VOD, and a word in none. Each is timed from the words typed to
// the results shown (or the "nothing found" line), as the page draws them.

import path from "node:path";

import { COMMON, RARE, grow } from "../fixtures/large-library.mjs";
import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";

const SEARCH = 'input[placeholder="Search for a phrase…"]';
const VODS = 60;

export default {
  name: "search-large",
  async run(app, ctx) {
    await app.toLibrary();
    await app.addAndTranscribe(ctx.fixtures.landed);
    await ctx.quit();
    const begun = Date.now();
    const { lines } = grow(path.join(dataDirs().at(-1), "library.sqlite"), { vods: VODS, hours: 2 });
    ctx.log(`the library grown by ${VODS} VODs, ${lines} lines, in ${((Date.now() - begun) / 1000).toFixed(1)} s`);
    app = await ctx.start();
    await app.toLibrary();
    await until("the large library listed", async () => (await app.invoke("list_vods")).length === VODS + 1, MINUTE);

    for (const [what, words, found] of [
      ["many lines", COMMON, true],
      ["one line a VOD", RARE, true],
      ["none", "zyzzyva", false],
    ]) {
      // In the page: the words in, then each frame looked at until the results (or none) show.
      const seen = await app.js(
        `const [selector, words] = arguments;
         const input = document.querySelector(selector);
         if (!input) throw new Error("no search box");
         // Out of the last search's results first, so they aren't taken for this one's.
         input.value = "";
         input.dispatchEvent(new Event("input", { bubbles: true }));
         await new Promise((r) => setTimeout(r, 500));
         const shown = () => {
           const rows = document.querySelectorAll(".results button.row").length;
           const none = document.querySelector(".none")?.textContent.trim() ?? "";
           return rows || none ? { rows, none, summary: document.querySelector(".results .summary")?.textContent.trim() ?? "" } : null;
         };
         const start = performance.now();
         input.value = words;
         input.dispatchEvent(new Event("input", { bubbles: true }));
         while (performance.now() - start < 60000) {
           await new Promise((r) => requestAnimationFrame(r));
           const s = shown();
           if (s) return { ...s, ms: Math.round(performance.now() - start) };
         }
         return { error: "nothing shown in a minute" };`,
        SEARCH,
        words,
      );
      if (seen.error) throw new Error(`searching "${words}": ${seen.error}`);
      if (found !== seen.rows > 0)
        throw new Error(`searching "${words}": ${seen.rows} rows (${seen.summary || seen.none})`);
      // The profile's window for it ends here and is as long as the time
      // measured: the call began by clearing the last search, which isn't this one's.
      const to = ctx.clock();
      ctx.timing(`search, ${what}`, seen.ms, to - seen.ms / 1000, to);
      ctx.log(`"${words}": ${seen.summary || seen.none} in ${seen.ms} ms`);
    }
  },
};
