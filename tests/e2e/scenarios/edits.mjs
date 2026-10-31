// Edits: a clip's pill dragged onto "Make new edit" in the library (a group, one band); the
// edit opened, renamed, a cut put on it (B) and a delete beside it making it a real split (and undone), its piece split, the group opened (double-click)
// and the second half moved first and back (Ctrl+← and →), a piece's end dragged showing a dashed guide at it, a line's slice taken out and brought back
// (Ctrl+Z), its picture held and carried onto another edit's row, a clip
// made with its Clip button, rendered to a file (E); all of it still there after a restart;
// removing the VOD says its pieces stay in the edit (Keep: nothing goes;
// Remove and delete the video: it leaves the library, and the stretches
// the edit uses are kept as videos of their own, which it renders); then the edit
// deleted, after asking, and with nothing using the VOD it's erased by the
// next start (added again, it's a new one). An edit of it then, not
// transcribed: the CC button in its bar all the same, off, as on a VOD.

import { spawnSync } from "node:child_process";
import { existsSync, rmSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, TIMED_POLL, until } from "../lib/app.mjs";
import { libraryPath } from "../lib/data.mjs";
import * as os from "../lib/os.mjs";

const NAME = "Landing edit";

/** Double-clicks clip `id`'s pill on the open edit's clip bar (as the page
 *  gets one: two clicks, then the double-click), and waits for the clip open
 *  in the editor. */
async function openPill(app, id) {
  await app.js(
    `const el = document.querySelector(arguments[0]);
     const box = el.getBoundingClientRect();
     const at = { bubbles: true, cancelable: true, clientX: box.left + 4, clientY: box.top + box.height / 2 };
     for (const detail of [1, 2]) el.dispatchEvent(new MouseEvent("click", { ...at, detail }));
     el.dispatchEvent(new MouseEvent("dblclick", { ...at, detail: 2 }));`,
    `section.edit-view .chip[data-clip="${id}"]`,
  );
  await until(
    "the clip open in the editor",
    () =>
      app.js(
        `return document.querySelector("section.edit-view input.title")?.getAttribute("aria-label") === "Clip name"`,
      ),
    10_000,
  );
}

/** A pointer event's fields at (x, y) with `buttons` down, in the page. */
const AT = `const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });`;

