// The list of jobs at the bottom of the library: three clips of all of a
// long file asked for at once, one made at a time, show there with their
// names, the first being made, the other two waiting. The last is dragged
// above the one before it, and the queue takes that order: it's the one
// made next. Cancel on the one being made ends it: its part file goes, it
// leaves the list, and the clip says it was canceled. Then a site's VOD with
// its video kept here and a long chat is removed from the library: it's
// erased at once, a job of its own in the list ("Erasing"), and quitting
// while that goes on is over at once (it used to wait up to three seconds
// for it); at the next start what was left of it is erased.

import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, libraryPath, makeSiteVod } from "../lib/data.mjs";
import { longVideo, TWITCH_VOD, videos } from "../lib/fixtures.mjs";

/** Quitting with an erasure under way takes no longer than this, ms (the process gone; a debug build's). */
const QUIT_WITHIN = 2000;
/** The jobs' list, opened (Jobs.svelte). */
const LIST = '[role="dialog"][aria-label="Jobs"]';

/** The part files of clips being made, anywhere in the app's data (`.<name>.part`). */
function partFiles() {
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const at = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(at);
      else if (entry.name.startsWith(".") && entry.name.endsWith(".part")) found.push(at);
    }
  };
  for (const dir of dataDirs()) if (existsSync(dir)) walk(dir);
  return found;
}

/** How many chat messages the removed VOD has: enough that its erasure,
 *  a part of 1000 at a time, is still under way at the quit. */
const MESSAGES = 120_000;
/** Writing them takes no longer than this, ms (a second or two here; CI's runners are slower). */
const WRITE_WITHIN = 30_000;

/**
 * Writes MESSAGES chat messages into the library for VOD `id`, with the app
 * stopped, as a fetch would have saved them; returns how long it took, ms.
 * Written, not fetched: the chat fixture is served only by a build with the
 * e2e feature (e2e.rs), and CI drives the release package, where the fetch
 * would go to Twitch and never be "done" with this chat (it timed out there,
 * 2026-10-31).
 */
function writeLongChat(id) {
  const began = Date.now();
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
  return Date.now() - began;
}

/** What's left in the library of VOD `id`: its row (null if gone) and how many chat messages. With the app stopped or not. */
function leftOf(id) {
  const db = new DatabaseSync(libraryPath(), { readOnly: true });
  try {
    const row = db.prepare("SELECT gone, media_path FROM vods WHERE id = ?").get(id) ?? null;
    const chat = Number(db.prepare("SELECT count(*) AS n FROM chat WHERE vod_id = ?").get(id).n);
    return { row, chat };
  } finally {
    db.close();
  }
}

