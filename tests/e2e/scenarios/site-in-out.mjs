// I right after a click on the timeline, on a site's VOD: the owner's test
// VOD on Twitch (or YouTube or Kick, with E2E_SITE=youtube or kick). A click
// moves the playhead at once, but their player says it's there only a while
// later (it loads or seeks, buffers, and reports now and then; Twitch's,
// opened at a time, says it's at the start for a second or so first); I
// pressed meanwhile starts the selection where the click put the playhead,
// not where their player last said it was. I is pressed at every other
// frame for a few seconds after the click (lib/inOut.mjs), so whatever
// their player reports meanwhile is caught: first a click before the player
// has opened (the click opens it there, paused), then a click while it plays.

import { MINUTE, until } from "../lib/app.mjs";
import { clickThenIn } from "../lib/inOut.mjs";

const SITES = {
  youtube: "https://www.youtube.com/watch?v=IaIo3d6cUuo",
  twitch: "https://www.twitch.tv/videos/2901646294",
  kick: "https://kick.com/airodonack/videos/01a169a1-9aa8-7653-8a9e-390e694cc11a",
};
const SITE = process.env.E2E_SITE || "twitch";

export default {
  name: "site-in-out",
  needs: ["network"],
  async run(app, ctx) {
    await app.toLibrary();
    await app.type('input[aria-label="VOD link"]', SITES[SITE]);
    await app.submit('input[aria-label="VOD link"]');
    const vod = await until(
      `the ${SITE} VOD in the library`,
      async () => {
        const error = await app.js(`return document.querySelector(".add-error")?.textContent.trim() ?? ""`);
        if (error) throw new Error(`adding it: ${error}`);
        return (await app.invoke("list_vods")).find((v) => v.platform === SITE);
      },
      5 * MINUTE,
    );
    await app.openVod(vod.id);
    await until(
      "the timeline",
      () => app.js(`return Number(document.querySelector(".scrubber")?.dataset.span) > 10`),
      MINUTE,
    );
    const span = await app.js(`return Number(document.querySelector(".scrubber").dataset.span)`);
    // A pixel of the timeline, the frames between a press and its reading,
    // and Twitch's player opened at a time: it takes whole seconds.
    const slack = Math.max(1, span / 400);
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));

    // 1. Before the player has opened: the click opens it there, paused,
    //    and their player loads and reports from there.
    await clickThenIn(app, ctx, "before the player opened", { fraction: 0.4, ms: 10_000, playing: false, slack });
    await until("their player", () => app.js(`return !!document.querySelector("iframe, video")`), MINUTE);

    // 2. Playing: a click further on, I as their player seeks there.
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "their player playing",
      async () => {
        const t = await app.shownTime();
        await wait(1500);
        return (await app.shownTime()) > t;
      },
      2 * MINUTE,
    );
    await clickThenIn(app, ctx, "while playing", { fraction: 0.7, ms: 5_000, playing: true, slack });
  },
};
