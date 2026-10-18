// Edits: a clip added to a new edit from its pill; the edit opened,
// renamed, its piece split (B), a line struck out and the strike undone
// (Ctrl+Z), rendered to a file (E); all of it still there after a restart;
// removing the VOD warns that its pieces leave the edit (Keep: nothing
// goes); then the edit deleted, after asking.

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

    // A line struck out, then the strike undone.
    await until(
      "a line to strike",
      () => app.js(`return !!document.querySelector(".lines .line button.text")`),
      10_000,
    );
    await app.click(".lines .line button.text");
    await until("the line struck", () => app.js(`return !!document.querySelector(".lines .line.struck")`), 10_000);
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
    await until("the strike undone", () => app.js(`return !document.querySelector(".lines .line.struck")`), 10_000);
    const toast = await app.js(`return document.querySelector(".toast .message")?.textContent.trim() ?? ""`);
    ctx.log(`split in two; struck a line, then "${toast}"`);

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
    if (!removing.includes(`taken out of the edit “${NAME}”`))
      throw new Error(`removing the VOD asked ${removing}, not naming the edit`);
    if ((await app.invoke("list_vods")).length !== 1) throw new Error(`answering "Keep" removed the VOD`);

    // Deleted from its row, after asking.
    const asked = await app.asked();
    await app.answer("Delete");
    await app.click(`.vod.edit[data-key="-${kept.id}"] button[aria-label="Delete edit"]`);
    await until("the edit gone", async () => (await app.invoke("list_edits")).length === 0, MINUTE);
    if ((await app.asked()) !== asked + 1) throw new Error("deleting didn't ask first");
  },
};
