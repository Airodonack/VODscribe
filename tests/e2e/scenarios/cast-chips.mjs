// The streamer's picture beside their name where it's cramped: the VOD's
// heading over the player and its row in the library, as the Cast list has
// it. Annotations imported for a Twitch VOD (fixtures/annotations-minute.json,
// on Marlowe's channel, whose cast has Marlowe with a Twitch account): the
// picture from the image server (the run's file host, lib/fileHost.mjs) shows
// beside "Marlowe" in both. A VOD whose annotations give the streamer no
// account (the same file, Marlowe's account taken out) shows its channel's
// picture all the same, by the channel's own login (Twitch's "marlowe").
// With streamer pictures turned off in Settings › Privacy, nothing is there,
// not even an initial, and the image server isn't asked. The picture sits
// on the middle of the name's line, not hanging below it.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { until } from "../lib/app.mjs";
import { serveFiles } from "../lib/fileHost.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const CHAT = path.join(here, "..", "fixtures", "annotations-minute.chat.json");
const ID = "v100000077";
const UNNAMED = "v100000078";

/** Marlowe's picture: four pixels square, all yellow. */
const PICTURE = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAEUlEQVR4nGP4eoQBjhiI4wAA3EIbkanqb5MAAAAASUVORK5CYII=",
  "base64",
);

/** The image server: Marlowe's account and channel sent on to their picture; anyone else's not there. */
const pictures = (method, at) =>
  at === "/avatar/twitch/marlowe_live" || at === "/avatar/twitch/marlowe"
    ? { status: 302, headers: { Location: "/pictures/marlowe.png" } }
    : at === "/pictures/marlowe.png"
      ? { status: 200, headers: { "Content-Type": "image/png" }, body: PICTURE }
      : { status: 404 };

const AVATARS = "Enable streamer pictures from img.vodscribe.com";

export default {
  name: "cast-chips",
  expects: {
    "cast-chips":
      'Over the player, the VOD\'s title "A minute with Marlowe" and under it "Twitch · Marlowe" with a small round yellow picture just before "Marlowe"; the two VODs\' rows in the library on the left have the same small yellow picture before "Marlowe" under their titles.',
  },
  async run(app, ctx) {
    await app.toLibrary();
    // A Twitch VOD of Marlowe's channel, made by importing its annotations
    // (with its chat beside them, so none is fetched).
    const dir = path.join(ctx.out, "cast-chips");
    mkdirSync(dir, { recursive: true });
    /** Imports the fixture as Twitch VOD `id`, its cast changed by `cast`; the VOD made. */
    const importAs = async (id, cast = (c) => c) => {
      const doc = JSON.parse(readFileSync(FIXTURE, "utf8"));
      doc.source.platform = "twitch";
      doc.source.id = id;
      doc.cast = cast(doc.cast);
      const file = path.join(dir, `${id}.json`);
      writeFileSync(file, JSON.stringify(doc));
      writeFileSync(path.join(dir, `${id}.chat.json`), readFileSync(CHAT));
      return (await app.invoke("import_annotations_file", { path: file })).vod;
    };
    const host = serveFiles(pictures);
    const vod = await importAs(ID);
    // The same channel's VOD, its annotations naming no account for the streamer.
    const unnamed = await importAs(UNNAMED, (cast) =>
      cast.map((m) => (m.name === "Marlowe" ? { ...m, account: undefined } : m)),
    );
    const row = `.vod[data-key="${vod.id}"] .meta`;
    const unnamedRow = `.vod[data-key="${unnamed.id}"] .meta`;
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
      ["the picture on the row of a VOD whose annotations give the streamer no account", unnamedRow],
    ]) {
      const got = await settles(what, where, "picture");
      if (got !== true) wrong.push(`${what}: ${got}`);
    }
    await ctx.checkpoint("cast-chips");
    // Said now: what's wrong with them showing is the news, whatever comes after.
    if (wrong.length) throw new Error(wrong.join("; "));
    // The picture sits on the middle of the name's line: its centre and the
    // name's (the text's own box, a Range over it) within a pixel, at both
    // sizes (14 px in a row, 16 px in the heading).
    const offsets = [];
    for (const [what, where] of [
      ["the library row's picture", row],
      ["the heading's picture", head],
    ]) {
      const got = await app.js(
        `const meta = document.querySelector(arguments[0]);
         const disc = meta.querySelector(".avatar").getBoundingClientRect();
         const walk = document.createTreeWalker(meta, NodeFilter.SHOW_TEXT);
         let name = null;
         while (walk.nextNode()) if (walk.currentNode.textContent.trim() === arguments[1]) name = walk.currentNode;
         if (!name) return "no name " + arguments[1];
         const range = document.createRange();
         range.selectNodeContents(name);
         const text = range.getBoundingClientRect();
         return { disc: disc.top + disc.height / 2, text: text.top + text.height / 2, size: disc.height };`,
        where,
        "Marlowe",
      );
      if (typeof got === "string") {
        wrong.push(`${what}: ${got}`);
        continue;
      }
      const off = got.disc - got.text;
      offsets.push(`${got.size} px ${off.toFixed(1)}`);
      if (Math.abs(off) > 1)
        wrong.push(
          `${what} (${got.size} px) is ${Math.abs(off).toFixed(1)} px ${off < 0 ? "above" : "below"} the name's middle`,
        );
    }
    if (wrong.length) throw new Error(wrong.join("; "));
    ctx.log(`the picture's centre below the name's middle: ${offsets.join(", ")}`);
    const asked = [...new Set(host.requests.map((r) => r.path))].sort();
    if (!asked.includes("/avatar/twitch/marlowe_live") || !asked.includes("/avatar/twitch/marlowe"))
      wrong.push(`the image server was asked: ${asked.join(", ")}`);
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
      ["the other VOD's row, avatars off", unnamedRow],
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
