// Searching two VODs: results grouped by VOD; a result double-clicked shows
// its line in its VOD's transcript, the words kept in the box, and the box
// focused shows the results again; "This VOD" (in the From filter) narrows
// to the open one; a phrase in neither says so, and offers to search every
// VOD. Then the search's language: a phrase across two lines, quoted words
// in any order, -word, OR with its grouping shown under the box, within:
// typed showing in the filter as set by the text, a query that can't be
// read saying so, and a filter's chip taking its text out of the box.
// (from: on chat is the core's tests': no VOD here has chat.)

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { landedTwice, videos } from "../lib/fixtures.mjs";

const SEARCH = 'input[placeholder="Search for a phrase…"]';

export default {
  name: "search",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "search-or":
      'Under the search box, which reads "zebra OR landed", the query is shown as two word chips, "zebra" and "landed", with OR between them; below, the matches are grouped under each video\'s name, the word "landed" highlighted in each.',
  },
  async run(app, ctx) {
    await app.toLibrary();
    const audio = await app.addAndTranscribe(ctx.fixtures.landed);
    const video = videos(path.join(ctx.out, "fixtures")).video;
    const [added] = await app.addFiles([video]);
    await app.transcribe(added.id);
    const results = () =>
      app.js(`return {
        sections: document.querySelectorAll(".results section").length,
        rows: document.querySelectorAll(".results button.row").length,
        summary: document.querySelector(".results .summary")?.textContent.trim() ?? "",
        none: document.querySelector(".none")?.textContent.trim() ?? "" }`);

    // An add opens what it added, and a search with a VOD open is of it
    // alone: closed, to search them all.
    await app.click(".list .vod.selected");
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
    // transcript in the results' place, with its line in view and the words
    // still in the box. The fixtures'
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
    if (shown.query !== "landed") throw new Error(`the search box says "${shown.query}", not "landed"`);
    if (!shown.open) throw new Error("the result's VOD isn't open");
    await app.js(`document.getElementById("e2e-room").remove()`);
    await app.focus(SEARCH);
    await until("the results again", async () => (await results()).rows === 2, 10_000);
    ctx.log("a double-clicked result shows its line in the transcript; the box focused, the results");

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

    // The landing said twice: two lines, the last word of the first and the
    // first of the second a phrase across them. Every VOD is searched still
    // (the lock), though the add opens this one.
    const [twice] = await app.addFiles([landedTwice(path.join(ctx.out, "fixtures"))]);
    await app.transcribe(twice.id);
    const lines = await app.invoke("transcript", { vodId: twice.id });
    if (lines.length < 2) throw new Error(`the landing said twice is ${lines.length} line(s), not two`);
    const words = (line) =>
      line.text
        .replace(/"/g, "")
        .split(/\s+/)
        .filter((w) => /\w/.test(w));
    const [first, second] = [words(lines[0]), words(lines[1])];
    const across = `"${first.at(-1)} ${second[0]}"`;
    await app.type(SEARCH, across);
    const crossed = await until(
      "a match across two lines",
      async () => {
        const r = await results();
        return r.rows === 1 && r;
      },
      MINUTE,
    );
    const [hit] = (await app.invoke("search", { query: across, filter: {} })).hits;
    if (hit?.lines !== 2) throw new Error(`${across} matches over ${hit?.lines} lines, not two`);
    const row = await app.js(`return document.querySelector(".results button.row .text").textContent`);
    for (const w of [first.at(-1), second[0]])
      if (!row.includes(w)) throw new Error(`the match across lines shows "${row}", without "${w}"`);
    await ctx.checkpoint("search-across-lines");
    ctx.log(`${across}: ${crossed.summary}`);

    // Quoted words in any order: two of the first line's, the other way round.
    const [a, b] = [first.at(-1), first.at(-2)];
    await app.type(SEARCH, `"${a} ${b}"`);
    await until("no match in that order", async () => (await results()).none, MINUTE);
    await app.type(SEARCH, `"${a}" "${b}"`);
    const any = await until(
      "matches in any order",
      async () => {
        const r = await results();
        return r.rows > 0 && r;
      },
      MINUTE,
    );
    ctx.log(`"${a}" "${b}": ${any.summary}`);

    // -word: a word of the audio file's one line leaves its match out.
    await app.type(SEARCH, "landed");
    const landed = await until(
      "every landing",
      async () => {
        const r = await results();
        return r.rows >= 4 && r;
      },
      MINUTE,
    );
    const [line] = await app.invoke("transcript", { vodId: audio.id });
    const minus = `landed -${words(line)[0]}`;
    await app.type(SEARCH, minus);
    const fewer = await until(
      "fewer with the minus",
      async () => {
        const r = await results();
        return r.rows < landed.rows && r;
      },
      MINUTE,
    );
    ctx.log(`${minus}: ${fewer.rows} of ${landed.rows}`);

    // OR, and how it was read under the box.
    const grouping = () =>
      app.js(`const g = document.querySelector(".grouping");
        return g && { text: g.textContent.replace(/\s+/g, " ").trim(), or: g.querySelectorAll(".or").length,
          chips: [...g.querySelectorAll(".chip")].map((c) => c.textContent.trim()) }`);
    await app.type(SEARCH, "zebra OR landed");
    const either = await until(
      "either side",
      async () => {
        const [r, g] = [await results(), await grouping()];
        return r.rows === landed.rows && g?.or === 1 && g;
      },
      MINUTE,
    );
    if (either.chips.join("|") !== "zebra|landed") throw new Error(`the chips are ${either.chips.join(", ")}`);
    await ctx.checkpoint("search-or");
    ctx.log(`zebra OR landed: "${either.text}"`);

    // within: typed: the filter shows it, set by the text, and its summary reads it.
    await app.type(SEARCH, "landed within:5");
    await until(
      "the window typed",
      async () => /within 5 s/.test(await app.js(`return document.querySelector("button.filter").textContent`)),
      MINUTE,
    );
    await app.click("button.filter");
    const control = await app.js(`
      const panel = document.querySelector('[role=dialog][aria-label="Search filter"]');
      const input = panel.querySelector('input[aria-label="Within"]');
      const group = panel.querySelector('[role=group][aria-label="Within"]');
      return { value: input.value, disabled: input.disabled, note: group.textContent };`);
    if (control.value !== "5" || !control.disabled || !/Set by the search text/.test(control.note))
      throw new Error(`the filter's Within shows ${JSON.stringify(control)}`);
    await ctx.checkpoint("search-within-typed");
    await app.key("Escape");
    ctx.log("within:5 typed: the filter shows 5 s, set by the search text");

    // A query that can't be read says what to fix, with no results.
    await app.type(SEARCH, '"unclosed');
    const error = await until(
      "the error",
      () => app.js(`return document.querySelector(".grouping.error-text")?.textContent.trim()`),
      MINUTE,
    );
    if (!/quote/i.test(error)) throw new Error(`for an open quote the page says "${error}"`);
    if ((await results()).rows !== 0) throw new Error("an open quote has results");
    await ctx.checkpoint("search-error");
    ctx.log(`"unclosed: ${error}`);

    // A filter's ×: its text out of the box, the box keeping the focus.
    await app.type(SEARCH, "landed in:chat");
    await app.focus(SEARCH);
    await until("the in:chat chip", async () => (await grouping())?.chips.includes("in:chat"), MINUTE);
    await app.click(".grouping .chip.filter .remove");
    const after = await until(
      "in:chat out of the box",
      () =>
        app.js(
          `const input = document.querySelector(arguments[0]);
          return input.value === "landed" && { focused: document.activeElement === input };`,
          SEARCH,
        ),
      10_000,
    );
    if (!after.focused) throw new Error("the chip's × took the focus from the box");
    await until("every landing again", async () => (await results()).rows === landed.rows, MINUTE);
    ctx.log("in:chat's × took it out of the box");
  },
};
