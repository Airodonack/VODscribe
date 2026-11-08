// A library that won't open: the app starts on the recovery screen instead
// of failing, and "Start a new library" sets the old one aside (never
// deleting it), in a folder with its kept videos and clips, for an empty
// library at the next start.

import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { clipsDir, dataDirs, keptVideo } from "../lib/data.mjs";

const DAMAGED = "This is not a SQLite database: the library's file, damaged.\n".repeat(100);

export default {
  name: "recovery",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "recovery-screen":
      "A screen of its own instead of the library: the VODscribe logo, the heading Your library couldn't be opened, a line saying nothing has been deleted, a box with the error naming library.sqlite and file is not a database, Try again and a filled Start a new library button each with a line of explanation, and Show the file and Report the problem links.",
  },
  before() {
    mkdirSync(dataDirs()[0], { recursive: true });
    writeFileSync(path.join(dataDirs()[0], "library.sqlite"), DAMAGED);
    // A VOD's kept video, named for its id as the library keeps them.
    mkdirSync(path.dirname(keptVideo(1, "mp4")), { recursive: true });
    writeFileSync(keptVideo(1, "mp4"), "a kept video");
    // And a clip's file, named for its id in the same way.
    mkdirSync(clipsDir(), { recursive: true });
    writeFileSync(path.join(clipsDir(), "1.mp4"), "a clip");
  },
  async run(app, ctx) {
    await until(
      "the recovery screen",
      () => app.js(`return document.querySelector("h1")?.textContent.includes("couldn't be opened")`),
      MINUTE,
    );
    await ctx.checkpoint("recovery-screen");
    await app.clickText("Start a new library");
    await until("the app to ask to restart", async () => (await app.restarts()) === 1, 10_000);
    const dir = dataDirs()[0];
    const aside = readdirSync(dir).filter((f) => f.startsWith("library-broken-"));
    if (aside.length !== 1) throw new Error(`set aside: ${JSON.stringify(aside)}, in ${readdirSync(dir).join(", ")}`);
    ctx.log(`set aside as ${aside[0]}`);

    app = await ctx.restart();
    await app.toLibrary();
    if ((await app.invoke("list_vods")).length !== 0) throw new Error("the new library isn't empty");
    const kept = readdirSync(dir).filter((f) => f.startsWith("library-broken-"));
    if (kept.length !== 1) throw new Error(`after the restart, the old library is: ${JSON.stringify(kept)}`);
    // Its video went with it, where the new library's own VOD 1 can't take its place.
    if (!existsSync(path.join(dataDirs().at(-1), kept[0], "media", "1.mp4")))
      throw new Error(
        `the old library's kept video is gone; ${kept[0]} has ${readdirSync(path.join(dataDirs().at(-1), kept[0])).join(", ")}`,
      );
    if (existsSync(keptVideo(1, "mp4"))) throw new Error("the old library's kept video was left in the new one");
    if (!existsSync(path.join(dataDirs().at(-1), kept[0], "clips", "1.mp4")))
      throw new Error(
        `the old library's clip is gone; ${kept[0]} has ${readdirSync(path.join(dataDirs().at(-1), kept[0])).join(", ")}`,
      );
    if (existsSync(path.join(clipsDir(), "1.mp4"))) throw new Error("the old library's clip was left in the new one");
  },
};
