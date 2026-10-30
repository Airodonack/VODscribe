// The sites (needs the network, so only with E2E_NETWORK=1, and not in CI):
// the test video on Twitch, Kick and YouTube (the project's own uploads,
// 70 s), each added by its link, then transcribed, its chat replay fetched where
// the site has one, and played in the app. YouTube may ask to confirm
// "you're not a bot": that's YouTube's block on the computer's address, not
// the app's fault, so it's said, not failed (the app never signs in).
// Playing, the bar has each site's sound (and speed, but on Twitch, whose
// player has none), and the captions show in a strip under the player
// where it doesn't draw them itself (always on Kick; on YouTube and Twitch
// unless the build takes their picture over: site-embed.mjs).

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const SITES = [
  ["twitch", "https://www.twitch.tv/videos/2901646294"],
  ["kick", "https://kick.com/airodonack/videos/01a18dfb-c590-7069-b36a-97fe9266ffde"],
  ["youtube", "https://www.youtube.com/watch?v=IaIo3d6cUuo"],
];

export default {
  name: "platforms",
  needs: ["network"],
  allowErrors: true,
  async run(app, ctx) {
    await app.toLibrary();
    const failures = [];
    // Where each VOD's playhead was left, to find it there when opened again.
    const left = [];
    for (const [site, url] of SITES) {
      try {
        await app.type('input[aria-label="VOD link"]', url);
        await app.submit('input[aria-label="VOD link"]');
        const transcribed = async (label, asked) =>
          until(
            `the ${site} VOD, ${label}`,
            async () => {
              const error = await app.js(`return document.querySelector(".add-error")?.textContent.trim() ?? ""`);
              if (error) return { error };
              const v = (await app.invoke("list_vods")).find((v) => v.platform === site);
              if (v?.status === "error") return { error: v.error };
              return (asked ? v?.status === "done" : !!v) && v;
            },
            15 * MINUTE,
          );
        // Added, it isn't transcribed: that's asked for.
        let vod = await transcribed("added", false);
        if (!vod.error) {
          await app.invoke("transcribe_stretch", { vodId: vod.id, start: 0, end: null });
          vod = await transcribed("transcribed", true);
        }
        if (vod.error) {
          if (site === "youtube" && /not a bot|sign in/i.test(vod.error)) {
            ctx.log(`youtube: blocked by YouTube's bot check (not the app's doing): ${vod.error.slice(0, 120)}`);
            continue;
          }
          throw new Error(vod.error);
        }
        const lines = await app.invoke("transcript", { vodId: vod.id });
        if (lines.length < 3) throw new Error(`the transcript has ${lines.length} lines, for 70 s of speech`);
        const chat = await until(
          `${site}'s chat`,
          async () => {
            const c = (await app.invoke("list_vods")).find((v) => v.id === vod.id).chat;
            return c !== "pending" && c;
          },
          5 * MINUTE,
        );
        if (chat === "failed") throw new Error("its chat replay failed");

        // Played in the app: in the site's own player (with the app's
        // picture under it where the build takes it over: site-embed.mjs).
        await app.openVod(vod.id);
        await until(
          "the Play button",
          () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
          MINUTE,
        );
        // An embedded player may start only on a real click (a page's own
        // click isn't trusted): the OS's where it can, else the page's.
        if (!os.unavailable()) {
          os.activate();
          const play = await os.rectOf(app, '.bar button[aria-label="Play"]');
          os.click(play.x + play.w / 2, play.y + play.h / 2, play.scale);
        } else {
          await app.click('.bar button[aria-label="Play"]');
        }
        const from = await app.shownTime();
        const asked = Date.now();
        const to = await until(
          `${site} to play`,
          async () => {
            const t = await app.shownTime();
            return t >= from + 3 && t;
          },
          2 * MINUTE,
        );
        await ctx.checkpoint(`${site}-playing`);
        const bar = await app.js(`return {
          mute: !!document.querySelector('.bar button[aria-label="Mute"], .bar button[aria-label="Unmute"]'),
          volume: !!document.querySelector('.bar input[aria-label="Volume"]'),
          speed: !!document.querySelector(".bar button.speed"),
          captions: document.querySelector('.bar button[aria-label="Captions"]')?.disabled === false,
        }`);
        const want = { mute: true, volume: true, speed: site !== "twitch", captions: true };
        if (Object.keys(want).some((k) => bar[k] !== want[k]))
          throw new Error(`playing, the bar has ${JSON.stringify(bar)}, not ${JSON.stringify(want)}`);
        if (site === "kick" || !(await app.invoke("embed_takeover"))) {
          const line = await until(
            "a caption in the strip under the player",
            () => app.js(`return document.querySelector(".caption-strip")?.textContent.trim()`),
            30_000,
          );
          await ctx.checkpoint(`${site}-caption-strip`);
          ctx.log(`${site}: the bar has ${Object.keys(want).join(", ")}; the strip says "${line}"`);
        }
        await app.pause().catch(() => {});
        left.push({ site, id: vod.id, time: await app.shownTime() });
        const own = await app.js(`return !!document.querySelector(".own-player")`);
        ctx.log(
          `${site}: ${lines.length} lines, chat ${chat}, played ${from} to ${to} s in ${own ? "the app's own player" : "the site's"}` +
            ` (${((Date.now() - asked) / 1000).toFixed(1)} s from Play to 3 s played)`,
        );

        // A clip, cut from footage downloaded for it; resized a little, cut
        // again from the same footage, with no new download.
        const made = async (clip, what) =>
          until(
            `${site}'s clip ${what}`,
            async () => {
              const c = (await app.invoke("list_clips", { vodId: vod.id })).find((c) => c.id === clip.id);
              if (c?.status === "error") throw new Error(`its clip failed: ${c.error}`);
              return c?.status === "ready" && c.start === clip.start && c.end === clip.end && c;
            },
            5 * MINUTE,
          );
        const clip = await made(await app.invoke("add_clip", { vodId: vod.id, start: 20, end: 25 }), "made");
        const began = Date.now();
        await app.invoke("set_clip_range", { id: clip.id, start: 18, end: 26 });
        await made({ ...clip, start: 18, end: 26 }, "made again");
        ctx.log(`${site}: a clip made, and made again longer in ${((Date.now() - began) / 1000).toFixed(1)} s`);
        // An edit of 10-40 s then 0-10 s: its chat is the VOD's there, on
        // the edit's timeline, and shows beside its transcript.
        if (chat === "done") {
          const edit = await app.invoke("create_edit");
          await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 10, end: 40 });
          await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 0, end: 10 });
          const [inVod] = await app.invoke("chat_windows", { vodId: vod.id, ranges: [[0, 40]], cap: 1 });
          const inEdit = await app.invoke("edit_chat_overview", { id: edit.id, duration: 40, bins: 40 });
          if (inEdit.count !== inVod.total)
            throw new Error(`the edit's chat has ${inEdit.count} messages, the VOD ${inVod.total} there`);
          if (inVod.total) {
            await app.click(`.vod.edit[data-key="-${edit.id}"]`);
            await until(
              "the edit's chat beside its transcript",
              () => app.js(`return document.querySelectorAll(".transcript .chat .message").length`),
              MINUTE,
            );
            await ctx.checkpoint(`${site}-edit-chat`);
            await app.toLibrary();
          }
          ctx.log(`${site}: an edit of 0-40 s has the VOD's ${inVod.total} messages there`);
        }
        // Each site on its own: the next one's link goes in a cleared box.
        await app.type('input[aria-label="VOD link"]', "");
      } catch (e) {
        failures.push(`${site}: ${e.message}`);
        ctx.log(`${site}: FAILED: ${e.message}`);
        await app.type('input[aria-label="VOD link"]', "").catch(() => {});
      }
    }
    // Each opened again from the library, after the others: where it was left.
    for (const { site, id, time } of left) {
      await app.openVod(id);
      const now = await until(
        `${site}'s playhead`,
        async () => (await app.shownTime()) > 0 && app.shownTime(),
        30_000,
      ).catch(() => 0);
      if (Math.abs(now - time) > 1) failures.push(`${site}: opened again at ${now} s, left at ${time} s`);
      else ctx.log(`${site}: opened again where it was left, at ${now} s`);
    }
    if (failures.length) throw new Error(failures.join("; "));
  },
};
