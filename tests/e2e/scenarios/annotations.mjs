// An annotations file imported for a video file (fixtures/annotations-minute.json,
// with its chat beside it), the way the offer of a VOD's annotations brings
// one in; the VOD opens transcribed whole with what the file brought. Its three
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
// The chapters and highlights pane is the window's right column, full
// height, the player and transcript between it and the library, the chapter
// and highlight playing lit. Pointing at a highlight's row lights its glyph
// and draws its line down the bar; pointing at a chapter's slice lights its
// row. A click on a highlight's row plays it. Dragged onto "Make new edit",
// a highlight's row makes an edit holding it as a group named for it, one
// band as a clip's pill goes in; from the keyboard, A on the other's row
// asks which edit and puts it there, a group too; the edit open, its pane
// has the highlights it holds. On a window too narrow for three columns the
// pane is a tab beside the transcript's.
// A character played by a streamer is named the streamer first, the
// character after ("Vance (Marlowe)"): on their heads, in the Cast button
// (with their channels) and in a highlight's tooltip, grouped and coloured as
// the character. A cast member with an account is headed by their picture,
// from the image server (the run's file host, lib/fileHost.mjs), their name a
// link to their channel; one whose picture isn't there keeps their initial,
// the streamer's where they're named first.
// The Timeline panel's Imported marks toggle hides the import's chapters
// and highlights from the timeline (and the pane with them), and shows them
// again. Imported again, the import's marks are replaced and a chapter of
// the user's own stays.

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { serveFiles } from "../lib/fileHost.mjs";
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

/** In the page: a pointer coming onto the element `arguments[0]`, and leaving it (as a row in the pane hears it). */
const ENTER = `
  const el = document.querySelector(arguments[0]);
  if (!el) throw new Error("nothing at " + arguments[0]);
  el.dispatchEvent(new PointerEvent("pointerenter", { pointerId: 1, pointerType: "mouse", isPrimary: true }));`;
const LEAVE = ENTER.replaceAll("pointerenter", "pointerleave");

/** In the page: a click on the element `arguments[0]`, as the timeline hears it (down on it, up on the window). */
const PRESS = `
  const el = document.querySelector(arguments[0]);
  const box = el.getBoundingClientRect();
  const at = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, bubbles: true,
    pointerId: 1, pointerType: "mouse", isPrimary: true };
  el.dispatchEvent(new PointerEvent("pointerdown", at));
  window.dispatchEvent(new PointerEvent("pointerup", at));`;

/** A pointer event's fields at (x, y) with `buttons` down, in the page. */
const AT = `const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });`;

/** The pane's rows, top to bottom: "chapter Hello", "highlight Close call"…, a lit one with a *. */
const PANE_ROWS = `return [...document.querySelectorAll(".annotations-pane .pane-row")].map((r) =>
  (r.classList.contains("chapter") ? "chapter " : "highlight ") + r.querySelector(".name").textContent.trim() +
  (r.classList.contains("lit") ? "*" : ""))`;

/** Marlowe's picture: four pixels square, all yellow. */
const PICTURE = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAEUlEQVR4nGP4eoQBjhiI4wAA3EIbkanqb5MAAAAASUVORK5CYII=",
  "base64",
);

/** The image server: Marlowe's account sent on to their picture, as the real one sends the browser to the site's; anyone else's not there. */
const pictures = (method, path) =>
  path === "/avatar/twitch/marlowe_live"
    ? { status: 302, headers: { Location: "/pictures/marlowe.png" } }
    : path === "/pictures/marlowe.png"
      ? { status: 200, headers: { "Content-Type": "image/png" }, body: PICTURE }
      : { status: 404 };

const tip = (app) =>
  app.js(`return document.querySelector(".scrubber .tip")?.textContent.replace(/\\s+/g, " ").trim() ?? ""`);

