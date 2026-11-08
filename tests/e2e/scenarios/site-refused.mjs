// Twitch's player refuses the app's Play when it's too small: it says it's
// ready and stays so (startWatch.ts). The test VOD in a player 150 px tall
// (the page's own limit on its height, as the website's demo sets it; at
// the window's least size, 375 by 211, WebKitGTK's Twitch still played): a
// toast at the window's bottom, clear of their player, asks for a press on
// Play in Twitch's player, and goes once Twitch plays: here by itself, as
// it does once its player is its size again (the app hears the same
// Playing whoever started it, and no place of Twitch's controls is aimed at).

import { MINUTE, until } from "../lib/app.mjs";
import { addTestVod, pressPlay } from "../lib/siteVod.mjs";

const ASKS = "Press Play in Twitch's player to start it.";

/** A box in the page: the element's, or null. */
const box = (app, selector) =>
  app.js(
    `const r = document.querySelector(arguments[0])?.getBoundingClientRect();
     return r ? { x: r.left, y: r.top, w: r.width, h: r.height } : null;`,
    selector,
  );

const overlap = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/** The player's height, at most (Player.svelte's .frame); null: as the page has it. */
const playerAtMost = (app, height) =>
  app.js(
    `const root = document.documentElement.style;
     if (arguments[0]) root.setProperty("--player-max-height", arguments[0]);
     else root.removeProperty("--player-max-height");`,
    height,
  );

/** What the bar's play button says ("Play", "Pause"). */
const label = (app) => app.js(`return document.querySelector(".bar button.control")?.getAttribute("aria-label")`);

export default {
  name: "site-refused",
  needs: ["network"],
  async run(app, ctx) {
    const vod = await addTestVod(app, "twitch");
    await app.openVod(vod.id);
    await until(
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    await playerAtMost(app, "150px");
    const frame = await box(app, ".stage .frame");
    ctx.log(`the player ${Math.round(frame.w)}x${Math.round(frame.h)}`);

    // The bar's Play: Twitch's player loads, and stays ready, not playing.
    const pressed = Date.now();
    await pressPlay(app);
    await until("the toast asking for Twitch's Play", async () => (await app.toastSays()) === ASKS, 2 * MINUTE).catch(
      async (e) => {
        throw new Error(`${e.message}: the toast says "${await app.toastSays()}", the bar ${await label(app)}`);
      },
    );
    ctx.log(`the toast ${((Date.now() - pressed) / 1000).toFixed(1)} s after Play`);
    if ((await label(app)) !== "Play") throw new Error(`with the toast up, the bar says ${await label(app)}`);
    // Never over their player.
    const [toast, theirs] = [await box(app, ".toast"), await box(app, ".stage iframe")];
    if (!theirs) throw new Error("no frame of Twitch's with the toast up");
    if (overlap(toast, theirs))
      throw new Error(`the toast ${JSON.stringify(toast)} is over Twitch's player ${JSON.stringify(theirs)}`);

    // The player its size again: Twitch plays, and the toast goes.
    await playerAtMost(app, null);
    await until("Twitch playing", async () => (await label(app)) === "Pause", MINUTE);
    await until("the toast gone", async () => (await app.toastSays()) !== ASKS, 10_000);
    const from = await app.shownTime();
    const now = await until(
      "the time to run on",
      async () => {
        const t = await app.shownTime();
        return t >= from + 2 && t;
      },
      MINUTE,
    );
    ctx.log(`the player its size again, Twitch played: the toast gone, the time at ${now} s`);
    await app.pause();
  },
};
