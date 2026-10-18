// A clip: cuts at 1 and 3 s (B), then Export with the playhead between
// them (App.clipOneToThree); then deleting it.

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "clip",
  async run(app, { fixtures, checkpoint }) {
    await app.toLibrary();
    const done = await app.addAndTranscribe(fixtures.landed);
    await app.playFromSearch("landed");
    const clip = await app.clipOneToThree(done.id);
    // The chip follows a moment after the clip is recorded as made.
    await until("the chip to show it made", () => app.js(`return !!document.querySelector(".chip.ready")`), 10_000);
    await checkpoint("clip-bar");
    await app.click('.chip.ready button[aria-label="Delete clip"]');
    await until(
      "the clip to be deleted",
      async () => (await app.invoke("list_clips", { vodId: done.id })).length === 0,
      MINUTE,
    );
    app.log(`clipped "${clip.title}" to ${clip.file_path}, and deleted it`);
  },
};
