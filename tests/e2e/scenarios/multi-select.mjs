// Several things picked at once (selection.svelte.ts). On a transcribed
// file: cuts at 1, 2 and 3 s (B), then Ctrl-click (⌘ on a Mac) on the
// stretch 2-3 s and then on 1-2 s: each shows its number in that order, the
// playhead stays where it was, and C makes a clip of each, in that order, as
// one change. The two pills Ctrl-clicked: Delete takes both off the clip
// bar, and one undo brings both back. A box dragged from empty space in the
// clip bar over both pills picks them; Escape lets them go. In the library:
// two more files, Ctrl-click on one and Shift-click on the other picks the
// run, Remove… asks once, naming them, and both go (the first VOD stays);
// Escape lets go of a row picked.

import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";

/** A click's modifier that picks one: ⌘ on a Mac, Ctrl elsewhere (in the page). */
const TOGGLE = `const mac = /Mac/.test(navigator.platform);
  const toggle = { ctrlKey: !mac, metaKey: mac };`;

export default {
  name: "multi-select",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "stretches-numbered":
      'On the timeline two neighbouring stretches, about 0:01 to 0:02 and 0:02 to 0:03, are outlined and shaded, each with a small round number: 2 on the left one, 1 on the right one; under the bar a row reads "2 stretches" with Clip and Add to edit….',
  },
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    await app.pause();
    await app.toStart();

    // Cuts at 1, 2 and 3 s, as a person would put them: Shift+→ a second on, B, Escape.
    const to = (s) => until(`the player at ${s} s`, async () => (await app.shownTime()) === s, 10_000);
    for (const s of [1, 2, 3]) {
      await app.key("ArrowRight", { shift: true });
      await to(s);
      await app.key("b");
      await until("the cut, selected", () => app.js(`return !!document.querySelector(".cut.selected")`), 10_000);
      await app.key("Escape");
      await until("the cut let go", () => app.js(`return !document.querySelector(".cut.selected")`), 10_000);
    }
    await until("three cuts", async () => (await app.invoke("list_marks", { vodId: vod.id })).length === 3, 10_000);

    // Ctrl-click on the timeline at a time: the stretch there picked.
    const pickAt = (time, { shift = false } = {}) =>
      app.js(
        `${TOGGLE}
         const bar = document.querySelector(".scrubber");
         const box = bar.getBoundingClientRect();
         const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
         const at = { bubbles: true, cancelable: true, clientX: x, clientY: box.top + box.height / 2, button: 0, buttons: 1, isPrimary: true, pointerId: 1 };
         const mods = arguments[1] ? { shiftKey: true } : toggle;
         bar.dispatchEvent(new PointerEvent("pointerdown", { ...at, ...mods }));
         window.dispatchEvent(new PointerEvent("pointerup", { ...at, ...mods, buttons: 0 }));`,
        time,
        shift,
      );
    await pickAt(2.5);
    await pickAt(1.5);
    const numbers = await until(
      "two stretches numbered",
      () =>
        app.js(`
          const picked = [...document.querySelectorAll(".scrubber .chosen")];
          return picked.length === 2 && picked.map((p) => [p.dataset.segment, p.querySelector(".order")?.textContent]);`),
      10_000,
    );
    const order = Object.fromEntries(numbers);
    if (order["2 3"] !== "1" || order["1 2"] !== "2")
      throw new Error(`the stretches' numbers: ${JSON.stringify(numbers)}`);
    if ((await app.shownTime()) !== 3) throw new Error(`picking moved the playhead to ${await app.shownTime()} s`);
    const head = () => app.js(`return document.querySelector(".clip-bar .picked .count")?.textContent.trim() ?? ""`);
    if ((await head()) !== "2 stretches") throw new Error(`the clip bar says "${await head()}"`);
    await ctx.checkpoint("stretches-numbered");

    // C: a clip of each, in the order picked.
    await app.key("c");
    const made = await until(
      "two clips, made",
      async () => {
        const list = (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => c.shown);
        if (list.some((c) => c.status === "error")) throw new Error(`a clip failed: ${JSON.stringify(list)}`);
        return list.length === 2 && list.every((c) => c.status === "ready") && list;
      },
      MINUTE,
    );
    const byId = [...made].sort((a, b) => a.id - b.id).map((c) => `${Math.round(c.start)}-${Math.round(c.end)}`);
    if (byId.join() !== "2-3,1-2") throw new Error(`the clips were made in the order ${byId.join(", ")}`);
    if (await app.js(`return !!document.querySelector(".scrubber .chosen")`))
      throw new Error("the stretches are still picked once clipped");
    ctx.log(`C made ${byId.join(", ")} in the order picked`);

    // The two pills Ctrl-clicked: the bar says so, with Remove.
    await until("two pills", () => app.js(`return document.querySelectorAll(".chip.ready").length === 2`), 10_000);
    for (const clip of made)
      await app.js(
        `${TOGGLE}
         document.querySelector('.chip[data-clip="' + arguments[0] + '"] .play')
           .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
        clip.id,
      );
    await until(
      "two pills picked",
      () => app.js(`return document.querySelectorAll(".chip.chosen").length === 2`),
      10_000,
    );
    if ((await head()) !== "2 clips") throw new Error(`the clip bar says "${await head()}"`);
    if (
      !(await app.js(
        `return [...document.querySelectorAll(".clip-bar .picked button")].some((b) => b.textContent.trim() === "Remove")`,
      ))
    )
      throw new Error("no Remove for the clips picked");
    await ctx.checkpoint("clips-picked");

    // Delete: both go; one undo, both are back.
    const shown = async () => (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => c.shown);
    await app.key("Delete");
    await until("both clips gone", async () => (await shown()).length === 0, 10_000);
    await app.key("z", { cmd: true });
    await until("both clips back, at one undo", async () => (await shown()).length === 2, 10_000);
    const toast = await app.js(`return document.querySelector(".toast .message")?.textContent ?? ""`);
    ctx.log(`one undo: "${toast}"`);
    if (!/Clips deleted/.test(toast)) throw new Error(`the undo says "${toast}"`);

    // A box from empty space in the clip bar, right of the pills, over both.
    await until("two pills again", () => app.js(`return document.querySelectorAll(".chip").length === 2`), MINUTE);
    await app.js(`
      const strip = document.querySelector(".clip-bar");
      const box = strip.getBoundingClientRect();
      const pills = [...strip.querySelectorAll(".chip")].map((c) => c.getBoundingClientRect());
      const from = { x: Math.min(box.right - 4, Math.max(...pills.map((p) => p.right)) + 30), y: box.top + 3 };
      const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });
      strip.dispatchEvent(new PointerEvent("pointerdown", at(from.x, from.y, 1)));
      window.dispatchEvent(new PointerEvent("pointermove", at(from.x - 20, from.y + 10, 1)));
      window.dispatchEvent(new PointerEvent("pointermove", at(Math.min(...pills.map((p) => p.left)) + 4, box.bottom - 3, 1)));`);
    await until(
      "the box drawn, over both pills",
      () =>
        app.js(
          `return !!document.querySelector(".select-box") && document.querySelectorAll(".chip.chosen").length === 2`,
        ),
      10_000,
    );
    await ctx.checkpoint("clip-bar-box");
    await app.js(
      `window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, buttons: 0, clientX: 0, clientY: 0 }))`,
    );
    await until(
      "the box gone, both still picked",
      () =>
        app.js(
          `return !document.querySelector(".select-box") && document.querySelectorAll(".chip.chosen").length === 2`,
        ),
      10_000,
    );
    await app.key("Escape");
    await until("Escape let them go", () => app.js(`return !document.querySelector(".chip.chosen")`), 10_000);

    // The library: two more files, the run of them picked, removed after one question.
    const dir = path.join(ctx.out, "multi-select");
    mkdirSync(dir, { recursive: true });
    const files = ["first.mp3", "second.mp3"].map((name) => {
      const file = path.join(dir, name);
      copyFileSync(ctx.fixtures.landed, file);
      return file;
    });
    const added = await app.addFiles(files);
    const [a, b] = added.map((v) => v.id);
    const row = (id) => `.list .vod[data-key="${id}"] .main`;
    await until(
      "both rows",
      () =>
        app.js(
          `return !!document.querySelector(arguments[0]) && !!document.querySelector(arguments[1])`,
          row(a),
          row(b),
        ),
      MINUTE,
    );
    const click = (id, mods) =>
      app.js(
        `${TOGGLE}
         const mods = arguments[1] === "shift" ? { shiftKey: true } : toggle;
         document.querySelector(arguments[0]).dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...mods }));`,
        row(id),
        mods,
      );
    await click(a, "toggle");
    await click(b, "shift");
    const listHead = () => app.js(`return document.querySelector("aside .picked .count")?.textContent.trim() ?? ""`);
    await until("the list says two VODs", async () => (await listHead()) === "2 VODs", 10_000);
    const picked = await app.js(
      `return [...document.querySelectorAll(".list .vod.chosen")].map((r) => Number(r.dataset.key))`,
    );
    if (picked.sort().join() !== [a, b].sort().join()) throw new Error(`picked rows ${picked}, not ${a} and ${b}`);
    await ctx.checkpoint("library-picked");

    const before = await app.asked();
    await app.answer("Remove");
    await app.clickText("Remove…");
    await until(
      "both removed",
      async () => {
        const ids = (await app.invoke("list_vods")).map((v) => v.id);
        return !ids.includes(a) && !ids.includes(b) && ids.includes(vod.id);
      },
      MINUTE,
    );
    const asked = await app.js(`return window.__e2e.asked.slice(arguments[0])`, before);
    if (asked.length !== 1 || !String(asked[0]).includes("Remove 2 VODs?"))
      throw new Error(`Remove… asked ${JSON.stringify(asked)}`);

    // Escape lets go of a row picked.
    await click(vod.id, "toggle");
    await until("the row picked", () => app.js(`return !!document.querySelector(".list .vod.chosen")`), 10_000);
    await app.key("Escape");
    await until(
      "Escape let it go",
      () => app.js(`return !document.querySelector(".list .vod.chosen, aside .picked")`),
      10_000,
    );
  },
};
