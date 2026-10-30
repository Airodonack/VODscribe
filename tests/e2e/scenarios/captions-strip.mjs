// A site's player can't be drawn over, so its captions show in a strip
// under it (Player.svelte); the strip's room is kept from the moment the
// VOD opens, with captions on, so nothing below moves on Play. A file's
// captions are drawn over its own picture: no strip, before Play or after.
// The VOD is a file transcribed here, then made the test YouTube VOD's and
// Kick's library entry: no network needed, as their player doesn't have to
// play (offline, YouTube's frame is taken for ready after its 10 s wait;
// Kick's stream is never found).

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { makeSiteVod } from "../lib/data.mjs";
import { videos } from "../lib/fixtures.mjs";

/** The strip (its height, null if there's none) and where the scrubber's top is. */
const layout = (app) =>
  app.js(`return {
    strip: document.querySelector(".caption-strip")?.getBoundingClientRect().height ?? null,
    scrubber: document.querySelector(".timeline")?.getBoundingClientRect().top ?? null }`);

export default {
  name: "captions-strip",
  async run(app, ctx) {
    if (await app.invoke("embed_takeover"))
      ctx.skip("this build draws a site's VOD's picture itself, and its captions over it (site-embed)");
    await app.toLibrary();
    const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    const vod = await app.transcribe(added.id);

    // A file: its captions over its picture, never a strip.
    await app.openVod(vod.id);
    if ((await layout(app)).strip !== null) throw new Error("a file shows a caption strip before Play");
    await app.plays();
    if ((await layout(app)).strip !== null) throw new Error("a file shows a caption strip while it plays");
    await app.pause();

    // YouTube: the strip from the start, the scrubber where it stays.
    await ctx.quit();
    makeSiteVod(vod.id, "youtube", "IaIo3d6cUuo");
    app = await ctx.start();
    await app.toLibrary();
    await app.openVod(vod.id);
    const opened = await until(
      "the scrubber",
      async () => (await layout(app)).scrubber !== null && layout(app),
      MINUTE,
    );
    if (opened.strip === null) throw new Error("with captions on, a site's VOD opens with no caption strip");
    ctx.log(`opened: a strip ${opened.strip} px high, the scrubber at ${opened.scrubber} px`);

    // Captions off: no room kept; back on, it's back.
    await app.key("C", { shift: true });
    const off = await until("the strip gone", async () => (await layout(app)).strip === null && layout(app), 10_000);
    if (!(off.scrubber < opened.scrubber))
      throw new Error(`with captions off the scrubber is at ${off.scrubber} px, not above ${opened.scrubber} px`);
    await app.key("C", { shift: true });
    await until("the strip back", async () => (await layout(app)).strip !== null, 10_000);

    // Play: their player made (its volume on the bar), nothing moved.
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "their player made",
      () =>
        app.js(`return !!document.querySelector('.bar button[aria-label="Mute"], .bar button[aria-label="Unmute"]')`),
      MINUTE,
    );
    await sleep(1000);
    const played = await layout(app);
    if (played.strip !== opened.strip || played.scrubber !== opened.scrubber)
      throw new Error(
        `on Play the strip went from ${opened.strip} to ${played.strip} px, the scrubber from ${opened.scrubber} to ${played.scrubber} px`,
      );
    ctx.log("played: the strip and the scrubber where they were");

    // Kick: its own <video>, under the app's controls, with the strip as well.
    await ctx.quit();
    makeSiteVod(vod.id, "kick", "e2e-captions-strip");
    app = await ctx.start();
    await app.toLibrary();
    await app.openVod(vod.id);
    const kick = await until("the scrubber", async () => (await layout(app)).scrubber !== null && layout(app), MINUTE);
    if (kick.strip !== opened.strip)
      throw new Error(`a Kick VOD opens with a strip ${kick.strip} px high, YouTube's ${opened.strip} px`);
  },
};
