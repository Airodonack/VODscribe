// Drags out of the window with the OS's mouse (lib/os.mjs). A clip's pill
// dragged out and the drag called off (Escape) before anything took its
// file: the file isn't counted as handed to another app, so deleting the clip
// deletes it. Then Clip pulled to the window's edge (no key held), and a
// stretch Alt-dragged there, each let go at once, before its file is made:
// no file is left behind (the Alt-drag skipped where the window manager takes
// it). Clip held out of the window: the system's drag starts within a second.
// Brought back over the window: let go on "Make new edit", it makes an edit;
// on the clip bar, it's a clip there, and stays. Linux (X11) only, so far.

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
      // What the page saw, for when something is left: its pointer events
      // (where, on what) and the app's news of clips, in order.
      await app.js(
        `const t0 = performance.now();
         const trace = (window.__trace = []);
         const at = (el) => (el?.tagName ?? "?") + (el?.className ? "." + String(el.className).replaceAll(" ", ".") : "");
         for (const type of ["pointerdown", "pointermove", "pointerup", "click"])
           window.addEventListener(type, (e) => trace.push(\`\${Math.round(performance.now() - t0)} \${type} \${e.clientX},\${e.clientY} \${at(e.target)}\`), true);
         const internals = window.__TAURI_INTERNALS__;
         for (const event of ["clip", "clip-removed", "drag-ended"]) {
           const handler = internals.transformCallback((e) => {
             const p = e.payload;
             const what = p && typeof p === "object" ? \`#\${p.id} \${p.status} shown=\${p.shown} \${p.file_path ?? ""}\` : String(p);
             trace.push(\`\${Math.round(performance.now() - t0)} \${event} \${what}\`);
           });
           await internals.invoke("plugin:event|listen", { event, target: { kind: "Any" }, handler });
         }`,
      );
      // Twice: let go as soon as the edge is reached, and again after a
      // pause long enough for the clip's file to be made first (the release
      // then arrives as the drag is being asked for: 0.5.0 began the system's
      // drag with no button held, which never ended, and the file stayed).
      for (const pause of [0, 500]) {
        await os.gesture([
          ["down", cx, cy],
          ["move", cx - 20, cy],
          ["move", cx - 60, cy],
          ...(pause ? [["wait", pause]] : []),
          ["letgo", edge.x, cy],
        ]);
        await sleep(5000);
        const left = readdirSync(folder).filter((f) => !before.has(f));
        await leftNothing(left, pause ? "after a pause" : "at once");
      }
    }
    async function leftNothing(left, how) {
      if (left.length) {
        const trace = await app.js(`return window.__trace`).catch(() => []);
        const marks = await app.js(`return performance.getEntriesByType("mark").map((m) => m.name)`).catch(() => []);
        const clips = await app.invoke("list_clips", { vodId: vod.id }).catch(() => []);
        ctx.log(`the page saw: ${trace.join(" | ")}`);
        ctx.log(`marks: ${marks.join(", ")}; clips now: ${JSON.stringify(clips)}`);
        throw new Error(`Clip dragged out and let go ${how} left ${left.join(", ")}`);
      }
      ctx.log(`Clip dragged out and let go ${how}: nothing left behind`);
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

    // Clip pulled out, then brought back over the window, the system's drag
    // on all the while: the drop targets show as for a carry, and let go on
    // "Make new edit", the stretch is in a new edit.
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
      await until(
        "the system's drag",
        () => app.js(`return performance.getEntriesByName("drag-start").length > 0`),
        30_000,
      );
      await os.gesture([
        ["move", edge.x + 40, cy],
        ["wait", 200],
        ["move", edge.x + 60, cy],
      ]);
      await until(
        '"Make new edit" while it\'s over the window',
        () => app.js(`return !!document.querySelector("[data-new-edit]")`),
        10_000,
      );
      const target = await os.rectOf(app, "[data-new-edit]");
      const [tx, ty] = [target.x + Math.round(target.w / 2), target.y + Math.round(target.h / 2)];
      await os.gesture([
        ["move", tx, ty],
        ["wait", 500],
        ["letgo", tx + 2, ty],
      ]);
      const [made] = await until(
        "an edit made with it",
        async () => {
          const all = await app.invoke("list_edits");
          return all.length === 1 && all;
        },
        15_000,
      );
      const pieces = (await app.invoke("get_edit", { id: made.id })).pieces.length;
      if (pieces !== 1) throw new Error(`the new edit has ${pieces} pieces, not 1`);
      ctx.log('a clip dragged out and brought back onto "Make new edit" makes an edit of it');
      await sleep(1500);
    }

    // The same, let go on the clip bar: the clip made for the drag is the
    // clip, and stays once the system's drag is over.
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
      await until(
        "the system's drag",
        () => app.js(`return performance.getEntriesByName("drag-start").length > 0`),
        30_000,
      );
      await os.gesture([
        ["move", edge.x + 40, cy],
        ["wait", 200],
        ["move", edge.x + 60, cy],
      ]);
      await until(
        "the clip bar while it's over the window",
        () => app.js(`return !!document.querySelector(".clip-bar")`),
        10_000,
      );
      const bar = await os.rectOf(app, ".clip-bar");
      const [tx, ty] = [bar.x + Math.round(bar.w / 2), bar.y + Math.round(bar.h / 2)];
      await os.gesture([
        ["move", tx, ty],
        ["wait", 500],
        ["letgo", tx + 2, ty],
      ]);
      const shownAt = async () =>
        (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => c.shown && Math.abs(c.start - 1) < 0.05);
      await until("the clip in the clip bar", async () => (await shownAt()).length === 1, 15_000);
      await sleep(3000);
      if ((await shownAt()).length !== 1)
        throw new Error("the clip dropped back on the clip bar went once the drag was over");
      ctx.log("a clip dragged out and brought back onto the clip bar is in it, and stays");
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
