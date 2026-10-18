// Marks and undo: a highlight (H) and a chapter (N), each written a title in
// its popover; the chapter deleted (Delete), brought back with Ctrl+Z, and
// deleted again with Ctrl+Shift+Z, each told in a toast.

import { until } from "../lib/app.mjs";

export default {
  name: "marks-undo",
  async run(app, { fixtures, log, checkpoint }) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.playFromSearch("landed");
    await app.pause();
    const marks = () => app.invoke("list_marks", { vodId: vod.id });
    const to = (s) => until(`the player at ${s} s`, async () => (await app.shownTime()) === s, 10_000);

    // A mark, then its title written in the popover that opens for it (Enter saves).
    const mark = async (key, popover, text) => {
      await app.key(key);
      const input = `[role=dialog][aria-label="${popover}"] input`;
      await until(`the ${popover} box`, () => app.js(`return !!document.querySelector(arguments[0])`, input), 10_000);
      await app.type(input, text);
      if (key === "n") await checkpoint("chapter-title-box");
      await app.js(`document.querySelector(arguments[0]).closest("form").requestSubmit()`, input);
      await until(
        `the ${popover} box to close`,
        () => app.js(`return !document.querySelector(arguments[0])`, input),
        10_000,
      );
    };
    await app.key("ArrowLeft");
    await to(0);
    await app.key("ArrowRight", { shift: true });
    await to(1);
    await mark("h", "Highlight note", "Tranquility Base");
    await app.key("Escape");
    await app.key("ArrowRight", { shift: true });
    await app.key("ArrowRight", { shift: true });
    await to(3);
    await mark("n", "Chapter title", "The landing");
    const made = await until(
      "both marks, with their text",
      async () => {
        const all = await marks();
        const h = all.find((m) => m.kind === "highlight");
        const c = all.find((m) => m.kind === "chapter");
        if (h?.text === "Tranquility Base" && c?.text === "The landing") return { h, c };
        throw new Error(`marks: ${JSON.stringify(all.map((m) => [m.kind, m.start, m.text]))}`);
      },
      10_000,
    );
    if (Math.abs(made.h.start - 1) > 0.05 || Math.abs(made.c.start - 3) > 0.05)
      throw new Error(`the highlight is at ${made.h.start} and the chapter at ${made.c.start}, not 1 and 3`);
    await until(
      "the chapter on the timeline",
      () => app.js(`return document.querySelector(".chapter .label")?.textContent.trim() === "The landing"`),
      10_000,
    );
    log("a highlight at 1 s and a chapter at 3 s, with their text");
    await checkpoint("timeline-marks");

    // The chapter is the selected mark (just made): Delete removes it.
    await app.key("Delete");
    await until("the chapter gone", async () => !(await marks()).some((m) => m.kind === "chapter"), 10_000);
    const toast = () => app.js(`return document.querySelector(".toast .message")?.textContent.trim() ?? ""`);
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
    await until(
      "the chapter back",
      async () => {
        if ((await marks()).some((m) => m.kind === "chapter")) return true;
        throw new Error(`the toast says "${await toast()}"`);
      },
      10_000,
    );
    const undone = await until("the toast", async () => (await toast()).startsWith("Undone") && toast(), 10_000);
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, shiftKey: true, bubbles: true }))`,
    );
    await until("the chapter gone again", async () => !(await marks()).some((m) => m.kind === "chapter"), 10_000);
    const redone = await until("the toast", async () => (await toast()).startsWith("Redone") && toast(), 10_000);
    log(`${undone}; ${redone}`);
    if ((await marks()).find((m) => m.kind === "highlight")?.text !== "Tranquility Base")
      throw new Error("undo and redo of the chapter changed the highlight");
  },
};
