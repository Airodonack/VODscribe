// Clips made as MOV (Settings): a video's clip is a .mov, a sound-only
// file's a .wav, each a file that's there.

import { existsSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

export default {
  name: "clip-format",
  async run(app, ctx) {
    await app.toLibrary();
    await app.click('button[aria-label="Settings"]');
    await until("Settings", () => app.js(`return !!document.querySelector('[aria-label="Clip format"]')`), MINUTE);
    await app.js(
      `[...document.querySelectorAll('[role="radiogroup"][aria-label="Clip format"] button')].find((b) => b.textContent.trim() === "MOV").click()`,
    );
    await until("MOV chosen", async () => (await app.invoke("get_settings")).clip_format === "mov", 10_000);
    await app.key("Escape");

    const audio = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.pickFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await app.clickText("add video files…");
    const video = await until(
      "the video, done",
      async () => (await app.invoke("list_vods")).find((v) => v.id !== audio.id && v.status === "done"),
      5 * MINUTE,
    );
    for (const [vod, ext] of [
      [video, ".mov"],
      [audio, ".wav"],
    ]) {
      await app.openVod(vod.id);
      // The player is made on the first play; paused once it plays.
      await app.click('.bar button[aria-label="Play"]');
      await until(
        "it to play",
        () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
        MINUTE,
      );
      const clip = await app.clipOneToThree(vod.id);
      if (path.extname(clip.file_path) !== ext || !existsSync(clip.file_path))
        throw new Error(`${vod.title}'s clip is ${clip.file_path}, not a ${ext} that's there`);
      ctx.log(`${vod.title}: ${path.basename(clip.file_path)}`);
    }
  },
};
