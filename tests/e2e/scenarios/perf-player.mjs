// The player, timed against its budgets (budgets.json, lib/budgets.mjs):
// from opening a video (and Play) to the app's own player showing its first
// frame, and from a click on the timeline to the time there shown on the bar
// and to its frame drawn. Each is taken several times (five videos opened in
// turn, the last one paused, and five seeks) and judged on the median; a
// miss fails with the numbers.
// The video is colours.mp4 (twelve seconds, one colour a second), as in
// player-picture, whose single timings these repeat.

import { copyFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { Measures } from "../lib/budgets.mjs";
import { colours } from "../lib/fixtures.mjs";

/** How many times each is measured. */
const TIMES = 5;
/** Where the seeks go (s): half a second into a second each, so the frame there is
 *  exactly the time asked (30 fps), and the whole second the bar shows changes. */
const SEEKS = [7.5, 2.5, 10.5, 4.5, 8.5];

export default {
  name: "perf-player",
  async run(app, ctx) {
    if (!(await app.invoke("player_available"))) ctx.skip("no screen for the app's own player here");
    const measures = new Measures(ctx, "perf-player");
    const dir = path.join(ctx.out, "fixtures");
    const original = colours(dir);
    // Copies under names of their own: each a VOD of its own, opened fresh.
    const files = Array.from({ length: TIMES }, (_, i) => {
      const file = path.join(dir, `colours-perf-${i + 1}.mp4`);
      copyFileSync(original, file);
      return file;
    });
    await app.toLibrary();
    const vods = await app.addFiles(files);
    // Each opened in turn while the last one stands paused: its player then
    // sends no frames that could be taken for the next one's.
    for (const vod of vods) {
      await sleep(300);
      const from = ctx.clock();
      const ms = await app.timeToFrame(
        "open to first frame",
        async () => {
          await app.openVod(vod.id);
          await app.click('.bar button[aria-label="Play"]');
        },
        null,
        { keep: false },
      );
      measures.add("open to first frame", ms, from, ctx.clock());
      await until(
        "it to play",
        () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
        MINUTE,
      );
      await app.pause();
    }

    // Seeks, paused, by a click on the timeline at a time: to the bar's time
    // changing (in the page, at the frame it's drawn in), and to the frame.
    // The frame's arrival is read through the e2e build's player-time
    // events: CI's Linux job tests the release package, which has none, so
    // there only the time shown is measured.
    const framed = await app.hasE2e();
    if (!framed) ctx.log("seek to frame shown: not measured (this build has no e2e feature)");
    for (const target of SEEKS) {
      let shown;
      const from = ctx.clock();
      const click = async () => {
        shown = await app.js(
          `const target = arguments[0];
             const bar = document.querySelector(".scrubber");
             const box = bar.querySelector("canvas").getBoundingClientRect();
             const x = box.left + ((target - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
             const at = { clientX: x, clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1 };
             const time = () => document.querySelector("button.time .now > span:not(.widest)")?.textContent ?? "";
             const before = time();
             const el = document.elementFromPoint(at.clientX, at.clientY);
             const start = performance.now();
             el.dispatchEvent(new PointerEvent("pointerdown", at));
             el.dispatchEvent(new PointerEvent("pointerup", at));
             while (performance.now() - start < 10000) {
               await new Promise((r) => requestAnimationFrame(r));
               if (time() !== before) return { ms: Math.round(performance.now() - start), before, after: time() };
             }
             return { error: "the time shown stayed " + before + " for 10 s" };`,
          target,
        );
      };
      const frame = framed ? await app.timeToFrame("seek to frame shown", click, target, { keep: false }) : null;
      if (!framed) await click();
      if (shown.error) throw new Error(`seek to ${target} s: ${shown.error}`);
      measures.add("seek: time shown", shown.ms, from, from + shown.ms / 1000);
      if (frame !== null) measures.add("seek to frame shown", frame, from, ctx.clock());
      ctx.log(
        `seek to ${target} s: ${shown.before} → ${shown.after} in ${shown.ms} ms` +
          (frame !== null ? `, its frame in ${frame} ms` : ""),
      );
    }
    measures.check();
  },
};