export default {
  name: "annotations",
  expects: {
    "annotations-timeline":
      'Above the timeline, three gray chapter slices, neighbours in different shades, read "💬 Hello", "🎮 The game" and "Goodbye", with a thin coloured band under them; the bar has two small emoji near its top (a laughing face and popcorn), and a dark tip in the bar reads "Close call 0:25–0:30" with "everyone gasped, then laughed" under it.',
    "annotations-pane":
      'On the window\'s right, a column the full height of the window lists "Hello", "The game" and "Goodbye" with their emoji and times, "Close call" (a laughing face) and "The twist" (popcorn) indented under "The game", which is lit with "The twist".',
    "annotations-cast":
      'The transcript\'s lines sit under headings, each a circle (Marlowe\'s a plain yellow picture, Ada\'s a coloured circle with "A"), a name in colour ("Vance (Marlowe)" twice in one colour, "Ada" in another) and a time; no heading reads "Mar"; the last two lines share one "Ada" heading; thin rules across the transcript read "💬 Hello", "🎮 The game" and "Goodbye". A small panel titled by its list shows Vance (Marlowe) (Streamer · also Mar, twitch.tv/marlowe_live) and Ada (Friend, kick.com/ada_plays), each with a coloured dot.',
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

    // The image server answers from before the import: the VOD is open from
    // its adding, so its transcript asks for the cast's pictures as soon as
    // the import reaches it, and a picture asked of nobody stays its initial.
    const host = serveFiles(pictures);
    const importIt = () => app.invoke("import_annotations_file", { path: annotations });
    await importIt();
    const imported = await until(
      "the import",
      async () => {
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
    if (names !== "Vance (Marlowe), Ada, Vance (Marlowe), Ada") wrong.push(`the lines' speakers: ${names || "none"}`);
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
    // Marlowe is played by Vance, named first: his disc's initial is Vance's.
    const expected = [
      "[💬Hello] 0 V Vance (Marlowe)",
      "8 A Ada",
      "[🎮Thegame] 15",
      "25 V Vance (Marlowe)",
      "[Goodbye] 35 A Ada",
      "45",
    ];
    if (read.join(" | ") !== expected.join(" | ")) wrong.push(`the transcript reads ${read.join(" | ")}`);
    if (chat.some((l) => l.named)) wrong.push("a line names its speaker as well as its group's head");

    // The heads of those with an account: the name a link to their channel,
    // saying where it goes (the head itself says who plays them); the disc their picture once it has loaded, or
    // their initial when there's none (Ada's), never empty.
    const heads = await until(
      "the cast's pictures",
      async () => {
        const got = await app.js(`return [...document.querySelectorAll("#transcript .group-head")].map((h) => {
          const name = h.querySelector(".speaker");
          const img = h.querySelector(".avatar img");
          return { name: name.textContent.trim(), link: name.tagName === "A" ? name.getAttribute("href") : null,
            title: name.title, disc: h.querySelector(".avatar").getAttribute("href"),
            picture: img ? (!img.complete ? "loading" : img.naturalWidth > 0 && img.classList.contains("loaded") ? "shown" : "none") : "no img",
            initial: getComputedStyle(h.querySelector(".avatar")).color !== "rgba(0, 0, 0, 0)" };
        })`);
        return got.length === 4 && got.every((h) => h.picture !== "loading") && got;
      },
      MINUTE,
    ).catch(() => []);
    const want = [
      {
        name: "Vance (Marlowe)",
        link: "https://www.twitch.tv/marlowe_live",
        title: "twitch.tv/marlowe_live",
        picture: "shown",
        initial: false,
      },
      { name: "Ada", link: "https://kick.com/ada_plays", title: "kick.com/ada_plays", picture: "none", initial: true },
    ];
    for (const [i, h] of heads.entries()) {
      const w = want[i % 2];
      const got = { name: h.name, link: h.link, title: h.title, picture: h.picture, initial: h.initial };
      if (JSON.stringify(got) !== JSON.stringify(w)) wrong.push(`head ${i + 1}: ${JSON.stringify(got)}`);
      if (h.disc !== w.link) wrong.push(`head ${i + 1}'s disc links to ${h.disc}`);
    }
    if (!heads.length) wrong.push("the heads' pictures never settled");
    const asked = [...new Set(host.requests.map((r) => `${r.method} ${r.path}`))].sort();
    if (
      JSON.stringify(asked) !==
      JSON.stringify(["GET /avatar/kick/ada_plays", "GET /avatar/twitch/marlowe_live", "GET /pictures/marlowe.png"])
    )
      wrong.push(`the image server was asked: ${asked.join(", ")}`);
    host.stop();

    // The cast, from the transcript's header.
    await app.click(".cast-toggle");
    const cast = await until(
      "the cast list",
      () =>
        app.js(`const list = document.querySelector('[role=dialog][aria-label="Cast"]');
          return list ? [...list.querySelectorAll("li")].map((li) => li.textContent.replace(/\\s+/g, " ").trim()) : false`),
      10_000,
    ).catch(() => []);
    if (
      cast.join(" | ") !== "Vance (Marlowe) Streamer · also Mar twitch.tv/marlowe_live | Ada Friend kick.com/ada_plays"
    )
      wrong.push(`the cast list: ${cast.join(" | ")}`);
    await ctx.checkpoint("annotations-cast");
    await app.key("Escape");

    // The Timeline panel's "Imported marks" toggle takes the import's chapters
    // and highlights off the timeline, and puts them back.
    const onTimeline = () =>
      app.js(`return document.querySelectorAll(".chapters .chapter.imported, .scrubber .emoji").length`);
    const flip = () =>
      app.js(`const b = [...document.querySelectorAll('[aria-label="Marks"] button')].find((b) => b.textContent.includes("Imported marks"));
        if (!b) throw new Error("no Imported marks toggle");
        b.click();`);
    await app.click("button.timeline");
    await until(
      "the Marks toggles",
      () => app.js(`return !!document.querySelector('[aria-label="Marks"] button')`),
      10_000,
    );
    await flip();
    await until("the imported marks hidden", async () => (await onTimeline()) === 0, 10_000).catch(async () =>
      wrong.push(`with imported marks hidden, ${await onTimeline()} still on the timeline`),
    );
    await until(
      "the pane gone with them",
      () => app.js(`return !document.querySelector(".annotations-pane")`),
      10_000,
    ).catch(() => wrong.push("with imported marks hidden, the chapters and highlights pane still shows"));
    await flip();
    await until("the imported marks back", async () => (await onTimeline()) === 5, 10_000).catch(async () =>
      wrong.push(`with imported marks shown again, ${await onTimeline()} on the timeline`),
    );
    await app.key("Escape");

    // The chapters and highlights pane, right of the transcript, the
    // chapter and highlight playing (at 0:35) lit.
    const [funny, drama] = marks.filter((m) => m.kind === "highlight");
    const row = (m) => `.annotations-pane button.row-main[data-row="highlight-${m.id}"]`;
    const paneRows = await until(
      "the pane's rows",
      async () => {
        const got = await app.js(PANE_ROWS);
        return got.length === 5 && got;
      },
      10_000,
    ).catch(async () => app.js(PANE_ROWS));
    const expectedRows = [
      "chapter Hello",
      "chapter The game*",
      "highlight Close call",
      "highlight The twist*",
      "chapter Goodbye",
    ];
    if (paneRows.join(" | ") !== expectedRows.join(" | ")) wrong.push(`the pane reads ${paneRows.join(" | ")}`);
    // Who's in the first highlight, its chips in the cast's colours, their
    // tooltip naming the streamer first ("Mar" in the file is Marlowe).
    const chips = await app.js(
      `const faces = document.querySelector(arguments[0])?.querySelector(".faces");
      return faces ? { title: faces.title, colors: [...faces.querySelectorAll(".face")].map((f) => getComputedStyle(f).backgroundColor) } : null`,
      row(funny),
    );
    if (chips?.title !== "Vance (Marlowe), Ada") wrong.push(`the first highlight's chips say "${chips?.title}"`);
    else if (
      speakers.length === 4 &&
      JSON.stringify(chips.colors) !== JSON.stringify([speakers[0].color, speakers[1].color])
    )
      wrong.push(`the chips' colours ${chips.colors} aren't the speakers' ${speakers[0].color}, ${speakers[1].color}`);
    const placed =
      await app.js(`const pane = document.querySelector(".app > .pane.side .annotations-pane")?.getBoundingClientRect();
      const main = document.querySelector(".main")?.getBoundingClientRect();
      return pane && main ? { left: pane.left, right: pane.right, top: pane.top, bottom: pane.bottom, width: pane.width,
        mainRight: main.right, innerWidth, innerHeight } : null`);
    if (
      !placed ||
      placed.left < placed.mainRight - 1 ||
      Math.abs(placed.right - placed.innerWidth) > 1 ||
      placed.top > 1 ||
      Math.abs(placed.bottom - placed.innerHeight) > 1 ||
      placed.width > 320
    )
      wrong.push(`the pane isn't the window's right column, full height: ${JSON.stringify(placed)}`);
    await ctx.checkpoint("annotations-pane");

    // Pointing at a highlight's row lights its glyph on the timeline and
    // draws its line down the bar; leaving the row lets go of both.
    // Lit, a mark keeps its own box (a ring round the glyph, an outline on
    // the slice, the star recoloured), never stretched to the bar's height.
    const paneRow = (m) => `.annotations-pane .pane-row:has(> .line > button.row-main[data-row="${m}"])`;
    const BOX = `const r = document.querySelector(arguments[0])?.getBoundingClientRect();
      return r ? [r.left, r.top, r.width, r.height].map((n) => Math.round(n)) : null`;
    const keepsBox = async (what, at, was) => {
      const now = await app.js(BOX, at);
      if (!was || !now || now.some((n, i) => Math.abs(n - was[i]) > 1))
        wrong.push(`${what}, lit, is drawn on ${JSON.stringify(now)}, not its own box ${JSON.stringify(was)}`);
    };
    const glyphAt = `.scrubber .emoji[data-category="funny"]`;
    const glyphBox = await app.js(BOX, glyphAt);
    if (!glyphBox || Math.abs(glyphBox[2] - 16) > 1 || Math.abs(glyphBox[3] - 16) > 1)
      wrong.push(`the first highlight's glyph is ${JSON.stringify(glyphBox)}, not 16 by 16`);
    await app.js(ENTER, paneRow(`highlight-${funny.id}`));
    const lit = await until(
      "the row's glyph lit",
      () =>
        app.js(`const g = document.querySelector(".scrubber .emoji.hovered");
          return !!g && !!document.querySelector(".scrubber .moment-line") && g.dataset.category`),
      10_000,
    ).catch(() => "(none)");
    if (lit !== "funny") wrong.push(`pointing at the first highlight's row lit the glyph "${lit}" and its line`);
    await keepsBox("the first highlight's glyph", glyphAt, glyphBox);
    await app.js(LEAVE, paneRow(`highlight-${funny.id}`));
    await until(
      "the glyph let go",
      () => app.js(`return !document.querySelector(".scrubber .emoji.hovered, .scrubber .moment-line")`),
      10_000,
    ).catch(() => wrong.push("off the row, its glyph stays lit"));
    // And the other way: pointing at a chapter's slice outlines its row (hover, not lit).
    const game = marks.find((m) => m.kind === "chapter" && m.text === "The game");
    const sliceAt = `.chapters .chapter[data-chapter="${game.id}"]`;
    const sliceBox = await app.js(BOX, sliceAt);
    const FILL = `const el = document.querySelector(arguments[0]); return el && getComputedStyle(el).backgroundColor`;
    const sliceFill = await app.js(FILL, sliceAt);
    await app.js(POINT, sliceAt);
    const rowLit = await until(
      "the slice's row lit",
      () =>
        app.js(`const rows = [...document.querySelectorAll(".annotations-pane .pane-row.hover")];
          const slice = document.querySelector(".chapters .chapter.hovered");
          return rows.length && slice ? rows.map((r) => r.querySelector(".name").textContent.trim()).join(", ") : false`),
      10_000,
    ).catch(() => "(none)");
    if (rowLit !== "The game") wrong.push(`pointing at "The game"'s slice lit the rows "${rowLit}"`);
    await keepsBox(`"The game"'s slice`, sliceAt, sliceBox);
    const litFill = await app.js(FILL, sliceAt);
    if (litFill !== sliceFill) wrong.push(`"The game"'s slice, lit, turns ${litFill} from ${sliceFill}`);
    await app.js(POINT, "#transcript");
    await until(
      "the row let go",
      () => app.js(`return !document.querySelector(".annotations-pane .pane-row.hover, .chapters .chapter.hovered")`),
      10_000,
    ).catch(() => wrong.push("off the slice, its row stays lit"));
    // A highlight of the user's own (M, at 0:45, clear of the imported
    // ones) is a star; its row pointed at lights the star, in its own box.
    // Delete removes it again.
    await app.key("ArrowRight");
    await app.key("ArrowRight");
    await until("the player at 0:45", async () => (await app.shownTime()) === 45, 10_000);
    await app.key("m");
    const note = `[role=dialog][aria-label="Highlight note"] input`;
    await until("the note box", () => app.js(`return !!document.querySelector(arguments[0])`, note), 10_000);
    await app.type(note, "Mine");
    await app.submit(note);
    await until("the note box to close", () => app.js(`return !document.querySelector(arguments[0])`, note), 10_000);
    const star = await until(
      "the user's highlight",
      async () =>
        (await app.invoke("list_marks", { vodId: vod.id })).find((m) => m.kind === "highlight" && m.source === "user"),
      10_000,
    );
    const starAt = `.scrubber .star[data-mark="${star.id}"]`;
    const starBox = await until("its star", () => app.js(BOX, starAt), 10_000);
    await app.js(ENTER, paneRow(`highlight-${star.id}`));
    await until(
      "its star lit",
      () => app.js(`return !!document.querySelector(arguments[0] + ".hovered")`, starAt),
      10_000,
    )
      .then(() => keepsBox("the user's star", starAt, starBox))
      .catch(() => wrong.push("pointing at the user's highlight's row didn't light its star"));
    await app.js(LEAVE, paneRow(`highlight-${star.id}`));
    await app.key("Delete");
    await until(
      "the user's highlight gone",
      async () => !(await app.invoke("list_marks", { vodId: vod.id })).some((m) => m.source === "user"),
      10_000,
    ).catch(() => wrong.push("Delete didn't remove the user's highlight"));

    // A click on the first highlight's row plays it, from its start.
    await app.click(row(funny));
    await until("the player at 0:25", async () => (await app.shownTime()) === 25, 10_000).catch(async () =>
      wrong.push(`a click on the first highlight's row went to ${await app.shownTime()} s, not 25`),
    );
    await app.pause().catch(() => {});

    // Its row dragged onto "Make new edit": an edit holding it as a group named for it.
    await app.js(
      `${AT}
       const el = document.querySelector(arguments[0]);
       const r = el.getBoundingClientRect();
       const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
       el.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
       window.dispatchEvent(new PointerEvent("pointermove", at(x - 10, y - 10, 1)));
       window.__drag = [x - 10, y - 10];`,
      row(funny),
    );
    await until(
      '"Make new edit", to drop on',
      () => app.js(`return !!document.querySelector("[data-new-edit]")`),
      10_000,
    );
    await app.js(
      `${AT}
       const r = document.querySelector("[data-new-edit]").getBoundingClientRect();
       const [x0, y0] = window.__drag;
       const [x1, y1] = [r.left + r.width / 2, r.top + r.height / 2];
       for (let i = 1; i <= 5; i++) window.dispatchEvent(new PointerEvent("pointermove", at(x0 + ((x1 - x0) * i) / 5, y0 + ((y1 - y0) * i) / 5, 1)));
       window.__drag = [x1, y1];`,
    );
    await until(
      '"Make new edit" lit as where it would go',
      () => app.js(`return !!document.querySelector("[data-new-edit].target")`),
      10_000,
    );
    await app.js(
      `${AT}
       const [x, y] = window.__drag;
       window.dispatchEvent(new PointerEvent("pointerup", at(x, y, 0)));`,
    );
    const [made] = await until(
      "the new edit",
      async () => {
        const all = await app.invoke("list_edits");
        return all.length === 1 && all;
      },
      10_000,
    ).catch(() => []);
    /** Edit `id`'s pieces as "start-end group", for what's checked. */
    const shapeOf = async (id) =>
      (await app.invoke("get_edit", { id })).pieces.map((p) => `${p.start}-${p.end} ${p.group?.title ?? "(no group)"}`);
    if (!made) wrong.push("the highlight dropped on Make new edit made no edit");
    else {
      const shape = await until(
        "the highlight in the edit",
        async () => {
          const got = await shapeOf(made.id);
          return got.length === 1 && got;
        },
        10_000,
      ).catch(() => shapeOf(made.id));
      if (shape.join(" | ") !== "25-30 Close call")
        wrong.push(`the dragged highlight came in as ${shape.join(" | ") || "nothing"}, not a group named for it`);
      else ctx.log("a highlight's row dragged onto Make new edit: an edit holding it as a group");

      // From the keyboard: A on the other's row asks which edit, its first
      // choice focused; Enter on it (a click, as Enter on a button is) and on
      // "Timeline, at the end" puts it there, a group too.
      await app.js(
        `const el = document.querySelector(arguments[0]);
         el.focus();
         el.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true }));`,
        row(drama),
      );
      const enter = async (label) => {
        const got = await until(
          `"${label}" focused`,
          () =>
            app.js(`const el = document.activeElement;
              return !!el?.closest('[role=dialog][aria-label="Add to an edit"]') && el.textContent.trim()`),
          10_000,
        ).catch(() => "(nothing)");
        if (!got.includes(label)) {
          wrong.push(`in Add to an edit, the focus is on "${got}", not "${label}"`);
          return false;
        }
        await app.js(`document.activeElement.click()`);
        return true;
      };
      if ((await enter(made.title)) && (await enter("Timeline, at the end"))) {
        const both = await until(
          "the second highlight in the edit",
          async () => {
            const got = await shapeOf(made.id);
            return got.length === 2 && got;
          },
          10_000,
        ).catch(() => shapeOf(made.id));
        if (both.join(" | ") !== "25-30 Close call | 35-38 The twist")
          wrong.push(`after A on the second highlight, the edit holds ${both.join(" | ")}`);
        else ctx.log("A on a highlight's row put it in the edit as a group");
      }

      // The edit open: its own highlights (those of its stretches) in the
      // pane, on its timeline; then the VOD again.
      await app.click(`.vod.edit[data-key="-${made.id}"] button.main`);
      const inEdit = await until(
        "the edit's pane",
        async () => {
          const got = await app.js(
            `return document.querySelector("section.edit-view") ? (${PANE_ROWS.replace(/^return /, "")}) : []`,
          );
          return got.length === 2 && got;
        },
        10_000,
      ).catch(async () => app.js(PANE_ROWS));
      if (inEdit.map((r) => r.replace("*", "")).join(" | ") !== "highlight Close call | highlight The twist")
        wrong.push(`the edit's pane reads ${inEdit.join(" | ") || "nothing"}`);
      await app.click(`.vod[data-key="${vod.id}"] button.main`);
      await until("the VOD's pane again", async () => (await app.js(PANE_ROWS)).length === 5, 10_000).catch(() =>
        wrong.push("back on the VOD, its pane didn't come back"),
      );
    }

    // A window too narrow for three columns (under 1100 px): no right
    // column; the pane is a tab beside the transcript's, the transcript's
    // open; the other tab shows the pane in its place.
    const was = await app.js(`return { width: innerWidth, height: innerHeight }`);
    const narrow = await resizeInner(app, 1000, Math.min(was.height, 760));
    if (narrow.width >= 1100) ctx.log(`the window stayed ${narrow.width} px wide: no tabs to try`);
    else {
      await until("the column gone", () => app.js(`return !document.querySelector(".app > .pane.side")`), 10_000).catch(
        () => wrong.push(`at ${narrow.width} px the pane is still a column`),
      );
      const tabs = () =>
        app.js(`return [...document.querySelectorAll(".pane-tabs [role=tab]")].map((t) =>
          t.textContent.trim() + (t.getAttribute("aria-selected") === "true" ? "*" : ""))`);
      const shown = await until("the tabs", async () => (await tabs()).length === 2 && tabs(), 10_000).catch(() =>
        tabs(),
      );
      if (shown.join(" | ") !== "Transcript* | Chapters and highlights")
        wrong.push(`at ${narrow.width} px the tabs read ${shown.join(" | ") || "nothing"}`);
      else {
        await app.click(".pane-tabs [role=tab]:nth-child(2)");
        const swapped = await until(
          "the pane in the transcript's place",
          () =>
            app.js(`return !!document.querySelector(".annotations-pane") &&
              getComputedStyle(document.querySelector(".body")).display === "none"`),
          10_000,
        ).catch(() => false);
        if (!swapped) wrong.push("the pane's tab didn't show the pane in the transcript's place");
        await app.click(".pane-tabs [role=tab]:nth-child(1)");
        await until(
          "the transcript back",
          () =>
            app.js(`return !document.querySelector(".annotations-pane") &&
              getComputedStyle(document.querySelector(".body")).display !== "none"`),
          10_000,
        ).catch(() => wrong.push("the transcript's tab didn't bring the transcript back"));
      }
    }
    await resizeInner(app, was.width, was.height);

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
