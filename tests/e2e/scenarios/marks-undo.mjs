// Marks and undo: a highlight (M, as in video editors; H does nothing) and a chapter (N), each written a title in
// its popover; the chapter deleted (Delete), brought back with Ctrl+Z, and
// deleted again with Ctrl+Shift+Z, each told in a toast. Then a chapter made
// by mistake (N) and cancelled with Escape in its box: gone, and not in undo's
// history (Ctrl+Z undoes the change before it). Last, a cut undone at once:
// the arrow keys seek again.

import { until } from "../lib/app.mjs";

export default {
  name: "marks-undo",
  async run(app, { fixtures, log, checkpoint }) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(fixtures.landed);
    // Played from a search hit (undo there is what 8867f3d fixed), on the
    // first word: one in the file's last second stalls WebKitGTK (TODO.md).
    await app.playFromSearch("Swift");
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      60_000,
    );
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
      await app.submit(input);
      await until(
        `the ${popover} box to close`,
        () => app.js(`return !document.querySelector(arguments[0])`, input),
        10_000,
      );
    };
    await app.toStart();
    await to(0);
    await app.key("ArrowRight", { shift: true });
    await to(1);
    // H does nothing (M took its place), and M marks rather than mutes.
    await app.key("h");
    await new Promise((r) => setTimeout(r, 500));
    if (await app.js(`return !!document.querySelector("[role=dialog]")`)) throw new Error("H opened a box");
    if ((await marks()).length) throw new Error("H made a mark");
    await mark("m", "Highlight note", "Tranquility Base");
    if (await app.js(`return !!document.querySelector('.bar button[aria-label="Unmute"]')`))
      throw new Error("M muted the player");
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

    await app.key("ArrowRight", { shift: true });
    await to(4);
    await app.key("n");
    const box = `[role=dialog][aria-label="Chapter title"] input`;
    await until("the chapter box", () => app.js(`return !!document.querySelector(arguments[0])`, box), 10_000);
    await app.js(
      `document.querySelector(arguments[0]).dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }))`,
      box,
    );
    await until("the box to close", () => app.js(`return !document.querySelector(arguments[0])`, box), 10_000);
    await until(
      "the cancelled chapter gone",
      async () => {
        const left = (await marks()).filter((m) => m.kind === "chapter");
        if (!left.length) return true;
        throw new Error(`chapters: ${JSON.stringify(left.map((m) => [m.start, m.text]))}`);
      },
      10_000,
    );
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
    const back = await until(
      "the removed chapter back",
      async () => {
        const left = (await marks()).filter((m) => m.kind === "chapter");
        if (left.length === 1 && left[0].text === "The landing") return left[0];
        throw new Error(`chapters: ${JSON.stringify(left.map((m) => [m.start, m.text]))}; "${await toast()}"`);
      },
      10_000,
    );
    if (Math.abs(back.start - 3) > 0.05) throw new Error(`the chapter came back at ${back.start}, not 3`);
    log("a chapter cancelled with Escape: gone, and Ctrl+Z undid the change before it");

    // Escape pressed as the box opens, before its input has the focus: the
    // new chapter goes all the same.
    await app.key("n");
    await until("the chapter box again", () => app.js(`return !!document.querySelector(arguments[0])`, box), 10_000);
    await app.key("Escape");
    await until("the box to close", () => app.js(`return !document.querySelector(arguments[0])`, box), 10_000);
    await until(
      "only the chapter brought back",
      async () => {
        const left = (await marks()).filter((m) => m.kind === "chapter");
        if (left.length === 1) return true;
        throw new Error(`chapters: ${JSON.stringify(left.map((m) => [m.start, m.text]))}`);
      },
      10_000,
    );

    // A cut (B, selected as it's made) undone: the arrows seek again, rather
    // than moving a cut that isn't there.
    const at = await app.shownTime();
    await app.key("b");
    await until("the cut", async () => (await marks()).some((m) => m.kind === "cut"), 10_000);
    await app.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`,
    );
    await until("the cut undone", async () => !(await marks()).some((m) => m.kind === "cut"), 10_000);
    await app.key("ArrowRight", { shift: true });
    await until(
      `the player at ${at + 1} s (the arrow seeking)`,
      async () => (await app.shownTime()) === at + 1,
      10_000,
    );
    log("a cut undone: the arrows seek again");
  },
};
