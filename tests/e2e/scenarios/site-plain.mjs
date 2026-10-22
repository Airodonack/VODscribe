// A site's VOD in its own player, left as it is (a release: no embed-takeover;
// site-embed.mjs is the other way): the owner's test VOD on Twitch (or
// YouTube, with E2E_SITE=youtube), played, jumped, paused and resumed from
// the app's controls. Nothing of the app's runs in their frame, the app's own
// player stays shut, and the time under the player follows theirs.

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const SITES = {
  youtube: "https://www.youtube.com/watch?v=IaIo3d6cUuo",
  twitch: "https://www.twitch.tv/videos/2901646294",
};
const SITE = process.env.E2E_SITE ?? "twitch";

export default {
  name: "site-plain",
  needs: ["network"],
  async run(app, ctx) {
    if (await app.invoke("embed_takeover")) ctx.skip("this build takes over the sites' players (site-embed)");
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
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    if (!os.unavailable()) {
      os.activate();
      const play = await os.rectOf(app, '.bar button[aria-label="Play"]');
      os.click(play.x + play.w / 2, play.y + play.h / 2, play.scale);
    } else {
      await app.click('.bar button[aria-label="Play"]');
    }
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    const ours = () => app.invoke("player_set_speed", { speed: 1 }).catch(() => null);
    const label = () => app.js(`return document.querySelector(".bar button.control")?.getAttribute("aria-label")`);

    // Playing: the time under the player runs on, from their player's reports.
    const started = await until(
      "their player playing",
      async () => {
        const t = await app.shownTime();
        await wait(1500);
        const t2 = await app.shownTime();
        return t2 > t && t2;
      },
      2 * MINUTE,
    );
    ctx.log(`playing: the time at ${started} s`);
    // Their frame as it is: nothing of the app's ran in it (it reports when it
    // does), and the app's own player never opened.
    const reports = await app.js(`return (window.__vodscribeEmbed?.reports ?? []).length`);
    if (reports) throw new Error(`the app's script ran in their frame: ${reports} reports`);
    if (await ours()) throw new Error("the app's own player opened for a site's VOD");
    if ((await app.js(`return document.querySelector("iframe")?.name ?? ""`)) !== "vodscribe-plain")
      throw new Error("their frame isn't the plain one");

    // A jump (L: 10 s each): their player goes there, and the time with it.
    const before = await app.shownTime();
    for (let i = 0; i < 3; i++) await app.key("l");
    const jumped = await until(
      "the time past the jump",
      async () => {
        const t = await app.shownTime();
        return t >= before + 25 && t;
      },
      MINUTE,
    );
    ctx.log(`jumped: from ${before} s to ${jumped} s`);

    // Paused from the app's controls: the time stands, and the button says so.
    await app.key(" ");
    await until("Play shown", async () => (await label()) === "Play", 10_000);
    await wait(1500);
    const a = await app.shownTime();
    await wait(2500);
    const b = await app.shownTime();
    ctx.log(`paused: ${a} s, then ${b} s`);
    if (a !== b) throw new Error(`paused, the time went from ${a} s to ${b} s`);

    // Resumed: on from where it stood, not back.
    await app.key(" ");
    const resumed = await until(
      "the time running again",
      async () => {
        const t = await app.shownTime();
        return t > b && t;
      },
      MINUTE,
    );
    ctx.log(`resumed: at ${resumed} s`);
    if ((await label()) !== "Pause") throw new Error("playing again, the button doesn't say Pause");
  },
};
