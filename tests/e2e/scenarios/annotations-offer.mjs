// Annotations published for a VOD from a site (lib/fileHost.mjs serves
// fixtures/annotations-minute.json as that VOD's): adding the VOD asks the
// file host about it, once, and the app offers them: "A transcript and
// annotations are available for this VOD", Use, Not now, Don't ask again.
// Use fetches the file and imports it: the transcript reads as the file's,
// under its speakers' names, with its chapters, and the
// library row has the gift button. The Timeline panel's Marks toggles hide
// the imported chapters and highlights, or the user's own (here, as the
// transcript's chapter dividers), and stay as left after a restart. The gift button,
// with the file unchanged, says the VOD has the latest already and offers
// Replace anyway (no Don't ask again); replaced, the import's marks are
// brought anew and a chapter of the user's own stays. The gift button asks the
// file host first: with the file published anew it offers the newer ones; with asking
// turned off it asks nothing and says its answer is from the last check.
//
// Offline: the VOD is the stand-in yt-dlp's (lib/siteVod.mjs).

import { readFileSync } from "node:fs";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { fileURLToPath } from "node:url";

import { MINUTE, until } from "../lib/app.mjs";
import { notFilter, serveFiles } from "../lib/fileHost.mjs";
import { addSiteVod, OFFER, withSiteVods } from "../lib/siteVod.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const ID = "xe2eoffer";
const FILE = `/annotations/youtube/${ID}.json`;

/** The offer's words and buttons, or null while there's none. */
const offer = (app) =>
  app.js(
    `const d = document.querySelector(arguments[0]);
    return d && { text: d.querySelector(".offer")?.textContent.replace(/\\s+/g, " ").trim() ?? "",
      buttons: [...d.querySelectorAll(".offer button")].map((b) => b.textContent.trim()) }`,
    OFFER,
  );

/** Shows VOD `id` (its transcript and timeline: YouTube's player loads only on Play). */
async function show(app, id) {
  if (!(await app.js(`return !!document.querySelector('.vod.selected[data-key="${id}"]')`)))
    await app.click(`.vod[data-key="${id}"]`);
  await until("its transcript", () => app.js(`return !!document.querySelector("#transcript")`), MINUTE);
}

/** The transcript's chapter dividers, as their icon and title (no spaces), in order. */
const chapterHeads = (app) =>
  app.js(
    `return [...document.querySelectorAll("#transcript .chapter-head")].map((d) => d.textContent.replace(/\\s+/g, ""))`,
  );

/** Waits for the transcript's chapters to read `expected` (joined by ", "); what they read. */
const chaptersRead = (app, expected) =>
  until(`the chapters to read ${expected}`, async () => (await chapterHeads(app)).join(", ") === expected, 10_000).then(
    () => expected,
    async () => (await chapterHeads(app)).join(", "),
  );

/** The Timeline panel's Marks toggles: whether each is on, by its name. */
async function toggles(app) {
  if (!(await app.js(`return !!document.querySelector('[role=dialog][aria-label="Timeline layers"]')`)))
    await app.click("button.timeline");
  return until(
    "the Marks toggles",
    () =>
      app.js(`const s = document.querySelector('[role=dialog][aria-label="Timeline layers"] [aria-label="Marks"]');
        return s && Object.fromEntries([...s.querySelectorAll("button")].map((b) =>
          [b.textContent.replace(/\\d+/g, "").trim(), b.getAttribute("aria-pressed") === "true"]))`),
    10_000,
  );
}

const toggle = (app, name) =>
  app.js(
    `const b = [...document.querySelectorAll('[aria-label="Marks"] button')].find((b) => b.textContent.includes(arguments[0]));
     if (!b) throw new Error("no toggle " + arguments[0]);
     b.click();`,
    name,
  );

