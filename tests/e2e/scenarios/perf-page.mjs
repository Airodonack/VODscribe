// The page's own drawing, timed against its budgets (budgets.json,
// lib/budgets.mjs): Settings opened from its gear, to the frame it's drawn
// in; and a VOD with twenty clips opened, to the frame its clip bar has all
// twenty pills. Each is taken several times (Settings closed with Escape,
// another VOD opened, between) and judged on the median; a miss fails with the
// numbers.

import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { Measures } from "../lib/budgets.mjs";

/** How many times each is measured. */
const TIMES = 5;
/** Twenty stretches of the landing (5.4 s), each a clip: a pill on its clip bar. */
const RANGES = [
  [0, 1],
  [0, 2],
  [0, 3],
  [0, 4],
  [0, 5],
  [1, 2],
  [1, 3],
  [1, 4],
  [1, 5],
  [2, 3],
  [2, 4],
  [2, 5],
  [3, 4],
  [3, 5],
  [4, 5],
  [0.5, 1.5],
  [1.5, 2.5],
  [2.5, 3.5],
  [3.5, 4.5],
  [0.5, 2.5],
];

/** In the page: does `act` (a function body), then waits a frame at a time
 *  until `drawn` (a function body) is true; the ms from act to that frame. */
const timeToDrawn = (app, act, drawn, ...args) =>
  app.js(
    `const act = () => { ${act} };
     const drawn = () => { ${drawn} };
     const start = performance.now();
     act();
     while (performance.now() - start < 10000) {
       await new Promise((r) => requestAnimationFrame(r));
       if (drawn()) return Math.round(performance.now() - start);
     }
     return null;`,
    ...args,
  );

export default {
  name: "perf-page",
  async run(app, ctx) {
    const measures = new Measures(ctx, "perf-page");
    await app.toLibrary();

    const dialog = '[role="dialog"][aria-label="Settings"]';
    for (let i = 0; i < TIMES; i++) {
      const from = ctx.clock();
      const ms = await timeToDrawn(
        app,
        `document.querySelector('button[aria-label="Settings"]').click();`,
        `return !!document.querySelector(arguments[0] + " nav button")`,
        dialog,
      );
      if (ms === null) throw new Error("Settings didn't open in 10 s");
      measures.add("Settings open", ms, from, ctx.clock());
      await app.key("Escape");
      await until("Settings closed", () => app.js(`return !document.querySelector(arguments[0])`, dialog), 10_000);
      await sleep(200);
    }

    // A second VOD, without clips, to switch to and back: a copy of the landing under a name of its own.
    const dir = path.join(ctx.out, "fixtures");
    mkdirSync(dir, { recursive: true });
    const plain = path.join(dir, "landed-perf.mp3");
    copyFileSync(ctx.fixtures.landed, plain);
    const [other] = await app.addFiles([plain]);
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    await app.invoke("add_clips", { vodId: vod.id, ranges: RANGES });
    await until(
      "twenty pills, made",
      () => app.js(`return document.querySelectorAll(".clip-bar .chip.ready").length === 20`),
      MINUTE,
    );
    const row = `.vod[data-key="${vod.id}"]`;
    for (let i = 0; i < TIMES; i++) {
      // The other VOD opened (no pills), then this one again with all of its.
      await app.click(`.vod[data-key="${other.id}"]`);
      await until("the other VOD", () => app.js(`return !document.querySelector(".clip-bar .chip")`), 10_000);
      await sleep(300);
      const from = ctx.clock();
      const ms = await timeToDrawn(
        app,
        `document.querySelector(arguments[0]).click();`,
        `return document.querySelectorAll(".clip-bar .chip").length === 20`,
        row,
      );
      if (ms === null) throw new Error("the clip bar didn't show its twenty pills in 10 s");
      measures.add("clip bar, 20 pills drawn", ms, from, ctx.clock());
    }
    measures.check();
  },
};
