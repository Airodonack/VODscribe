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
// name), the lines in it unnamed, a line no one is named for under an
// "Unknown" head of its own (a person's outline, no link, as the transcript
// names others), and each chapter has a divider with its
// icon and title; the Cast button lists the cast, with roles and aliases.
// The chapters and highlights pane is the window's right column, full
// height, the player and transcript between it and the library, the chapter
// and highlight playing lit. Pointing at a highlight's row lights its glyph
// and draws its line down the bar; pointing at a chapter's slice lights its
// row. A click on a highlight's row plays it; opened, it names who's in it
// by their short names, those with an account a link to the streamer's
// channel that leaves the row open. Dragged onto "Make new edit",
// a highlight's row makes an edit holding it as a group named for it, one
// band as a clip's pill goes in; from the keyboard, A on the other's row
// asks which edit and puts it there, a group too; the edit open, its pane
// has the highlights it holds, as the import made them: their kind's emoji
// on its timeline, their length, why and who's in them in its pane, the
// Imported marks toggle taking them off. On a window too narrow for three
// columns the pane is a tab beside the transcript's.
// A character played by a streamer is named the character first, the
// streamer after ("Marlowe (Vance)"): on their heads, in the Cast button
// (with their channels) and in a highlight's tooltip, grouped and coloured as
// the character. A cast member with an account is headed by their picture,
// from the image server (the run's file host, lib/fileHost.mjs), their name a
// link to their channel; one whose picture isn't there keeps their initial,
// the character's. With pictures turned off in Settings › Privacy, every disc
// is its initial and the image server isn't asked. Where the file has the streamer's own VOD running at the
// same time (its povs: Marlowe's, from 0:20 in), the name and disc link to
// that VOD at the same moment instead: a heading at its lines' start, a chip
// at its highlight's start, the Cast list at the playhead; outside it, the
// channel. (Only the links are read: the opener can't be stood in for.)
// The Timeline panel's Imported marks toggle hides the import's chapters
// and highlights from the timeline (and the pane with them), and shows them
// again. Imported again, the import's marks are replaced and a chapter of
// the user's own stays. The library and the pane's column each hide by a
// button at the toolbar's end, the centre taking their room, the pane's
// column then fitting a narrower window; both stay hidden over a reload.
// A longer transcript imported last: scrolled past a speaker's heading, the
// chapter and the speaker stay pinned at the top; the next heading pushes
// the pinned one out without overlapping it, a new chapter both; over lines
// no one is named for, the chapter over Unknown. A line the playhead moves to,
// under the pinned headings, is brought out below them; a click on the
// pinned speaker goes back to where their lines start.
// Last, an imported chapter renamed is the user's, counted under Your marks
// at once; and a chapter made while Your marks are hidden shows them again.

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

/** In the page: a press on the timeline at `arguments[0]` seconds, as the timeline hears it. */
const PRESS_AT = `
  const bar = document.querySelector(".scrubber");
  const box = bar.getBoundingClientRect();
  const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
  const at = { clientX: x, clientY: box.top + box.height / 2, button: 0, buttons: 1, bubbles: true, cancelable: true,
    pointerId: 1, pointerType: "mouse", isPrimary: true };
  bar.dispatchEvent(new PointerEvent("pointerdown", at));
  window.dispatchEvent(new PointerEvent("pointerup", { ...at, buttons: 0 }));`;

/** In the page: the transcript's scroller, and `put` (arguments[0] scrolled
 *  to arguments[1] px below the view's top, laid out first) and `pins` (the
 *  pinned headings, their boxes from the view's top). */
const PINS = `
  const scroller = document.querySelector("#transcript").closest(".body");
  const frames = () => new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)));
  const viewTop = () => document.querySelector("#transcript .pinned-anchor").getBoundingClientRect().top;
  const from = (el) => { const b = el.getBoundingClientRect(); return { top: b.top - viewTop(), bottom: b.bottom - viewTop() }; };
  const put = async (sel, below) => {
    if (!document.querySelector(sel)) scroller.scrollTop = 0;
    for (let i = 0; i < 100 && !document.querySelector(sel); i++) { scroller.scrollTop += 200; await frames(); }
    const el = document.querySelector(sel);
    if (!el) throw new Error("never laid out: " + sel);
    scroller.scrollTop += el.getBoundingClientRect().top - (viewTop() + below);
    await frames();
    await frames();
    return from(el);
  };
  const pins = () => {
    const box = document.querySelector("#transcript .pinned");
    const chapter = document.querySelector("#transcript .pinned .pinned-chapter");
    const speaker = document.querySelector("#transcript .pinned .pinned-speaker");
    return { hidden: box?.getAttribute("aria-hidden") ?? null, bottom: box ? from(box).bottom : 0,
      chapter: chapter ? { text: chapter.textContent.replace(/\\s+/g, ""), ...from(chapter.parentElement) } : null,
      speaker: speaker ? { text: speaker.querySelector(".speaker").textContent.trim(), ...from(speaker.parentElement) } : null };
  };`;

