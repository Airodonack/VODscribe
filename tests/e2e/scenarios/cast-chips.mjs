// The streamer's picture beside their name where it's cramped: the VOD's
// heading over the player and its row in the library, as the Cast list has
// it. Annotations imported for a Twitch VOD (fixtures/annotations-minute.json,
// on Marlowe's channel, whose cast has Marlowe with a Twitch account): the
// picture from the image server (the run's file host, lib/fileHost.mjs) shows
// beside "Marlowe" in both. With streamer avatars turned off in Settings ›
// Privacy, nothing is there, not even an initial, and the image server isn't
// asked.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { until } from "../lib/app.mjs";
import { serveFiles } from "../lib/fileHost.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const CHAT = path.join(here, "..", "fixtures", "annotations-minute.chat.json");
const ID = "v100000077";

/** Marlowe's picture: four pixels square, all yellow. */
const PICTURE = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAEUlEQVR4nGP4eoQBjhiI4wAA3EIbkanqb5MAAAAASUVORK5CYII=",
  "base64",
);

/** The image server: Marlowe's account sent on to their picture; anyone else's not there. */
const pictures = (method, at) =>
  at === "/avatar/twitch/marlowe_live"
    ? { status: 302, headers: { Location: "/pictures/marlowe.png" } }
    : at === "/pictures/marlowe.png"
      ? { status: 200, headers: { "Content-Type": "image/png" }, body: PICTURE }
      : { status: 404 };

const AVATARS = "Enable streamer avatars from img.vodscribe.com";

export default {
  name: "cast-chips",
  expects: {
    "cast-chips":
      'Over the player, the VOD\'s title "A minute with Marlowe" and under it "Twitch · Marlowe" with a small round yellow picture just before "Marlowe"; the VOD\'s row in the library on the left has the same small yellow picture before "Marlowe" under its title.',
  },
  async run(app, ctx) {
    await app.toLibrary();
    // A Twitch VOD of Marlowe's channel, made by importing its annotations
    // (with its chat beside them, so none is fetched).
    const doc = JSON.parse(readFileSync(FIXTURE, "utf8"));
    doc.source.platform = "twitch";
    doc.source.id = ID;
    const dir = path.join(ctx.out, "cast-chips");
    mkdirSync(dir, { recursive: true });
    const file = path.join(dir, `${ID}.json`);
    writeFileSync(file, JSON.stringify(doc));
    writeFileSync(path.join(dir, `${ID}.chat.json`), readFileSync(CHAT));
    const host = serveFiles(pictures);
    const { vod } = await app.invoke("import_annotations_file", { path: file });
    const row = `.vod[data-key="${vod.id}"] .meta`;
    const head = ".info .meta";
    /** What's beside the name in `where`: "picture" (shown), "initial X", or "none". */
    const chip = (where) =>
      app.js(
        `const meta = document.querySelector(arguments[0]);
         if (!meta) return "no " + arguments[0];
         const disc = meta.querySelector(".avatar");
         if (!disc) return "none";
         const img = disc.querySelector("img");
         if (disc.classList.contains("pictured") && img?.complete && img.naturalWidth > 0 && getComputedStyle(disc).display !== "none") return "picture";
         return "initial " + disc.textContent.trim();`,
        where,
      );
    const settles = (what, where, want) =>
      until(what, async () => (await chip(where)) === want, 10_000).catch(async () => chip(where));

    const wrong = [];
    await app.openVod(vod.id);
    for (const [what, where] of [
      ["the library row's picture", row],
      ["the heading's picture", head],
    ]) {
      const got = await settles(what, where, "picture");
      if (got !== true) wrong.push(`${what}: ${got}`);
    }
    await ctx.checkpoint("cast-chips");
    // Said now: what's wrong with them showing is the news, whatever comes after.
    if (wrong.length) throw new Error(wrong.join("; "));
    const asked = [...new Set(host.requests.map((r) => r.path))].sort().join(", ");
    if (!asked.includes("/avatar/twitch/marlowe_live")) wrong.push(`the image server was asked: ${asked}`);
    host.stop();

    // Turned off: nothing beside the name, and the image server isn't asked.
    const quiet = serveFiles(pictures);
    await app.openSettings("Privacy");
    await app.js(
      `const label = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label')].find((l) => l.textContent.trim() === arguments[0]);
       if (!label) throw new Error("no setting " + arguments[0]);
       const box = label.querySelector("input");
       if (box.checked) box.click();`,
      AVATARS,
    );
    await until("avatars off", async () => (await app.invoke("get_settings")).load_cast_pictures === false, 10_000);
    await app.key("Escape");
    for (const [what, where] of [
      ["the library row, avatars off", row],
      ["the heading, avatars off", head],
    ]) {
      const got = await settles(what, where, "none");
      if (got !== true) wrong.push(`${what}: ${got}`);
    }
    // Back to the library and the VOD again: still nothing, nothing asked.
    await app.click(`.vod[data-key="${vod.id}"]`);
    await app.openVod(vod.id);
    const again = await chip(head);
    if (again !== "none") wrong.push(`opened again with avatars off, the heading has ${again}`);
    if (quiet.requests.length)
      wrong.push(`with avatars off, the image server was asked: ${quiet.requests.map((r) => r.path).join(", ")}`);
    quiet.stop();
    if (wrong.length) throw new Error(wrong.join("; "));
    ctx.log("the streamer's picture beside their name in the heading and the library row; off, nothing");
  },
};
