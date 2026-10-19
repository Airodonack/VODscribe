// Edits: a clip added to a new edit from its pill (a group, one band); the
// edit opened, renamed, its piece split (B), the group opened (double-click)
// and the second half moved first and back (Ctrl+← and →), a line struck out and the strike undone
// (Ctrl+Z), rendered to a file (E); all of it still there after a restart;
// removing the VOD says its pieces stay in the edit (Keep: nothing goes;
// Remove and delete the video: it leaves the library, and the stretches
// the edit uses are kept as videos of their own, which it renders); then the edit
// deleted, after asking, and with nothing using the VOD it's erased by the
// next start (added again, it's a new one).

import { existsSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";

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

    // Split at the playhead, a second into the edit (its one piece, 2 s long).
    await app.toStart();
    await until("the start", async () => (await app.shownTime()) === 0, 10_000);
    await app.key("ArrowRight", { shift: true });
    await until("1 s in", async () => (await app.shownTime()) === 1, 10_000);
    await app.key("b");
    await until("two pieces", async () => pieces(await edit()).length === 2, 10_000);
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

    // A line struck out, then the strike undone.
    await until(
      "a line to strike",
      () => app.js(`return !!document.querySelector(".lines .line button.text")`),
      10_000,
    );
    // The timeline shows all of the edit, before the strike and after it's undone.
    const spanNow = () =>
      app.js(`return Number(document.querySelector("section.edit-view .scrubber")?.dataset.span ?? 0)`);
    const whole = await spanNow();
    await app.click(".lines .line button.text");
    await until("the line struck", () => app.js(`return !!document.querySelector(".lines .line.struck")`), 10_000);
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
    await until("the strike undone", () => app.js(`return !document.querySelector(".lines .line.struck")`), 10_000);
    await until(
      "the timeline showing all of the edit again",
      async () => {
        const span = await spanNow();
        if (Math.abs(span - whole) < 0.01) return true;
        throw new Error(`it shows ${span} s of ${whole} s`);
      },
      10_000,
    );
    const toast = await app.js(`return document.querySelector(".toast .message")?.textContent.trim() ?? ""`);
    ctx.log(`split in two; struck a line, then "${toast}"`);

    // Two lines of a piece struck in quick succession, before the first is
    // saved: both are struck. Then both brought back.
    const quick = await app.js(
      `const groups = [];
       for (const el of document.querySelector(".lines").children) {
         if (el.matches(".piece-head")) groups.push([]);
         else if (el.matches(".line")) groups.at(-1)?.push(el.querySelector("button.text"));
       }
       const piece = groups.find((g) => g.length >= 2);
       if (!piece) return 0;
       const [a, b] = piece;
       a.click();
       b.click();
       return 2;`,
    );
    if (quick) {
      await until(
        "both lines struck",
        async () => {
          const struck = (await edit()).pieces.reduce((n, p) => n + p.struck.length, 0);
          if (struck >= 2) return true;
          throw new Error(`${struck} struck`);
        },
        10_000,
      );
      await app.js(`for (const b of document.querySelectorAll(".lines .line.struck button.text")) b.click();`);
      await until("the strikes taken back", async () => (await edit()).pieces.every((p) => !p.struck.length), 10_000);
      ctx.log("two lines struck at once: both struck");
    } else ctx.log("no piece with two lines: the quick strikes weren't tried");

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
