// The page reloaded (the web view's own Reload, in its right-click menu)
// with a video open in the app's own player: the window comes back to what
// it showed, the video open where it was and its picture on the screen,
// rather than a stage left in an error, or a picture of the video the page
// no longer knows is there.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { colourNamed, colours } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

export default {
  name: "reload",
  async run(app, ctx) {
    const file = colours(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    const [vod] = await app.addFiles([file]);
    await app.openVod(vod.id);
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    await app.pause();
    // Paused away from the start, so where it was is seen to come back.
    await app.toStart();
    for (let i = 0; i < 3; i++) await app.key("ArrowRight", { shift: true });
    await until("the player at 3 s", async () => (await app.shownTime()) === 3, 10_000);
    const seeing = os.unavailable() ?? os.cannotSee();
    const screen = async () => {
      if (seeing) return null;
      const picture = await app.js(
        `return document.querySelector(".player .picture") ? ".player .picture" : document.querySelector(".player video") ? ".player video" : null`,
      );
      return picture ? colourNamed(os.colourIn(os.screenshot(), os.middle(await os.rectOf(app, picture)))) : null;
    };
    const before = { time: await app.shownTime(), screen: await screen() };
    ctx.log(`open and paused at ${before.time} s; the screen shows ${before.screen ?? "(not looked at)"}`);

    await app.js(`setTimeout(() => location.reload(), 0)`);
    await sleep(1000);
    await app.loaded();
    // What the window shows once the page has settled again.
    const state = () =>
      app.js(
        `const row = document.querySelector(".list .vod.selected");
         return { open: row ? Number(row.dataset.key) : null, player: !!document.querySelector(".player"),
           errors: [...document.querySelectorAll(".error-text, .toast")].map((e) => e.textContent.trim()).filter(Boolean),
           stage: document.querySelector(".stage")?.className ?? null }`,
      );
    const after = await until(
      "the video open again after the reload",
      async () => {
        const now = await state();
        return now.open === vod.id && now.player && now.errors.length === 0 && now.stage !== null && now;
      },
      30_000,
    ).catch(async (e) => {
      const now = await state();
      const native = await app.invoke("e2e_player_state").catch((err) => String(err));
      throw new Error(`${e.message}: ${JSON.stringify(now)}; the app's player: ${JSON.stringify(native)}`);
    });
    ctx.log(`after the reload: ${JSON.stringify(after)}`);
    await until(
      "its time where it was",
      async () => Math.abs((await app.shownTime()) - before.time) < 0.5,
      10_000,
    ).catch(async (e) => {
      throw new Error(`${e.message}: it shows ${await app.shownTime()} s, not ${before.time}`);
    });
    if (before.screen) {
      await until("its picture on the screen", async () => (await screen()) === before.screen, 10_000).catch(
        async (e) => {
          throw new Error(`${e.message}: the screen shows ${await screen()}, not ${before.screen}`);
        },
      );
    }
    ctx.log("a reload came back to the video, where it was, its picture shown");
  },
};
