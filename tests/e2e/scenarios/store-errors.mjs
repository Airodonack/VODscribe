// Errors show wherever the user is: what goes wrong with an edit, a clip
// bar or the marks shows as a toast, not in one view's bar where it went
// unseen from another. An edit rendered and its file then deleted outside
// the app: its row's Show in folder, with a VOD open, says it isn't rendered.
// Then, in that edit, its pieces taken out behind the page's back (as by
// another window): N (a chapter) says it has to be within the edit, and C
// (a clip) that nothing of the edit plays there; each was silent in an edit,
// or shown only in its own bar. An error stays until it's dismissed.

import { rmSync } from "node:fs";
import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "store-errors",
  async run(app, { fixtures, log }) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(vod.id);
    const says = () => app.js(`return document.querySelector(".toast .message")?.textContent.trim() ?? ""`);
    const toastOf = (what, want) =>
      until(
        what,
        async () => {
          const message = await says();
          if (want.test(message)) return message;
          throw new Error(`the toast says "${message}"`);
        },
        10_000,
      );
    const dismiss = () => app.js(`document.querySelector(".toast button.close")?.click()`);

    // The edit's row, with the VOD open: its Show in folder, its file gone.
    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 0, end: 2 });
    await app.invoke("render_edit_now", { id: edit.id });
    const rendered = await until(
      "the edit rendered",
      async () => {
        const e = (await app.invoke("list_edits")).find((e) => e.id === edit.id);
        if (e?.status === "error") throw new Error(`it failed: ${e.error}`);
        return e?.status === "ready" && e.file_path && e;
      },
      MINUTE,
    );
    rmSync(rendered.file_path);
    const folder = `.vod.edit[data-key="-${edit.id}"] button[aria-label="Show in folder"]`;
    await until("its Show in folder", () => app.js(`return !!document.querySelector(arguments[0])`, folder), 10_000);
    if (await app.js(`return !!document.querySelector("section.edit-view") || !document.querySelector(".bar")`))
      throw new Error("the VOD isn't the one open");
    await app.js(`document.querySelector(arguments[0]).click()`, folder);
    log(`Show in folder, its file gone: "${await toastOf("the toast that it isn't rendered", /isn't rendered/)}"`);
    await dismiss();

    // The edit open, its pieces taken out behind the page's back.
    await app.click(`.vod.edit[data-key="-${edit.id}"] button.main`);
    await until(
      "the edit open",
      () => app.js(`return !!document.querySelector('section.edit-view input.title[aria-label="Edit name"]')`),
      MINUTE,
    );
    await until(
      "its timeline",
      () => app.js(`return !!document.querySelector("section.edit-view [data-piece]")`),
      10_000,
    );
    for (const piece of (await app.invoke("get_edit", { id: edit.id })).pieces)
      await app.invoke("remove_piece", { id: piece.id });
    await app.key("n");
    log(`N in it: "${await toastOf("the toast for the chapter", /within the edit/)}"`);
    await dismiss();
    await app.key("c");
    log(`C in it: "${await toastOf("the toast for the clip", /plays there/)}"`);
    // An error stays until it's dismissed (or put right), unlike a toast that tells what was done.
    await new Promise((r) => setTimeout(r, 7000));
    if (!/plays there/.test(await says())) throw new Error("the error went by itself");
    await dismiss();
  },
};
