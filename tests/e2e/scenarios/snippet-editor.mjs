// A clip opened in the editor: double-click its name in the clip bar. It's
// a clip there (named "Clip name", no Render or Delete, not a row of the
// library's list); cut a second in (B) and the rest taken out (Delete), its
// file is made again, a second long; undone (Ctrl+Z), it's two seconds
// again, made again too.

import { existsSync } from "node:fs";

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "snippet-editor",
  async run(app, { fixtures, log }) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(vod.id);
    const clip = await app.clipOneToThree(vod.id);
    await until("its pill, made", () => app.js(`return !!document.querySelector(".chip.ready .name")`), 10_000);

    await app.js(
      `document.querySelector('.chip.ready[data-clip="${clip.id}"] .name').dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))`,
    );
    const name = 'section.edit-view input.title[aria-label="Clip name"]';
    await until(
      "the clip open in the editor",
      () => app.js(`return !!document.querySelector(arguments[0])`, name),
      MINUTE,
    );
    const shown = await app.js(
      `return [!!document.querySelector("section.edit-view button.make"), !!document.querySelector('section.edit-view [aria-label="Delete edit"]')]`,
    );
    if (shown[0] || shown[1]) throw new Error(`a clip in the editor shows Render ${shown[0]}, Delete ${shown[1]}`);
    if ((await app.invoke("list_edits")).length) throw new Error("the clip is in the library's list of edits");

    // A cut a second in, and the rest taken out: its file made again, a second long.
    const before = (await app.invoke("list_clips", { vodId: vod.id }))[0].file_path;
    const length = async () =>
      (await app.invoke("get_edit", { id: clip.id })).pieces.reduce((t, p) => t + p.end - p.start, 0);
    await app.toStart();
    await app.key("ArrowRight", { shift: true });
    await until("1 s in", async () => (await app.shownTime()) === 1, 10_000);
    await app.key("b");
    await until(
      "the cut",
      async () => (await app.invoke("edit_marks", { id: clip.id })).some((m) => m.kind === "cut"),
      10_000,
    );
    // The page has it too, or Delete takes out the whole piece.
    await until(
      "the cut drawn",
      () => app.js(`return !!document.querySelector("section.edit-view .scrubber .cut")`),
      10_000,
    );
    await app.key("Escape");
    await app.key("Delete");
    await until("a second long", async () => Math.abs((await length()) - 1) < 0.05, 10_000);
    // Made again from the revision now (the cut was a change of its own, made too).
    const now = (await app.invoke("list_clips", { vodId: vod.id }))[0].revision;
    const made = (what) =>
      until(
        `the clip made again (${what})`,
        async () => {
          const [c] = await app.invoke("list_clips", { vodId: vod.id });
          if (c?.status === "error") throw new Error(`it failed: ${c.error}`);
          return (
            c?.status === "ready" &&
            c.revision >= now &&
            c.revision > clip.revision &&
            existsSync(c.file_path ?? "") &&
            c
          );
        },
        MINUTE,
      );
    const split = await made("shortened");
    log(`shortened in the editor: made again, ${before} → ${split.file_path}`);

    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
    await until("two seconds again", async () => Math.abs((await length()) - 2) < 0.05, 10_000);
    const back = await until(
      "the clip made again after the undo",
      async () => {
        const [c] = await app.invoke("list_clips", { vodId: vod.id });
        return c?.status === "ready" && c.revision > split.revision && existsSync(c.file_path ?? "") && c;
      },
      MINUTE,
    );
    log(`undone: two seconds, made again: ${back.file_path}`);
  },
};
