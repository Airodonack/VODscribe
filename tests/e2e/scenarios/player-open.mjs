// A file opened shows its picture before Play: the app's own player opens
// with the VOD, paused, and shows the frame at the playhead (its first, or
// where it was left), not the row's picture; Play then starts from the
// frames already there, with no black between. With a video whose every
// second is one flat colour (fixtures colours.mp4), the screen says which
// frame is up. One VOD's player at a time: opening another closes the
// first. Timed: from the click on the row to its first frame on the
// screen, and from Play to the picture moving. Shift+→ moves the time
// shown at once, before the new frame is drawn.

import { copyFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { colourNamed, colours } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** How far a colour may be from the one meant: the video's own YUV rounding, scaling, the screen's colour depth. */
/** How long after Play the screen is watched for black, in milliseconds (the first second is red). */
const WATCHED = 900;

export default {
  name: "player-open",
  async run(app, ctx) {
    const why = os.unavailable();
    if (why) ctx.skip(why);
    try {
      os.screenshot();
    } catch (e) {
      ctx.skip(`no screenshot here: ${e.message.split("\n")[0]}`);
    }
    if (!(await app.invoke("player_available"))) ctx.skip("the app's own player doesn't draw here");
    if (!(await app.hasE2e())) ctx.skip("a build without the e2e feature (no player state to read)");
    const dir = path.join(ctx.out, "fixtures");
    const file = colours(dir);
    const other = path.join(dir, "colours-again.mp4");
    copyFileSync(file, other);
    await app.toLibrary();
    const [a, b] = await app.addFiles([file, other]);
    // Their library pictures made (what a stage showed before Play till now).
    await until(
      "the files' library pictures",
      async () => (await app.invoke("list_vods")).filter((v) => [a.id, b.id].includes(v.id) && v.thumb).length === 2,
      MINUTE,
    );
    // The add opened one of them: the other is opened first, afresh.
    const selected = Number(await app.js(`return document.querySelector(".vod.selected")?.dataset.key ?? null`));
    const [first, second] = selected === a.id ? [b, a] : [a, b];

    // What fails is gathered and thrown at the end, so the timings after a
    // failure are still measured (and compared with a build without the change).
    const problems = [];
    const stage = ".stage .frame";
    const shown = async () =>
      colourNamed(os.colourIn(os.screenshot(), os.middle(await os.rectOf(app, stage))), { black: true });
    const state = () => app.invoke("e2e_player_state");
    /** Opens VOD `id` and waits for its stage to show `colour`, paused, in the app's player open for it. */
    const openShowing = async (id, colour, label) => {
      const began = Date.now();
      await app.click(`.vod[data-key="${id}"]`);
      await until("the player", () => app.js(`return !!document.querySelector("button.time")`), MINUTE);
      try {
        await until(
          `${label}: the app's player open for it, paused, its stage ${colour}`,
          async () => {
            const s = await state();
            if (!s.open || s.doc?.kind !== "vod" || s.doc.id !== id)
              throw new Error(`no player open for it before Play (${JSON.stringify(s.doc ?? null)})`);
            if (s.player?.state?.playing) throw new Error("it plays");
            const now = await shown();
            if (now !== colour) throw new Error(`the stage shows ${now}`);
            return true;
          },
          10_000,
        );
        ctx.log(`${label}: ${colour} on the screen ${Date.now() - began} ms after the click, paused`);
      } catch (e) {
        problems.push(e.message);
        ctx.log(`${label}: ${e.message}`);
      }
    };

    await openShowing(first.id, "red", "opened at its start");

    // Play: the picture moves on from the frame shown, with nothing black between.
    const seen = [];
    let pressed = 0;
    const moving = await app.timeToFrame(
      "play to the picture moving",
      async () => {
        pressed = await app.js(`return performance.now()`);
        await app.click('.bar button[aria-label="Play"]');
        const until = Date.now() + WATCHED;
        while (Date.now() < until) seen.push(await shown());
      },
      1 / 30,
    );
    const statuses = await app.js(`return window.__e2eFrames.filter((f) => f.at >= arguments[0])`, pressed);
    const firstShown = statuses.find((f) => f.shown !== null);
    ctx.log(
      `Play: a frame shown ${firstShown ? Math.round(firstShown.at - pressed) : "never"} ms after, ` +
        `the picture moving ${moving} ms after; the screen meanwhile: ${seen.join(", ")}`,
    );
    const unshown = statuses.filter((f) => f.at < (firstShown?.at ?? Infinity) && f.shown === null);
    if (unshown.length)
      problems.push(`after Play the player said ${unshown.length} times that no frame showed (black)`);
    if (seen.includes("black")) problems.push(`after Play the screen went black (${seen.join(", ")})`);
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      10_000,
    );

    // Left at 0:03, another opened (its player alone open), then back: the
    // stage shows the frame where it was left (yellow) before Play.
    await app.pause();
    await app.toStart();
    // The first step forward (Shift+→) moves the time shown at once, as every
    // other move of the playhead does, not only once the new frame is drawn:
    // read in the page on the two frames after the key.
    await until("the frame at 0 s shown", async () => (await state()).player?.shown === 0, 10_000);
    const stepped = await app.js(`
      const text = () => document.querySelector("button.time .now > span:not(.widest)")?.textContent;
      const frame = () => new Promise((r) => requestAnimationFrame(r));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", code: "ArrowRight", shiftKey: true, bubbles: true, cancelable: true }));
      await frame();
      await frame();
      return text();`);
    if (stepped !== "0:01") problems.push(`Shift+→ from 0:00: the time said ${stepped} on the next frame, not 0:01`);
    await until("the player at 1 s", async () => (await app.shownTime()) === 1, 10_000);
    for (const target of [2, 3]) {
      await app.key("ArrowRight", { shift: true });
      await until(`the player at ${target} s`, async () => (await app.shownTime()) === target, 10_000);
    }
    await until("the frame at 3 s shown", async () => (await state()).player?.shown === 3, 10_000);
    await openShowing(second.id, "red", "another opened");
    await openShowing(first.id, "yellow", "back where it was left");

    if (problems.length) throw new Error(problems.join("; "));
  },
};