export default {
  name: "edits",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "edit-clip-bar":
      "An edit is open (its name at the top, with Render): under its player and timeline, on one row, a dashed link to the VOD its pieces come from, then one clip pill showing a start time, a few words of its line, and small play, folder and × icons, none of it cut off.",
    "edit-clip-open":
      "A clip is open in the editor: at the top, the edit's name in grey, a › and the clip's name, on one line, with no Render button; under its player and timeline, on one row, a grey pill with a dashed border reading ‹ Back to the edit's name · clip bar, then a dashed link to the VOD it comes from.",
  },
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    const clip = await app.clipOneToThree(vod.id);

    // The clip's pill dragged (in the page's events) onto "Make new edit",
    // which shows in the library while it's carried.
    await until(
      "the pill",
      () => app.js(`return !!document.querySelector('.chip.ready[data-clip="${clip.id}"]')`),
      10_000,
    );
    await app.js(
      `${AT}
       const el = document.querySelector('.chip.ready[data-clip="' + arguments[0] + '"] .time');
       const r = el.getBoundingClientRect();
       const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
       el.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
       window.dispatchEvent(new PointerEvent("pointermove", at(x + 10, y + 10, 1)));
       window.__drag = [x + 10, y + 10];`,
      clip.id,
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
        const edits = await app.invoke("list_edits");
        return edits.length === 1 && edits;
      },
      10_000,
    );
    ctx.log(`made "${made.title}"`);

    // The VOD's playhead left at 4 s, to find there again after the edit.
    await app.toStart();
    for (let i = 0; i < 4; i++) await app.key("ArrowRight", { shift: true });
    await until("the VOD at 4 s", async () => (await app.shownTime()) === 4, 10_000);

    // Opened from its row (the search cleared: results take the main area).
    await app.type('input[placeholder="Search for a phrase…"]', "");
    await app.click(`.vod.edit[data-key="-${made.id}"] button.main`);
    const title = 'section.edit-view input.title[aria-label="Edit name"]';
    await until("the edit open", () => app.js(`return !!document.querySelector(arguments[0])`, title), MINUTE);
    // Renamed as typed: focused, typed into, then left (it saves on blur).
    await app.js(`document.querySelector(arguments[0]).focus()`, title);
    await app.type(title, NAME);
    await app.blur(title);
    await until("the new name", async () => (await app.invoke("list_edits"))[0]?.title === NAME, 10_000);
    await until(
      "the row's new name",
      () => app.js(`return document.querySelector('.vod.edit[data-key="-${made.id}"] .title')?.textContent.trim()`),
      10_000,
    ).then((t) => {
      if (t !== NAME) throw new Error(`the row says "${t}"`);
    });
    // Back to the VOD: where its playhead was left, not its start; then the edit again.
    await app.click(`.vod[data-key="${vod.id}"] button.main`);
    await until("the VOD where it was left, at 4 s", async () => (await app.shownTime()) === 4, 10_000);
    await app.click(`.vod.edit[data-key="-${made.id}"] button.main`);
    await until("the edit open again", () => app.js(`return !!document.querySelector(arguments[0])`, title), MINUTE);

    await ctx.checkpoint("edit-view");
    // Played (not to its end: it's 2 s) and paused, the time by the button is
    // where the player is (sound alone here: its time); then back at the
    // start, where it opened, for what follows.
    if (await app.invoke("player_available")) {
      const opened = await app.shownTime();
      await app.click('.bar button[aria-label="Play"]');
      await until(
        "the edit to play",
        async () => (await app.invoke("player_set_speed", { speed: 1 }))?.running,
        10_000,
      );
      ctx.log(await app.timeIsPicture("the edit playing", { playing: true }));
      await app.pause();
      ctx.log(await app.timeIsPicture("the edit paused"));
      await app.toStart();
      if (opened !== 0) throw new Error(`the edit opened at ${opened} s, not its start: left at its start`);
    }
    const edit = () => app.invoke("get_edit", { id: made.id });
    const first = await edit();
    const pieces = (e) => e.pieces;
    if (pieces(first).length !== 1) throw new Error(`the new edit has ${pieces(first).length} pieces, not 1`);
    if (pieces(first)[0].group?.title !== clip.title)
      throw new Error(`the clip came in as ${JSON.stringify(pieces(first)[0].group)}, not a group named for it`);

    // Chat written into the library for the VOD (a file has none of its
    // own) at 1.5 and 2.5 s, in the edit's stretch, and at 4 s, outside it:
    // once the edit next changes (the split below), its transcript has the
    // two beside it, at 0.5 and 1.5 s of the edit.
    const library = libraryPath();
    const db = new DatabaseSync(library);
    const add = db.prepare("INSERT INTO chat (vod_id, time, author, color, text, parts) VALUES (?, ?, ?, NULL, ?, ?)");
    for (const [time, text] of [
      [1.5, "liftoff"],
      [2.5, "eagle"],
      [4, "after"],
    ])
      add.run(vod.id, time, "viewer", text, JSON.stringify([{ type: "text", text }]));
    db.close();

    // Split at the playhead, a second into the edit (its one piece, 2 s long).
    await app.toStart();
    await until("the start", async () => (await app.shownTime()) === 0, 10_000);
    await app.key("ArrowRight", { shift: true });
    await until("1 s in", async () => (await app.shownTime()) === 1, 10_000);
    // B puts a cut there, as on a VOD: a mark, the piece still one. Delete
    // takes out the stretch from it to the piece's end, the cut then a real
    // split (gone as a mark); Ctrl+Z brings both back, and B takes it away.
    const editCuts = async () => (await app.invoke("edit_marks", { id: made.id })).filter((m) => m.kind === "cut");
    // How long the edit plays (nothing's struck: a slice taken out splits the pieces).
    const lengthNow = async () => (await edit()).pieces.reduce((t, p) => t + p.end - p.start, 0);
    await app.key("b");
    const [cut] = await until(
      "a cut at 1 s",
      async () => {
        const list = await editCuts();
        return list.length === 1 && Math.abs(list[0].start - 1) < 0.05 && list;
      },
      10_000,
    );
    if (pieces(await edit()).length !== 1) throw new Error("B split the piece, not put a cut");
    await until(
      "the cut drawn",
      () => app.js(`return !!document.querySelector(\`section.edit-view .scrubber .cut[data-mark="${cut.id}"]\`)`),
      10_000,
    );
    // A click on the timeline lets go of the cut (it was selected as made):
    // Delete then takes out the stretch, not the cut. 1.5 s in, after the cut.
    await app.js(
      `const box = document.querySelector("section.edit-view .scrubber canvas").getBoundingClientRect();
       const at = { clientX: box.left + box.width * 0.75, clientY: box.top + box.height * 0.75, button: 0, bubbles: true, pointerId: 1 };
       const el = document.elementFromPoint(at.clientX, at.clientY);
       el.dispatchEvent(new PointerEvent("pointerdown", at));
       el.dispatchEvent(new PointerEvent("pointerup", at));`,
    );
    await until(
      "the cut let go of",
      () => app.js(`return !document.querySelector("section.edit-view .scrubber .cut.selected")`),
      10_000,
    );
    await app.key("Delete");
    await until(
      "the stretch after the cut taken out",
      async () => Math.abs((await lengthNow()) - 1) < 0.05 && (await editCuts()).length === 0,
      10_000,
    );
    if (pieces(await edit()).length !== 1) throw new Error("Delete after a cut removed the piece");
    await app.key("z", { cmd: true });
    await until(
      "the stretch and its cut back",
      async () => Math.abs((await lengthNow()) - 2) < 0.05 && (await editCuts()).length === 1,
      10_000,
    );
    await app.toStart();
    await app.key("ArrowRight", { shift: true });
    await until("1 s in again", async () => (await app.shownTime()) === 1, 10_000);
    await app.key("b");
    await until("the cut taken away", async () => (await editCuts()).length === 0, 10_000);
    ctx.log("a cut on the edit: put, made a real split by a delete, undone, taken away");
    // Two pieces, for what follows: the piece split where the cut was.
    const one = pieces(await edit())[0];
    await app.invoke("split_piece", { id: one.id, at: one.start + 1 });
    await until("two pieces", async () => pieces(await edit()).length === 2, 10_000);
    // Made behind the page's back: opened again, it shows them.
    await app.click(`.vod[data-key="${vod.id}"] button.main`);
    await until(
      "the VOD open",
      async () => !(await app.js(`return !!document.querySelector(arguments[0])`, title)),
      MINUTE,
    );
    await app.click(`.vod.edit[data-key="-${made.id}"] button.main`);
    await until("the edit open again", () => app.js(`return !!document.querySelector(arguments[0])`, title), MINUTE);
    await until(
      "both pieces in the editor",
      () => app.js(`return document.querySelector("section.edit-view .context")?.textContent.includes("of 2")`),
      10_000,
    );
    await app.toStart();
    await app.key("ArrowRight", { shift: true });
    await until("back at 1 s", async () => (await app.shownTime()) === 1, 10_000);
    // Captions, on the edit's timeline: the VOD's where its pieces play. The
    // CC button shows, and Shift+C turns them off and on.
    const cues = await app.invoke("edit_captions", { id: made.id });
    if (!cues.length || cues.some((c) => c.start < 0 || c.end > 2.05 || c.end <= c.start))
      throw new Error(`the edit's captions: ${JSON.stringify(cues)}`);
    const cc = 'section.edit-view .bar button[aria-label="Captions"]';
    const ccOn = () => app.js(`return document.querySelector(arguments[0])?.getAttribute("aria-pressed")`, cc);
    // Its captions read (the button's off until they are).
    const was = await until(
      "the CC button, its captions read",
      () =>
        app.js(
          `const b = document.querySelector(arguments[0]); return b && !b.disabled && b.getAttribute("aria-pressed")`,
          cc,
        ),
      10_000,
    );
    // Where the screen can be read, the picture's lower part changes with them.
    const screen =
      !os.unavailable() &&
      (() => {
        try {
          return !!os.screenshot();
        } catch {
          return false;
        }
      })();
    const lower = async () => {
      if (!screen) return null;
      const r = await os.rectOf(app, "section.edit-view .frame");
      await new Promise((r) => setTimeout(r, 500));
      return os.pixels(os.screenshot(), { x: r.x, y: Math.round(r.y + r.h * 0.7), w: r.w, h: Math.round(r.h * 0.3) });
    };
    const seen = [await lower()];
    for (const want of [was !== "true", was === "true"]) {
      await app.key("C", { shift: true });
      await until(`captions ${want ? "on" : "off"} (Shift+C)`, async () => (await ccOn()) === String(want), 10_000);
      seen.push(await lower());
    }
    if (screen) {
      const changed = os.difference(seen[0], seen[1]);
      const same = os.difference(seen[0], seen[2]);
      ctx.log(
        `the edit's picture, lower part: ${changed.toFixed(1)} apart with captions toggled, ${same.toFixed(1)} back`,
      );
      // (A short caption, a few words, changes little of it: against nothing at all back.)
      if (changed < 0.5 || same > changed / 3) throw new Error("the edit's captions don't show over its picture");
    }
    ctx.log(`captions: ${cues.length}, the first "${cues[0].text}"`);
    const said = await until(
      "the edit's chat beside its transcript",
      async () => {
        const shown = await app.js(
          `return [...document.querySelectorAll(".transcript .chat .message")].map((m) => m.textContent.trim())`,
        );
        return shown.length === 2 && shown;
      },
      10_000,
    );
    if (!said[0].includes("liftoff") || !said[1].includes("eagle")) throw new Error(`the edit's chat: ${said}`);
    const map = await app.invoke("edit_chat_overview", { id: made.id, duration: 2, bins: 4 });
    if (map.count !== 2 || map.histogram[1] !== 1 || map.histogram[3] !== 1)
      throw new Error(`the edit's chat overview: ${JSON.stringify(map)}`);
    await ctx.checkpoint("edit-chat");
    // Its Chat button, in the transcript's header as on a VOD, hides and shows it.
    const messages = () => app.js(`return document.querySelectorAll(".transcript .chat .message").length`);
    await app.click(".transcript-head .chat-toggle, .head .chat-toggle");
    await until("the edit's chat hidden", async () => (await messages()) === 0, 10_000);
    await app.click(".transcript-head .chat-toggle, .head .chat-toggle");
    await until("the edit's chat back", async () => (await messages()) === 2, 10_000);
    ctx.log("the edit's chat: the VOD's two messages in its stretch, beside its transcript");
    // Opened afresh, unchanged since: its chat shows at once (it's asked
    // for again once the edit's length is known).
    await app.click(`.vod[data-key="${vod.id}"] button.main`);
    await until(
      "the VOD open",
      async () => !(await app.js(`return !!document.querySelector(arguments[0])`, title)),
      MINUTE,
    );
    await app.click(`.vod.edit[data-key="-${made.id}"] button.main`);
    await until("the edit's chat on opening it", async () => (await messages()) === 2, 10_000);
    // A sub in its VOD's chat at 2 s: one of the edit's events, at 1 s of it.
    const events = new DatabaseSync(library);
    events
      .prepare(
        "INSERT INTO chat (vod_id, time, author, color, text, parts, kind, amount) VALUES (?, 2, 'fan', NULL, 'subbed', '[]', 'sub', 1)",
      )
      .run(vod.id);
    events.close();
    const withEvents = await app.invoke("edit_chat_overview", { id: made.id, duration: 2, bins: 4, events: true });
    if (withEvents.events?.length !== 1 || Math.abs(withEvents.events[0].time - 1) > 0.01)
      throw new Error(`the edit's chat events: ${JSON.stringify(withEvents.events)}`);
    ctx.log("the edit's chat events: its VOD's sub, where it plays");
    // Delete held down: its repeats remove nothing (only a press does).
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", repeat: true, bubbles: true }))`,
    );
    await new Promise((r) => setTimeout(r, 1000));
    if (pieces(await edit()).length !== 2) throw new Error("a held-down Delete's repeat removed a piece");

    // The clip came in as a group, one band; both halves are in it. Opened
    // (double-click), its pieces show one by one and move among themselves.
    await app.js(
      `document.querySelector("section.edit-view .piece.group").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))`,
    );
    await until(
      "the group open",
      () => app.js(`return document.querySelectorAll("section.edit-view .piece.in-group").length === 2`),
      10_000,
    );

    // Ctrl+← (⌘← on a Mac) moves the piece at the playhead (the second, at
    // 1 s) a place earlier, the playhead with it; Ctrl+→ moves it back.
    const order = async () => pieces(await edit()).map((p) => p.id);
    const before = await order();
    const move = (key) =>
      app.js(
        `const mac = /Mac/.test(navigator.platform);
         document.body.dispatchEvent(new KeyboardEvent("keydown", { key: arguments[0], ctrlKey: !mac, metaKey: mac, bubbles: true }))`,
        key,
      );
    await move("ArrowLeft");
    await until(
      "the second piece first",
      async () => {
        const now = await order();
        if (now[0] === before[1] && now[1] === before[0]) return true;
        throw new Error(`pieces ${JSON.stringify(now)}, were ${JSON.stringify(before)}`);
      },
      10_000,
    );
    await until("the playhead with it, at 0 s", async () => (await app.shownTime()) === 0, 10_000);
    await move("ArrowRight");
    await until(
      "the pieces as they were",
      async () => JSON.stringify(await order()) === JSON.stringify(before),
      10_000,
    );
    await until("the playhead back at 1 s", async () => (await app.shownTime()) === 1, 10_000);

    // A piece's end dragged: a dashed guide down the waveform where the
    // edge being moved is, from its first move; the drag cancelled, the
    // guide goes and the edit is as it was.
    const sizedBefore = JSON.stringify(pieces(await edit()));
    const guide = await app.js(
      `${AT}
       const grip = document.querySelector("section.edit-view .piece .grip.end");
       const band = grip.closest(".piece");
       const g = grip.getBoundingClientRect();
       const [x0, y0] = [g.left + g.width / 2, g.top + g.height / 2];
       grip.dispatchEvent(new PointerEvent("pointerdown", at(x0, y0, 1)));
       window.dispatchEvent(new PointerEvent("pointermove", at(x0 - 20, y0, 1)));
       await new Promise((r) => requestAnimationFrame(() => r()));
       const wave = document.querySelector("section.edit-view .scrubber").getBoundingClientRect();
       const line = document.querySelector("section.edit-view .scrubber .size-guide");
       const l = line?.getBoundingClientRect();
       const out = { edge: band.getBoundingClientRect().right, wave: [wave.top, wave.bottom],
         guide: l ? { x: l.left, top: l.top, bottom: l.bottom, style: getComputedStyle(line).borderLeftStyle } : null };
       window.dispatchEvent(new PointerEvent("pointercancel", at(x0 - 20, y0, 0)));
       return out;`,
    );
    const g = guide.guide;
    if (
      !g ||
      Math.abs(g.x - guide.edge) > 1.5 ||
      g.style !== "dashed" ||
      Math.abs(g.top - guide.wave[0]) > 1 ||
      Math.abs(g.bottom - guide.wave[1]) > 1
    )
      throw new Error(
        `a piece's end dragged: the guide ${JSON.stringify(g)}, the edge at ${guide.edge} px, the waveform ${guide.wave}`,
      );
    await until(
      "the guide gone once cancelled",
      () => app.js(`return !document.querySelector("section.edit-view .size-guide")`),
      10_000,
    );
    if (JSON.stringify(pieces(await edit())) !== sizedBefore) throw new Error("a cancelled resize changed the edit");
    ctx.log(`resizing a piece: a dashed guide at ${Math.round(g.x)} px, on its edge (${Math.round(guide.edge)} px)`);

    // Remove hovered: the piece it would take out (the one at the playhead)
    // lights up on the timeline, and goes dark as the pointer leaves.
    const removeLit = () =>
      app.js(
        `const lit = document.querySelector("section.edit-view .lit.removing")?.style;
         const piece = document.querySelector("section.edit-view .piece.selected")?.style;
         return lit ? [lit.left, lit.width, piece?.left, piece?.width] : null;`,
      );
    await app.js(`document.querySelector("button.action.remove").dispatchEvent(new MouseEvent("mouseenter"))`);
    await until(
      "the piece at the playhead lit up as going",
      async () => {
        const lit = await removeLit();
        if (lit && lit[0] === lit[2] && lit[1] === lit[3]) return true;
        throw new Error(`lit, then the piece: ${JSON.stringify(lit)}`);
      },
      10_000,
    );
    await app.js(`document.querySelector("button.action.remove").dispatchEvent(new MouseEvent("mouseleave"))`);
    await until("it dark again", async () => (await removeLit()) === null, 10_000);

    // The edit's transcript, under its player as a VOD's is: only what
    // plays. A click on a line plays from it and changes nothing; Shift+click
    // selects it, and Delete takes it out of the edit (its slice of the timeline,
    // the pieces split there, as any removal), gone from the transcript;
    // Ctrl+Z brings it back.
    const rows = "section.edit-view ~ .below .body .transcript .line button.row";
    const lineCount = () => app.js(`return document.querySelectorAll(arguments[0]).length`, rows);
    const lines = await until("the edit's transcript", lineCount, 10_000);
    const struckCount = async () => (await edit()).pieces.reduce((n, p) => n + p.struck.length, 0);
    await app.click(rows);
    await new Promise((r) => setTimeout(r, 1000));
    if (await struckCount()) throw new Error("a click on a line struck it");
    // The timeline shows all of the edit, before the removal and after it's undone.
    const spanNow = () =>
      app.js(`return Number(document.querySelector("section.edit-view .scrubber")?.dataset.span ?? 0)`);
    const whole = await spanNow();
    await app.js(
      `document.querySelector(arguments[0]).dispatchEvent(new MouseEvent("click", { shiftKey: true, bubbles: true }))`,
      rows,
    );
    await until(
      "the line selected",
      () => app.js(`return !!document.querySelector(".transcript .line.picked")`),
      10_000,
    );
    const played = await lengthNow();
    await app.key("Delete");
    await until(
      "the line taken out",
      async () => (await lengthNow()) < played - 0.01 && (await lineCount()) < lines && (await struckCount()) === 0,
      10_000,
    );
    const shorter = await until(
      "the timeline shorter",
      async () => (await spanNow()) < whole - 0.01 && spanNow(),
      10_000,
    );
    await app.key("z", { cmd: true });
    await until(
      "the line back",
      async () => Math.abs((await lengthNow()) - played) < 0.01 && (await lineCount()) === lines,
      10_000,
    );
    await until(
      "the timeline showing all of the edit again",
      async () => {
        const span = await spanNow();
        if (Math.abs(span - whole) < 0.01) return true;
        throw new Error(`it shows ${span} s of ${whole} s`);
      },
      10_000,
    );
    ctx.log(`split in two; a line of ${lines} taken out (${whole} s → ${shorter} s) and brought back`);

    // A chapter in the edit (N) at 1 s, its title written: on the edit's
    // timeline, in its chapters lane and over its transcript; Ctrl+Z takes
    // it back, as the edit's own change.
    const editMarks = () => app.invoke("edit_marks", { id: made.id });
    // Paused: the click on a line above played it, and N puts the chapter
    // where the playhead is when it's pressed.
    await app.pause();
    await app.toStart();
    await until("the start", async () => (await app.shownTime()) === 0, 10_000);
    await app.key("ArrowRight", { shift: true });
    await until("1 s in", async () => (await app.shownTime()) === 1, 10_000);
    await app.key("n");
    const titleBox = '[role=dialog][aria-label="Chapter title"] input';
    await until(
      "the chapter's title box",
      () => app.js(`return !!document.querySelector(arguments[0])`, titleBox),
      10_000,
    );
    await app.type(titleBox, "Touchdown");
    await app.submit(titleBox);
    const chapter = await until(
      "the edit's chapter",
      async () => {
        const [m, ...more] = await editMarks();
        if (m?.kind === "chapter" && m.text === "Touchdown" && Math.abs(m.start - 1) < 0.05 && !more.length) return m;
        throw new Error(`marks: ${JSON.stringify([m, ...more])}`);
      },
      10_000,
    );
    await until(
      "the chapter in the lane and over the transcript",
      () =>
        app.js(
          `return !!document.querySelector('section.edit-view .chapters [data-chapter="${chapter.id}"]') &&
             [...document.querySelectorAll(".transcript .chapter-head")].some((h) => h.textContent.includes("Touchdown"))`,
        ),
      10_000,
    );
    await ctx.checkpoint("edit-chapter");

    // The edit's selection is the marks' one (cuts.svelte.ts), as a VOD's.
    // With the chapter selected, a line chosen in the transcript (Shift+click
    // ends in the select() a drag across lines does; a drag across both
    // lines would take the chapter's too) lets go of it: Delete takes the
    // line out, and the chapter stays.
    const bandAt = `section.edit-view .chapters [data-chapter="${chapter.id}"]`;
    await app.js(
      `const band = document.querySelector(arguments[0]);
       const box = band.getBoundingClientRect();
       const at = { bubbles: true, cancelable: true, clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, isPrimary: true, pointerId: 1 };
       band.dispatchEvent(new PointerEvent("pointerdown", { ...at, buttons: 1 }));
       window.dispatchEvent(new PointerEvent("pointerup", { ...at, buttons: 0 }));`,
      bandAt,
    );
    await until(
      "the chapter selected",
      () => app.js(`return !!document.querySelector(arguments[0] + ".selected")`, bandAt),
      10_000,
    );
    const shiftClickFirst = () =>
      app.js(
        `document.querySelector(arguments[0]).dispatchEvent(new MouseEvent("click", { shiftKey: true, bubbles: true }))`,
        rows,
      );
    await shiftClickFirst();
    await until(
      "the line selected",
      () => app.js(`return !!document.querySelector(".transcript .line.picked")`),
      10_000,
    );
    const playedThen = await lengthNow();
    const linesThen = await lineCount();
    await app.key("Delete");
    await until(
      "the line taken out, not the chapter",
      async () => {
        const kept = (await editMarks()).some((m) => m.id === chapter.id);
        const length = await lengthNow();
        if (!kept) throw new Error("Delete took the chapter selected before the line, not the line");
        if (length < playedThen - 0.01) return true;
        throw new Error(`the edit still plays ${length} s`);
      },
      10_000,
    );
    // The toast's Undo brings the line back and then offers Redo, which takes
    // it out again (undo.ts, as Ctrl+Z's toast does); Ctrl+Z then brings it back.
    const toastButton = (label) =>
      until(
        `the toast's ${label}`,
        () =>
          app.js(
            `const b = document.querySelector(".toast button.act");
             if (b?.textContent.trim() !== arguments[0]) return false;
             b.click();
             return true;`,
            label,
          ),
        10_000,
      ).catch(async (e) => {
        const says = await app.js(`return document.querySelector(".toast")?.textContent.trim() ?? "(no toast)"`);
        throw new Error(`${e.message}: the toast says "${says}"`);
      });
    await toastButton("Undo");
    await until("the line back from the toast", async () => Math.abs((await lengthNow()) - playedThen) < 0.01, 10_000);
    await toastButton("Redo");
    await until("the line out again from the toast", async () => (await lengthNow()) < playedThen - 0.01, 10_000);
    await app.key("z", { cmd: true });
    await until(
      "the line back",
      async () => {
        const [length, count] = [await lengthNow(), await lineCount()];
        if (Math.abs(length - playedThen) < 0.01 && count === linesThen) return true;
        throw new Error(`${length} s of ${playedThen} s, ${count} lines of ${linesThen}`);
      },
      10_000,
    );
    ctx.log("a chapter selected, then a line: Delete took the line, the chapter stayed; the toast's Undo, then Redo");

    // The selection's edges show on the edit's timeline as on a VOD's: the
    // end dragged later, then a frame back with "," (its edge selected by the
    // drag), and Escape lets go of it.
    await shiftClickFirst();
    const EDIT_SELECTION = `
      const bar = document.querySelector("section.edit-view .scrubber");
      const picked = bar?.querySelector(".selection");
      if (!picked) return null;
      const box = bar.getBoundingClientRect();
      const { left, right } = picked.getBoundingClientRect();
      const time = (x) => Number(bar.dataset.viewStart) + ((x - box.left) / box.width) * Number(bar.dataset.span);
      return [time(left), time(right)];`;
    const lineSpan = await until("the line's stretch on the timeline", () => app.js(EDIT_SELECTION), 10_000);
    const endAt = 'section.edit-view .scrubber [data-edge="end"]';
    await until(
      "the selection's end to drag",
      () => app.js(`return !!document.querySelector(arguments[0])`, endAt),
      10_000,
    ).catch(() => {
      throw new Error("the edit's selection has no edges to drag");
    });
    const dragEdge = (kind, time) =>
      app.js(
        `const [kind, time, target] = arguments;
         const bar = document.querySelector("section.edit-view .scrubber");
         const box = bar.getBoundingClientRect();
         const x = box.left + ((time - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
         const at = { clientX: x, clientY: box.top + box.height / 2, button: 0, buttons: kind === "pointerup" ? 0 : 1, bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true };
         (kind === "pointerdown" ? document.querySelector(target) : window).dispatchEvent(new PointerEvent(kind, at));`,
        kind,
        time,
        endAt,
      );
    const goal = lineSpan[1] + 0.5;
    await dragEdge("pointerdown", lineSpan[1]);
    await dragEdge("pointermove", (lineSpan[1] + goal) / 2);
    await dragEdge("pointermove", goal);
    await dragEdge("pointerup", goal);
    const dragged = await until(
      "the end dragged later",
      async () => {
        const at = await app.js(EDIT_SELECTION);
        if (at && Math.abs(at[0] - lineSpan[0]) < 0.05 && Math.abs(at[1] - goal) < 0.1) return at;
        throw new Error(`the selection is ${JSON.stringify(at)}, from ${JSON.stringify(lineSpan)}`);
      },
      10_000,
    );
    await app.key(",");
    const nudged = await until(
      "the end a frame back",
      async () => {
        const at = await app.js(EDIT_SELECTION);
        if (at && at[1] < dragged[1] - 0.005 && at[1] > dragged[1] - 0.1) return at;
        throw new Error(`the selection is ${JSON.stringify(at)}, dragged to ${JSON.stringify(dragged)}`);
      },
      10_000,
    );
    await app.key("Escape");
    await until("the selection let go", async () => (await app.js(EDIT_SELECTION)) === null, 10_000);
    ctx.log(
      `the edit's selection: its end dragged ${lineSpan[1].toFixed(2)} → ${dragged[1].toFixed(2)} s, a frame back to ${nudged[1].toFixed(2)} s`,
    );
    // Its title, then the chapter: two steps back.
    for (const step of ["its title undone", "the chapter undone"]) {
      await app.key("z", { cmd: true });
      await until(
        step,
        async () => {
          const [m] = await editMarks();
          return step === "its title undone" ? m && !m.text : !m;
        },
        10_000,
      );
    }
    await until(
      "the lane without it",
      () => app.js(`return !document.querySelector("section.edit-view .chapters [data-chapter]")`),
      10_000,
    );
    ctx.log(`a chapter in the edit at ${chapter.start} s, then undone`);

    // Held on the edit's picture, the piece at the playhead is carried, as
    // on a VOD's player: onto another edit's row, it goes in there.
    const other = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: other.id, vodId: vod.id, start: 0, end: 1, title: "Other" });
    // (Where it drops shows over the row while something's carried.)
    const otherRow = `[data-edit-drop="${other.id}"]`;
    await until(
      "the other edit's row",
      () => app.js(`return !!document.querySelector(arguments[0])`, `.list .vod[data-key="${-other.id}"]`),
      MINUTE,
    );
    const held = await app.js(
      `${AT}
       const frame = document.querySelector("section.edit-view .frame");
       const r = frame.getBoundingClientRect();
       const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
       frame.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
       await new Promise((done) => setTimeout(done, 500));
       const carried = !!document.querySelector(".carry-ghost");
       const drop = document.querySelector(arguments[0]);
       if (!drop) {
         window.dispatchEvent(new PointerEvent("pointerup", at(x, y, 0)));
         return { carried, lit: "no row to drop on" };
       }
       const row = drop.getBoundingClientRect();
       const [x1, y1] = [row.left + row.width / 2, row.top + row.height / 2];
       for (let i = 1; i <= 5; i++) window.dispatchEvent(new PointerEvent("pointermove", at(x + ((x1 - x) * i) / 5, y + ((y1 - y) * i) / 5, 1)));
       await new Promise((done) => requestAnimationFrame(done));
       const lit = !!document.querySelector(arguments[0] + ".target");
       window.dispatchEvent(new PointerEvent("pointerup", at(x1, y1, 0)));
       frame.dispatchEvent(new MouseEvent("click", { bubbles: true }));
       return { carried, lit };`,
      otherRow,
    );
    if (!held.carried || held.lit !== true) throw new Error(`the edit's picture held: ${JSON.stringify(held)}`);
    await until(
      "the piece in the other edit",
      async () => (await app.invoke("get_edit", { id: other.id })).pieces.length >= 2,
      10_000,
    );
    await app.invoke("delete_edit", { id: other.id });
    ctx.log("the edit's picture held: its piece carried onto another edit's row, in there");

    // Clip (the button), then C: the piece at the playhead (the second, 1 s
    // long) a clip on the edit's own clip bar, its file made; pressed twice,
    // and again once it's made, it's still the one clip. Ctrl+Z takes it off,
    // Ctrl+Shift+Z brings it back (made again).
    const barClips = () => app.invoke("edit_clips", { id: made.id });
    await app.click("section.edit-view .bar button.clip");
    await app.key("c");
    const barClip = await until(
      "the edit's clip, made",
      async () => {
        const [c, ...more] = await barClips();
        if (c?.status === "error") throw new Error(`it failed: ${c.error}`);
        if (more.length) throw new Error(`${more.length + 1} clips`);
        return c?.status === "ready" && existsSync(c.file_path ?? "") && c;
      },
      MINUTE,
    );
    if (Math.abs(barClip.length - 1) > 0.05) throw new Error(`the clip is ${barClip.length} s long, not 1`);
    await app.key("c");
    await new Promise((r) => setTimeout(r, 1500));
    const once = await barClips();
    if (once.length !== 1) throw new Error(`C again on the same stretch: ${once.length} clips`);
    await until(
      "its pill on the edit's clip bar",
      () => app.js(`return !!document.querySelector('section.edit-view .clip-bar .chip[data-clip="${barClip.id}"]')`),
      10_000,
    );
    await ctx.checkpoint("edit-clip-bar");
    // Its pill double-clicked opens it in the editor as a clip (the head says
    // "Clip name", no Render), the way a VOD's does: it's no edit, so the
    // library lists no row for it. Its head names the edit before it; the
    // way back under its player opens the edit, its pill picked there.
    await openPill(app, barClip.id);
    const opened = await app.js(`return {
      row: !!document.querySelector('.list .vod[data-key="${-barClip.id}"]'),
      render: !!document.querySelector("section.edit-view button.make") };`);
    if (opened.row) throw new Error("the edit's clip, opened, is listed in the library");
    if (opened.render) throw new Error("the edit's clip, opened, has Render");
    const { edit: owner } = await app.invoke("get_edit", { id: made.id });
    const crumb = await until(
      "the edit's name before the clip's",
      () => app.js(`return document.querySelector("section.edit-view .head .crumb")?.textContent.trim()`),
      10_000,
    );
    if (crumb !== owner.title) throw new Error(`the clip's head says "${crumb}" before its name, not "${owner.title}"`);
    await ctx.checkpoint("edit-clip-open");
    await app.click("section.edit-view .sources button.back");
    await until(
      "the edit back, the clip's pill picked",
      () =>
        app.js(
          `return !!document.querySelector('${title}') && !!document.querySelector(arguments[0])`,
          `section.edit-view .chip.chosen[data-clip="${barClip.id}"]`,
        ),
      10_000,
    );
    ctx.log(`the edit's clip double-clicked: open as a clip under "${crumb}", not listed; back to the edit, picked`);
    const undoKey = (shift) => app.key("z", { cmd: true, shift });
    await undoKey(false);
    await until("the clip taken off", async () => (await barClips()).length === 0, 10_000);
    await undoKey(true);
    const back = await until(
      "the clip back, made again",
      async () => {
        const [c] = await barClips();
        return c?.status === "ready" && existsSync(c.file_path ?? "") && c;
      },
      MINUTE,
    );
    ctx.log(`C in the edit: a ${barClip.length} s clip on its bar (${back.file_path}), undone and redone`);
    // Its file deleted outside the app (in a file manager): back in the
    // window, the clip has left the bar; Ctrl+Z brings it back, made again.
    rmSync(back.file_path);
    await app.js(`window.dispatchEvent(new Event("focus"))`);
    await until("the clip gone with its file", async () => (await barClips()).length === 0, 10_000);
    await undoKey(false);
    const barKept = await until(
      "the clip back after its file went",
      async () => {
        const [c] = await barClips();
        return c?.status === "ready" && existsSync(c.file_path ?? "") && c;
      },
      MINUTE,
    );
    ctx.log("its file deleted outside the app: off the bar, and back with Ctrl+Z");

    // Rendered (E) with a chapter, which its file has.
    await app.invoke("add_edit_mark", { editId: made.id, time: 0.5, kind: "chapter", text: "Landing" });
    // Timed: from E to the edit's file made.
    const rendered = await ctx.time("edit rendered", async () => {
      await app.key("e");
      return until(
        "the render",
        async () => {
          const [e] = await app.invoke("list_edits");
          if (e?.status === "error") throw new Error(`the render failed: ${e.error}`);
          return e?.status === "ready" && e;
        },
        2 * MINUTE,
        { every: TIMED_POLL },
      );
    });
    if (!rendered.file_path || !existsSync(rendered.file_path))
      throw new Error(`the render says ${rendered.file_path}, which isn't there`);
    // Sound only, made as MP4 (the default): an .m4a.
    if (path.extname(rendered.file_path) !== ".m4a") throw new Error(`rendered as ${rendered.file_path}, not an .m4a`);
    ctx.log(`rendered ${rendered.file_path}`);
    await ctx.checkpoint("edit-rendered");
    const probe = spawnSync(
      "ffprobe",
      ["-v", "error", "-show_entries", "chapter_tags=title", "-of", "csv=p=0", rendered.file_path],
      { encoding: "utf8" },
    );
    if (probe.error) ctx.log(`no ffprobe here: the file's chapters aren't looked at (${probe.error.message})`);
    else if (!probe.stdout.includes("Landing"))
      throw new Error(`the rendered file's chapters: ${probe.stdout || probe.stderr}`);
    else ctx.log("its file has the edit's chapter");
    // A highlight added since leaves it as rendered; a chapter doesn't.
    if (!rendered.current) throw new Error("just rendered, it says it's changed since");
    await app.invoke("add_edit_mark", { editId: made.id, time: 1.5, kind: "highlight", text: null });
    await new Promise((r) => setTimeout(r, 500));
    if (!(await app.invoke("list_edits"))[0].current) throw new Error("a highlight made the render out of date");
    const later = await app.invoke("add_edit_mark", { editId: made.id, time: 1.5, kind: "chapter", text: "Later" });
    await until("a chapter making it out of date", async () => !(await app.invoke("list_edits"))[0].current, 10_000);
    await app.invoke("delete_mark", { id: later.id });
    await until("current again with the chapter gone", async () => (await app.invoke("list_edits"))[0].current, 10_000);

    // Renamed and rendered again: a file of the new name; the one before goes
    // (it was never dragged out).
    await app.invoke("rename_edit", { id: made.id, title: `${NAME} 2` });
    await until("the rename", async () => (await app.invoke("list_edits"))[0]?.title === `${NAME} 2`, 10_000);
    await app.invoke("render_edit_now", { id: made.id });
    const again = await until(
      "the second render",
      async () => {
        const [e] = await app.invoke("list_edits");
        if (e?.status === "error") throw new Error(`the render failed: ${e.error}`);
        return e?.status === "ready" && e.file_path !== rendered.file_path && e;
      },
      2 * MINUTE,
    );
    if (existsSync(rendered.file_path)) throw new Error(`the render before, ${rendered.file_path}, is still there`);
    await app.invoke("rename_edit", { id: made.id, title: NAME });
    await until("the name back", async () => (await app.invoke("list_edits"))[0]?.title === NAME, 10_000);
    ctx.log(`renamed and rendered again: ${again.file_path}, the one before gone`);

    // A name being typed stays while the edit changes under it (a render's news).
    await app.js(`document.querySelector(arguments[0]).focus()`, title);
    await app.type(title, "Half typ");
    await app.invoke("render_edit_now", { id: made.id });
    await until(
      "a render, begun and done",
      async () => (await app.invoke("list_edits"))[0]?.file_path !== again.file_path,
      2 * MINUTE,
    );
    await new Promise((r) => setTimeout(r, 500));
    const typed = await app.js(`return document.querySelector(arguments[0]).value`, title);
    if (typed !== "Half typ") throw new Error(`the name being typed became "${typed}" when the edit changed`);
    await app.type(title, NAME);
    await app.blur(title);

    // Still there after a restart.
    app = await ctx.restart();
    await app.toLibrary();
    const [kept] = await app.invoke("list_edits");
    if (kept?.title !== NAME || kept.status !== "ready")
      throw new Error(`after a restart the edit is ${JSON.stringify(kept)}`);
    if (pieces(await app.invoke("get_edit", { id: kept.id })).length !== 2)
      throw new Error("after a restart the edit hasn't its two pieces");

    // Removing the VOD names the edit its pieces would be taken out of.
    await app.answer("Keep");
    await app.click('button[aria-label="Remove"]');
    const removing = await until(
      "Remove to ask",
      () => app.js(`return window.__e2e.asked.length ? JSON.stringify(window.__e2e.asked.at(-1)) : null`),
      10_000,
    );
    if (!removing.includes(`Its pieces in the edit “${NAME}” stay`))
      throw new Error(`removing the VOD asked ${removing}, not naming the edit`);
    if ((await app.invoke("list_vods")).length !== 1) throw new Error(`answering "Keep" removed the VOD`);

    // Removed, its video deleted: out of the library, and the stretches the
    // edit uses kept as videos of their own ("realized"), which it renders from.
    await app.answer("Remove and delete the video");
    await app.click('button[aria-label="Remove"]');
    await until("the VOD out of the list", async () => (await app.invoke("list_vods")).length === 0, MINUTE);
    const realized = pieces(await app.invoke("get_edit", { id: kept.id }));
    if (realized.length !== 2) throw new Error("removing the VOD took pieces out of the edit");
    if (realized.some((p) => p.vod_id === vod.id || p.hole))
      throw new Error(`the edit's pieces weren't kept: ${JSON.stringify(realized.map((p) => [p.vod_id, p.hole]))}`);
    const renderedBefore = (await app.invoke("list_edits"))[0].file_path;
    await app.invoke("render_edit_now", { id: kept.id });
    await until(
      "the edit rendered from what was kept",
      async () => {
        const [e] = await app.invoke("list_edits");
        if (e?.status === "error") throw new Error(`the render failed: ${e.error}`);
        return e?.status === "ready" && e.current && existsSync(e.file_path) && e;
      },
      2 * MINUTE,
    );
    ctx.log(
      `the VOD removed with its video: the edit's pieces kept (VODs ${realized.map((p) => p.vod_id)}), rendered again over ${renderedBefore}`,
    );

    // Deleted from its row, after asking, with the clip on its clip bar open
    // in the editor: the clip goes with it, as a VOD's clips do: its file
    // deleted, out of the library, and the page told ("clip-removed"), which
    // closes the editor.
    const [onBar] = await app.invoke("edit_clips", { id: kept.id });
    if (onBar?.id !== barKept.id || !existsSync(onBar.file_path ?? ""))
      throw new Error(`before the edit's deleted, its clip bar has ${JSON.stringify(onBar)}`);
    await app.js(
      `window.__clipsRemoved = [];
       const internals = window.__TAURI_INTERNALS__;
       const handler = internals.transformCallback((e) => window.__clipsRemoved.push(e.payload));
       await internals.invoke("plugin:event|listen", { event: "clip-removed", target: { kind: "Any" }, handler });`,
    );
    await app.click(`.vod.edit[data-key="-${kept.id}"] button.main`);
    await until(
      "the edit open, its clip on its bar",
      () => app.js(`return !!document.querySelector(arguments[0])`, `section.edit-view .chip[data-clip="${onBar.id}"]`),
      MINUTE,
    );
    await openPill(app, onBar.id);
    const asked = await app.asked();
    await app.answer("Delete");
    await app.click(`.vod.edit[data-key="-${kept.id}"] button[aria-label="Delete edit"]`);
    await until("the edit gone", async () => (await app.invoke("list_edits")).length === 0, MINUTE);
    if ((await app.asked()) !== asked + 1) throw new Error("deleting didn't ask first");
    await until("its clip's file gone with it", () => !existsSync(onBar.file_path), 10_000);
    const records = new DatabaseSync(libraryPath());
    const row = records.prepare("SELECT id FROM clips WHERE id = ?").get(onBar.id);
    records.close();
    if (row) throw new Error(`its clip ${onBar.id} is still in the library`);
    const told = await app.js("return window.__clipsRemoved");
    if (!told.includes(onBar.id)) throw new Error(`the page was told of ${JSON.stringify(told)}, not clip ${onBar.id}`);
    await until("the editor closed", () => app.js(`return !document.querySelector("section.edit-view")`), 10_000);
    ctx.log(`the edit deleted: its clip ${onBar.id} with it, file and all (${onBar.file_path}), its editor closed`);

    // Nothing uses the VOD now: erased by the time the app is back.
    app = await ctx.restart();
    await app.toLibrary();
    const { vods: readded } = await app.invoke("add_files", { paths: [ctx.fixtures.landed] });
    if (readded[0]?.id === vod.id) throw new Error("the VOD nothing used was still there after a restart");

    // An edit of it, not transcribed: its bar has the CC button all the same,
    // off, as a VOD's does, so the bar doesn't change as captions come.
    const bare = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: bare.id, vodId: readded[0].id, start: 0, end: 2, title: "Bare" });
    await until(
      "its row",
      () => app.js(`return !!document.querySelector(arguments[0])`, `.vod.edit[data-key="-${bare.id}"] button.main`),
      MINUTE,
    );
    await app.click(`.vod.edit[data-key="-${bare.id}"] button.main`);
    await until("its bar", () => app.js(`return !!document.querySelector("section.edit-view .bar")`), MINUTE);
    const off = await app.js(`return document.querySelector(arguments[0])?.disabled ?? "none"`, cc);
    if (off !== true)
      throw new Error(`an edit with no captions: its CC button is ${off === "none" ? "missing" : "on"}`);
    ctx.log("an edit with no captions: its CC button there, off");
  },
};
