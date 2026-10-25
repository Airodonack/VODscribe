// An annotations file imported for a video file (fixtures/annotations-minute.json,
// with its chat beside it): the library's "import annotations…" picks it, and
// the VOD opens transcribed whole with what the file brought. Its three
// chapters are slices on the timeline, each in its own colour; its two
// highlights are dots on the bar's upper part, coloured by kind, and pointing
// at one draws a line down the bar and says its title, when and why; a click
// on one goes to its start, and the header names the chapter playing there.
// The activity band says what the VOD is doing where it's pointed at. The
// transcript's lines name their speaker in their cast colour (an alias in the
// same colour as the name), and the Cast button lists both, with roles and
// aliases. Imported again, the import's marks are replaced and a chapter of
// the user's own stays.

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { MINUTE, until } from "../lib/app.mjs";
import { minute } from "../lib/fixtures.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "..", "fixtures", "annotations-minute.json");
const CHAT = path.join(here, "..", "fixtures", "annotations-minute.chat.json");

/** In the page: a pointer moved onto the middle of the element `arguments[0]` (a hover, as the timeline hears it). */
const POINT = `
  const el = document.querySelector(arguments[0]);
  if (!el) return false;
  const box = el.getBoundingClientRect();
  const at = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, bubbles: true,
    pointerId: 1, pointerType: "mouse", isPrimary: true };
  el.dispatchEvent(new PointerEvent("pointermove", at));
  return true;`;

/** In the page: a click on the element `arguments[0]`, as the timeline hears it (down on it, up on the window). */
const PRESS = `
  const el = document.querySelector(arguments[0]);
  const box = el.getBoundingClientRect();
  const at = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, bubbles: true,
    pointerId: 1, pointerType: "mouse", isPrimary: true };
  el.dispatchEvent(new PointerEvent("pointerdown", at));
  window.dispatchEvent(new PointerEvent("pointerup", at));`;

const tip = (app) =>
  app.js(`return document.querySelector(".scrubber .tip")?.textContent.replace(/\\s+/g, " ").trim() ?? ""`);

