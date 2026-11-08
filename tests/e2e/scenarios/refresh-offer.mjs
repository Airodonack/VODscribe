// Asking the file host about a VOD again while its answer is slow (the
// file host holds it back). Refresh details twice, the second while the
// first's look-up still waits: the VODscription published since is offered
// once, and answered Not now, isn't offered again when the file host has
// answered everything. The row menu's "Check for a VODscription" says so in
// the VOD's row while the file host is asked, and offers it once answered.
//
// Offline: the VOD is the stand-in yt-dlp's (lib/siteVod.mjs).

import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { notFilter, serveFiles } from "../lib/fileHost.mjs";
import { addSiteVod, offerShown, withSiteVods } from "../lib/siteVod.mjs";

const ID = "xe2eslow";
const FILE = `/annotations/youtube/${ID}.json`;

/** What VOD `id`'s row says under its title. */
const rowSays = (app, id) =>
  app.js(
    `return document.querySelector('.vod[data-key="' + arguments[0] + '"]')?.querySelector(".status-area")?.textContent.replace(/\\s+/g, " ").trim() ?? ""`,
    String(id),
  );

export default {
  name: "refresh-offer",
  async run(app, ctx) {
    let published = false;
    // The answers held back, each its resolve, and whether to hold them.
    const held = [];
    let holding = false;
    const answer = () => (published ? { status: 200, headers: { ETag: '"v1"' }, body: "{}" } : { status: 404 });
    const host = serveFiles((method, at) => {
      if (at !== FILE) return { status: 404 };
      if (!holding) return answer();
      return new Promise((resolve) => held.push(() => resolve(answer())));
    });
    const asked = () => host.requests.filter(notFilter).filter((r) => r.path === FILE).length;
    const releaseAll = () => held.splice(0).forEach((release) => release());
    try {
      await withSiteVods(ctx, async (app) => {
        const wrong = [];
        // Added while nothing is published: asked once, nothing offered.
        const vod = await addSiteVod(app, ID);
        await until(
          "the add's answer kept",
          async () => (await app.invoke("list_vods")).find((v) => v.id === vod.id)?.annotations === "none",
          MINUTE,
        );

        // Published since, and the file host slow: two refreshes, the second
        // while the first's look-up waits.
        published = true;
        holding = true;
        const before = asked();
        await app.invoke("refresh_details", { vodId: vod.id });
        await until("the first refresh's look-up", () => asked() > before || null, 10_000);
        await app.invoke("refresh_details", { vodId: vod.id });
        // Time for a second look-up to ask, if one does.
        await sleep(1500);
        ctx.log(`the file host was asked ${asked() - before} times by the two refreshes`);
        held.shift()();
        await until("the offer", () => offerShown(app), 10_000).then(
          () => app.clickText("Not now"),
          () => wrong.push("the VODscription published since wasn't offered after Refresh details"),
        );
        await until("the offer closed", async () => !(await offerShown(app)), 10_000);
        releaseAll();
        await sleep(2000);
        if (await offerShown(app)) {
          wrong.push("answered Not now, the VODscription was offered again by the second refresh");
          await app.clickText("Not now");
        }

        // The row menu's check, the file host slow: the row says it's
        // asking until the answer, then it's offered.
        const since = asked();
        await app.rowMenu(vod.id, "Check for a VODscription");
        const checking = await until(
          "the row saying it's checking",
          async () => {
            const says = await rowSays(app, vod.id);
            return says.includes("Checking for a VODscription…") && says;
          },
          10_000,
        ).catch(async () => {
          wrong.push(`checking, the row says "${await rowSays(app, vod.id)}"`);
          return false;
        });
        if (checking) await until("the check's look-up", () => asked() > since || null, 10_000);
        releaseAll();
        holding = false;
        await until("the check's offer", () => offerShown(app), 10_000).then(
          () => app.clickText("Not now"),
          () => wrong.push("Check for a VODscription didn't offer the one published"),
        );
        await until(
          "the row done checking",
          async () => !(await rowSays(app, vod.id)).includes("Checking for a VODscription"),
          10_000,
        ).catch(async () => wrong.push(`checked, the row still says "${await rowSays(app, vod.id)}"`));
        if (wrong.length) throw new Error(wrong.join("; "));
      });
    } finally {
      releaseAll();
      host.stop();
    }
  },
};
