// The timeline's thumbnail (Scrubber.svelte, ScrubFrame.svelte): with a
// video whose every second is one flat colour, a keyframe each second
// (fixtures colours.mp4), the thumbnail says which frame it shows by its
// colour, read from its canvas, which says the time it shows (data-time)
// and whether that's the exact frame (data-exact). The pointer is the
// page's own (synthetic pointer events on the window, as the timeline
// follows the pointer there).
//
// The file's storyboard is made on adding it, and its library picture
// comes from it (red: the frame an eighth in, the keyframe at 0). Hovering
// shows the storyboard's nearest frame, then, the player open and the
// pointer resting, the exact frame. A selection begun with I keeps through
// a click on the timeline, for O there; its edges show their frames: its
// start the frame there, its end the frame before. Dragging the
// playhead shows it; a finger doesn't. Rested at a moment and played from
// there, the player takes the decoder that previewed it over: no new one
// starts, and the screen shows the colour from there on.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { COLOURS, colours } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** How far a colour may be from the one meant: JPEG and YUV rounding, scaling. */
const NEAR = 90;
const FPS = 30;

/** The colour named for `rgb`, or "none (…)". */
function named(rgb) {
  const [name, , d] = COLOURS.map(([n, c]) => [n, c, os.distance(rgb, c)]).sort((a, b) => a[2] - b[2])[0];
  return d < NEAR ? name : `none (${rgb.join(", ")})`;
}

/** In the page: the pointer (`type`, mouse by default) moved over the timeline to `time`, and pressed or let go there. */
const POINTER = `
  const [kind, time, type, target] = arguments;
  const bar = document.querySelector(".scrubber");
  const box = bar.getBoundingClientRect();
  const x = box.left + ((time - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
  const at = { clientX: x, clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1, pointerType: type ?? "mouse", isPrimary: true };
  const on = target ? document.querySelector(target) : kind === "pointerdown" ? document.elementFromPoint(at.clientX, at.clientY) : window;
  on.dispatchEvent(new PointerEvent(kind, at));`;

/** In the page: where the timeline's selection starts and ends, in seconds (null: none). */
const SELECTION = `
  const bar = document.querySelector(".scrubber");
  const picked = bar.querySelector(".selection");
  if (!picked) return null;
  const box = bar.getBoundingClientRect();
  const { left, right } = picked.getBoundingClientRect();
  const time = (x) => Number(bar.dataset.viewStart) + ((x - box.left) / box.width) * Number(bar.dataset.span);
  return [time(left), time(right)];`;

/** In the page: what the thumbnail shows (null: none), its colour read at its middle. */
const THUMBNAIL = `
  const canvas = document.querySelector(".timeline .scrub-frame:not(.empty)");
  if (!canvas || !canvas.width) return null;
  const [r, g, b] = canvas.getContext("2d").getImageData(canvas.width >> 1, canvas.height >> 1, 1, 1).data;
  return { time: Number(canvas.dataset.time), exact: canvas.dataset.exact === "true", rgb: [r, g, b],
           tip: document.querySelector(".timeline .tip")?.textContent.trim() ?? null };`;

