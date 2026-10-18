// Searching two VODs: results grouped by VOD; "This VOD" (in the From
// filter) narrows to the open one; a phrase in neither says so, and offers
// to search every VOD.

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
