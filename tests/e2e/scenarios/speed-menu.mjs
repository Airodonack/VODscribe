// The speed button, with the OS's mouse (lib/os.mjs): a click steps to the
// next speed, as it always has; pressed and held, it opens a menu of the
// speeds beside it (above, where there's room), the current one marked, and let go over 1.5× that's the
// speed. Held and let go away from the menu, it closes with the speed as it
// was. Linux (X11) only, so far.

import { setTimeout as sleep } from "node:timers/promises";

import { until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const BUTTON = ".bar button.speed";
const MENU = '[role=dialog][aria-label="Playback speed"]';

export default {
  name: "speed-menu",
  async run(app, ctx) {
    const why = os.unavailable() ?? (process.platform !== "linux" ? "gestures are driven on Linux only, so far" : null);
    if (why) ctx.skip(why);
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    const label = () => app.js(`return document.querySelector(arguments[0])?.textContent.trim()`, BUTTON);
    const speedIs = (text) =>
      until(
        `the speed at ${text}`,
        async () => {
          const now = await label();
          if (now === text) return true;
          throw new Error(`the button says ${now}`);
        },
        10_000,
      );
    const menuOpen = () => app.js(`return !!document.querySelector(arguments[0])`, MENU);
    await speedIs("1×");
    const at = await os.rectOf(app, BUTTON);
    const [x, y] = [at.x + Math.round(at.w / 2), at.y + Math.round(at.h / 2)];

    // A click: the next speed, and no menu.
    os.click(x, y, at.scale);
    await speedIs("1.25×");
    await sleep(600);
    if (await menuOpen()) throw new Error("a click opened the menu");
    ctx.log("a click steps 1× to 1.25×, no menu");

    // Held: the menu, 1.25× marked. Let go over 1.5×.
    await os.gesture([
      ["down", x, y],
      ["wait", 700],
    ]);
    try {
      await until("the menu, held open", menuOpen, 5_000);
      const marked = await app.js(
        `return [...document.querySelectorAll(arguments[0] + " [data-speed]")].filter((b) => b.classList.contains("current")).map((b) => b.dataset.speed)`,
        MENU,
      );
      if (marked.join() !== "1.25") throw new Error(`the menu marks ${marked.join()}, not 1.25`);
      // Above the button, or below it where the window has no room above
      // (an audio file's bar is near the top); never over it.
      const menu = await os.rectOf(app, MENU);
      const side = menu.y + menu.h <= at.y ? "above" : menu.y >= at.y + at.h ? "below" : null;
      if (!side) throw new Error(`the menu (${JSON.stringify(menu)}) covers the button (${JSON.stringify(at)})`);
      ctx.log(`the menu opened ${side} the button`);
      const item = await os.rectOf(app, `${MENU} [data-speed="1.5"]`);
      await os.gesture([
        ["move", item.x + Math.round(item.w / 2), item.y + Math.round(item.h / 2)],
        ["wait", 150],
        ["up"],
      ]);
    } finally {
      // Never leave the button held for the scenarios after.
      await os.gesture([["up"]]).catch(() => {});
    }
    await speedIs("1.5×");
    await until("the menu closed", async () => !(await menuOpen()), 5_000);
    ctx.log("held, the menu opened with 1.25× marked; let go over 1.5×, it's 1.5×");

    // Held and let go over the time, away from the menu: closed, as it was.
    const time = await os.rectOf(app, "button.time");
    await os.gesture([
      ["down", x, y],
      ["wait", 700],
    ]);
    await until("the menu, held open", menuOpen, 5_000);
    await os.gesture([
      ["move", time.x + Math.round(time.w / 2), time.y + Math.round(time.h / 2)],
      ["wait", 150],
      ["up"],
    ]);
    await until("the menu closed", async () => !(await menuOpen()), 5_000);
    await sleep(300);
    await speedIs("1.5×");
    ctx.log("let go away from the menu: it closed, still 1.5×");
  },
};