export default {
  name: "jobs",
  async run(app, ctx) {
    const { log } = ctx;
    const dir = path.join(ctx.out, "fixtures");
    await app.toLibrary();
    const [vod] = await app.addFiles([longVideo(dir)]);

    // Three clips of all of it (each half a minute or more to make): one
    // made, two waiting.
    const made = await app.invoke("add_clips", {
      vodId: vod.id,
      ranges: [
        [0, 180],
        [1, 180],
        [2, 180],
      ],
    });
    const [a, b, c] = made.map((m) => m.id);
    const jobs = () => app.invoke("list_jobs");
    const order = async () => (await jobs()).map((j) => `${j.id}${j.running ? "*" : ""}`).join(" ");
    await until(
      "the three in the list, the first being made",
      async () => (await order()) === `${a}* ${b} ${c}`,
      MINUTE,
    );
    await until("a part file of the first", () => partFiles().length > 0 || null, MINUTE);

    // The line at the bottom of the library says so, and opens the list.
    const summary = await until(
      "the jobs' line under the library",
      () => app.js(`return document.querySelector(".library-pane .jobs-bar .summary")?.textContent.trim()`),
      10_000,
    );
    if (summary !== "Making 3 clips") throw new Error(`the jobs' line says "${summary}"`);
    await app.click(".library-pane .jobs-bar button.open");
    const rows = () =>
      app.js(
        `return [...document.querySelectorAll(arguments[0] + " [data-job]")].map((r) => ({
           job: r.dataset.job,
           label: r.querySelector(".label").textContent.trim(),
           line: r.querySelector(".line").textContent.trim(),
         }))`,
        LIST,
      );
    const listed = await until(
      "the list open with the three",
      async () => {
        const r = await rows();
        return r.length === 3 && r;
      },
      10_000,
    );
    const labels = new Map((await jobs()).map((j) => [`${j.kind}-${j.id}`, j.label]));
    for (const [at, id] of [a, b, c].entries()) {
      const row = listed[at];
      if (row.job !== `clip-${id}` || row.label !== labels.get(row.job))
        throw new Error(`row ${at + 1} is ${JSON.stringify(row)}, not clip ${id} "${labels.get(`clip-${id}`)}"`);
    }
    if (!listed[0].line.startsWith("Making the clip")) throw new Error(`the first says "${listed[0].line}"`);
    if (listed[1].line !== "Waiting" || listed[2].line !== "Waiting")
      throw new Error(`the others say "${listed[1].line}", "${listed[2].line}"`);
    log(`listed: ${listed.map((r) => `${r.label} (${r.line})`).join("; ")}`);
    await ctx.checkpoint("jobs-list");

    // The last dragged above the one before it, a frame at a time (the
    // list moves as it passes the other's middle: reorder.svelte.ts).
    await app.js(
      `const [list, from, over] = arguments;
       const row = (job) => document.querySelector(list + ' [data-job="' + job + '"]');
       const moved = row(from).getBoundingClientRect(), above = row(over).getBoundingClientRect();
       const x = moved.left + 30, y0 = moved.top + moved.height / 2, y1 = above.top + 2;
       const at = (y, buttons) => ({ bubbles: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, isPrimary: true });
       const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
       row(from).dispatchEvent(new PointerEvent("pointerdown", at(y0, 1)));
       for (let i = 1; i <= 10; i++) {
         window.dispatchEvent(new PointerEvent("pointermove", at(y0 + ((y1 - y0) * i) / 10, 1)));
         await frame();
       }
       window.dispatchEvent(new PointerEvent("pointerup", at(y1, 0)));
       await frame();`,
      LIST,
      `clip-${c}`,
      `clip-${b}`,
    );
    await until("the queue in the order dragged", async () => (await order()) === `${a}* ${c} ${b}`, 10_000).catch(
      async (e) => {
        throw new Error(`${e.message}: it's ${await order()}`);
      },
    );
    const shown = (await rows()).map((r) => r.job).join(" ");
    if (shown !== `clip-${a} clip-${c} clip-${b}`) throw new Error(`the list shows ${shown} after the drag`);
    log("dragged: the last is made next");

    // Cancel on the one being made: its ffmpeg ends, its part file goes,
    // and the one dragged up is made next.
    const parts = partFiles();
    await app.click(`${LIST} [data-job="clip-${a}"] button.cancel`);
    await until("the next one made", async () => (await order()).startsWith(`${c}* ${b}`), 30_000).catch(async (e) => {
      throw new Error(`${e.message}: it's ${await order()}`);
    });
    await until("its part file gone", () => parts.every((p) => !existsSync(p)) || null, 10_000).catch(() => {
      throw new Error(`still there after its cancel: ${parts.filter(existsSync).join(", ")}`);
    });
    const canceled = (await app.invoke("list_clips", { vodId: vod.id })).find((m) => m.id === a);
    if (canceled.status !== "error" || canceled.error !== "Canceled" || canceled.file_path)
      throw new Error(`the clip canceled is ${JSON.stringify(canceled)}`);
    await until("its row gone", async () => !(await rows()).some((r) => r.job === `clip-${a}`), 10_000);
    log(`canceled while made: ${parts.map((p) => path.basename(p)).join(", ")} gone; the clip says "Canceled"`);

    // The other two canceled (one made, one waiting): the list and its line go.
    for (const id of [b, c]) await app.click(`${LIST} [data-job="clip-${id}"] button.cancel`);
    await until("no jobs", async () => (await jobs()).length === 0, 30_000);
    await until("the jobs' line gone", () => app.js(`return !document.querySelector(".jobs-bar")`), 10_000);
    for (const m of await app.invoke("list_clips", { vodId: vod.id }))
      if (m.status !== "error" || m.error !== "Canceled") throw new Error(`a clip canceled is ${JSON.stringify(m)}`);
    await until("no part files", () => partFiles().length === 0 || null, 10_000);

    // A site's VOD with its video kept here and a long chat, written into
    // the library while the app is stopped.
    const [site] = await app.addFiles([videos(dir).video]);
    await ctx.quit();
    const title = "A stream with chat";
    const media = path.join(dataDirs()[0], "media");
    mkdirSync(media, { recursive: true });
    const kept = path.join(media, `${title} [${String(site.id).padStart(6, "0")}].mp4`);
    copyFileSync(videos(dir).video, kept);
    makeSiteVod(site.id, "twitch", TWITCH_VOD, {
      title,
      channel: "Marlowe",
      // As long as the chat (its last message is at four hours).
      duration: 14400,
      chat: "done",
      chatError: null,
      mediaPath: kept,
    });
    const wrote = writeLongChat(site.id);
    const messages = leftOf(site.id).chat;
    log(`${messages} chat messages written in ${wrote} ms`);
    if (messages !== MESSAGES) throw new Error(`${messages} chat messages in the library, not ${MESSAGES}`);
    if (wrote > WRITE_WITHIN) throw new Error(`writing the chat took ${wrote} ms: fewer messages, or a faster way`);
    app = await ctx.start();
    await app.toLibrary();

    // Removed: the erasure shows at once, waited for in the page (it can be
    // over in a second or two), and the app quit during it.
    const proc = app.driver.proc;
    const seen = await app.js(
      `const [id] = arguments;
       const erasing = () => document.querySelector(".jobs-bar .summary")?.textContent.trim().startsWith("Erasing");
       const shown = new Promise((resolve) => {
         const look = () => erasing() && (watch.disconnect(), resolve(document.querySelector(".jobs-bar .summary").textContent.trim()));
         const watch = new MutationObserver(look);
         watch.observe(document.body, { subtree: true, childList: true, characterData: true });
       });
       await window.__TAURI_INTERNALS__.invoke("delete_vod", { id });
       const jobs = await window.__TAURI_INTERNALS__.invoke("list_jobs");
       const listed = jobs.find((j) => j.kind === "erase" && j.id === id);
       const line = await Promise.race([shown, new Promise((r) => setTimeout(() => r(null), 10000))]);
       return { line, listed: listed ? listed.label : null };`,
      site.id,
    );
    if (!seen.line) throw new Error("no erasure shown under the library after the VOD was removed");
    log(`removed: "${seen.line}"${seen.listed ? `, listed as "${seen.listed}"` : " (over by the list's read)"}`);
    if (seen.listed !== null && seen.listed !== title) throw new Error(`the erasure is listed as "${seen.listed}"`);
    if (!proc?.pid) {
      log("the app's process isn't known here: the quit's time not taken");
      await ctx.quit();
    } else {
      const asked = Date.now();
      // Its answer may never come: the app ends.
      await app.js(`window.__TAURI_INTERNALS__.invoke("quit_app").catch(() => {}); return true`).catch(() => {});
      await until("the app to end", () => proc.exitedAt !== null || null, 30_000, { every: 20 });
      const took = proc.exitedAt - asked;
      await ctx.quit();
      const left = leftOf(site.id);
      log(
        `quit ${took} ms after the ask, with ${left.chat} of ${messages} chat messages left to erase` +
          (left.row
            ? ` (its row ${left.row.gone ? "marked gone" : "not yet marked gone"})`
            : " (all of it erased already)"),
      );
      if (took > QUIT_WITHIN) throw new Error(`quitting took ${took} ms with an erasure under way: it waited for it`);
    }

    // At the next start, what's left of it is erased: its chat, its kept video, its row.
    app = await ctx.start();
    await app.toLibrary();
    await until("it erased whole at the next start", () => leftOf(site.id).row === null || null, MINUTE).catch(() => {
      throw new Error(`still in the library after the next start: ${JSON.stringify(leftOf(site.id))}`);
    });
    if (existsSync(kept)) throw new Error(`its kept video is still there: ${kept}`);
    if (leftOf(site.id).chat) throw new Error("some of its chat is still there");
    await until("no jobs at the end", async () => (await jobs()).length === 0, 30_000);
    log("erased at the next start: its chat, its kept video and its row");
  },
};
