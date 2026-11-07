// A VOD removed and added again at once while its work won't stop: its
// erasure gives up waiting (10 s), and the add goes on without it. The add
// used to delete the old VOD's row then and leave its files on disk for
// good, nothing left to name them; they must go once its work has
// stopped. The work is kept from stopping by a test hook (e2e_work_busy),
// as a download stuck on the network would, so this needs the e2e build.
//
// The files are written in with the app stopped, as readd-erasing.mjs's
// are: what's deleted is decided by name, not by what's in them.

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, libraryPath } from "../lib/data.mjs";
import { videos } from "../lib/fixtures.mjs";

/** How long the old VOD's work won't stop: past the add's wait, short of the sweep's after it. */
const BUSY = 15_000;

/** The ids of the VODs in the library, gone ones too. */
function rows() {
  const db = new DatabaseSync(libraryPath(), { readOnly: true });
  try {
    return db
      .prepare("SELECT id FROM vods")
      .all()
      .map((r) => Number(r.id));
  } finally {
    db.close();
  }
}

/** A file of each kind erase_source deletes for VOD `id`, by the names the app gives them. */
function filesOf(id) {
  const local = dataDirs().at(-1);
  return [
    path.join(local, "work", `${id}.info.json`),
    path.join(local, "media", `${id}.mp4`),
    path.join(local, "thumbs", `${id}.jpg`),
    path.join(local, "peaks", `${id}.peaks`),
    path.join(local, "footage", `${id}-0-5000.mp4`),
  ];
}

export default {
  name: "readd-stuck",
  async run(app, ctx) {
    const { log } = ctx;
    if (!(await app.hasE2e())) ctx.skip("keeping a VOD's work from stopping needs the e2e build");
    const file = videos(path.join(ctx.out, "fixtures")).video;
    await app.toLibrary();
    const [old] = await app.addFiles([file]);

    await ctx.quit();
    const planted = filesOf(old.id);
    for (const at of planted) {
      mkdirSync(path.dirname(at), { recursive: true });
      writeFileSync(at, "x");
    }
    app = await ctx.start();
    await app.toLibrary();
    const missing = planted.filter((at) => !existsSync(at));
    if (missing.length) throw new Error(`gone at the start, before the removal: ${missing.join(", ")}`);

    // Its work kept from stopping, then removed and added again at once,
    // in one script, with nothing between.
    const { added, errors, ms } = await app.js(
      `const [id, file, busy] = arguments;
       const invoke = window.__TAURI_INTERNALS__.invoke;
       await invoke("e2e_work_busy", { vodId: id, ms: busy });
       await invoke("delete_vod", { id });
       const began = Date.now();
       const { vods, errors } = await invoke("add_files", { paths: [file] });
       return { added: vods[0] ?? null, errors, ms: Date.now() - began };`,
      old.id,
      file,
      BUSY,
    );
    if (!added) throw new Error(`not added again: ${errors.join("; ")}`);
    log(`removed VOD ${old.id}, its work busy for ${BUSY} ms; added again as ${added.id} in ${ms} ms`);
    if (added.id === old.id) throw new Error(`added again as the old VOD ${old.id}, with what was left of it`);
    // The add waits for the old one's erasure (10 s) and for its work (10 s), no longer.
    if (ms > 25_000) throw new Error(`the add took ${ms} ms`);

    // Once its work has stopped, the old one goes: its row and every file.
    const left = () => planted.filter(existsSync);
    await until("the old VOD's files gone", () => (!left().length && !rows().includes(old.id)) || null, MINUTE).catch(
      () => {
        throw new Error(
          `a minute on, its work stopped, the old VOD's ${rows().includes(old.id) ? "row and " : ""}files are still there: ${left().join(", ")}`,
        );
      },
    );
    log(`the old VOD erased once its work stopped: its row and ${planted.length} files`);

    // The new one is listed, and nothing's left in the jobs list.
    if (!(await app.invoke("list_vods")).some((v) => v.id === added.id)) throw new Error("the new VOD isn't listed");
    await until("no jobs at the end", async () => (await app.invoke("list_jobs")).length === 0, 30_000);
  },
};
