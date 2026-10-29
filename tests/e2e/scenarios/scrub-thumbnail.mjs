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
// start the frame there, its end the frame before. The thumbnail is
// centred on the pointer, the time tip across its lower edge, both inside
// the timeline's bar, shifted in whole at its ends rather than cut. Dragging the
// playhead shows it; a finger doesn't. Rested at a moment and played from
// there, the player takes the decoder that previewed it over: no new one
// starts, and the screen shows the colour from there on.
//
// A file with a keyframe only every few seconds (as a screen recording
// has) shows, anywhere between two, the keyframe at or before the pointer,
// however far back: never nothing, the tip over it never moving back up
// to the bar's top. A file with no picture shows the tip alone.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { COLOURS, GOP, LANDED, colours, longGop } from "../lib/fixtures.mjs";
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
  const tip = document.querySelector(".timeline .tip");
  return { time: Number(canvas.dataset.time), exact: canvas.dataset.exact === "true", rgb: [r, g, b],
           tip: tip?.textContent.trim() ?? null, over: !!tip?.classList.contains("over") };`;

/** In the page: where the pointer at `time` is, and the boxes of the timeline's bar, the thumbnail and the tip. */
const PLACED = `
  const bar = document.querySelector(".scrubber");
  const box = bar.getBoundingClientRect();
  const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
  const rect = (el) => {
    if (!el) return null;
    const { left, right, top, bottom, width } = el.getBoundingClientRect();
    return { left, right, top, bottom, width };
  };
  return { x, bar: rect(bar), frame: rect(document.querySelector(".timeline .scrub-frame:not(.none)")),
           tip: rect(document.querySelector(".timeline .tip")) };`;

/** In the page: the pointer moved over the timeline to `time` (arguments
 *  [0]), then the first thumbnail it shows (as THUMBNAIL reads it) that is
 *  the frame at `want` s ([1]; any frame, null), exact or not as [2] says,
 *  or after [3] ms none, with the last one seen. Looked at on each change
 *  to the timeline as it's made, not polled from outside: the storyboard's
 *  frame draws within a few tens of milliseconds and the exact frame
 *  replaces it once the pointer has rested, both between two looks over
 *  WebDriver. Not looked at before the move's changes land, so the frame
 *  shown for the last moment can't count for this one. Only the first match
 *  counts: a frame asked for a moment the pointer has left can show for an
 *  instant on the way. */
const HOVER = `
  const [time, want, exact, ms] = arguments;
  const move = function () {${POINTER}
  };
  const read = () => {${THUMBNAIL}
  };
  return new Promise((resolve) => {
    let last = null;
    const done = (seen) => {
      watch.disconnect();
      clearTimeout(timer);
      resolve(seen);
    };
    const check = () => {
      const t = read();
      if (!t) return;
      last = t;
      if (want === null || (Math.abs(t.time - want) < 1e-3 && t.exact === exact)) done({ ...t, found: true });
    };
    const watch = new MutationObserver(check);
    watch.observe(document.querySelector(".timeline"), { subtree: true, childList: true, attributes: true });
    const timer = setTimeout(() => done({ found: false, last }), ms);
    move("pointermove", time);
  });`;

/** In the page: where the tip is, and whether it's over a thumbnail (or its box). */
const TIP = `
  const tip = document.querySelector(".timeline .tip");
  return tip && { text: tip.textContent.trim(), over: tip.classList.contains("over"),
                  box: !!document.querySelector(".timeline .scrub-frame:not(.none)") };`;

export default {
  name: "scrub-thumbnail",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "thumbnail-at-the-end":
      "Inside the timeline, centred on the line where the pointer is (the selection's right end), a small slightly see-through thumbnail of the video shows a flat yellow frame, with a dark time tip reading 0:03 centred across its bottom edge; both sit within the timeline's bar, not over the video picture above it and not off to one side of the pointer, and a stretch from about 0:02 to 0:04 is marked as selected.",
    "light-middle":
      "Light theme. In the middle of the timeline, a small slightly see-through thumbnail of the video (a flat-coloured frame) sits centred on the thin hover line, inside the timeline's bar, with a dark time tip reading 0:10 centred across its bottom edge; neither is beside the line or over the video picture above the timeline.",
    "light-end":
      "Light theme. At the right end of the timeline, a small slightly see-through thumbnail of the video (a flat-coloured frame) sits inside the timeline's bar with its right edge at the bar's right end, whole, not cut off, with a dark time tip reading 0:19 centred across its bottom edge.",
    "dark-middle":
      "Dark theme. In the middle of the timeline, a small slightly see-through thumbnail of the video (a flat-coloured frame) sits centred on the thin hover line, inside the timeline's bar, with a light time tip reading 0:10 centred across its bottom edge, readable against the dark page.",
    "dark-end":
      "Dark theme. At the right end of the timeline, a small slightly see-through thumbnail of the video (a flat-coloured frame) sits inside the timeline's bar with its right edge at the bar's right end, whole, not cut off, with a light time tip reading 0:19 centred across its bottom edge.",
  },
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
    // The thumbnail centred on the pointer at `time` (within 2 px), shifted
    // in whole at the bar's ends, inside the bar; the tip centred on it,
    // across its lower edge, inside the bar too. Says whether it was shifted.
    const placed = async (what, time) => {
      const { x, bar, frame, tip } = await app.js(PLACED, time);
      const at = `${what}: the pointer at ${x.toFixed(1)}, the bar ${JSON.stringify(bar)}, the thumbnail ${JSON.stringify(frame)}, the tip ${JSON.stringify(tip)}`;
      if (!frame || !tip) throw new Error(`${at}: no thumbnail or tip`);
      const inside = (r) =>
        r.left >= bar.left - 0.5 &&
        r.right <= bar.right + 0.5 &&
        r.top >= bar.top - 0.5 &&
        r.bottom <= bar.bottom + 0.5;
      if (!inside(frame)) throw new Error(`${at}: the thumbnail passes the bar`);
      if (!inside(tip)) throw new Error(`${at}: the tip passes the bar`);
      const half = Math.max(frame.width, tip.width) / 2;
      const want = Math.min(Math.max(x, bar.left + half), bar.right - half);
      const middle = (frame.left + frame.right) / 2;
      if (Math.abs(middle - want) > 2)
        throw new Error(`${at}: the thumbnail's middle is ${middle.toFixed(1)}, not ${want.toFixed(1)}`);
      if (Math.abs((tip.left + tip.right) / 2 - middle) > 1)
        throw new Error(`${at}: the tip isn't centred on the thumbnail`);
      if (!(tip.top < frame.bottom && tip.bottom >= frame.bottom))
        throw new Error(`${at}: the tip isn't across the thumbnail's lower edge`);
      return Math.abs(want - x) > 0.5;
    };

    // 2. Hovering 2.5 s: first the storyboard's frame, the keyframe at 2 s,
    //    blue. The player is open (it opens with the file); the exact frame
    //    waits for the pointer to rest.
    await app.openVod(vod.id);
    const hovered = await app.js(HOVER, 2.5, null, false, 15_000);
    if (!hovered.found) throw new Error("hovering 2.5 s, no thumbnail");
    ctx.log(`hovering 2.5 s: first ${named(hovered.rgb)} from ${hovered.time} s (exact: ${hovered.exact})`);
    if (hovered.exact)
      throw new Error(`hovering 2.5 s, the exact frame (${hovered.time} s) came before the storyboard's`);
    if (named(hovered.rgb) !== "blue" || Math.abs(hovered.time - 2) > 1e-6)
      throw new Error(`hovering 2.5 s, first ${named(hovered.rgb)} at ${hovered.time} s, not the keyframe at 2 s`);

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
    await placed("dragging the end", 4 + half);
    await ctx.checkpoint("thumbnail-at-the-end");
    await pointer("pointerup", 4 + half);
    ctx.log(`the end at 4 s: ${named(end.rgb)} at ${end.time.toFixed(4)} s, tip ${end.tip}`);
    if (Math.abs(end.time - 119 / FPS) > 1e-6) throw new Error(`the end shows ${end.time}, not the frame before 4 s`);
    await pointer("pointerdown", 2 + half, "mouse", '[data-edge="start"]');
    await pointer("pointermove", 1.6);
    await pointer("pointermove", 2 + half);
    const start = await shows("the start's frame while dragging it", "blue", { exact: true, tip: "0:02" });
    await placed("dragging the start", 2 + half);
    await pointer("pointerup", 2 + half);
    ctx.log(`the start at 2 s: ${named(start.rgb)} at ${start.time.toFixed(4)} s, tip ${start.tip}`);
    if (Math.abs(start.time - 2) > 1e-6) throw new Error(`the start shows ${start.time}, not 2 s`);
    await app.key("Escape");

    // 4. Dragging the playhead: it shows while the pointer's down.
    await pointer("pointerdown", 0.5);
    await pointer("pointermove", 1.5);
    const dragged = await shows("the thumbnail while the playhead's dragged", "lime");
    await placed("dragging the playhead", 1.5);
    await pointer("pointerup", 1.5);
    ctx.log(`dragging the playhead to 1.5 s: ${named(dragged.rgb)}`);

    // 5. A finger: no thumbnail (nor tip of one).
    await pointer("pointermove", 3.5, "touch");
    await sleep(500);
    if (await thumbnail()) throw new Error("a thumbnail for a finger");
    ctx.log("a finger: no thumbnail");

    // 6. Rested at 4.5 s, then a click there and Play: the decoder that
    //    previewed it plays on, none started, and the screen is magenta.
    //    Inside the frame, half a frame on, as step 3's edges are: a WebKit
    //    that drops the pointer's fraction of a pixel (Ubuntu 22.04's) loses
    //    at most 0.0145 s, where on 4.5 itself a pixel short is the frame
    //    before.
    await pointer("pointermove", 4.6);
    await pointer("pointermove", 4.5 + half);
    await shows("the exact frame at 4.5 s", "magenta", { exact: true });
    const before = await app.invoke("player_decoders_started");
    await pointer("pointerdown", 4.5 + half);
    await pointer("pointerup", 4.5 + half);
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

    // 7. A keyframe only every five seconds: hovering anywhere shows first
    //    the keyframe at or before the pointer (the one its picture there is
    //    made from), however far back, centred on the pointer (shifted in
    //    whole near the ends), and the tip stays over its lower edge; the
    //    exact frame follows once the pointer rests. First each keyframe's
    //    own moment, so the storyboard is all made; then across the whole
    //    timeline.
    const gop = longGop(path.join(ctx.out, "fixtures"));
    await app.pickFiles([gop]);
    await app.clickText("add video files…");
    const long = await until(
      "the long-GOP video, with its picture",
      async () => {
        const v = (await app.invoke("list_vods")).find((v) => v.source_path === gop);
        if (v?.status === "error") throw new Error(v.error);
        return v?.thumb && v;
      },
      5 * MINUTE,
    );
    await app.openVod(long.id);
    const keyframeOf = (t) => Math.floor(t / GOP) * GOP;
    const colourAt = (t) => COLOURS[Math.round(t) % COLOURS.length][0];
    for (let k = 0; k < 20; k += GOP) {
      await pointer("pointermove", k + 0.25);
      await shows(`the keyframe at ${k} s`, colourAt(k));
    }
    const missed = [];
    let shifted = 0;
    for (let t = 0.25; t < 20; t += 0.25) {
      const want = keyframeOf(t);
      const seen = await app.js(HOVER, t, want, false, 2_000);
      if (!seen.found)
        missed.push(
          `${t} s (${seen.last ? `the ${seen.last.exact ? "exact " : ""}frame at ${seen.last.time} s` : "no thumbnail"})`,
        );
      else if (named(seen.rgb) !== colourAt(want) || !seen.over)
        missed.push(`${t} s (${named(seen.rgb)}, the tip ${seen.over ? "over it" : "alone"})`);
      else if (await placed(`hovering ${t} s`, t)) shifted++;
    }
    if (missed.length) throw new Error(`hovering, no keyframe at or before the pointer at ${missed.join(", ")}`);
    // The ends' moments are within the thumbnail's half-width of the bar's ends.
    if (!shifted) throw new Error("hovering near the timeline's ends, the thumbnail was never shifted in");
    ctx.log(
      `hovering 0.25-19.75 s every 0.25 s: each shows the keyframe at or before it, ${GOP} s apart, centred on the pointer (${shifted} shifted in at the ends), the tip across its lower edge`,
    );
    // The hover elements at the start, the middle and the end, in each theme.
    for (const theme of ["Light", "Dark"]) {
      await app.openSettings("Appearance");
      await until("Settings", () => app.js(`return !!document.querySelector('[aria-label="Theme"]')`), MINUTE);
      await app.js(
        `[...document.querySelectorAll('[role="radiogroup"][aria-label="Theme"] button')].find((b) => b.textContent.trim() === arguments[0]).click()`,
        theme,
      );
      await app.key("Escape");
      await until(
        `the ${theme.toLowerCase()} theme`,
        async () => (await app.invoke("get_settings")).theme === theme.toLowerCase(),
        10_000,
      );
      for (const [where, t] of [
        ["start", 0.3],
        ["middle", 10.3],
        ["end", 19.7],
      ]) {
        await pointer("pointermove", t);
        await shows(`the keyframe at ${keyframeOf(t)} s`, colourAt(keyframeOf(t)));
        await placed(`hovering ${t} s, ${theme.toLowerCase()}`, t);
        await ctx.checkpoint(`${theme.toLowerCase()}-${where}`);
      }
    }

    // 8. A file with no picture: the tip alone, centred on the line.
    await app.pickFiles([LANDED]);
    await app.clickText("add video files…");
    const sound = await until(
      "the sound file",
      async () => (await app.invoke("list_vods")).find((v) => v.source_path === LANDED),
      MINUTE,
    );
    await app.openVod(sound.id);
    await pointer("pointermove", 2.5);
    await sleep(500);
    const alone = await app.js(TIP);
    ctx.log(`a sound file: the tip ${alone?.text}, ${alone?.over ? "over a box" : "alone"}`);
    if (!alone || alone.over || alone.box) throw new Error(`a sound file's tip: ${JSON.stringify(alone)}`);
    await ctx.checkpoint("no-picture");
  },
};
