// An MP4 clip dragged out onto DaVinci Resolve on Linux, which can't open
// MP4: the app refuses it there and, once the drag is over, asks whether to
// switch clips to MOV. The stand-in for Resolve is the test's app that takes
// finished files only (lib/fileManager.mjs) with Resolve's window class,
// "resolve"; with its own class it stands for any other app. A pill of a
// 5-second video, made as MP4: dropped on the other app, it lands and
// nothing asks. Dropped on Resolve, nothing lands, the pill stays, and the
// note asks; Switch to MOV makes clips as MOV DNxHR and the pill again as
// MOV, which then lands in Resolve with nothing asked. Back to MP4, dropped
// on Resolve again: the note asks again; Don't show again, ticked, and Not
// now turn it off, and the MP4 then lands. Driven with the OS's pointer
// (lib/os.mjs). Linux (X11) only: on Wayland the drop's app isn't named, and
// macOS and Windows never ask.

import path from "node:path";
import { rmSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { noFinishedOnly, openFinishedOnly, setFormat, windowsOf } from "../lib/fileManager.mjs";
import { videos } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

const NOTE = '[role=dialog][aria-label="DaVinci Resolve"]';
/** What the note says of one clip refused to Resolve (resolveDrop.svelte.ts `noteText`). */
const REFUSED = "DaVinci Resolve can't open MP4 clips on Linux, so the clip wasn't dropped there. MOV clips work.";
/** What's said once switched to MOV (resolveDrop.svelte.ts `switched`). */
const SWITCHED = "Clips are MOV now. Drag it again from the clip bar.";

export default {
  name: "drag-resolve",
  expects: {
    "resolve-note":
      'A small dialog titled "DaVinci Resolve" over the player: "DaVinci Resolve can\'t open MP4 clips on Linux, so the clip wasn\'t dropped there. MOV clips work.", a "Don\'t show again" checkbox, unticked, then "Not now" and a highlighted "Switch to MOV" at the right.',
  },
  async run(app, ctx) {
    const why = process.platform !== "linux" ? "Resolve's note is Linux's only" : noFinishedOnly();
    if (why) ctx.skip(why);
    const work = path.join(ctx.out, "drag-resolve");
    rmSync(work, { recursive: true, force: true });
    await app.toLibrary();
    const [vod] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await until("the video read", async () => (await app.invoke("list_vods"))[0]?.duration > 0, 5 * MINUTE);
    await setFormat(app, "MP4");
    const noteUp = () => app.js(`return !!document.querySelector(arguments[0])`, NOTE);
    if (await noteUp()) throw new Error("the note is up before any drop");
    const made = await app.invoke("add_clip", { vodId: vod.id, start: 1, end: 4, shown: true });
    /** The pill's clip, once its file is made as `ext`. */
    const madeAs = (ext) =>
      until(
        `the clip made as ${ext}`,
        async () => {
          const c = (await app.invoke("list_clips", { vodId: vod.id })).find((c) => c.id === made.id);
          if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
          return c?.status === "ready" && c.file_path?.endsWith(`.${ext}`) && c;
        },
        MINUTE,
      );
    await madeAs("mp4");
    await app.openPaused(vod.id);
    const pill = `.chip[data-clip="${made.id}"] .length`;
    await until("the pill", () => app.js(`return !!document.querySelector(arguments[0])`, pill), 10_000);
    // What the page hears of each drag's end.
    await app.js(
      `window.__ended = [];
       const internals = window.__TAURI_INTERNALS__;
       const handler = internals.transformCallback((e) => window.__ended.push(e.payload));
       await internals.invoke("plugin:event|listen", { event: "drag-ended", target: { kind: "Any" }, handler });`,
    );
    const ended = () => until("the system's drag, over", () => app.js(`return window.__ended[0] ?? false`), MINUTE);

    /** The pill carried onto `taker` and let go there, the system's drag up on the way. */
    const dropOn = async (taker) => {
      const to = taker.drop;
      await os.toFront(app);
      await app.js(`window.__ended = []`);
      const r = await os.rectOf(app, pill);
      const from = { x: r.x + Math.round(r.w / 2), y: r.y + Math.round(r.h / 2) };
      const before = new Set(windowsOf(app.driver.proc.pid));
      const steps = [
        ["down", from.x, from.y],
        ["wait", 100],
      ];
      for (let i = 1; i <= 18; i++)
        steps.push(
          ["move", Math.round(from.x + (to.x - from.x) * (i / 18)), Math.round(from.y + (to.y - from.y) * (i / 18))],
          ["wait", 50],
        );
      await os.gesture(steps);
      await until("its picture", () => windowsOf(app.driver.proc.pid).some((w) => !before.has(w)), 10_000, {
        every: 25,
      });
      await os.gesture([
        ["move", to.x + 8, to.y + 4],
        ["wait", 200],
        ["move", to.x + 16, to.y + 8],
        ["wait", 300],
        ["letgo", to.x + 16, to.y + 8],
      ]);
      return ended();
    };
    /** What `taker` said it took last, once it says. */
    const took = (taker, from) =>
      until(
        "the drop taken",
        () =>
          taker
            .said()
            .slice(from)
            .find((l) => / took \d+ of \d+/.test(l)) ?? false,
        10_000,
      );
    const clickIn = (label) =>
      app.js(
        `const b = [...document.querySelectorAll(arguments[0] + " button")].find((b) => b.textContent.trim() === arguments[1]);
         if (!b) throw new Error("no " + arguments[1]);
         b.click();`,
        NOTE,
        label,
      );

    let taker = null;
    try {
      // 1. Another app: the MP4 lands, nothing asks.
      taker = await openFinishedOnly(app, ctx, path.join(work, "other"));
      let heard = await dropOn(taker);
      const other = await took(taker, 0);
      if (!/ took 1 of 1: .*\.mp4$/.test(other)) throw new Error(`the other app says "${other}"`);
      if (heard.resolve || !heard.delivered)
        throw new Error(`the page heard ${JSON.stringify(heard)} of a drop on another app`);
      await sleep(500);
      if (await noteUp()) throw new Error("the note asks of a drop on another app");
      ctx.log(`dropped on another app: it took the MP4, nothing asked`);
      taker.close();

      // 2. Resolve: nothing lands, the pill stays, the note asks.
      taker = await openFinishedOnly(app, ctx, path.join(work, "resolve"), { name: "resolve" });
      heard = await dropOn(taker);
      if (heard.resolve !== "refused" || heard.delivered)
        throw new Error(`the page heard ${JSON.stringify(heard)} of a drop on Resolve`);
      const refused = await took(taker, 0);
      if (!/ took 0 of 0/.test(refused)) throw new Error(`Resolve says "${refused}"`);
      await until("the note", noteUp, 10_000);
      const said = await app.js(`return document.querySelector(arguments[0] + " p").textContent.trim()`, NOTE);
      if (said !== REFUSED) throw new Error(`the note says "${said}"`);
      if (!(await app.js(`return !!document.querySelector(arguments[0])`, pill))) throw new Error("the pill went");
      await ctx.checkpoint("resolve-note");
      ctx.log(`dropped on Resolve: it took nothing, the note says "${said}"`);

      // 3. Switch to MOV: clips are MOV DNxHR, the pill made again so, and it lands.
      await clickIn("Switch to MOV");
      await until("the note gone", async () => !(await noteUp()), 10_000);
      await until(
        "clips made as MOV DNxHR",
        async () => {
          const s = await app.invoke("get_settings");
          return s.clip_codec === "dnxhr" && s.clip_format === "mov";
        },
        10_000,
      );
      await until("the toast", async () => (await app.toastSays()) === SWITCHED, 10_000).catch(async (e) => {
        throw new Error(`${e.message}: it says "${await app.toastSays()}"`);
      });
      await madeAs("mov");
      const before = taker.said().length;
      heard = await dropOn(taker);
      const mov = await took(taker, before);
      if (!/ took 1 of 1: .*\.mov$/.test(mov) || heard.resolve)
        throw new Error(`Resolve says "${mov}", the page heard ${JSON.stringify(heard)}`);
      await sleep(500);
      if (await noteUp()) throw new Error("the note asks of a MOV");
      ctx.log(`Switch to MOV: said "${SWITCHED}", the pill made again as MOV, and Resolve took it`);

      // 4. MP4 again: asked again; Don't show again ticked, Not now, and the MP4 lands.
      await setFormat(app, "MP4");
      // (Made again as MP4 before the drag, as Resolve takes finished files only.)
      await app.invoke("set_clip_range", { id: made.id, start: 1, end: 4 });
      await madeAs("mp4");
      heard = await dropOn(taker);
      if (heard.resolve !== "refused") throw new Error(`the page heard ${JSON.stringify(heard)} of an MP4 on Resolve`);
      await until("the note again", noteUp, 10_000);
      await app.js(`document.querySelector(arguments[0] + " input[type=checkbox]").click()`, NOTE);
      await clickIn("Not now");
      await until(
        "the note turned off",
        async () => (await app.invoke("get_settings")).suggest_mov_for_resolve === false,
        10_000,
      );
      if ((await app.invoke("get_settings")).clip_format !== "mp4") throw new Error("Not now changed the clip format");
      const from = taker.said().length;
      heard = await dropOn(taker);
      const mp4 = await took(taker, from);
      if (!/ took 1 of 1: .*\.mp4$/.test(mp4) || heard.resolve)
        throw new Error(`Resolve says "${mp4}", the page heard ${JSON.stringify(heard)}`);
      await sleep(500);
      if (await noteUp()) throw new Error("the note asks once turned off");
      ctx.log("asked again of an MP4; Don't show again and Not now turned it off, and Resolve then took the MP4");
    } finally {
      taker?.close();
    }
  },
};
