// The test VOD on Twitch starts on the bar's Play at a page height where its
// player's frame ends on a fraction of a pixel that rounds into the controls
// under it. Twitch plays only when its player isn't covered at all ("style
// visibility"), and a frame in the flow there lost its last row to the
// controls, which paint over it: in a Windows web view 781 px tall, Twitch
// said "Autoplay disabled" and stayed at Ready (Player.svelte gives a site's
// player a layer of its own). The height is found by measuring: where the
// frame's top is on a whole pixel no height rounds that way, and it skips.

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { addTestVod, pressPlay } from "../lib/siteVod.mjs";

/** The poster's box (where their frame goes), and whether its rounded top
 *  and height reach a row past its rounded bottom. */
const edge = (app) =>
  app.js(`const r = document.querySelector(".stage .frame").getBoundingClientRect();
    return { top: r.top, tall: r.height, into: Math.round(r.top) + Math.round(r.height) > Math.round(r.bottom) };`);

export default {
  name: "site-autoplay",
  needs: ["network"],
  async run(app, ctx) {
    const vod = await addTestVod(app, "twitch");
    await app.openVod(vod.id);
    await until(
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    // A height (around the one it opened at) where the frame's box rounds into the controls.
    const opened = await app.js(`return { width: innerWidth, height: innerHeight }`);
    let found = null;
    for (let height = opened.height; height > opened.height - 40 && !found; height--) {
      const got = await resizeInner(app, opened.width, height);
      const box = await edge(app);
      if (box.into) found = { ...got, ...box };
    }
    if (!found) ctx.skip(`the frame's top is on a whole pixel here (${(await edge(app)).top})`);
    ctx.log(`at ${found.width}x${found.height}: the frame from ${found.top} px, ${found.tall} px tall`);

    const started = await app.plays({ press: () => pressPlay(app), ms: 2 * MINUTE }).catch(async (e) => {
      const said = await app.js(`return document.querySelector(".bar button.time")?.textContent ?? ""`);
      throw new Error(`${e.message}: Twitch didn't start at a frame ending on a fraction (the bar says ${said})`);
    });
    ctx.log(`Twitch played: the time at ${started} s`);
    await app.pause();
  },
};
