// I right after a click on the timeline, on a video file (the app's own
// player): as site-in-out.mjs does for a site's VOD. The click moves the
// playhead at once and the player follows; I pressed at every other frame
// for a few seconds after it (lib/inOut.mjs) starts the selection at the
// click, before the player has opened (the click opens it there, paused)
// and while it plays.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { minute } from "../lib/fixtures.mjs";
import { clickThenIn } from "../lib/inOut.mjs";

export default {
  name: "in-out-file",
  async run(app, ctx) {
    const file = minute(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    await app.pickFiles([file]);
    await app.clickText("add video files…");
    const vod = await until(
      "the video in the library",
      async () => {
        const [vod] = await app.invoke("list_vods");
        if (vod?.status === "error") throw new Error(vod.error);
        return vod?.duration && vod;
      },
      5 * MINUTE,
    );
    await app.openVod(vod.id);
    await until(
      "the timeline",
      () => app.js(`return Number(document.querySelector(".scrubber")?.dataset.span) > 10`),
      MINUTE,
    );
    // A frame, and the frames between a press and its reading.
    const slack = 0.25;

    await clickThenIn(app, ctx, "before the player opened", { fraction: 0.3, ms: 6_000, playing: false, slack });
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    await clickThenIn(app, ctx, "while playing", { fraction: 0.7, ms: 4_000, playing: true, slack });
  },
};
