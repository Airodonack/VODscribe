// A clip: cuts at 1 and 3 s (B), then Clip with the playhead between them
// (App.clipOneToThree); resized, it's made again as a new file and the one
// before goes (it was never dragged out); then deleting it.

import { existsSync } from "node:fs";

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
  },
};
