// A VOD removed and added again at once, while its erasure is under way:
// it's erased first, then added anew. The add used to take the gone row
// from under the erasure, which stopped there and left the old VOD's files
// on disk for good: what erase_source deletes (its work files, kept video,
// thumbnail, peaks, storyboard and footage). Here a file's VOD with a long
// chat (so its erasure takes a while) and a file of each of those is
// removed and added back in one go; the old VOD's files and row must all
// go, the new VOD gets an id of its own, a Transcribe asked for at once
// isn't paused by the old one's erasure, and it opens and plays.
//
// The other files are written in with the app stopped, as a site's VOD
// would have them (a file's VOD has a storyboard alone): what's deleted is
// decided by name, not by what's in them.

import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, libraryPath } from "../lib/data.mjs";
import { videos } from "../lib/fixtures.mjs";

/** Chat messages the removed VOD has: enough that its erasure, 1000 at a time, outlasts the add. */
const MESSAGES = 120_000;

/** Writes MESSAGES chat messages into the library for VOD `id`, with the app stopped. */
function writeChat(id) {
  const db = new DatabaseSync(libraryPath());
  try {
    const add = db.prepare("INSERT INTO chat (vod_id, time, author, color, text, parts) VALUES (?, ?, ?, NULL, ?, ?)");
    db.exec("BEGIN");
    for (let i = 0; i < MESSAGES; i++) {
      const text = `message ${i + 1}`;
      add.run(id, i * 0.12, `viewer${(i % 30) + 1}`, text, JSON.stringify([{ type: "text", text }]));
    }
    db.exec("COMMIT");
  } finally {
    db.close();
  }
}

/** VOD `id`'s row (null once it's gone) and how many chat messages it has left. */
function leftOf(id) {
  const db = new DatabaseSync(libraryPath(), { readOnly: true });
  try {
    const row = db.prepare("SELECT gone FROM vods WHERE id = ?").get(id) ?? null;
    const chat = Number(db.prepare("SELECT count(*) AS n FROM chat WHERE vod_id = ?").get(id).n);
    return { row, chat };
  } finally {
    db.close();
  }
}

/** VOD `id`'s storyboards (`<id>-<hash>.sb`). */
const storyboards = (id) => {
  const dir = path.join(dataDirs().at(-1), "storyboards");
  return existsSync(dir) ? readdirSync(dir).filter((n) => n.startsWith(`${id}-`) && n.endsWith(".sb")) : [];
};

/** A file of each kind erase_source deletes for VOD `id`, by the names the app gives them. */
function filesOf(id) {
  const local = dataDirs().at(-1);
  return [
    path.join(local, "work", `${id}.info.json`),
    path.join(local, "media", `A kept video [${String(id).padStart(6, "0")}].mp4`),
    path.join(local, "thumbs", `${id}.jpg`),
    path.join(local, "peaks", `${id}.peaks`),
    path.join(local, "footage", `${id}-0-5000.mp4`),
  ];
}

export default {
  name: "readd-erasing",
  async run(app, ctx) {
    const { log } = ctx;
    const file = videos(path.join(ctx.out, "fixtures")).video;
    await app.toLibrary();
    const [old] = await app.addFiles([file]);
    await until("its storyboard", () => storyboards(old.id).length > 0 || null, MINUTE);

    await ctx.quit();
    const began = Date.now();
    writeChat(old.id);
    const planted = filesOf(old.id);
    for (const at of planted) {
      mkdirSync(path.dirname(at), { recursive: true });
      writeFileSync(at, "x");
    }
    log(`${leftOf(old.id).chat} chat messages and ${planted.length} files written in ${Date.now() - began} ms`);
    app = await ctx.start();
    await app.toLibrary();
    const missing = planted.filter((at) => !existsSync(at));
    if (missing.length) throw new Error(`gone at the start, before the removal: ${missing.join(", ")}`);
    const boards = storyboards(old.id).map((n) => path.join(dataDirs().at(-1), "storyboards", n));

    // Removed, added again at once, and transcribed at once: in one script,
    // with nothing between.
    const { added, errors } = await app.js(
      `const [id, file] = arguments;
       const invoke = window.__TAURI_INTERNALS__.invoke;
       await invoke("delete_vod", { id });
       const { vods, errors } = await invoke("add_files", { paths: [file] });
       if (vods.length) await invoke("transcribe_stretch", { vodId: vods[0].id, start: 0, end: null });
       return { added: vods[0] ?? null, errors };`,
      old.id,
      file,
    );
    if (!added) throw new Error(`not added again: ${errors.join("; ")}`);
    log(`removed VOD ${old.id}, added again as ${added.id}; the old one now ${JSON.stringify(leftOf(old.id))}`);
    if (added.id === old.id) throw new Error(`added again as the old VOD ${old.id}, with what was left of it`);

    // The old one erased whole: its row, its chat and every file of it.
    await until("the old VOD's row gone", () => leftOf(old.id).row === null || null, MINUTE).catch(() => {
      throw new Error(`the old VOD is still in the library: ${JSON.stringify(leftOf(old.id))}`);
    });
    if (leftOf(old.id).chat) throw new Error(`${leftOf(old.id).chat} of the old VOD's chat messages are left`);
    const files = [...planted, ...boards];
    await until("the old VOD's files gone", () => files.every((at) => !existsSync(at)) || null, 30_000).catch(() => {
      throw new Error(`the old VOD's files are still there: ${files.filter(existsSync).join(", ")}`);
    });
    log(`the old VOD erased: its row, its chat and ${files.length} files`);

    // The new one's transcription goes on (the old one's erasure pauses
    // only the old one's work), and it opens and plays.
    await until(
      "the new VOD transcribed",
      async () => {
        const vod = (await app.invoke("list_vods")).find((v) => v.id === added.id);
        if (!vod) throw new Error("the new VOD isn't listed");
        if (vod.status === "paused" || vod.status === "error")
          throw new Error(`the new VOD's transcription is ${vod.status}${vod.error ? `: ${vod.error}` : ""}`);
        return vod.status === "done";
      },
      15 * MINUTE,
    );
    await app.openVod(added.id);
    log(`the new VOD transcribed and plays to ${await app.plays()} s`);
    await until("no jobs at the end", async () => (await app.invoke("list_jobs")).length === 0, 30_000);
  },
};