export default {
  name: "annotations",
  expects: {
    "annotations-timeline":
      'Above the timeline, three tinted chapter slices in different colours read "Hello", "The game" and "Goodbye", with a thin coloured band under them; the bar has two small coloured dots near its top, and a dark tip in the bar reads "Close call 0:25–0:30" with "everyone gasped, then laughed" under it.',
    "annotations-cast":
      'The transcript\'s lines start with a speaker\'s name in colour ("Marlowe" and "Mar" in the same colour, "Ada" in another), and a small panel titled by its list shows Marlowe (Streamer · also Mar) and Ada (Friend), each with a coloured dot.',
  },
  async run(app, ctx) {
    const dir = path.join(ctx.out, "fixtures");
    const file = minute(dir);
    await app.toLibrary();
    const [vod] = await app.addFiles([file]);
    // The fixture, made this file's: its source names the VOD the library made of it.
    const json = readFileSync(FIXTURE, "utf8")
      .replace('"{PLATFORM}"', JSON.stringify(vod.platform))
      .replace('"{ID}"', JSON.stringify(vod.platform_id));
    const annotations = path.join(dir, "minute-annotations.json");
    writeFileSync(annotations, json);
    writeFileSync(path.join(dir, "minute-annotations.chat.json"), readFileSync(CHAT));

    const importIt = async () => {
      await app.pickFiles([annotations]);
      await app.clickText("import annotations…");
    };
    await importIt();
    const imported = await until(
      "the import",
      async () => {
        const error = await app.js(`return document.querySelector(".add-error")?.textContent.trim() ?? ""`);
        if (error) throw new Error(`the import failed: ${error}`);
        const v = (await app.invoke("list_vods")).find((v) => v.id === vod.id);
        return v.transcribed === "all" && v.chat === "done" && v;
      },
      MINUTE,
    );
    if (imported.outdated) throw new Error("the imported transcript says it's from an older version");
    const marks = await app.invoke("list_marks", { vodId: vod.id });
    const summary = marks.map((m) => `${m.kind} ${m.start} ${m.text} ${m.source}`);
    if (marks.length !== 5 || marks.some((m) => m.source !== "annotations"))
      throw new Error(`the marks: ${JSON.stringify(summary)}`);
    ctx.log(`imported: transcribed whole, chat in, ${marks.length} marks`);

    // The import opens the VOD.
    await app.openVod(vod.id);
    const wrong = [];

    // The chapters: slices, each its own colour.
    const slices = await until(
      "the chapters' slices",
      async () => {
        const got = await app.js(`return [...document.querySelectorAll(".chapters .chapter.imported")].map((c) =>
          ({ label: c.textContent.trim(), color: getComputedStyle(c).backgroundColor }))`);
        return got.length === 3 && got;
      },
      MINUTE,
    );
    const labels = slices.map((s) => s.label).join(", ");
    if (labels !== "Hello, The game, Goodbye") wrong.push(`the chapters read ${labels}`);
    if (new Set(slices.map((s) => s.color)).size !== 3)
      wrong.push(`the chapters' colours: ${slices.map((s) => s.color)}`);

    // The highlights: dots in their kind's colour.
    const dots = await app.js(`return [...document.querySelectorAll(".scrubber .dot")].map((d) =>
      ({ category: d.dataset.category, color: getComputedStyle(d).backgroundColor }))`);
    if (dots.map((d) => d.category).join(" ") !== "funny drama") wrong.push(`the dots: ${JSON.stringify(dots)}`);
    else if (dots[0].color === dots[1].color) wrong.push(`both dots are ${dots[0].color}`);

    // Pointing at the first: a line down the bar, and its title, times and why in the tip.
    await app.js(POINT, ".scrubber .dot[data-category=funny]");
    const said = await until(
      "the highlight's tip",
      async () => (await tip(app)).includes("Close call") && tip(app),
      10_000,
    ).catch(async () => `(none: "${await tip(app)}")`);
    if (said !== "Close call 0:25–0:30everyone gasped, then laughed")
      wrong.push(`pointing at the first highlight, the tip says "${said}"`);
    if (!(await app.js(`return !!document.querySelector(".scrubber .moment-line")`)))
      wrong.push("pointing at a highlight draws no line down the timeline");
    await ctx.checkpoint("annotations-timeline");

    // The activity band: what the VOD is doing there.
    await app.js(POINT, '.activity .doing[data-activity="1"]');
    const doing = await until(
      "the activity's tip",
      async () => (await tip(app)).startsWith("Game") && tip(app),
      10_000,
    ).catch(async () => `(none: "${await tip(app)}")`);
    if (doing !== "Game: Chess 0:20–0:40") wrong.push(`pointing at the activity band, the tip says "${doing}"`);

    // A click on the second highlight goes to its start, in the chapter the header then names.
    await app.js(PRESS, ".scrubber .dot[data-category=drama]");
    await until("the player at 0:35", async () => (await app.shownTime()) === 35, 10_000).catch(async () =>
      wrong.push(`a click on the second highlight went to ${await app.shownTime()} s, not 35`),
    );
    const chapter = await until(
      "the header's chapter",
      () => app.js(`return document.querySelector(".chapter-now")?.textContent.trim() ?? false`),
      10_000,
    ).catch(() => "(none)");
    if (chapter !== "The game") wrong.push(`at 0:35 the header names the chapter "${chapter}"`);

    // The speakers, in their cast colours: an alias as its name.
    const speakers = await until(
      "the speakers' names",
      async () => {
        const got = await app.js(`return [...document.querySelectorAll("#transcript .speaker")].map((s) =>
          ({ name: s.textContent.trim(), color: getComputedStyle(s).color, unknown: s.classList.contains("unknown") }))`);
        return got.length === 4 && got;
      },
      MINUTE,
    ).catch(() => []);
    const names = speakers.map((s) => s.name).join(", ");
    if (names !== "Marlowe, Ada, Mar, Ada") wrong.push(`the lines' speakers: ${names || "none"}`);
    else {
      const [marlowe, ada, mar] = speakers;
      if (speakers.some((s) => s.unknown)) wrong.push("a cast member's line has no cast colour");
      if (marlowe.color !== mar.color) wrong.push(`Marlowe is ${marlowe.color} and Mar ${mar.color}`);
      if (marlowe.color === ada.color) wrong.push(`Marlowe and Ada are both ${ada.color}`);
    }

    // The cast, from the transcript's header.
    await app.click(".cast-toggle");
    const cast = await until(
      "the cast list",
      () =>
        app.js(`const list = document.querySelector('[role=dialog][aria-label="Cast"]');
          return list ? [...list.querySelectorAll("li")].map((li) => li.textContent.replace(/\\s+/g, " ").trim()) : false`),
      10_000,
    ).catch(() => []);
    if (cast.join(" | ") !== "Marlowe Streamer · also Mar | Ada Friend")
      wrong.push(`the cast list: ${cast.join(" | ")}`);
    await ctx.checkpoint("annotations-cast");
    await app.key("Escape");

    // Imported again: the import's marks replaced, the user's chapter kept.
    await app.invoke("add_mark", { vodId: vod.id, time: 50, kind: "chapter", text: "Mine" });
    await importIt();
    const again = await until(
      "the second import",
      async () => {
        const all = await app.invoke("list_marks", { vodId: vod.id });
        return all.length === 6 && all;
      },
      MINUTE,
    ).catch(async () => app.invoke("list_marks", { vodId: vod.id }));
    const mine = again.filter((m) => m.source === "user");
    if (again.length !== 6 || mine.length !== 1 || mine[0].text !== "Mine")
      wrong.push(`imported again, the marks: ${JSON.stringify(again.map((m) => [m.kind, m.text, m.source]))}`);
    else ctx.log("imported again: the import's five marks anew, and the user's chapter kept");

    if (wrong.length) throw new Error(wrong.join("; "));
  },
};
