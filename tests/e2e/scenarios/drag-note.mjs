// A clip of a site's VOD dragged out of the window before the note on
// permission was ever answered: a question can't be answered in the middle of
// the system's drag, so none begins. The carry ends at the window's edge and
// the note asks there, by the Clip button, and stays up through the release
// of the button that pulled the clip out, wherever it's let go: back over the
// app, or out past the window. Cancel makes nothing; Download clip makes the
// clip in the clip bar, to drag out once it's made. Driven with the OS's
// pointer (lib/os.mjs), as a person would.
//
// Offline: the VOD is a file added here, then made the test YouTube VOD's
// library entry; the clip asked for is deleted before it's downloaded.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { makeSiteVod } from "../lib/data.mjs";
import * as explorer from "../lib/explorer.mjs";
import * as os from "../lib/os.mjs";

const NOTE = '[role=dialog][aria-label="Before downloading a clip"]';

export default {
  name: "drag-note",
  async run(app, ctx) {
    const why = os.unavailable();
    if (why) ctx.skip(why);
    await app.toLibrary();
    const [vod] = await app.addFiles([ctx.fixtures.landed]);
    await ctx.quit();
    makeSiteVod(vod.id, "youtube", "noa29qsTc4g");
    app = await ctx.start();
    // On Windows, the window placed with room left of it to let go in.
    if (process.platform === "win32") {
      const screen = explorer.screenSize();
      explorer.placeWindowOf(app.driver.proc.pid, { x: 160, y: 0, w: screen.w - 160, h: screen.h - 48 });
    }
    await app.toLibrary();
    await app.openVod(vod.id);
    await until(
      "the Clip button",
      () => app.js(`const b = document.querySelector("button.action.clip"); return !!b && !b.disabled`),
      MINUTE,
    );
    const noteUp = () => app.js(`return !!document.querySelector(arguments[0])`, NOTE);
    const clipsOf = () => app.invoke("list_clips", { vodId: vod.id });

    /**
     * Presses on Clip, pulls it to the window's left edge and holds it there,
     * then lets go at (`x`, `y`); the note is up after, or this throws.
     */
    const pullOut = async (where, x, y) => {
      // The app's window in front, as a click on its title bar puts it.
      const edge = await os.windowRect(app);
      os.click(edge.x + Math.round(edge.w / 2), edge.y - 16, edge.scale);
      await sleep(300);
      const button = await os.rectOf(app, "button.action.clip");
      const page = await os.windowRect(app);
      // Screen points on macOS (its runners have a scale of 1).
      const s = process.platform === "darwin" ? button.scale : 1;
      const [bx, by] = [button.x + Math.round(button.w / 2), button.y + Math.round(button.h / 2)];
      const steps = [
        ["down", bx / s, by / s],
        ["wait", 100],
      ];
      for (let i = 1; i <= 10; i++) steps.push(["move", (bx + (page.x - bx) * (i / 10)) / s, by / s], ["wait", 40]);
      // Held at the edge, where it would go out as the system's drag; then
      // carried on to where it's let go.
      steps.push(["wait", 1500]);
      // (A picture of the screen then, where the OS layer takes one in a gesture.)
      if (process.platform === "win32")
        steps.push(["shot", path.join(ctx.out, `drag-note-${where.replaceAll(" ", "-")}.png`)]);
      for (let i = 1; i <= 5; i++)
        steps.push(["move", (page.x + (x - page.x) * (i / 5)) / s, (by + (y - by) * (i / 5)) / s], ["wait", 40]);
      steps.push(["wait", 200], ["up"], ["wait", 500]);
      // What the page saw, for when the note isn't up: the note coming and
      // going, and the presses, releases, clicks and focus around it.
      await app.js(
        `const t0 = performance.now();
         const trace = (window.__noteTrace = []);
         const note = () => !!document.querySelector(arguments[0]);
         let up = note();
         new MutationObserver(() => {
           if (note() !== up) trace.push(Math.round(performance.now() - t0) + " note " + ((up = note()) ? "up" : "gone"));
         }).observe(document.body, { childList: true, subtree: true });
         for (const type of ["pointerdown", "pointerup", "click", "blur", "focus"])
           window.addEventListener(type, (e) => trace.push(Math.round(performance.now() - t0) + " " + type +
             (e.target?.tagName ? " on " + e.target.tagName.toLowerCase() : "")), true);`,
        NOTE,
      );
      await os.gesture(steps);
      const trace = await app.js("return window.__noteTrace.splice(0).join(', ')");
      if (!(await noteUp()))
        throw new Error(
          `the note before downloading the clip wasn't up after letting go ${where} (the page saw: ${trace})`,
        );
      if ((await clipsOf()).length) throw new Error(`a clip was made before the note was answered (let go ${where})`);
      ctx.log(`pulled out and let go ${where}: the note is up, nothing made`);
    };
    /** Clicks the note's button `label` with the OS's pointer. */
    const answer = async (label) => {
      const sel = `${NOTE} button${label === "Download clip" ? ".primary" : ":not(.primary)"}`;
      const r = await os.rectOf(app, sel);
      os.click(r.x + Math.round(r.w / 2), r.y + Math.round(r.h / 2), r.scale);
      await until("the note answered", async () => !(await noteUp()), 10_000);
    };

    // Let go back over the app, below the Clip button: Cancel makes nothing.
    {
      const page = await os.windowRect(app);
      const button = await os.rectOf(app, "button.action.clip");
      await pullOut("over the app", page.x + Math.round(page.w / 2), Math.min(button.y + 120, page.y + page.h - 20));
      await answer("Cancel");
      if ((await clipsOf()).length) throw new Error("Cancel made a clip");
      ctx.log("Cancel: nothing made");
    }

    // Let go out past the window (left of it if there's room, else above it):
    // Download clip makes the clip, in the clip bar.
    {
      const page = await os.windowRect(app);
      const [x, y] = page.x >= 60 ? [page.x - 40, page.y + 80] : [page.x + 80, Math.max(2, page.y - 40)];
      await pullOut(page.x >= 60 ? "left of the window" : "above the page", x, y);
      await answer("Download clip");
      const made = await until("the clip asked for", async () => (await clipsOf()).find((c) => c.shown), 10_000);
      ctx.log(`Download clip: clip ${made.id} (${made.start} to ${made.end}) in the clip bar`);
      await app.invoke("delete_clip", { id: made.id });
    }
  },
};