/** A minute of lines every half second, in long stretches: Marlowe to 0:12, Ada
 *  to 0:20, Marlowe (as Mar) to 0:32, no one named to 0:40, then Ada to the end. */
function longLines() {
  const who = (t) => (t < 12 ? "Marlowe" : t < 20 ? "Ada" : t < 32 ? "Mar" : t < 40 ? null : "Ada");
  return Array.from({ length: 120 }, (_, i) => {
    const start = i / 2;
    const text = `Line ${i + 1}, ${"and so on ".repeat(1 + (i % 4)).trim()}`;
    return { start, end: start + 0.4, text, ...(who(start) ? { speaker: who(start) } : {}) };
  });
}

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

const PICTURES = "Load streamers' pictures from img.vodscribe.com";

/** Turns the cast's pictures on or off in Settings › Privacy, and closes Settings. */
async function setPictures(app, on) {
  await app.openSettings("Privacy");
  await app.js(
    `const label = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label')].find((l) => l.textContent.trim() === arguments[0]);
     if (!label) throw new Error("no setting " + arguments[0]);
     const box = label.querySelector("input");
     if (box.checked !== arguments[1]) box.click();`,
    PICTURES,
    on,
  );
  await until(
    `pictures turned ${on ? "on" : "off"}`,
    async () => (await app.invoke("get_settings")).load_cast_pictures === on,
    10_000,
  );
  await app.key("Escape");
}

const tip = (app) =>
  app.js(`return document.querySelector(".scrubber .tip")?.textContent.replace(/\\s+/g, " ").trim() ?? ""`);

