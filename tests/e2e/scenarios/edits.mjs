// Edits: a clip added to a new edit from its pill (a group, one band); the
// edit opened, renamed, a cut put on it (B) and a delete beside it making it a real split (and undone), its piece split, the group opened (double-click)
// and the second half moved first and back (Ctrl+← and →), a line's slice taken out and brought back
// (Ctrl+Z), rendered to a file (E); all of it still there after a restart;
// removing the VOD says its pieces stay in the edit (Keep: nothing goes;
// Remove and delete the video: it leaves the library, and the stretches
// the edit uses are kept as videos of their own, which it renders); then the edit
// deleted, after asking, and with nothing using the VOD it's erased by the
// next start (added again, it's a new one).

import { existsSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const NAME = "Landing edit";

export default {
  name: "edits",
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    const clip = await app.clipOneToThree(vod.id);

    // From the clip's pill: Add to an edit › New edit.
    await until(
      "the pill",
      () => app.js(`return !!document.querySelector('.chip.ready[data-clip="${clip.id}"]')`),
      10_000,
    );
    await app.click(`.chip.ready[data-clip="${clip.id}"] button[aria-label="Add to an edit"]`);
    await until(
      "the Add to an edit box",
      () => app.js(`return !!document.querySelector('[role=dialog][aria-label="Add to an edit"] button.new')`),
      10_000,
    );
    await app.click('[role=dialog][aria-label="Add to an edit"] button.new');
    const [made] = await until(
      "the new edit",
      async () => {
        const edits = await app.invoke("list_edits");
        return edits.length === 1 && edits;
      },
      10_000,
    );
    ctx.log(`made "${made.title}"`);

    // Opened from its row (the search cleared: results take the main area).
    await app.type('input[placeholder="Search for a phrase…"]', "");
    await app.click(`.vod.edit[data-key="-${made.id}"] button.main`);
    const title = 'section.edit-view input.title[aria-label="Edit name"]';
    await until("the edit open", () => app.js(`return !!document.querySelector(arguments[0])`, title), MINUTE);
    // Renamed as typed: focused, typed into, then left (it saves on blur).
    await app.js(`document.querySelector(arguments[0]).focus()`, title);
    await app.type(title, NAME);
    await app.js(`document.querySelector(arguments[0]).blur()`, title);
    await until("the new name", async () => (await app.invoke("list_edits"))[0]?.title === NAME, 10_000);
    await until(
      "the row's new name",
      () => app.js(`return document.querySelector('.vod.edit[data-key="-${made.id}"] .title')?.textContent.trim()`),
      10_000,
    ).then((t) => {
      if (t !== NAME) throw new Error(`the row says "${t}"`);
    });

    await ctx.checkpoint("edit-view");
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
    const library = path.join(process.env.XDG_DATA_HOME ?? "", "app.vodscribe", "library.sqlite");
    const chatWritten = existsSync(library);
    if (chatWritten) {
      const { DatabaseSync } = await import("node:sqlite");
      const db = new DatabaseSync(library);
      const add = db.prepare(
        "INSERT INTO chat (vod_id, time, author, color, text, parts) VALUES (?, ?, ?, NULL, ?, ?)",
      );
      for (const [time, text] of [
        [1.5, "liftoff"],
        [2.5, "eagle"],
        [4, "after"],
      ])
        add.run(vod.id, time, "viewer", text, JSON.stringify([{ type: "text", text }]));
      db.close();
    } else ctx.log("no library here to write chat into: the edit's chat isn't tried");

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
    await app.key("Escape");
    await app.key("Delete");
    await until(
      "the stretch after the cut taken out",
      async () => Math.abs((await lengthNow()) - 1) < 0.05 && (await editCuts()).length === 0,
      10_000,
    );
    if (pieces(await edit()).length !== 1) throw new Error("Delete after a cut removed the piece");
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
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
    // Captions, on the edit's timeline: the VOD's where its pieces play. The
    // CC button shows, and Shift+C turns them off and on.
    const cues = await app.invoke("edit_captions", { id: made.id });
    if (!cues.length || cues.some((c) => c.start < 0 || c.end > 2.05 || c.end <= c.start))
      throw new Error(`the edit's captions: ${JSON.stringify(cues)}`);
    const cc = 'section.edit-view .bar button[aria-label="Captions"]';
    const ccOn = () => app.js(`return document.querySelector(arguments[0])?.getAttribute("aria-pressed")`, cc);
    const was = await until("the CC button", ccOn, 10_000);
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
      if (changed < 3 || same > changed / 3) throw new Error("the edit's captions don't show over its picture");
    }
    ctx.log(`captions: ${cues.length}, the first "${cues[0].text}"`);
    if (chatWritten) {
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
    }
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
    const rows = "section.edit-view ~ .body .transcript .line button.row";
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
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
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
    // Its title, then the chapter: two steps back.
    for (const step of ["its title undone", "the chapter undone"]) {
      await app.js(
        `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
      );
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

    // C: the piece at the playhead (the second, 1 s long) a clip on the
    // edit's own clip bar, its file made. Ctrl+Z takes it off, Ctrl+Shift+Z
    // brings it back (made again).
    const barClips = () => app.invoke("edit_clips", { id: made.id });
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
    await until(
      "its pill on the edit's clip bar",
      () => app.js(`return !!document.querySelector('section.edit-view .clip-bar .chip[data-clip="${barClip.id}"]')`),
      10_000,
    );
    await ctx.checkpoint("edit-clip-bar");
    const undoKey = (shift) =>
      app.js(
        `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, shiftKey: arguments[0], bubbles: true }))`,
        shift,
      );
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

    // Rendered: E.
    await app.key("e");
    const rendered = await until(
      "the render",
      async () => {
        const [e] = await app.invoke("list_edits");
        if (e?.status === "error") throw new Error(`the render failed: ${e.error}`);
        return e?.status === "ready" && e;
      },
      2 * MINUTE,
    );
    if (!rendered.file_path || !existsSync(rendered.file_path))
      throw new Error(`the render says ${rendered.file_path}, which isn't there`);
    // Sound only, made as MP4 (the default): an .m4a.
    if (path.extname(rendered.file_path) !== ".m4a") throw new Error(`rendered as ${rendered.file_path}, not an .m4a`);
    ctx.log(`rendered ${rendered.file_path}`);
    await ctx.checkpoint("edit-rendered");

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
    await app.js(`document.querySelector(arguments[0]).blur()`, title);

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

    // Deleted from its row, after asking.
    const asked = await app.asked();
    await app.answer("Delete");
    await app.click(`.vod.edit[data-key="-${kept.id}"] button[aria-label="Delete edit"]`);
    await until("the edit gone", async () => (await app.invoke("list_edits")).length === 0, MINUTE);
    if ((await app.asked()) !== asked + 1) throw new Error("deleting didn't ask first");

    // Nothing uses the VOD now: erased by the time the app is back.
    app = await ctx.restart();
    await app.toLibrary();
    const { vods: readded } = await app.invoke("add_files", { paths: [ctx.fixtures.landed] });
    if (readded[0]?.id === vod.id) throw new Error("the VOD nothing used was still there after a restart");
  },
};
