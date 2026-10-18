// Clip with no cuts: E makes the whole video a clip, start to end.

import { existsSync } from "node:fs";

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "export-whole",
  async run(app, { fixtures, log }) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(vod.id);
    if ((await app.invoke("list_marks", { vodId: vod.id })).length) throw new Error("the new VOD has cuts already");
    if (await app.js(`return document.querySelector("button.action.clip")?.disabled ?? true`))
      throw new Error("Clip is off with no cuts");
    await app.key("e");
    const clip = await until(
      "the whole video's clip",
      async () => {
        const [c] = await app.invoke("list_clips", { vodId: vod.id });
        if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
        return c?.status === "ready" && c;
      },
      MINUTE,
    );
    if (clip.start > 0.05 || Math.abs(clip.end - vod.duration) > 0.1)
      throw new Error(`the clip runs ${clip.start} to ${clip.end}, not the whole ${vod.duration} s`);
    if (!existsSync(clip.file_path)) throw new Error(`no file at ${clip.file_path}`);
    log(`the whole video, 0 to ${clip.end.toFixed(2)} s, as ${clip.file_path}`);
  },
};
