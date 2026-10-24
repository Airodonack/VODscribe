// The timeline's thumbnail (ScrubFrame.svelte) over an edit's piece of a
// VOD from a site, its footage a section downloaded into the footage
// folder (footage.rs): the section has its own storyboard, as a file added
// has, and hovering the piece shows its frames. Offline: the VOD is a file
// added (colours.mp4, one flat colour a second, a keyframe each), its
// library entry then made a YouTube VOD's with the file moved into the
// footage folder as the section of it an edit's piece needs, as the
// footage queue leaves one.
//
// Each look at the edit's footage marks the section used, for tidying
// (`footage::touch`); that mustn't count as the file changing, or its
// storyboard is thrown away and made again at every look, and none shows
// (0.6.0). First the frames are asked for as the timeline asks, now and
// then, each ask after the two seconds an edit's timeline is kept for its
// thumbnails, so each finds its footage afresh; then the pointer moves to
// and fro over the piece, never resting (the edit's player is open, and
// rested, the player's exact frame would show instead).

import { copyFileSync, mkdirSync, renameSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import { COLOURS, colours } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** How far a colour may be from the one meant: JPEG and YUV rounding, scaling. */
const NEAR = 90;
/** The piece: 1-9 s of the VOD. */
const PIECE = [1, 9];

/** The colour named for `rgb`, or "none (…)". */
function named(rgb) {
  const [name, , d] = COLOURS.map(([n, c]) => [n, c, os.distance(rgb, c)]).sort((a, b) => a[2] - b[2])[0];
  return d < NEAR ? name : `none (${rgb.join(", ")})`;
}

/** In the page: the mouse moved over the edit's timeline to `time` (the edit's). */
const POINTER = `
  const bar = document.querySelector("section.edit-view .scrubber");
  const box = bar.getBoundingClientRect();
  const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
  window.dispatchEvent(new PointerEvent("pointermove", { clientX: x, clientY: box.top + box.height / 2, bubbles: true,
    pointerId: 1, pointerType: "mouse", isPrimary: true }));`;

/** In the page: what the thumbnail shows (null: none), its colour read at its middle. */
const THUMBNAIL = `
  const canvas = document.querySelector("section.edit-view .timeline .scrub-frame:not(.empty)");
  if (!canvas || !canvas.width) return null;
  const [r, g, b] = canvas.getContext("2d").getImageData(canvas.width >> 1, canvas.height >> 1, 1, 1).data;
  return { time: Number(canvas.dataset.time), exact: canvas.dataset.exact === "true", rgb: [r, g, b] };`;

/** In the page: the storyboard frame of edit `arguments[0]` at `arguments[1]` s, as the timeline asks for it (null: none). */
const ASK = `
  const b = await window.__TAURI_INTERNALS__.invoke("storyboard_frame",
    { doc: { kind: "edit", id: arguments[0] }, time: arguments[1], within: 0.1 });
  if (b.byteLength < 8) return null;
  const image = await createImageBitmap(new Blob([new Uint8Array(b, 8)], { type: "image/jpeg" }));
  const canvas = new OffscreenCanvas(image.width, image.height);
  const ctx = canvas.getContext("2d");
  ctx.drawImage(image, 0, 0);
  return { time: new DataView(b, 0, 8).getFloat64(0, true),
           rgb: [...ctx.getImageData(image.width >> 1, image.height >> 1, 1, 1).data.slice(0, 3)] };`;

export default {
  name: "edit-footage-thumbnail",
  async run(app, ctx) {
    await app.toLibrary();
    const file = colours(path.join(ctx.out, "fixtures"));
    // A copy of its own, moved into the footage folder below.
    const own = path.join(ctx.out, "edit-footage", "colours.mp4");
    mkdirSync(path.dirname(own), { recursive: true });
    copyFileSync(file, own);
    const { vods } = await app.invoke("add_files", { paths: [own] });
    const id = vods[0]?.id;
    if (!id) throw new Error("the file wasn't added");
    const vod = await until(
      "its length known",
      async () => (await app.invoke("list_vods")).find((v) => v.id === id && v.duration > 10),
      MINUTE,
    );
    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", {
      editId: edit.id,
      vodId: id,
      start: PIECE[0],
      end: PIECE[1],
      title: "From the site",
    });

    // The entry made a YouTube VOD's (its chat known to have none, so
    // nothing is fetched), the file the section of it the piece needs.
    await ctx.quit();
    const [roaming] = dataDirs();
    const footage = path.join(dataDirs().at(-1), "footage");
    mkdirSync(footage, { recursive: true });
    const section = path.join(footage, `${id}-0-${Math.round(vod.duration * 1000)}.mp4`);
    renameSync(own, section);
    const db = new DatabaseSync(path.join(roaming, "library.sqlite"));
    db.prepare(
      `UPDATE vods SET platform = 'youtube', platform_id = 'IaIo3d6cUuo', url = 'https://www.youtube.com/watch?v=IaIo3d6cUuo',
         title = 'A VOD from a site', source_path = NULL, chat = 'none' WHERE id = ?`,
    ).run(id);
    db.close();
    app = await ctx.start();
    await app.toLibrary();
    await app.click(`.vod.edit[data-key="-${edit.id}"] button.main`);
    await until(
      "the edit open",
      () => app.js(`return !!document.querySelector("section.edit-view .scrubber")`),
      MINUTE,
    );
    const pieces = (await app.invoke("get_edit", { id: edit.id })).pieces;
    if (pieces.some((p) => p.hole)) throw new Error("the piece of the section is a hole");

    // 1. Asked now and then, as the timeline asks at each move: 1.4 s and
    //    1.6 s of the edit (2.4 s and 2.6 s of the VOD) in turn, the
    //    storyboard's keyframe at 2 s of the VOD, blue, shown at 1 s of the
    //    edit.
    const tries = [];
    let asked = null;
    for (let i = 0; i < 16 && !asked; i++) {
      await sleep(2500);
      const got = await app.js(ASK, edit.id, i % 2 ? 1.6 : 1.4);
      tries.push(got ? `${named(got.rgb)} at ${got.time}` : "none");
      if (got) asked = got;
    }
    if (!asked) throw new Error(`the section's storyboard gave no frame, asked ${tries.length} times`);
    ctx.log(`asked now and then: ${named(asked.rgb)} at ${asked.time} s, the answer to ask ${tries.length}`);
    if (named(asked.rgb) !== "blue") throw new Error(`it gave ${named(asked.rgb)}, not blue`);
    if (Math.abs(asked.time - 1) > 1e-3) throw new Error(`it gave the frame at ${asked.time} s of the edit, not 1 s`);

    // 2. On the timeline, the pointer moving to and fro there (never resting,
    //    so the player's exact frame isn't asked for): the same frame.
    const moved = await until(
      "the storyboard's frame while the pointer moves",
      async () => {
        for (let i = 0; i < 10; i++) {
          await app.js(POINTER, i % 2 ? 1.45 : 1.4);
          await sleep(40);
        }
        const t = await app.js(THUMBNAIL);
        if (!t) throw new Error("no thumbnail");
        if (t.exact) throw new Error("the exact frame, the pointer moving");
        return t;
      },
      30_000,
      { every: 100 },
    );
    ctx.log(`moving over the section's piece: ${named(moved.rgb)} at ${moved.time} s`);
    if (named(moved.rgb) !== "blue") throw new Error(`it shows ${named(moved.rgb)}, not blue`);
  },
};
