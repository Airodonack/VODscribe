// A VOD taken out of the library while the file host is still answering
// about it (lib/fileHost.mjs holds its answers back, so each step lands in
// that gap every time). Use pressed in the offer, then the VOD deleted
// before the file arrives: the offer closes, and the file arriving doesn't
// bring the VOD back into the library. And a VOD deleted while the app is
// still asking whether anything is published for it: nothing is offered
// for it once the answer comes.
//
// Offline: the VODs are the stand-in yt-dlp's (lib/siteVod.mjs).

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { serveFiles } from "../lib/fileHost.mjs";
import { addSiteVod, withSiteVods } from "../lib/siteVod.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const DIALOG = '[role="dialog"][aria-label="Transcript and annotations"]';
const fileOf = (id) => `/annotations/dailymotion/${id}.json`;

const offerShown = (app) => app.js(`return !!document.querySelector(arguments[0])`, DIALOG);
const listed = async (app, id) => (await app.invoke("list_vods")).some((v) => v.id === id);
const rowShown = (app, id) => app.js(`return !!document.querySelector('.vod[data-key="' + arguments[0] + '"]')`, id);

/** An answer held back until `release()`. */
function held() {
  let release;
  const answer = new Promise((resolve) => (release = resolve));
  return { answer, release };
}

export default {
  name: "annotations-gone",
  async run(app, ctx) {
    const FETCHED = "xe2egone";
    const ASKED = "xe2egoneasked";
    const json = readFileSync(FIXTURE, "utf8")
      .replace('"{PLATFORM}"', '"dailymotion"')
      .replace('"{ID}"', `"${FETCHED}"`);
    const file = held();
    const lookup = held();
    const host = serveFiles((method, at) => {
      if (at === fileOf(FETCHED))
        return method === "HEAD"
          ? { status: 200, headers: { ETag: '"v1"' } }
          : file.answer.then(() => ({
              status: 200,
              headers: { ETag: '"v1"', "Content-Type": "application/json" },
              body: json,
            }));
      if (at === fileOf(ASKED)) return lookup.answer.then(() => ({ status: 200, headers: { ETag: '"v1"' } }));
      return { status: 404 };
    });
    try {
      await withSiteVods(ctx, async (app) => {
        const wrong = [];

        // Use pressed; the VOD deleted while the file is on its way.
        const vod = await addSiteVod(app, FETCHED);
        await until("the offer", () => offerShown(app), MINUTE);
        await app.clickText("Use");
        await until(
          "the file asked for",
          () => host.requests.some((r) => r.method === "GET" && r.path === fileOf(FETCHED)),
          MINUTE,
        );
        await app.invoke("delete_vod", { id: vod.id });
        await until("the VOD deleted", async () => !(await listed(app, vod.id)), 10_000);
        await until("the offer closed with its VOD", async () => !(await offerShown(app)), 10_000).catch(() =>
          wrong.push("the offer stayed open after its VOD was deleted"),
        );
        file.release();
        // The fetch ends either way: the dialog's button stops saying it's getting them.
        await until(
          "the fetch over",
          async () =>
            !(await app.js(
              `return [...document.querySelectorAll("button")].some((b) => b.textContent.includes("Getting them"))`,
            )),
          MINUTE,
        );
        await sleep(1000);
        if (await listed(app, vod.id))
          wrong.push("the VOD deleted while its annotations were fetched is back in the library");
        if (await rowShown(app, vod.id)) wrong.push("the deleted VOD's row is back in the library");
        else ctx.log("deleted while its annotations were fetched: it stays deleted");
        if (await offerShown(app)) await app.key("Escape");

        // Asking about a VOD just added; deleted before the answer comes.
        const asked = await addSiteVod(app, ASKED);
        await until(
          "the file host asked",
          () => host.requests.some((r) => r.method === "HEAD" && r.path === fileOf(ASKED)),
          MINUTE,
        );
        await app.invoke("delete_vod", { id: asked.id });
        await until("the second VOD deleted", async () => !(await listed(app, asked.id)), 10_000);
        lookup.release();
        await sleep(1500);
        if (await offerShown(app)) {
          wrong.push("annotations were offered for a VOD deleted while the app asked about it");
          await app.key("Escape");
        }
        if (await rowShown(app, asked.id))
          wrong.push("the VOD deleted while it was asked about is back in the library");
        if (wrong.length) throw new Error(wrong.join("; "));
      });
    } finally {
      file.release();
      lookup.release();
      host.stop();
    }
  },
};