export default {
  name: "annotations",
  expects: {
    "annotations-timeline":
      'Above the timeline, three gray chapter slices, neighbours in different shades, read "💬 Hello", "🎮 The game" and "Goodbye", with a thin coloured band under them; the bar has two small emoji near its top (a laughing face and popcorn), and a dark tip in the bar reads "Close call 0:25–0:30" with "everyone gasped, then laughed" under it.',
    "annotations-pane":
      'On the window\'s right, a column the full height of the window lists "Hello", "The game" and "Goodbye" with their emoji and times, "Close call" (a laughing face) and "The twist" (popcorn) indented under "The game", which is lit with "The twist".',
    "annotations-pinned":
      'Under the transcript\'s header, a thin rule reading "💬 Hello" and under it a heading with a plain yellow picture, "Marlowe (Vance)" in colour and "0:00", over lines reading "Line 13" and on; nothing overlaps.',
    "annotations-cast":
      'The transcript\'s lines sit under headings, each a circle (Marlowe\'s a plain yellow picture, Ada\'s a coloured circle with "A"), a name in colour ("Marlowe (Vance)" twice in one colour, "Ada" in another) and a time; no heading reads "Mar"; the last two lines share one "Ada" heading; the line at 0:15 is under a heading "Unknown" with a gray circle holding a person\'s outline; thin rules across the transcript read "💬 Hello", "🎮 The game" and "Goodbye". A small panel titled by its list shows Marlowe (Vance) (Streamer · also Mar, Their stream at 0:15) and Ada (Friend, kick.com/ada_plays), each with the same circle as their heading.',
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
    if (JSON.stringify(inThem) !== '[["Mar","Ada","Jules"],[]]')
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
        const got = await app.js(`return [...document.querySelectorAll("#transcript .line .speaker")].map((s) =>
          ({ name: s.textContent.trim(), color: getComputedStyle(s).color, unknown: s.classList.contains("unknown") }))`);
        return got.length === 5 && got;
      },
      MINUTE,
    ).catch(() => []);
    const names = speakers.map((s) => s.name).join(", ");
    if (names !== "Marlowe (Vance), Ada, Unknown, Marlowe (Vance), Ada")
      wrong.push(`the lines' speakers: ${names || "none"}`);
    else {
      const [marlowe, ada, nobody, mar] = speakers;
      if ([marlowe, ada, mar].some((s) => s.unknown)) wrong.push("a cast member's line has no cast colour");
      if (!nobody.unknown) wrong.push(`the line no one is named for is headed in a cast colour, ${nobody.color}`);
      if (marlowe.color !== mar.color) wrong.push(`Marlowe is ${marlowe.color} and Marlowe (said as Mar) ${mar.color}`);
      if (marlowe.color === ada.color) wrong.push(`Marlowe and Ada are both ${ada.color}`);
    }

    // Each line, top to bottom: the head over it, if it starts a group, and the
    // chapter's divider over it, if any. Ada's last two lines are one group,
    // with no divider between them; the line no one is named for has a head
    // of its own, "Unknown", its disc a person's outline (no letter);
    // and no line names its speaker itself.
    const chat = await app.js(`return [...document.querySelectorAll("#transcript .line")].flatMap((l) => {
      const row = l.querySelector(":scope > button.row:not(.chapter-head)");
      return row ? [{
        time: Number(row.dataset.time),
        head: l.querySelector(":scope > .group-head .speaker")?.textContent.trim() ?? null,
        disc: ((a) => a && (a.textContent.trim() || (a.querySelector("svg") ? "(outline)" : "")))(
          l.querySelector(":scope > .group-head .avatar")) ?? null,
        named: !!row.querySelector(".speaker"),
        divider: l.querySelector(":scope > .chapter-head")?.textContent.replace(/\\s+/g, "") ?? null,
      }] : [];
    })`);
    const read = chat.map(
      (l) => `${l.divider ? `[${l.divider}] ` : ""}${l.time}${l.head ? ` ${l.disc} ${l.head}` : ""}`,
    );
    // Marlowe is played by Vance, named first: the disc's initial is Marlowe's.
    const expected = [
      "[💬Hello] 0 M Marlowe (Vance)",
      "8 A Ada",
      "[🎮Thegame] 15 (outline) Unknown",
      "25 M Marlowe (Vance)",
      "[Goodbye] 35 A Ada",
      "45",
    ];
    if (read.join(" | ") !== expected.join(" | ")) wrong.push(`the transcript reads ${read.join(" | ")}`);
    if (chat.some((l) => l.named)) wrong.push("a line names its speaker as well as its group's head");

    // The heads of those with an account: the name a link to their channel,
    // saying where it goes (the head itself says who plays them), or for
    // Marlowe's second, at 0:25, 5 s into Vance's own VOD, to that VOD there;
    // the disc the same link, their picture once it has loaded, or
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
        return got.length === 5 && got.every((h) => h.picture !== "loading") && got;
      },
      MINUTE,
    ).catch(() => []);
    const marloweHead = (link, title) => ({ name: "Marlowe (Vance)", link, title, picture: "shown", initial: false });
    const adaHead = {
      name: "Ada",
      link: "https://kick.com/ada_plays",
      title: "kick.com/ada_plays",
      picture: "none",
      initial: true,
    };
    const want = [
      marloweHead("https://www.twitch.tv/marlowe_live", "twitch.tv/marlowe_live"),
      adaHead,
      { name: "Unknown", link: null, title: "", picture: "no img", initial: true },
      marloweHead("https://www.twitch.tv/videos/1000000001?t=5s", "Vance's stream at 0:05 · twitch.tv"),
      adaHead,
    ];
    for (const [i, h] of heads.entries()) {
      const w = want[i];
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

    // The cast, from the transcript's header: each with the disc their lines
    // have (its initial, in their colour), then who they are; at 0:35,
    // Marlowe's link is to Vance's own VOD 15 s in, Ada's to her channel.
    await app.click(".cast-toggle");
    const cast = await until(
      "the cast list",
      () =>
        app.js(`const list = document.querySelector('[role=dialog][aria-label="Cast"]');
          return list ? [...list.querySelectorAll("li")].map((li) => {
            const disc = li.querySelector(".avatar");
            return (disc ? disc.textContent.trim() + " " + getComputedStyle(disc).backgroundColor + ": " : "no disc: ") +
              li.querySelector(".who").textContent.replace(/\\s+/g, " ").trim() + " → " +
              li.querySelector(".who a")?.getAttribute("href");
          }) : false`),
      10_000,
    ).catch(() => []);
    const castWant = [
      `M ${speakers[0]?.color}: Marlowe (Vance) Streamer · also Mar Their stream at 0:15 → https://www.twitch.tv/videos/1000000001?t=15s`,
      `A ${speakers[1]?.color}: Ada Friend kick.com/ada_plays → https://kick.com/ada_plays`,
    ];
    if (cast.join(" | ") !== castWant.join(" | ")) wrong.push(`the cast list: ${cast.join(" | ")}`);
    await ctx.checkpoint("annotations-cast");
    await app.key("Escape");

    // Pictures turned off: each disc is its initial (Unknown's the outline),
    // in the transcript and in Cast, and the image server isn't asked.
    const quiet = serveFiles(pictures);
    await setPictures(app, false);
    const discs = (where) =>
      app.js(
        `return [...document.querySelectorAll(arguments[0])].map((d) =>
          (d.querySelector("img") ? "picture " : "") + (d.textContent.trim() || "outline") +
          (getComputedStyle(d).color === "rgba(0, 0, 0, 0)" ? " hidden" : ""))`,
        where,
      );
    const initials = "M | A | outline | M | A";
    const plain = await until(
      "the transcript's discs without pictures",
      async () => (await discs("#transcript .group-head .avatar")).join(" | ") === initials,
      10_000,
    ).catch(async () => (await discs("#transcript .group-head .avatar")).join(" | "));
    if (plain !== true) wrong.push(`with pictures off, the transcript's discs read ${plain}`);
    await app.click(".cast-toggle");
    const castDiscs = await until(
      "the cast list's discs",
      async () => {
        const got = await discs('[role=dialog][aria-label="Cast"] li .avatar');
        return got.length && got.join(" | ");
      },
      10_000,
    ).catch(() => "none");
    if (castDiscs !== "M | A") wrong.push(`with pictures off, the cast list's discs read ${castDiscs}`);
    await app.key("Escape");
    if (quiet.requests.length)
      wrong.push(`with pictures off, the image server was asked: ${quiet.requests.map((r) => r.path).join(", ")}`);
    quiet.stop();
    // Back on, with the image server there again: the pinned headings below show the picture too.
    const picturesBack = serveFiles(pictures);
    await setPictures(app, true);

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
    // Who's in the first highlight: their discs in the cast's colours, with
    // the character's initial, their tooltip naming the character first ("Mar"
    // in the file is Marlowe; Jules isn't in the cast, so is as written). They sit at the end of the row's times line,
    // right of the times, under the title (which has the row's width).
    const chips = await app.js(
      `const row = document.querySelector(arguments[0]);
      const faces = row?.querySelector(".faces");
      if (!faces) return null;
      const box = (r) => ({ left: r.left, right: r.right, top: r.top, bottom: r.bottom });
      const times = document.createRange();
      times.selectNodeContents(row.querySelector(".meta"));
      return { title: faces.title,
        discs: [...faces.querySelectorAll(".face .avatar")].map((f) => f.textContent.trim() + " " + getComputedStyle(f).backgroundColor),
        inLine: faces.parentElement === row.querySelector(".meta-line"),
        faces: box(faces.getBoundingClientRect()), times: box(times.getBoundingClientRect()),
        name: box(row.querySelector(".name").getBoundingClientRect()), line: box(row.querySelector(".meta-line").getBoundingClientRect()) }`,
      row(funny),
    );
    if (chips?.title !== "Marlowe (Vance), Ada, Jules") wrong.push(`the first highlight's discs say "${chips?.title}"`);
    else {
      if (
        speakers.length === 5 &&
        JSON.stringify(chips.discs.slice(0, 2)) !== JSON.stringify([`M ${speakers[0].color}`, `A ${speakers[1].color}`])
      )
        wrong.push(`the discs ${chips.discs} aren't the speakers' M ${speakers[0].color}, A ${speakers[1].color}`);
      const { faces, times, name, line } = chips;
      if (
        !chips.inLine ||
        faces.left < times.right ||
        faces.top < name.bottom ||
        Math.abs(faces.right - line.right) > 1 ||
        faces.bottom > line.bottom + 1
      )
        wrong.push(`the discs aren't at the end of the times line, under the title: ${JSON.stringify(chips)}`);
    }
    const placed =
      await app.js(`const pane = document.querySelector(".app > .pane.side .annotations-pane")?.getBoundingClientRect();
      const main = document.querySelector(".app > .main")?.getBoundingClientRect();
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

    // Opened, the first highlight names who's in it on chips under its why,
    // outside the row's button: each by the cast's own name for them
    // (Marlowe, not "Marlowe (Vance)" nor the file's "Mar"), Jules as
    // written. One with an account is a link to the streamer's channel, its
    // tooltip naming the streamer and where, reached from the keyboard
    // (Marlowe's to Vance's own VOD at the highlight's start, 5 s in);
    // Jules's is no link. No chip's name is cut short. A click on a chip
    // leaves the row open and the player where it was. (The link's own click
    // isn't made: the opener would start the system's browser, and the page
    // can't stand in for it, its invoke being read-only.)
    const funnyRow = paneRow(`highlight-${funny.id}`);
    await app.click(`${funnyRow} .chevron`);
    const people = await until(
      "the opened highlight's chips",
      async () => {
        const got = await app.js(
          `return [...document.querySelectorAll(arguments[0] + " .detail .people .chip")].map((c) => {
            const name = c.querySelector("strong");
            c.focus();
            return { tag: c.tagName, text: name?.textContent.trim(), href: c.getAttribute("href"), title: c.title,
              focusable: document.activeElement === c, inButton: !!c.closest("button"),
              disc: !!c.querySelector(".avatar"), cut: !name || name.scrollWidth > name.clientWidth + 1 };
          })`,
          funnyRow,
        );
        return got.length === 3 && got;
      },
      10_000,
    ).catch(() => []);
    const peopleWant = [
      {
        tag: "A",
        text: "Marlowe",
        href: "https://www.twitch.tv/videos/1000000001?t=5s",
        title: "Vance's stream at 0:05 · twitch.tv",
        focusable: true,
      },
      { tag: "A", text: "Ada", href: "https://kick.com/ada_plays", title: "Ada · kick.com/ada_plays", focusable: true },
      { tag: "SPAN", text: "Jules", href: null, title: "", focusable: false },
    ];
    for (const [i, w] of peopleWant.entries()) {
      const p = people[i];
      const got = p && { tag: p.tag, text: p.text, href: p.href, title: p.title, focusable: p.focusable };
      if (JSON.stringify(got) !== JSON.stringify(w) || p.inButton || !p.disc || p.cut)
        wrong.push(`the opened highlight's chip ${i + 1}: ${JSON.stringify(p)}`);
    }
    await app.click(`${funnyRow} .detail span.chip strong`);
    const stillOpen = await app.js(
      `const row = document.querySelector(arguments[0]);
      return { open: row?.classList.contains("expanded"), chevron: row?.querySelector(".chevron")?.getAttribute("aria-expanded") }`,
      funnyRow,
    );
    if (!stillOpen.open || stillOpen.chevron !== "true")
      wrong.push(`a click on a chip closed its highlight: ${JSON.stringify(stillOpen)}`);
    if ((await app.shownTime()) !== 25) wrong.push(`a click on a chip moved the player to ${await app.shownTime()} s`);
    await app.click(`${funnyRow} .chevron`);

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
      // They keep what the import gave them: their kind's emoji on the
      // edit's timeline, their length, why and who's in it in its pane, and
      // the Imported marks toggle takes them off and puts them back.
      const editGlyphs = await until(
        "the edit's highlights' glyphs",
        async () => {
          const got = await app.js(`return [...document.querySelectorAll(".scrubber .emoji")].map((d) =>
            d.dataset.category + " " + d.firstChild.textContent)`);
          return got.length === 2 && got;
        },
        10_000,
      ).catch(() => app.js(`return [...document.querySelectorAll(".scrubber .emoji")].map((d) => d.dataset.category)`));
      if (String(editGlyphs) !== "funny 😂,drama 🍿") wrong.push(`the edit's timeline shows ${editGlyphs}`);
      const editRow = paneRow(`highlight-${(await app.invoke("edit_marks", { id: made.id }))[0]?.id}`);
      const meta = await app.js(
        `const row = document.querySelector(arguments[0]);
        return row && { meta: row.querySelector(".meta").textContent.replace(/\\s+/g, ""),
          faces: row.querySelector(".faces")?.title ?? null }`,
        editRow,
      );
      if (JSON.stringify(meta) !== JSON.stringify({ meta: "0:00·5s", faces: "Mar, Ada, Jules" }))
        wrong.push(`in the edit, Close call's row reads ${JSON.stringify(meta)}`);
      // Opened (a highlight with nothing to show has nothing to open).
      const opens = await app.js(`return !!document.querySelector(arguments[0] + " .chevron")`, editRow);
      let opened = null;
      if (opens) {
        await app.click(`${editRow} .chevron`);
        opened = await until(
          "the edit's highlight opened",
          () =>
            app.js(
              `const d = document.querySelector(arguments[0] + " .detail");
              return d && { why: d.querySelector(".why")?.textContent.trim() ?? null,
                people: [...d.querySelectorAll(".people .chip strong")].map((c) => c.textContent.trim()) }`,
              editRow,
            ),
          10_000,
        ).catch(() => null);
      }
      const openedWant = { why: "everyone gasped, then laughed", people: ["Mar", "Ada", "Jules"] };
      const openedGot = opened && { why: opened.why, people: opened.people };
      if (JSON.stringify(openedGot) !== JSON.stringify(openedWant))
        wrong.push(`in the edit, Close call opened shows ${JSON.stringify(openedGot)}`);
      else ctx.log("in the edit, the highlights keep their emoji, length, why and people");
      if (opens) await app.click(`${editRow} .chevron`);
      await app.click("button.timeline");
      await until(
        "the edit's Marks toggles",
        () => app.js(`return !!document.querySelector('[aria-label="Marks"] button')`),
        10_000,
      );
      await flip();
      await until("the edit's imported marks hidden", async () => (await onTimeline()) === 0, 10_000).catch(async () =>
        wrong.push(`in the edit, with imported marks hidden, ${await onTimeline()} still on the timeline`),
      );
      await flip();
      await until("the edit's imported marks back", async () => (await onTimeline()) === 2, 10_000).catch(async () =>
        wrong.push(`in the edit, with imported marks shown again, ${await onTimeline()} on the timeline`),
      );
      await app.key("Escape");
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

    // The library and the pane's column hidden, each by its button at the
    // toolbar's end: the centre takes their room; with the library hidden,
    // a window of 1000 px has room for the pane's column. Both stay hidden
    // over a reload, and their buttons show them again.
    const widths = () =>
      app.js(`const width = (el) => (el && getComputedStyle(el).display !== "none" ? Math.round(el.getBoundingClientRect().width) : 0);
        return { library: width(document.querySelector(".library-pane")), main: width(document.querySelector(".app > .main")),
          pane: width(document.querySelector(".app > .pane.side")),
          buttons: [...document.querySelectorAll(".toolbar .library-toggle, .toolbar .pane-toggle")].map((b) =>
            b.classList[1] + "=" + b.getAttribute("aria-pressed")).join(" ") }`);
    const settled = (what, check) =>
      until(what, async () => check(await widths()) && widths(), 10_000).catch(async () => {
        const got = await widths();
        wrong.push(`${what}: ${JSON.stringify(got)}`);
        return got;
      });
    const all = await settled("both columns", (w) => w.library > 0 && w.pane > 0);
    await app.click(".toolbar .library-toggle");
    const noLibrary = await settled(
      "the library hidden, the centre wider",
      (w) => !w.library && w.pane > 0 && w.main >= all.main + all.library - 1,
    );
    if (noLibrary.buttons !== "library-toggle=false pane-toggle=true")
      wrong.push(`the library hidden, the buttons read ${noLibrary.buttons}`);
    const narrowed = await resizeInner(app, 1000, Math.min(was.height, 760));
    if (narrowed.width < 1100)
      await until(
        "the pane's column at 1000 px",
        () => app.js(`return !!document.querySelector(".app > .pane.side")`),
        10_000,
      ).catch(() => wrong.push(`the library hidden, at ${narrowed.width} px the pane isn't a column`));
    await resizeInner(app, was.width, was.height);
    await app.click(".toolbar .pane-toggle");
    await settled(
      "the pane hidden too, the centre all of it",
      (w) => !w.library && !w.pane && w.main >= all.main + all.library + all.pane - 1,
    );
    if (await app.js(`return !!document.querySelector(".pane-tabs")`)) wrong.push("the pane hidden left tabs");
    await ctx.checkpoint("columns-hidden");
    // Over a reload: the page's own storage keeps them. (Not over a restart
    // yet: the installed app's page is served from a new port each launch,
    // so what it stores is another origin's then.)
    await app.js(`setTimeout(() => location.reload(), 0)`);
    await new Promise((r) => setTimeout(r, 1000));
    await app.loaded();
    await until(
      "the library after the reload",
      () => app.js(`return !!document.querySelector(arguments[0])`, `.vod[data-key="${vod.id}"] button.main`),
      MINUTE,
    );
    const after = await settled("the library still hidden after a reload", (w) => !w.library && w.main > 0);
    if (!after.buttons.startsWith("library-toggle=false"))
      wrong.push(`after a reload, the buttons read ${after.buttons}`);
    // The library shown again, the VOD opened from it: the pane still hidden.
    await app.click(".toolbar .library-toggle");
    await app.click(`.vod[data-key="${vod.id}"] button.main`);
    const reopened = await settled(
      "the VOD open, the pane still hidden",
      (w) => w.library > 0 && !w.pane && w.main > 0,
    );
    if (reopened.buttons !== "library-toggle=true pane-toggle=false")
      wrong.push(`after a reload, the VOD open, the buttons read ${reopened.buttons}`);
    await app.click(".toolbar .pane-toggle");
    const back = await settled("both shown again", (w) => w.library > 0 && w.pane > 0);
    if (Math.abs(back.main - all.main) > 1) wrong.push(`shown again, the centre is ${back.main} px, not ${all.main}`);
    else ctx.log("the library and the pane hidden and shown by their buttons, and remembered over a reload");
    // And by their keys: Ctrl+Shift+B (⇧⌘B) the library, Ctrl+Alt+B (⌥⌘B) the pane.
    // `repeats` more presses follow as the key held down sends them (`repeat` set), all in one go.
    const chord = (key, shift, alt, repeats = 0) =>
      app.js(
        `for (let i = 0; i <= arguments[4]; i++)
          (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key: arguments[0], shiftKey: arguments[1], altKey: arguments[2],
            ctrlKey: !arguments[3], metaKey: arguments[3], repeat: i > 0, bubbles: true, cancelable: true }))`,
        key,
        shift,
        alt,
        process.platform === "darwin",
        repeats,
      );
    await chord("B", true, false);
    await settled("the library hidden by its key", (w) => !w.library && w.pane > 0);
    await chord("b", false, true);
    await settled("the pane hidden by its key", (w) => !w.library && !w.pane);
    await chord("B", true, false);
    await chord("b", false, true);
    await settled("both shown again by their keys", (w) => w.library > 0 && w.pane > 0);
    // Held down, each chord hides its column once: the key's repeats don't show it again and again.
    await chord("B", true, false, 3);
    await chord("b", false, true, 3);
    await app.js(`await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))`);
    await settled("both hidden by their keys held down", (w) => !w.library && !w.pane);
    await chord("B", true, false);
    await chord("b", false, true);
    await settled("both shown again by their keys", (w) => w.library > 0 && w.pane > 0);

    // Pinned headings, over a longer transcript of the same VOD: the chapters
    // as before (and the user's own at 0:50), the lines every half second.
    const long = JSON.parse(json);
    long.transcript.segments = longLines();
    const longFile = path.join(dir, "minute-annotations-long.json");
    writeFileSync(longFile, JSON.stringify(long));
    await app.invoke("import_annotations_file", { path: longFile });
    await until(
      "the longer transcript",
      () => app.js(`return !!document.querySelector('#transcript .line button.row[data-time="0.5"]')`),
      MINUTE,
    );
    // Marlowe's 13th line at the top: his heading is above, and pinned.
    await app.js(`${PINS} await put('#transcript .line button.row[data-time="6"]', 0);`);
    const atRest = await app.js(`${PINS} return pins();`);
    if (atRest.chapter?.text !== "💬Hello" || atRest.speaker?.text !== "Marlowe (Vance)" || atRest.hidden !== "true")
      wrong.push(`mid-stretch, the pinned headings: ${JSON.stringify(atRest)}`);
    else ctx.log(`mid-stretch, pinned: ${atRest.chapter.text} over ${atRest.speaker.text}, ${atRest.bottom} px`);
    // The pinned copy of Marlowe's heading shows his picture, as his heading does: a disc
    // drawn anew over a picture already loaded shows it too.
    const pinnedDiscs = () =>
      app.js(`const disc = (el) => { const img = el?.querySelector("img");
          return el && { pictured: el.classList.contains("pictured"), img: img && { complete: img.complete, width: img.naturalWidth, loaded: img.classList.contains("loaded"), lazy: img.loading } }; };
        return { pinned: disc(document.querySelector("#transcript .pinned .avatar")),
          heading: disc(document.querySelector('#transcript .line > .group-head[data-time="0"] .avatar')) };`);
    const pictured = await until(
      "the pinned heading's picture",
      async () => {
        const d = await pinnedDiscs();
        return d.pinned?.pictured && d.heading?.pictured && d;
      },
      5000,
    ).catch(async () => {
      const d = await pinnedDiscs();
      wrong.push(`the pinned heading's disc has no picture: ${JSON.stringify(d)}`);
      return d;
    });
    ctx.log(`the pinned and real headings' discs: ${JSON.stringify(pictured)}`);
    await ctx.checkpoint("annotations-pinned");
    picturesBack.stop();
    const stack = atRest.bottom;
    // Ada's heading 10 px into the pinned Marlowe's: he's pushed up by as much, touching it.
    const ada = await app.js(`${PINS} return put('#transcript .line > .group-head[data-time="12"]', ${stack - 10});`);
    const pushed = await app.js(`${PINS} return pins();`);
    if (
      pushed.speaker?.text !== "Marlowe (Vance)" ||
      Math.abs(pushed.speaker.bottom - ada.top) > 1 ||
      Math.abs(pushed.speaker.top - (atRest.speaker.top - 10)) > 1 ||
      Math.abs(pushed.chapter?.top ?? 99) > 1
    )
      wrong.push(`Ada's heading 10 px into Marlowe's pinned one at ${JSON.stringify(ada)}: ${JSON.stringify(pushed)}`);
    // A row above the view turning out 30 px taller (measured as it's laid
    // out, as scrolling up does): the view moves by as much to keep what's
    // read in place, and in that very frame the pinned heading still touches
    // Ada's. Read after the transcript's own measuring (its ResizeObserver
    // was made first, so it's told first), before the frame is painted.
    const grown = await app.js(
      `${PINS}
       const ada = () => from(document.querySelector('#transcript .line > .group-head[data-time="12"]'));
       const above = [...document.querySelectorAll("#transcript .line[data-index]")]
         .find((r) => r.getBoundingClientRect().bottom < viewTop() - 20 && r.querySelector("button.row"));
       if (!above) throw new Error("no row laid out above the view");
       const extra = document.createElement("div");
       extra.style.height = "30px";
       const painted = await new Promise((ok) => {
         const ro = new ResizeObserver(() => { ro.disconnect(); ok({ ada: ada(), pins: pins() }); });
         ro.observe(above);
         above.append(extra);
       });
       await frames();
       await frames();
       const settled = { ada: ada(), pins: pins() };
       extra.remove();
       await frames();
       await frames();
       return { painted, settled };`,
    );
    for (const [when, at] of Object.entries(grown))
      if (!at.pins.speaker || Math.abs(at.pins.speaker.bottom - at.ada.top) > 1)
        wrong.push(`a row above grown 30 px, ${when}, the pinned heading and Ada's: ${JSON.stringify(at)}`);
    // "The game"'s divider (its row, with the space over it) 10 px into the pinned pair: both pushed up together.
    const divider = await app.js(`${PINS} return put('#transcript .line > .chapter-head[data-time="20"]', ${stack});`);
    const pair = await app.js(`${PINS} return pins();`);
    const rowTop = divider.top - 10;
    if (
      pair.chapter?.text !== "💬Hello" ||
      pair.speaker?.text !== "Ada" ||
      Math.abs(pair.chapter.top + 10) > 1 ||
      Math.abs(pair.bottom - rowTop) > 1
    )
      wrong.push(
        `"The game"'s divider 10 px into the pinned pair at ${JSON.stringify(divider)}: ${JSON.stringify(pair)}`,
      );
    // Over the lines no one is named for: the chapter, and Unknown under it.
    await app.js(`${PINS} await put('#transcript .line button.row[data-time="35"]', 0);`);
    const unnamed = await app.js(`${PINS} return pins();`);
    if (unnamed.chapter?.text !== "🎮Thegame" || unnamed.speaker?.text !== "Unknown")
      wrong.push(`over lines no one is named for, pinned: ${JSON.stringify(unnamed)}`);

    // The playhead moved to a line under the pinned headings (a click on the
    // timeline), following: that line is brought out below them.
    if (await app.js(`return !!document.querySelector("#transcript .follow")`)) await app.click("#transcript .follow");
    await app.js(PRESS_AT, 24.2);
    const nowAt = (time) =>
      until(
        `the line at ${time} s lit`,
        () =>
          app.js(
            `return !!document.querySelector(arguments[0])`,
            `#transcript .line.now > button.row[data-time="${time}"]`,
          ),
        10_000,
      );
    /** In the page: once the view has held still for a few frames, where line `arguments[0]` is and what's pinned. */
    const still = (time) =>
      app.js(
        `${PINS}
         let last = NaN;
         for (let same = 0, i = 0; same < 5 && i < 200; i++) { await frames(); same = scroller.scrollTop === last ? same + 1 : 0; last = scroller.scrollTop; }
         const line = document.querySelector('#transcript .line > button.row[data-time="' + arguments[0] + '"]');
         return { line: line && from(line), pins: pins() };`,
        time,
      );
    await nowAt(24);
    await still(24);
    // Line 24 s just under the view's top, under the pinned headings, the line after it too.
    const under = await app.js(`${PINS} return put('#transcript .line > button.row[data-time="24"]', 5);`);
    const hiding = await app.js(`${PINS} return pins();`);
    const next = await app.js(
      `${PINS} return from(document.querySelector('#transcript .line > button.row[data-time="24.5"]'));`,
    );
    if (!(next.top < hiding.bottom))
      ctx.log(`the line at 0:24.5 isn't under the pinned headings (${JSON.stringify({ under, next, hiding })})`);
    await app.js(PRESS_AT, 24.7);
    await nowAt(24.5);
    const brought = await still(24.5);
    if (!brought.line || brought.line.top < brought.pins.bottom - 1)
      wrong.push(`the line the playhead moved to is under the pinned headings: ${JSON.stringify(brought)}`);
    else
      ctx.log(
        `the line the playhead moved to: ${brought.line.top} px down, the pinned headings ${brought.pins.bottom} px`,
      );

    // A click on the pinned speaker (not their name): back to where their lines start, the heading in view.
    await app.js(`${PINS} await put('#transcript .line button.row[data-time="46"]', 0);`);
    const before = await app.js(`${PINS} return pins();`);
    if (before.speaker?.text !== "Ada") wrong.push(`at 0:46, pinned: ${JSON.stringify(before)}`);
    else {
      await app.js(`document.querySelector("#transcript .pinned .pinned-speaker .head-time").click();`);
      const returned = await app.js(
        `${PINS} await frames(); await frames();
         const head = document.querySelector('#transcript .line > .group-head[data-time="40"]');
         return { head: head && from(head), pins: pins() };`,
      );
      if (!returned.head || returned.head.top < returned.pins.bottom - 1 || returned.head.top > 80)
        wrong.push(`a click on the pinned Ada: ${JSON.stringify(returned)}`);
    }

    // The import's first chapter renamed (N on its frame opens its title):
    // the user's from then on, so the Marks toggles count it as theirs at
    // once, as the library does. Then Your marks hidden and a chapter made:
    // they show again, the new one with them.
    const toggles = async () => {
      const there = `return !!document.querySelector('[aria-label="Marks"] button')`;
      if (!(await app.js(there))) await app.click("button.timeline");
      await until("the Marks toggles", () => app.js(there), 10_000);
      return app.js(`return Object.fromEntries([...document.querySelectorAll('[aria-label="Marks"] button')].map((b) =>
        [b.textContent.includes("Imported") ? "imported" : "user",
         { count: Number(b.querySelector(".count").textContent), on: b.getAttribute("aria-pressed") === "true" }]));`);
    };
    const title = `[role=dialog][aria-label="Chapter title"] input`;
    const name = async (was, text) => {
      await app.key("n");
      await until(
        `the title box of ${JSON.stringify(was)}`,
        () => app.js(`return document.querySelector(arguments[0])?.value === arguments[1]`, title, was),
        10_000,
      );
      await app.type(title, text);
      await app.submit(title);
      await until(
        "the title box to close",
        () => app.js(`return !document.querySelector(arguments[0])`, title),
        10_000,
      );
      // The chapter let go of, so the arrows move the playhead, not it.
      await app.key("Escape");
    };
    // (A click on the timeline's start: the arrows go back 5 s at a time from 0:46.)
    await app.js(PRESS_AT, 0);
    await until("the player at the start", async () => (await app.shownTime()) === 0, 10_000);
    await name("Hello", "Hi");
    const renamed = await toggles();
    if (renamed.user.count !== 2 || renamed.imported.count !== 4)
      wrong.push(`an imported chapter renamed, the Marks toggles say ${JSON.stringify(renamed)}`);
    await app.js(
      `[...document.querySelectorAll('[aria-label="Marks"] button')].find((b) => b.textContent.includes("Your marks")).click();`,
    );
    await until("Your marks hidden", async () => !(await toggles()).user.on, 10_000);
    await app.key("Escape");
    await app.key("ArrowRight", { shift: true });
    await until("the player at 1 s", async () => (await app.shownTime()) === 1, 10_000);
    await name("", "Ours");
    const ours = (await app.invoke("list_marks", { vodId: vod.id })).find((m) => m.text === "Ours");
    const drawn = await until(
      "the new chapter on the timeline",
      () => app.js(`return !!document.querySelector('[data-mark="' + arguments[0] + '"]')`, ours?.id),
      10_000,
    ).catch(() => false);
    const marksNow = await toggles();
    if (!ours || !drawn || !marksNow.user.on)
      wrong.push(`a chapter made with Your marks hidden: ${JSON.stringify({ ours, drawn, marksNow })}`);
    else ctx.log(`a chapter made with Your marks hidden shows them again: ${JSON.stringify(marksNow)}`);
    await app.key("Escape");

    if (wrong.length) throw new Error(wrong.join("; "));
  },
};
