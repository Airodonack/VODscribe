// A clip: cuts at 1 and 3 s (B), then Clip with the playhead between them
// (App.clipOneToThree); resized, it's made again as a new file and the one
// before goes (it was never dragged out); then deleting it, and undoing
// that and the resize. Last, its file deleted outside the app (as in a file
// manager): back in the window, its pill goes; undone, the clip is made
// again, and the chapter added before that goes only with the next undo.

import { existsSync, rmSync } from "node:fs";

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "clip",
  async run(app, { fixtures, checkpoint }) {
    await app.toLibrary();
    const done = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(done.id);
    const clip = await app.clipOneToThree(done.id);
    // The chip follows a moment after the clip is recorded as made.
    await until("the chip to show it made", () => app.js(`return !!document.querySelector(".chip.ready")`), 10_000);
    await checkpoint("clip-bar");

    // Resized to 1 to 4 s: a new file, named for its new range; the old one goes.
    const first = clip.file_path;
    await app.invoke("set_clip_range", { id: clip.id, start: 1, end: 4 });
    const moved = await until(
      "the clip made again",
      async () => {
        const [c] = await app.invoke("list_clips", { vodId: done.id });
        return c?.status === "ready" && c.file_path !== first && c;
      },
      MINUTE,
    );
    if (!existsSync(moved.file_path)) throw new Error(`the resized clip says ${moved.file_path}, which isn't there`);
    await until(
      "the old range's file gone",
      () => {
        if (!existsSync(first)) return true;
        throw new Error(`${first} is still there`);
      },
      10_000,
    );
    await until("the chip to show it made", () => app.js(`return !!document.querySelector(".chip.ready")`), 10_000);
    await app.click('.chip.ready button[aria-label="Delete clip"]');
    await until(
      "the clip to be deleted",
      async () => (await app.invoke("list_clips", { vodId: done.id })).length === 0,
      MINUTE,
    );
    app.log(`clipped "${clip.title}" to ${clip.file_path}, and deleted it`);

    // Undone (history::restore_bar): the clip comes back as it was, 1 to 4 s
    // with its title, and is made again; undone once more, so is the resize.
    const undo = () =>
      app.js(`document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`);
    const madeAt = (start, end) =>
      until(
        `the clip back at ${start}-${end} s, made`,
        async () => {
          const all = await app.invoke("list_clips", { vodId: done.id });
          const [c] = all;
          if (all.length === 1 && c.start === start && c.end === end && c.status === "ready" && existsSync(c.file_path))
            return c;
          throw new Error(`clips: ${JSON.stringify(all.map((c) => [c.start, c.end, c.status]))}`);
        },
        MINUTE,
      );
    await undo();
    const back = await madeAt(1, 4);
    if (back.title !== clip.title) throw new Error(`it came back as "${back.title}", not "${clip.title}"`);
    await undo();
    const made = await madeAt(1, 3);
    app.log("deleting it and resizing it undone: back as it was, and made again");

    await until("its pill", () => app.js(`return !!document.querySelector(".chip.ready")`), 10_000);
    // A chapter added first, to undo once the clip has gone with its file.
    await app.invoke("add_mark", { vodId: done.id, time: 2, kind: "chapter", text: "Landing" });
    rmSync(made.file_path);
    await app.js(`window.dispatchEvent(new Event("focus"))`);
    await until(
      "the clip gone with its file",
      async () => (await app.invoke("list_clips", { vodId: done.id })).length === 0,
      10_000,
    );
    await until("its pill gone", () => app.js(`return !document.querySelector(".chip")`), 10_000);
    app.log("its file deleted outside the app: the pill went");

    // Undo takes back one change at a time: first the file's deletion (the
    // clip made again, the chapter still there), then the chapter.
    const chapters = async () =>
      (await app.invoke("list_marks", { vodId: done.id })).filter((m) => m.kind === "chapter").length;
    await undo();
    await madeAt(1, 3);
    if ((await chapters()) !== 1) throw new Error("undoing the file's deletion took the chapter away too");
    await undo();
    await until("the chapter undone", async () => (await chapters()) === 0, 10_000);
    if ((await app.invoke("list_clips", { vodId: done.id })).length !== 1)
      throw new Error("undoing the chapter changed the clip bar too");
    app.log("undone: the file's deletion (the clip made again), then the chapter, one at a time");
  },
};
