// An annotations file imported for a video file (fixtures/annotations-minute.json,
// with its chat beside it): the library's "import annotations…" picks it, and
// the VOD opens transcribed whole with what the file brought. Its three
// chapters are gray slices on the timeline, neighbours in two tints, the two
// with an icon showing it before their title; its two highlights are their
// kind's emoji on the bar's upper part, and pointing at one draws a line
// down the bar and says its title, when and why (the first's mark names
// who's in it, its cast); a click on one goes to its start, and the header
// names the chapter playing there, its icon first.
// The activity band says what the VOD is doing where it's pointed at. The
// transcript reads as a chat: one speaker's lines one after another sit under
// one head naming them in their cast colour (an alias by the cast member's
// name), the lines in it unnamed, and each chapter has a divider with its
// icon and title; the Cast button lists the cast, with roles and aliases.
// Imported again, the import's marks are replaced and a chapter of the
// user's own stays.

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
      'Above the timeline, three gray chapter slices, neighbours in different shades, read "💬 Hello", "🎮 The game" and "Goodbye", with a thin coloured band under them; the bar has two small emoji near its top (a laughing face and popcorn), and a dark tip in the bar reads "Close call 0:25–0:30" with "everyone gasped, then laughed" under it.',
    "annotations-cast":
      'The transcript\'s lines sit under headings, each a coloured circle with an initial, a name in colour ("Marlowe" twice in one colour, "Ada" in another) and a time; no heading reads "Mar"; the last two lines share one "Ada" heading; thin rules across the transcript read "💬 Hello", "🎮 The game" and "Goodbye". A small panel titled by its list shows Marlowe (Streamer · also Mar) and Ada (Friend), each with a coloured dot.',
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
    // A highlight's cast comes with its mark; one with none has none.
    const inThem = marks.filter((m) => m.kind === "highlight").map((m) => m.cast ?? []);
    if (JSON.stringify(inThem) !== '[["Mar","Ada"],[]]')
      throw new Error(`the highlights' cast: ${JSON.stringify(inThem)}`);
    // A chapter's icon comes with its mark; one with none has none.
    const icons = marks.filter((m) => m.kind === "chapter").map((m) => m.icon ?? null);
    if (JSON.stringify(icons) !== '["💬","🎮",null]') throw new Error(`the chapters' icons: ${JSON.stringify(icons)}`);
    ctx.log(`imported: transcribed whole, chat in, ${marks.length} marks`);

    // The import opens the VOD.
    await app.openVod(vod.id);
    const wrong = [];

    // The chapters: gray slices, neighbours apart, an icon before the title of those with one.
    const slices = await until(
      "the chapters' slices",
      async () => {
        const got = await app.js(`return [...document.querySelectorAll(".chapters .chapter.imported")].map((c) =>
          ({ label: c.querySelector(".label").textContent.trim(), icon: c.querySelector(".icon")?.textContent ?? "",
             color: getComputedStyle(c).backgroundColor }))`);
        return got.length === 3 && got;
      },
      MINUTE,
    );
    const labels = slices.map((s) => `${s.icon}${s.label}`).join(", ");
    if (labels !== "💬Hello, 🎮The game, Goodbye") wrong.push(`the chapters read ${labels}`);
    if (slices[0].color === slices[1].color || slices[1].color === slices[2].color)
      wrong.push(`the chapters' tints, neighbours alike: ${slices.map((s) => s.color)}`);

    // The highlights: their kind's emoji.
    const glyphs = await app.js(`return [...document.querySelectorAll(".scrubber .emoji")].map((d) =>
      ({ category: d.dataset.category, count: d.dataset.count, glyph: d.firstChild.textContent }))`);
    const shown = glyphs.map((d) => `${d.category} ${d.glyph} ${d.count}`).join(", ");
    if (shown !== "funny 😂 1, drama 🍿 1") wrong.push(`the highlights' glyphs: ${shown}`);

    // Pointing at the first: a line down the bar, and its title, times and why in the tip.
    await app.js(POINT, ".scrubber .emoji[data-category=funny]");
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
    await app.js(PRESS, ".scrubber .emoji[data-category=drama]");
    await until("the player at 0:35", async () => (await app.shownTime()) === 35, 10_000).catch(async () =>
      wrong.push(`a click on the second highlight went to ${await app.shownTime()} s, not 35`),
    );
    const chapter = await until(
      "the header's chapter",
      () => app.js(`return document.querySelector(".chapter-now")?.textContent.trim() ?? false`),
      10_000,
    ).catch(() => "(none)");
    if (chapter !== "🎮The game") wrong.push(`at 0:35 the header names the chapter "${chapter}"`);

    // The speakers, in their cast colours, over their groups of lines: an alias by its cast member's name.
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
    if (names !== "Marlowe, Ada, Marlowe, Ada") wrong.push(`the lines' speakers: ${names || "none"}`);
    else {
      const [marlowe, ada, mar] = speakers;
      if (speakers.some((s) => s.unknown)) wrong.push("a cast member's line has no cast colour");
      if (marlowe.color !== mar.color) wrong.push(`Marlowe is ${marlowe.color} and Marlowe (said as Mar) ${mar.color}`);
      if (marlowe.color === ada.color) wrong.push(`Marlowe and Ada are both ${ada.color}`);
    }

    // Each line, top to bottom: the head over it, if it starts a group, and the
    // chapter's divider over it, if any. Ada's last two lines are one group,
    // with no divider between them; the line no one is named for has no head;
    // and no line names its speaker itself.
    const chat = await app.js(`return [...document.querySelectorAll("#transcript .line")].flatMap((l) => {
      const row = l.querySelector(":scope > button.row:not(.chapter-head)");
      return row ? [{
        time: Number(row.dataset.time),
        head: l.querySelector(":scope > .group-head .speaker")?.textContent.trim() ?? null,
        disc: l.querySelector(":scope > .group-head .avatar")?.textContent.trim() ?? null,
        named: !!row.querySelector(".speaker"),
        divider: l.querySelector(":scope > .chapter-head")?.textContent.replace(/\\s+/g, "") ?? null,
      }] : [];
    })`);
    const read = chat.map(
      (l) => `${l.divider ? `[${l.divider}] ` : ""}${l.time}${l.head ? ` ${l.disc} ${l.head}` : ""}`,
    );
    const expected = ["[💬Hello] 0 M Marlowe", "8 A Ada", "[🎮Thegame] 15", "25 M Marlowe", "[Goodbye] 35 A Ada", "45"];
    if (read.join(" | ") !== expected.join(" | ")) wrong.push(`the transcript reads ${read.join(" | ")}`);
    if (chat.some((l) => l.named)) wrong.push("a line names its speaker as well as its group's head");

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
