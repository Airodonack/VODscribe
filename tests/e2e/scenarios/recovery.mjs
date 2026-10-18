// A library that won't open: the app starts on the recovery screen instead
// of failing, and "Start a new library" sets the old file aside (never
// deleting it) for an empty library at the next start.

import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";

const DAMAGED = "This is not a SQLite database: the library's file, damaged.\n".repeat(100);

export default {
  name: "recovery",
  before() {
    mkdirSync(dataDirs()[0], { recursive: true });
    writeFileSync(path.join(dataDirs()[0], "library.sqlite"), DAMAGED);
  },
  async run(app, ctx) {
    await until(
      "the recovery screen",
      () => app.js(`return document.querySelector("h1")?.textContent.includes("couldn't be opened")`),
      MINUTE,
    );
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
  },
};
