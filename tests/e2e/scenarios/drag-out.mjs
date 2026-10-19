// Drags out of the window with the OS's mouse (lib/os.mjs). A clip's pill
// dragged out and the drag called off (Escape) before anything took its
// file: the file isn't counted as handed to another app, so deleting the clip
// deletes it. Then Clip pulled to the window's edge (no key held), and a
// stretch Alt-dragged there, each let go at once, before its file is made:
// no file is left behind (the Alt-drag skipped where the window manager takes
// it). Linux (X11) only, so far.

import { existsSync, readdirSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

export default {
  name: "drag-out",
  async run(app, ctx) {
    const why = os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux only, so far" : null);
    if (why) ctx.skip(why);
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    const clip = await app.clipOneToThree(vod.id);
    const pill = `.chip.ready[data-clip="${clip.id}"]`;
    await until("the pill", () => app.js(`return !!document.querySelector(arguments[0])`, pill), 30_000);
    const ready = await until(
      "the clip's file",
      async () => (await app.invoke("list_clips", { vodId: vod.id })).find((c) => c.id === clip.id)?.file_path,
      30_000,
    );
    // Pressed on the pill, pulled to the window's left edge: the system's drag
    // of its file. Then Escape, before it's dropped anywhere.
    const at = await os.rectOf(app, `${pill} .length`);
    const page = await os.windowRect(app);
    const [x, y] = [at.x + Math.round(at.w / 2), at.y + Math.round(at.h / 2)];
    const steps = [
      ["down", x, y],
      ["wait", 100],
    ];
    for (let i = 1; i <= 10; i++) steps.push(["move", Math.round(x + (page.x - x) * (i / 10)), y], ["wait", 50]);
    // The app says when the system's drag is over ("drag-ended"): counted, to
    // know there was one.
    await app.js(
      `window.__dragsEnded = 0;
       const internals = window.__TAURI_INTERNALS__;
       const handler = internals.transformCallback(() => window.__dragsEnded++);
       await internals.invoke("plugin:event|listen", { event: "drag-ended", target: { kind: "Any" }, handler });`,
    );
    await os.gesture(steps);
    await os.gesture([["wait", 1000], ["key", "Escape"], ["wait", 300], ["up"]]);
    await until("the system's drag, over", () => app.js("return window.__dragsEnded"), 10_000);
    ctx.log("dragged out, then called off");

    await app.invoke("delete_clip", { id: clip.id });
    await until(
      "the clip's file gone with it",
      () => {
        if (!existsSync(ready)) return true;
        throw new Error(`${ready} is still there`);
      },
      10_000,
    );
    // Clip pressed and pulled to the window's left edge, no key held, and let
    // go there at once (its stretch, 1 to 3 s, has no clip now): nothing left behind.
    {
      const folder = path.dirname(ready);
      const before = new Set(readdirSync(folder));
      const button = await os.rectOf(app, "button.action.clip");
      const edge = await os.windowRect(app);
      const [cx, cy] = [button.x + Math.round(button.w / 2), button.y + Math.round(button.h / 2)];
      await os.gesture([
        ["down", cx, cy],
        ["move", cx - 20, cy],
        ["move", cx - 60, cy],
        ["letgo", edge.x, cy],
      ]);
      await sleep(5000);
      const left = readdirSync(folder).filter((f) => !before.has(f));
      if (left.length) throw new Error(`Clip dragged out and let go at once left ${left.join(", ")}`);
      ctx.log("Clip dragged out and let go at once: nothing left behind");
    }

    // Clip pulled out and held there: how long the system's drag takes to
    // start once the pointer is out (the clip's file and picture are made
    // during the carry, from a file here). Then called off.
    {
      const button = await os.rectOf(app, "button.action.clip");
      const edge = await os.windowRect(app);
      const [cx, cy] = [button.x + Math.round(button.w / 2), button.y + Math.round(button.h / 2)];
      await app.js(`performance.clearMarks()`);
      await os.gesture([
        ["down", cx, cy],
        ["move", cx - 20, cy],
        ["move", cx - 60, cy],
        ["wait", 1500],
        ["move", edge.x, cy],
      ]);
      const waited = await until(
        "the system's drag to start",
        () =>
          app.js(
            `const [out] = performance.getEntriesByName("carry-out"), [drag] = performance.getEntriesByName("drag-start");
             return out && drag ? Math.round(drag.startTime - out.startTime) : false;`,
          ),
        30_000,
      );
      await os.gesture([["key", "Escape"], ["up"]]);
      ctx.log(`Clip held out of the window: the system's drag started ${waited} ms after it left`);
      if (waited > 1000) throw new Error(`the system's drag took ${waited} ms to start once out of the window`);
      await sleep(1500);
    }

    // The stretch after the cut at 3 s (no clip yet), Alt-dragged to the left
    // edge and let go there at once.
    const folder = path.dirname(ready);
    const before = new Set(readdirSync(folder));
    const bar = await os.rectOf(app, ".scrubber");
    const edge = await os.windowRect(app);
    const [bx, by] = [bar.x + Math.round(bar.w * 0.8), bar.y + Math.round(bar.h / 2)];
    // The OS's mouse reaches only what's on the screen.
    if (edge.x < 0 || edge.y < 0 || bx < 0 || by < 0)
      ctx.skip(`the window isn't all on the screen (at ${edge.x},${edge.y}): a bigger screen is needed`);
    await os.gesture([
      ["hold", "alt"],
      ["down", bx, by],
      ["move", bx - 20, by],
      ["move", bx - 60, by],
      ["letgo", edge.x, by],
      ["release", "alt"],
    ]);
    await sleep(5000);
    // A window manager that moves windows on Alt-drag takes it instead (as
    // some desktops do, and openbox here does even without its bindings).
    const moved = await os.windowRect(app);
    if (moved.x !== edge.x || moved.y !== edge.y)
      ctx.skip(
        `the window manager took the Alt-drag (it moved the window from ${edge.x},${edge.y} to ${moved.x},${moved.y})`,
      );
    const left = readdirSync(folder).filter((f) => !before.has(f));
    if (left.length) throw new Error(`a drag let go before its file was ready left ${left.join(", ")}`);
    ctx.log("a drag out let go at once: nothing left behind");
  },
};