export default {
  name: "scrub-thumbnail",
  async run(app, ctx) {
    const file = colours(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    await app.pickFiles([file]);
    await app.clickText("add video files…");
    // Added, it isn't transcribed (it has no sound anyway): its picture is what's waited for.
    const vod = await until(
      "the video, with its picture",
      async () => {
        const [vod] = await app.invoke("list_vods");
        if (vod?.status === "error") throw new Error(vod.error);
        return vod?.thumb && vod;
      },
      5 * MINUTE,
    );

    // 1. The library's picture, from the storyboard: the keyframe at or before 0.75 s, red.
    const picture = await app.js(
      `const bytes = await window.__TAURI_INTERNALS__.invoke("thumbnail", { name: arguments[0] });
       const image = await createImageBitmap(new Blob([bytes], { type: "image/jpeg" }));
       const canvas = new OffscreenCanvas(image.width, image.height);
       const ctx = canvas.getContext("2d");
       ctx.drawImage(image, 0, 0);
       return { size: [image.width, image.height], rgb: [...ctx.getImageData(image.width >> 1, image.height >> 1, 1, 1).data.slice(0, 3)] };`,
      vod.thumb,
    );
    ctx.log(`the library picture: ${picture.size.join("x")}, ${named(picture.rgb)}`);
    if (named(picture.rgb) !== "red") throw new Error(`the library picture is ${named(picture.rgb)}, not red`);
    if (picture.size[1] !== 90) throw new Error(`the library picture is ${picture.size.join("x")}, not 90 tall`);

    const thumbnail = () => app.js(THUMBNAIL);
    const pointer = (kind, time, type, target) => app.js(POINTER, kind, time, type ?? null, target ?? null);
    const shows = (what, want, { exact = false, tip = null } = {}) =>
      until(
        what,
        async () => {
          const t = await thumbnail();
          if (!t) throw new Error("no thumbnail");
          if (named(t.rgb) !== want) throw new Error(`it shows ${named(t.rgb)} at ${t.time}`);
          if (exact && !t.exact) throw new Error(`the storyboard's frame at ${t.time}, not the exact one yet`);
          if (tip !== null && t.tip !== tip) throw new Error(`the tip says ${t.tip}`);
          return t;
        },
        15_000,
        { every: 100 },
      );

    // 2. Hovering 2.5 s before the player is open: the storyboard's frame, the keyframe at 2 s, blue.
    await app.openVod(vod.id);
    await pointer("pointermove", 2.5);
    const hovered = await shows("the storyboard's frame at 2.5 s", "blue");
    ctx.log(`hovering 2.5 s: ${named(hovered.rgb)} from ${hovered.time} s (exact: ${hovered.exact})`);
    if (hovered.exact) throw new Error("exact before the player was open");

    // The player open: resting there, the exact frame on the grid.
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    // Rested inside the frame that starts at 2.5 s, not on its first instant:
    // the pointer's time comes back from its pixel, and a hair below 2.5 the
    // frame before (2.4667) is the right answer.
    await app.pause();
    await pointer("pointermove", 2.4);
    await pointer("pointermove", 2.52);
    const rested = await shows("the exact frame at 2.52 s", "blue", { exact: true });
    ctx.log(`rested at 2.52 s: ${named(rested.rgb)}, exact, at ${rested.time.toFixed(4)} s`);
    if (Math.abs(rested.time - 2.5) > 1e-6) throw new Error(`the exact frame is at ${rested.time}, not 2.5`);

    // 3. A selection 2-4 s (I at 2, a click on the timeline at 4, O there:
    //    the click moves the playhead and keeps the selection begun); its end
    //    dragged to 4 s shows the frame before it, yellow, at 3.967 and
    //    "0:03"; its start at 2 s, blue, "0:02".
    await app.toStart();
    for (let i = 0; i < 2; i++) await app.key("ArrowRight", { shift: true });
    await until("the player at 2 s", async () => (await app.shownTime()) === 2, 10_000);
    await app.key("i");
    await pointer("pointerdown", 4 + 0.5 / FPS);
    await pointer("pointerup", 4 + 0.5 / FPS);
    await until("the player at 4 s", async () => (await app.shownTime()) === 4, 10_000);
    await app.key("o");
    const selected = await until(
      "the selection 2-4 s",
      async () => {
        const at = await app.js(SELECTION);
        if (at && Math.abs(at[0] - 2) < 0.05 && Math.abs(at[1] - 4) < 0.05) return at;
        throw new Error(`the selection is ${at ? at.map((t) => t.toFixed(2)).join("-") + " s" : "not there"}`);
      },
      10_000,
    );
    ctx.log(`I at 2 s, a click at 4 s, O: the selection ${selected.map((t) => t.toFixed(2)).join("-")} s`);
    const half = 0.5 / FPS;
    await pointer("pointerdown", 4 + half, "mouse", '[data-edge="end"]');
    await pointer("pointermove", 4.4);
    await pointer("pointermove", 4 + half);
    const end = await shows("the end's frame while dragging it", "yellow", { exact: true, tip: "0:03" });
    await pointer("pointerup", 4 + half);
    ctx.log(`the end at 4 s: ${named(end.rgb)} at ${end.time.toFixed(4)} s, tip ${end.tip}`);
    if (Math.abs(end.time - 119 / FPS) > 1e-6) throw new Error(`the end shows ${end.time}, not the frame before 4 s`);
    await pointer("pointerdown", 2 + half, "mouse", '[data-edge="start"]');
    await pointer("pointermove", 1.6);
    await pointer("pointermove", 2 + half);
    const start = await shows("the start's frame while dragging it", "blue", { exact: true, tip: "0:02" });
    await pointer("pointerup", 2 + half);
    ctx.log(`the start at 2 s: ${named(start.rgb)} at ${start.time.toFixed(4)} s, tip ${start.tip}`);
    if (Math.abs(start.time - 2) > 1e-6) throw new Error(`the start shows ${start.time}, not 2 s`);
    await app.key("Escape");

    // 4. Dragging the playhead: it shows while the pointer's down.
    await pointer("pointerdown", 0.5);
    await pointer("pointermove", 1.5);
    const dragged = await shows("the thumbnail while the playhead's dragged", "lime");
    await pointer("pointerup", 1.5);
    ctx.log(`dragging the playhead to 1.5 s: ${named(dragged.rgb)}`);

    // 5. A finger: no thumbnail (nor tip of one).
    await pointer("pointermove", 3.5, "touch");
    await sleep(500);
    if (await thumbnail()) throw new Error("a thumbnail for a finger");
    ctx.log("a finger: no thumbnail");

    // 6. Rested at 4.5 s, then a click there and Play: the decoder that
    //    previewed it plays on, none started, and the screen is magenta.
    await pointer("pointermove", 4.6);
    await pointer("pointermove", 4.5);
    await shows("the exact frame at 4.5 s", "magenta", { exact: true });
    const before = await app.invoke("player_decoders_started");
    await pointer("pointerdown", 4.5);
    await pointer("pointerup", 4.5);
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      10_000,
    );
    const status = await app.invoke("player_pause");
    const after = await app.invoke("player_decoders_started");
    ctx.log(`played from 4.5 s to ${status.shown?.toFixed(3)} s: decoders started ${before}, then ${after}`);
    if (after !== before) throw new Error(`play where the pointer rested started ${after - before} new decoder(s)`);
    if (!(status.shown >= 4.5 - 1e-6)) throw new Error(`the picture is at ${status.shown}, not from 4.5 s on`);
    if (!os.unavailable()) {
      let png;
      try {
        png = os.screenshot();
      } catch (e) {
        ctx.log(`no screenshot here: ${e.message.split("\n")[0]}`);
      }
      if (png) {
        // Paused where it got to: magenta from 4.5 s, cyan if it ran past 5 s first.
        const want = COLOURS[Math.min(COLOURS.length - 1, Math.floor(status.shown + 1e-6))][0];
        const seen = named(os.colourIn(png, os.middle(await os.rectOf(app, ".player .picture"))));
        ctx.log(`the screen shows ${seen} (the frame at ${status.shown.toFixed(3)} s is ${want})`);
        if (seen !== want) throw new Error(`the screen shows ${seen}, not ${want}`);
      }
    }
  },
};
