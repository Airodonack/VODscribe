// The whole library checked for published annotations against the file
// host's filter (lib/fileHost.mjs serves it; crates/core
// annotations_filter.rs), which tells the host nothing of the library. A VOD
// added with asking turned off is never asked about; turned on, the next
// start fetches the filter and asks about that VOD alone, once, and its gift
// button appears; a VOD the filter hasn't is never asked about. Brought in,
// then published anew: the library's "Check all VODs for annotations" (any
// row's menu) fetches the filter again, asks about that VOD once, and its gift
// is marked newer. With nothing changed, a check, and every start, costs one
// request for the filter, answered "unchanged".
//
// Offline: the VODs are the stand-in yt-dlp's (lib/siteVod.mjs).

import { readFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { MINUTE, until } from "../lib/app.mjs";
import { FILTER_PATH, annotationsFilter, md5, serveFiles } from "../lib/fileHost.mjs";
import { addSiteVod, offerShown, withSiteVods } from "../lib/siteVod.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const ID = "xe2elate";
const PLAIN = "xe2eplain";
const FILE = `/annotations/youtube/${ID}.json`;

const gift = (id) => `.vod[data-key="${id}"] button.gift`;

export default {
  name: "annotations-library",
  expects: {
    "library-newer":
      "The library: the VOD's row has a gift icon at the left of its row of buttons, with a small dot at its top right corner.",
  },
  async run(app, ctx) {
    const fileOf = (title) =>
      readFileSync(FIXTURE, "utf8")
        .replace('"{PLATFORM}"', '"youtube"')
        .replace('"{ID}"', `"${ID}"`)
        .replace(/"title":\s*"[^"]*"/, `"title": "${title}"`);
    // What's published: nothing, then the file (and the filter naming it), then a newer file.
    let file = null;
    let filter = null;
    const publish = (body, round) => {
      file = body;
      // An ETag the app keeps is MD5-shaped, as the file host's are; one per round, so a change is seen.
      filter = {
        body: annotationsFilter([`youtube:${ID}`, `youtube:${ID}:${md5(body)}`]),
        etag: `"${md5(`${round}`)}"`,
      };
    };
    const host = serveFiles((method, at, since) => {
      if (at === FILTER_PATH && filter)
        return since === filter.etag
          ? { status: 304, headers: { ETag: filter.etag } }
          : { status: 200, headers: { ETag: filter.etag }, body: filter.body };
      if (at === FILE && file)
        return { status: 200, headers: { ETag: `"${md5(file)}"`, "Content-Type": "application/json" }, body: file };
      return { status: 404 };
    });
    // The file host's questions: the image server is the same host here, and
    // the library row of a VOD with annotations asks it for the channel's picture.
    const asked = (from = 0) =>
      host.requests
        .slice(from)
        .filter((r) => !r.path.startsWith("/avatar/"))
        .map((r) => `${r.method} ${r.path}${r.since ? " since" : ""}`);
    try {
      await withSiteVods(ctx, async (app) => {
        const wrong = [];
        // The start's check first (nothing published yet: the filter is 404).
        await until("the start's check", async () => host.requests.length > 0, MINUTE);
        const settings = await app.invoke("get_settings");
        await app.invoke("set_settings", { settings: { ...settings, ask_for_annotations: false } });
        const off = host.requests.length;
        const vod = await addSiteVod(app, ID);
        const plain = await addSiteVod(app, PLAIN);
        await sleep(2000);
        if (host.requests.length > off)
          wrong.push(`with asking off, the file host was asked: ${asked(off).join(", ")}`);

        // Published since, asking turned on: the next start asks about this VOD alone.
        publish(fileOf("Published"), 1);
        await app.invoke("set_settings", { settings: { ...settings, ask_for_annotations: true } });
        let start = host.requests.length;
        app = await ctx.restart();
        await app.toLibrary();
        await until(
          "the gift button",
          () => app.js(`return !!document.querySelector(arguments[0])`, gift(vod.id)),
          MINUTE,
        );
        await sleep(1500);
        const atStart = asked(start).join(", ");
        if (atStart !== `GET ${FILTER_PATH}, HEAD ${FILE}`)
          wrong.push(`at the start, the file host was asked: ${atStart}`);
        if (await app.js(`return !!document.querySelector(arguments[0])`, gift(plain.id)))
          wrong.push("the VOD the filter hasn't has the gift button");
        if (await offerShown(app)) wrong.push("the start's check opened an offer");

        // Brought in from the gift.
        await app.click(gift(vod.id));
        await until("the offer", () => offerShown(app), 10_000);
        await app.clickText("Use");
        await until(
          "the import",
          async () => (await app.invoke("list_vods")).find((v) => v.id === vod.id)?.transcribed === "all",
          MINUTE,
        );
        await until("the offer closed", async () => !(await offerShown(app)), 10_000);
        await until("the toast gone", async () => !(await app.toastSays()), MINUTE).catch(() => {});

        // Published anew: the library's check finds it, at once, with one question about the VOD.
        publish(fileOf("Published anew"), 2);
        start = host.requests.length;
        await app.rowMenu(plain.id, "Check all VODs for annotations");
        const found = await until("the check's toast", () => app.toastSays(), MINUTE).catch(() => "");
        if (found !== "Found newer annotations for 1 VOD: use the gift button in the library.")
          wrong.push(`the library's check says "${found}"`);
        const checked = asked(start).join(", ");
        if (checked !== `GET ${FILTER_PATH} since, HEAD ${FILE}`) wrong.push(`the library's check asked: ${checked}`);
        const marked = await until(
          "the gift marked newer",
          () => app.js(`return !!document.querySelector(arguments[0] + ".newer")`, gift(vod.id)),
          10_000,
        ).catch(() => false);
        if (!marked) wrong.push("the gift isn't marked newer");
        await ctx.checkpoint("library-newer");
        await until("the toast gone", async () => !(await app.toastSays()), MINUTE).catch(() => {});

        // Nothing changed: the check, and the next start, ask only whether the filter did;
        // the check still counts what it knows (the gift is marked newer).
        start = host.requests.length;
        await app.rowMenu(plain.id, "Check all VODs for annotations");
        const again = await until("the check's toast", () => app.toastSays(), MINUTE).catch(() => "");
        if (again !== "Found newer annotations for 1 VOD: use the gift button in the library.")
          wrong.push(`checked again: "${again}"`);
        if (asked(start).join(", ") !== `GET ${FILTER_PATH} since`) wrong.push(`checked again, asked: ${asked(start)}`);
        start = host.requests.length;
        app = await ctx.restart();
        await app.toLibrary();
        await until("the start's check", async () => host.requests.length > start, MINUTE).catch(() => {});
        await sleep(1500);
        if (asked(start).join(", ") !== `GET ${FILTER_PATH} since`) wrong.push(`started again, asked: ${asked(start)}`);
        if (wrong.length) throw new Error(wrong.join("; "));
      });
    } finally {
      host.stop();
    }
  },
};
