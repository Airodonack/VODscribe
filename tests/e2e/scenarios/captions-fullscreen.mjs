// The OS-level layer again (lib/os.mjs). The app's own player draws its
// captions over the picture, outside the page, so only the screen shows
// them: C turns them off and on, and the picture's lower part changes with
// them. F fills the screen with the window; Escape gives it back.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

export default {
  name: "captions-fullscreen",
  async run(app, ctx) {
    const why = os.unavailable();
    if (why) ctx.skip(why);
    try {
      os.screenshot();
    } catch (e) {
      ctx.skip(`no screenshot here: ${e.message.split("\n")[0]}`);
    }
    await app.toLibrary();
    const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    const vod = await app.transcribe(added.id);
    await app.openVod(vod.id);
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    if (!(await app.js(`return !!document.querySelector(".player .picture")`)))
      ctx.skip("plays in the web view here, which draws its own captions in the page");
    await app.pause();
    // In the speech (one cue over the whole landing), paused: the picture stands.
    await app.toStart();
    await app.key("ArrowRight", { shift: true });
    await app.key("ArrowRight", { shift: true });
    await until("2 s in", async () => (await app.shownTime()) === 2, 10_000);
    const lower = async () => {
      const r = await os.rectOf(app, ".player .picture");
      const band = { x: r.x, y: Math.round(r.y + r.h * 0.7), w: r.w, h: Math.round(r.h * 0.3) };
      await sleep(500);
      return os.pixels(os.screenshot(), band);
    };
    const on = await lower();
    await app.key("C", { shift: true });
    const off = await lower();
    await app.key("C", { shift: true });
    const back = await lower();
    const changed = os.difference(on, off);
    const same = os.difference(on, back);
    ctx.log(`the picture's lower part: ${changed.toFixed(1)} apart with captions off, ${same.toFixed(1)} once back on`);
    if (changed < 3)
      throw new Error(`turning captions off changed the picture by ${changed.toFixed(1)}: no captions drawn?`);
    if (same > changed / 3)
      throw new Error(`turning them back on didn't bring the same picture back (${same.toFixed(1)})`);
    await ctx.checkpoint("captions-on");

    // Full screen: the window says it is, as big as the screen; then back
    // as it was (a small screen's window may fill it before as well).
    const screen = os.sizeOf(os.screenshot());
    const full = () =>
      app.js(`return window.__TAURI_INTERNALS__.invoke("plugin:window|is_fullscreen", { label: "main" })`);
    const before = await os.windowRect(app);
    await app.key("f");
    await until(
      "the window full screen, filling the screen",
      async () => {
        const r = await os.windowRect(app);
        return (await full()) && r.w >= screen.w * 0.95 && r.h >= screen.h * 0.9 && r;
      },
      10_000,
    );
    await ctx.checkpoint("fullscreen");
    await app.key("Escape");
    const after = await until(
      "the window back as it was",
      async () => {
        const r = await os.windowRect(app);
        return !(await full()) && Math.abs(r.w - before.w) <= 4 && Math.abs(r.h - before.h) <= 4 && r;
      },
      10_000,
    );
    ctx.log(`window ${before.w}x${before.h}, full screen, then ${after.w}x${after.h} (screen ${screen.w}x${screen.h})`);
  },
};
