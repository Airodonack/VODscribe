// A clip: cuts at 1 and 3 s (B), then Export with the playhead between
// them; then deleting it.

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "clip",
  async run(app, { fixtures }) {
    await app.toLibrary();
    const done = await app.addAndTranscribe(fixtures.landed);
    await app.type('input[placeholder="Search for a phrase…"]', "landed");
    await until("a search result", () => app.js(`return !!document.querySelector("button.row")`), MINUTE);
    await app.click("button.row");
    await until("the player", () => app.js(`return !!document.querySelector(".player video")`), MINUTE);

    // Moved as a person would, with the keys (Player.svelte): ← back 5 s (to
    // the start of this 5 s file), Shift+→ on a second. The media element's
    // own time isn't the player's: set behind its back, the player's clock
    // (which places cuts) may not follow, as on macOS once it has ended.
    const to = async (seconds) => {
      await until(`the player at ${seconds} s`, async () => (await app.shownTime()) === seconds, 10_000);
    };
    // B makes a cut and, once it's recorded, selects it; the arrows would then
    // move it, so Escape lets it go, after it's selected (a quicker Escape finds nothing to let go).
    const cut = async () => {
      await app.key("b");
      await until("the cut, selected", () => app.js(`return !!document.querySelector(".cut.selected")`), 10_000);
      await app.key("Escape");
      await until("the cut let go", () => app.js(`return !document.querySelector(".cut.selected")`), 10_000);
    };
    await app.pause();
    await app.key("ArrowLeft");
    await to(0);
    await app.key("ArrowRight", { shift: true });
    await to(1);
    await cut();
    await app.key("ArrowRight", { shift: true });
    await app.key("ArrowRight", { shift: true });
    await to(3);
    await cut();
    const marks = await until(
      "two cuts",
      async () => {
        const marks = await app.invoke("list_marks", { vodId: done.id });
        if (marks.length === 2) return marks;
        throw new Error(`cuts at ${JSON.stringify(marks.map((m) => m.start))}`);
      },
      10_000,
    );
    const at = marks.map((m) => m.start).sort((a, b) => a - b);
    if (Math.abs(at[0] - 1) > 0.05 || Math.abs(at[1] - 3) > 0.05) throw new Error(`cuts at ${at}, not 1 and 3`);
    await app.key("ArrowLeft", { shift: true });
    await to(2);
    await app.click("button.export");
    const clip = await until(
      "the clip's file",
      async () => {
        const [c] = await app.invoke("list_clips", { vodId: done.id });
        if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
        return c?.status === "ready" && c;
      },
      MINUTE,
    );
    if (Math.abs(clip.start - 1) > 0.05 || Math.abs(clip.end - 3) > 0.05)
      throw new Error(`the clip runs ${clip.start} to ${clip.end}, not 1 to 3`);
    // The chip follows a moment after the clip is recorded as made.
    await until("the chip to show it made", () => app.js(`return !!document.querySelector(".chip.ready")`), 10_000);
    await app.click('.chip.ready button[aria-label="Delete clip"]');
    await until(
      "the clip to be deleted",
      async () => (await app.invoke("list_clips", { vodId: done.id })).length === 0,
      MINUTE,
    );
    app.log(`clipped "${clip.title}" to ${clip.file_path}, and deleted it`);
  },
};
