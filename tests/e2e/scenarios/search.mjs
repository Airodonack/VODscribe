// Searching two VODs: results grouped by VOD; a result double-clicked shows
// its line in its VOD's transcript; "This VOD" (in the From filter) narrows
// to the open one; a phrase in neither says so, and offers to search every
// VOD.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

const SEARCH = 'input[placeholder="Search for a phrase…"]';

export default {
  name: "search",
  async run(app, ctx) {
    await app.toLibrary();
    const audio = await app.addAndTranscribe(ctx.fixtures.landed);
    const video = videos(path.join(ctx.out, "fixtures")).video;
    await app.pickFiles([video]);
    await app.clickText("add video files…");
    await until(
      "both transcribed",
      async () => {
        const vods = await app.invoke("list_vods");
        return vods.length === 2 && vods.every((v) => v.status === "done");
      },
      5 * MINUTE,
    );
    const results = () =>
      app.js(`return {
        sections: document.querySelectorAll(".results section").length,
        rows: document.querySelectorAll(".results button.row").length,
        summary: document.querySelector(".results .summary")?.textContent.trim() ?? "",
        none: document.querySelector(".none")?.textContent.trim() ?? "" }`);

    await app.type(SEARCH, "landed");
    const all = await until(
      "results from both",
      async () => {
        const r = await results();
        return r.rows === 2 && r;
      },
      MINUTE,
    );
    if (all.sections !== 2) throw new Error(`the results are in ${all.sections} groups, not one for each VOD`);
    ctx.log(`all VODs: ${all.summary}`);
    await ctx.checkpoint("search-grouped");

    // A result double-clicked plays it (its clicks) and opens its VOD's
    // transcript in the results' place, with its line in view. The fixtures'
    // transcripts fit in the view, so here they're given room either side,
    // for the line to be out of it unless scrolled to.
    await app.js(
      `const room = document.createElement("style");
       room.id = "e2e-room";
       room.textContent = "#transcript { padding-top: 3000px !important; padding-bottom: 3000px !important }";
       document.head.append(room);`,
    );
    await app.doubleClick(".results button.row");
    const shown = await until(
      "the line in the transcript, in view",
      () =>
        app.js(`
          if (document.querySelector(".results")) return false;
          const body = document.querySelector(".body").getBoundingClientRect();
          const line = [...document.querySelectorAll("#transcript .line > .row:not(.chapter-head)")]
            .find((b) => /landed/i.test(b.textContent));
          if (!line) return false;
          const box = line.getBoundingClientRect();
          return box.top >= body.top && box.bottom <= body.bottom && {
            query: document.querySelector('input[aria-label="Search"]').value,
            open: Number(document.querySelector(".list .vod.selected")?.dataset.key ?? 0),
          };`),
      MINUTE,
    );
    if (shown.query !== "") throw new Error(`the search box still says "${shown.query}"`);
    if (!shown.open) throw new Error("the result's VOD isn't open");
    await app.js(`document.getElementById("e2e-room").remove()`);
    ctx.log("a double-clicked result shows its line in the transcript");

    // The audio file opened, then the search narrowed to it.
    await app.type(SEARCH, "");
    await app.openVod(audio.id);
    await app.type(SEARCH, "landed");
    await until("results", async () => (await results()).rows > 0, MINUTE);
    await app.click("button.filter");
    const choose = (label) =>
      app.js(
        `const b = [...document.querySelectorAll('[role=dialog][aria-label="Search filter"] button')]
           .find((b) => b.querySelector(".label")?.textContent.trim() === arguments[0] || b.textContent.trim() === arguments[0]);
         if (!b) throw new Error("no " + arguments[0]);
         b.click();`,
        label,
      );
    await choose("This VOD");
    const one = await until(
      "results from this VOD only",
      async () => {
        const r = await results();
        return r.rows === 1 && r;
      },
      MINUTE,
    );
    ctx.log(`this VOD: ${one.summary}`);

    // Nothing found here: said, with a way to search every VOD.
    await app.key("Escape");
    await app.type(SEARCH, "zebra crossing");
    const none = await until("the empty result", async () => (await results()).none, MINUTE);
    if (!/No matches/.test(none)) throw new Error(`for no matches the page says "${none}"`);
    await ctx.checkpoint("search-none");
    await app.clickText("Search all VODs");
    await until(
      "the scope widened",
      () => app.js(`return /All VODs/.test(document.querySelector("button.filter")?.textContent ?? "")`),
      10_000,
    );
    ctx.log(`none: "${none.split("\n")[0]}"; then all VODs`);
  },
};