export default {
  name: "annotations-offer",
  expects: {
    "offer-after-add":
      'A small dialog titled "Transcript and annotations" over the app: the VOD\'s title in bold, "A transcript and annotations are available for this VOD.", a line in small gray print, and a row of buttons: "Don\'t ask again" as a link at the left, then "Not now" and a highlighted "Use".',
    "offer-gift":
      'In the library, the VOD\'s row has a gift icon at the left of its row of buttons. A small dialog titled "Transcript and annotations" says the VOD has the latest transcript and annotations already, with "Not now" and a highlighted "Replace anyway", and no "Don\'t ask again".',
  },
  async run(app, ctx) {
    const json = readFileSync(FIXTURE, "utf8").replace('"{PLATFORM}"', '"youtube"').replace('"{ID}"', `"${ID}"`);
    // The file's version, as its ETag: published anew when this changes.
    let etag = '"v1"';
    const host = serveFiles((method, at) =>
      at === FILE
        ? { status: 200, headers: { ETag: etag, "Content-Type": "application/json" }, body: json }
        : { status: 404 },
    );
    try {
      await withSiteVods(ctx, async (app) => {
        const vod = await addSiteVod(app, ID);
        const offered = await until("the offer", () => offer(app), MINUTE);
        await ctx.checkpoint("offer-after-add");
        const wrong = [];
        if (!offered.text.includes("A transcript and annotations are available for this VOD."))
          wrong.push(`the offer says "${offered.text}"`);
        if (offered.buttons.join(", ") !== "Don't ask again, Not now, Use")
          wrong.push(`the offer's buttons: ${offered.buttons.join(", ")}`);
        // The filter the app asks for at its start (none published here) aside.
        const asked = host.requests.filter(notFilter).map((r) => `${r.method} ${r.path}`);
        if (asked.join() !== `HEAD ${FILE}`) wrong.push(`the file host was asked: ${asked.join(", ")}`);

        // Use: fetched and imported.
        await app.clickText("Use");
        const imported = await until(
          "the import",
          async () => {
            const v = (await app.invoke("list_vods")).find((v) => v.id === vod.id);
            return v.transcribed === "all" && !(await offer(app)) && v;
          },
          MINUTE,
        );
        if (imported.annotations !== "available") wrong.push(`the VOD's annotations are "${imported.annotations}"`);
        if (!host.requests.some((r) => r.method === "GET" && r.path === FILE)) wrong.push("the file was never fetched");
        const marks = await app.invoke("list_marks", { vodId: vod.id });
        if (marks.length !== 5 || marks.some((m) => m.source !== "annotations"))
          wrong.push(`the marks: ${JSON.stringify(marks.map((m) => [m.kind, m.text, m.source]))}`);

        // The VOD (open since it was added) shows the file's speakers and chapters.
        await show(app, vod.id);
        const names = await until(
          "the speakers",
          async () => {
            const got = await app.js(
              `return [...document.querySelectorAll("#transcript .line .speaker")].map((s) => s.textContent.trim())`,
            );
            return got.length === 5 && got;
          },
          MINUTE,
        ).catch(() => []);
        // The line no one is named for is under an "Unknown" heading of its own.
        if (names.join(", ") !== "Marlowe (Vance), Ada, Unknown, Marlowe (Vance), Ada")
          wrong.push(`the transcript's speakers: ${names.join(", ")}`);
        const chapters = await chaptersRead(app, "💬Hello, 🎮Thegame, Goodbye");
        if (chapters !== "💬Hello, 🎮Thegame, Goodbye") wrong.push(`the transcript's chapters: ${chapters}`);
        const gift = `.vod[data-key="${vod.id}"] .actions button.gift`;
        await until(
          "the gift button",
          () => app.js(`return !!document.querySelector(arguments[0])`, gift),
          10_000,
        ).catch(() => wrong.push("no gift button in the VOD's row"));

        // A chapter of the user's own, then the gift button: up to date, Replace anyway.
        await app.invoke("add_mark", { vodId: vod.id, time: 50, kind: "chapter", text: "Mine" });
        await app.click(gift);
        const again = await until("the offer from the gift button", () => offer(app), 10_000);
        await ctx.checkpoint("offer-gift");
        if (!again.text.includes("This VOD has the latest transcript and annotations already."))
          wrong.push(`the gift button's offer says "${again.text}"`);
        if (again.buttons.join(", ") !== "Not now, Replace anyway")
          wrong.push(`the gift button's offer's buttons: ${again.buttons.join(", ")}`);
        const fetched = host.requests.filter((r) => r.method === "GET").length;
        await app.clickText("Replace anyway");
        await until(
          "the file fetched again",
          async () => host.requests.filter((r) => r.method === "GET").length > fetched && !(await offer(app)),
          MINUTE,
        );
        const replaced = await until(
          "the marks replaced",
          async () => {
            const all = await app.invoke("list_marks", { vodId: vod.id });
            return all.length === 6 && all;
          },
          MINUTE,
        ).catch(() => app.invoke("list_marks", { vodId: vod.id }));
        const mine = replaced.filter((m) => m.source === "user");
        if (replaced.length !== 6 || mine.length !== 1 || mine[0].text !== "Mine")
          wrong.push(`replaced, the marks: ${JSON.stringify(replaced.map((m) => [m.kind, m.text, m.source]))}`);
        else ctx.log("replaced: the import's five marks anew, and the user's chapter kept");
        const all = "💬Hello, 🎮Thegame, Goodbye, Mine";
        const both = await chaptersRead(app, all);
        if (both !== all) wrong.push(`replaced, the transcript's chapters: ${both}`);

        // The gift button asks the file host before it answers, when asking is allowed.
        // Off, it goes by the last check, and says so; on, a file published anew since is offered.
        const settings = await app.invoke("get_settings");
        const heads = () => host.requests.filter((r) => r.method === "HEAD").length;
        await app.invoke("set_settings", { settings: { ...settings, ask_for_annotations: false } });
        etag = '"v2"';
        const headsBefore = heads();
        await app.click(gift);
        const unasked = await until("the gift button's offer, asking off", () => offer(app), 10_000);
        if (heads() !== headsBefore) wrong.push("with asking off, the gift button asked the file host");
        if (
          !unasked.text.includes("When last checked, this VOD had the latest transcript and annotations.") ||
          !unasked.text.includes("turned off in Settings › Privacy")
        )
          wrong.push(`with asking off, the gift button's offer says "${unasked.text}"`);
        else ctx.log(`asking off, the gift: "${unasked.text}"`);
        await app.clickText("Not now");
        await until("the offer closed", async () => !(await offer(app)), 10_000);
        await app.invoke("set_settings", { settings: { ...settings, ask_for_annotations: true } });
        await app.click(gift);
        const published = await until("the gift button's offer, the file published anew", () => offer(app), 10_000);
        if (heads() !== headsBefore + 1)
          wrong.push(`the gift button asked the file host ${heads() - headsBefore} times`);
        if (
          !published.text.includes("A newer transcript and annotations are available for this VOD.") ||
          published.buttons.join(", ") !== "Not now, Use"
        )
          wrong.push(
            `the file published anew, the gift button's offer says "${published.text}" (${published.buttons.join(", ")})`,
          );
        else ctx.log("published anew, the gift asked the file host and offers the newer ones");
        await app.clickText("Not now");
        await until("the offer closed", async () => !(await offer(app)), 10_000);

        // The Marks toggles: both on at first; each hides its own.
        const first = await toggles(app);
        // Field by field: a page's object may come back with its keys in another order (Windows, macOS 12).
        if (!isDeepStrictEqual(first, { "Your marks": true, "Imported marks": true }))
          wrong.push(`the Marks toggles start ${JSON.stringify(first)}`);
        await toggle(app, "Imported marks");
        const mineOnly = await chaptersRead(app, "Mine");
        if (mineOnly !== "Mine") wrong.push(`imported marks hidden, the transcript's chapters: ${mineOnly}`);
        await toggle(app, "Imported marks");
        await toggle(app, "Your marks");
        const importedOnly = await chaptersRead(app, "💬Hello, 🎮Thegame, Goodbye");
        if (importedOnly !== "💬Hello, 🎮Thegame, Goodbye")
          wrong.push(`your marks hidden, the transcript's chapters: ${importedOnly}`);
        // Saved a moment after the change (layers.svelte.ts): restarted once it is.
        await until("the toggles saved", async () => (await app.invoke("get_timeline")).marks?.user === false, 10_000);

        // Remembered across a restart.
        app = await ctx.restart();
        await show(app, vod.id);
        const kept = await toggles(app);
        if (!isDeepStrictEqual(kept, { "Your marks": false, "Imported marks": true }))
          wrong.push(`after a restart, the Marks toggles are ${JSON.stringify(kept)}`);
        await toggle(app, "Your marks");
        const back = await chaptersRead(app, all);
        if (back !== all) wrong.push(`after a restart, both shown again, the transcript's chapters: ${back}`);
        await app.key("Escape");
        if (wrong.length) throw new Error(wrong.join("; "));
      });
    } finally {
      host.stop();
    }
  },
};
