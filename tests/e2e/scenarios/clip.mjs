// A clip: cuts at 0.5 and 2.5 s (B), then Export with the playhead between
// them; then deleting it.

import { MINUTE, until } from "../lib/app.mjs";
import { setTimeout as sleep } from "node:timers/promises";

export default {
  name: "clip",
  async run(app, { fixtures }) {
    await app.toLibrary();
    const done = await app.addAndTranscribe(fixtures.landed);
    await app.type('input[placeholder="Search for a phrase…"]', "landed");
    await until("a search result", () => app.js(`return !!document.querySelector("button.row")`), MINUTE);
    await app.click("button.row");
    await until("the player", () => app.js(`return !!document.querySelector(".player video")`), MINUTE);

    const seekTo = async (t) => {
      await app.js(`document.querySelector(".player video").currentTime = arguments[0]`, t);
      await sleep(300);
    };
    await app.js(`document.querySelector(".player video").pause()`);
    await seekTo(0.5);
    await app.key("b");
    await seekTo(2.5);
    await app.key("b");
    await until("two marks", async () => (await app.invoke("list_marks", { vodId: done.id })).length === 2, 10_000);
    await seekTo(1);
    await app.click("button.export");
    const clip = await until(
      "the clip's file",
      async () => {
        const [c] = await app.invoke("list_clips", { vodId: done.id });
        if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
        return c?.status === "ready" && c;
      },
      MINUTE,
    );
    if (Math.abs(clip.start - 0.5) > 0.05 || Math.abs(clip.end - 2.5) > 0.05)
      throw new Error(`the clip runs ${clip.start} to ${clip.end}, not 0.5 to 2.5`);
    // The chip follows a moment after the clip is recorded as made.
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
