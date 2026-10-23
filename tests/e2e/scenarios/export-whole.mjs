// Clip with no cuts: C makes the whole video a clip, start to end. On the
// way, Shift+M mutes and unmutes it, and F shows it full screen, black
// with its caption large.

import { existsSync } from "node:fs";

import { MINUTE, TIMED_POLL, until } from "../lib/app.mjs";

export default {
  name: "export-whole",
  async run(app, ctx) {
    const { fixtures, log } = ctx;
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(vod.id);
    if ((await app.invoke("list_marks", { vodId: vod.id })).length) throw new Error("the new VOD has cuts already");
    if (await app.js(`return document.querySelector("button.action.clip")?.disabled ?? true`))
      throw new Error("Clip is off with no cuts");
    const keycap = await app.js(`return document.querySelector("button.action.clip kbd")?.textContent.trim()`);
    if (keycap !== "C") throw new Error(`the Clip button says its key is ${keycap}`);
    const muted = () => app.js(`return !!document.querySelector('.bar button[aria-label="Unmute"]')`);
    for (const want of [true, false]) {
      await app.key("M", { shift: true });
      await until(want ? "muted (Shift+M)" : "unmuted (Shift+M)", async () => (await muted()) === want, 10_000);
    }
    // Sound alone, F: full screen anyway, black with its caption (a second
    // in, in the speech) large in the middle.
    await app.toStart();
    await app.key("ArrowRight", { shift: true });
    await app.key("f");
    const big = await until(
      "the caption large, full screen",
      () =>
        app.js(`const c = document.querySelector("section.fullscreen .caption.under");
                return c && parseFloat(getComputedStyle(c).fontSize) >= 24 && c.textContent.trim()`),
      10_000,
    );
    await ctx.checkpoint("sound-only-fullscreen");
    await app.key("Escape");
    await until("full screen left", () => app.js(`return !document.querySelector("section.fullscreen")`), 10_000);
    log(`full screen with sound alone: "${big}"`);
    // Timed: from C to the clip's file made.
    const clip = await ctx.time("whole video's clip made (5 s)", async () => {
      await app.key("c");
      return until(
        "the whole video's clip",
        async () => {
          const [c] = await app.invoke("list_clips", { vodId: vod.id });
          if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
          return c?.status === "ready" && c;
        },
        MINUTE,
        { every: TIMED_POLL },
      );
    });
    if (clip.start > 0.05 || Math.abs(clip.end - vod.duration) > 0.1)
      throw new Error(`the clip runs ${clip.start} to ${clip.end}, not the whole ${vod.duration} s`);
    if (!existsSync(clip.file_path)) throw new Error(`no file at ${clip.file_path}`);
    log(`the whole video, 0 to ${clip.end.toFixed(2)} s, as ${clip.file_path}`);
  },
};
