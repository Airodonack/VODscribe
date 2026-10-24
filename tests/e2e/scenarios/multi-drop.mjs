// Several things picked (selection.svelte.ts), put in an edit together
// (dragout.ts `carrySet`). On a transcribed file with cuts at 1, 2 and 3 s
// and a clip of each of 1-2 and 2-3 s, an edit holding 0-1 s: the two pills
// Ctrl-clicked (⌘ on a Mac), 2-3 first, and one dragged onto the edit's row
// in the library: both go at the end of its timeline, in the order picked.
// The edit opened: two of its pieces picked on its timeline, one pulled off
// it and let go on its clip bar: a clip of each, in that order. Those two
// pills picked the other way round and dragged onto the timeline's start:
// they go in there, before the rest, as one step one undo takes back. Then
// (Linux) the two pills dragged out of the window with the system's mouse:
// one drag, both files. In the library: two more files picked, Add to edit…
// above the list (after New edit let go of before saying where, which makes
// no edit) puts each whole on the edit's clip bar; pointing at
// Transcribe on one lights up both, and its click asks for both.

import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

/** A click's modifier that picks one: ⌘ on a Mac, Ctrl elsewhere (in the page). */
const TOGGLE = `const mac = /Mac/.test(navigator.platform);
  const toggle = { ctrlKey: !mac, metaKey: mac };`;

/** A pointer event's fields at (x, y) with `buttons` down, in the page. */
const AT = `const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });`;

export default {
  name: "multi-drop",
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

    // Ctrl-click on the timeline at a time (its own, or the edit's): the stretch there picked.
    const pickAt = (time) =>
      app.js(
        `${TOGGLE} ${AT}
         const bar = document.querySelector(".scrubber");
         const box = bar.getBoundingClientRect();
         const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
         const y = box.top + box.height / 2;
         bar.dispatchEvent(new PointerEvent("pointerdown", { ...at(x, y, 1), ...toggle }));
         window.dispatchEvent(new PointerEvent("pointerup", { ...at(x, y, 0), ...toggle }));`,
        time,
      );
    // Ctrl-click on a pill.
    const pickClip = (id) =>
      app.js(
        `${TOGGLE}
         document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
           .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
        id,
      );
    // A drag in the page's events: pressed on `from`, moved a little (the
    // carry begins), or `up` px up and off a timeline.
    const press = (from, up = 0) =>
      app.js(
        `${AT}
         const el = document.querySelector(arguments[0]);
         if (!el) throw new Error("nothing at " + arguments[0]);
         const r = el.getBoundingClientRect();
         const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
         el.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
         window.dispatchEvent(new PointerEvent("pointermove", at(x + 10, y + 10 - arguments[1], 1)));
         window.__drag = [x + 10, y + 10 - arguments[1]];`,
        from,
        up,
      );
    // ...then over `onto` (at a share of its width), let go there.
    const moveOnto = (onto, share = 0.5) =>
      app.js(
        `${AT}
         const el = document.querySelector(arguments[0]);
         if (!el) throw new Error("nothing at " + arguments[0]);
         const r = el.getBoundingClientRect();
         const [x0, y0] = window.__drag;
         const [x1, y1] = [r.left + r.width * arguments[1], r.top + r.height / 2];
         for (let i = 1; i <= 5; i++) window.dispatchEvent(new PointerEvent("pointermove", at(x0 + ((x1 - x0) * i) / 5, y0 + ((y1 - y0) * i) / 5, 1)));
         window.__drag = [x1, y1];`,
        onto,
        share,
      );
    const letGo = () =>
      app.js(
        `${AT}
         const [x, y] = window.__drag;
         window.dispatchEvent(new PointerEvent("pointerup", at(x, y, 0)));`,
      );

    // A clip of 1-2 and one of 2-3.
    await pickAt(1.5);
    await pickAt(2.5);
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
    const clipAt = (start) => made.find((c) => Math.abs(c.start - start) < 0.05);
    const [one, two] = [clipAt(1), clipAt(2)];

    // An edit holding 0-1 s, in the library's list.
    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 0, end: 1, title: "Start" });
    const editRow = `.list .vod[data-key="${-edit.id}"]`;
    await until("the edit's row", () => app.js(`return !!document.querySelector(arguments[0])`, editRow), 10_000);
    const starts = async () => (await app.invoke("get_edit", { id: edit.id })).pieces.map((p) => Math.round(p.start));

    // The pills picked, 2-3 first, and one dragged onto the edit's row.
    await until("two pills", () => app.js(`return document.querySelectorAll(".chip.ready").length === 2`), 10_000);
    await pickClip(two.id);
    await pickClip(one.id);
    await until("both picked", () => app.js(`return document.querySelectorAll(".chip.chosen").length === 2`), 10_000);
    await press(`.chip[data-clip="${one.id}"]`);
    await until(
      "the edit's row, to drop on",
      () => app.js(`return !!document.querySelector(arguments[0])`, `[data-edit-drop="${edit.id}"]`),
      10_000,
    );
    await moveOnto(`[data-edit-drop="${edit.id}"]`);
    await until(
      "the row lit as where they'd go",
      () => app.js(`return !!document.querySelector(arguments[0])`, `[data-edit-drop="${edit.id}"].target`),
      10_000,
    );
    await ctx.checkpoint("clips-over-an-edit");
    await letGo();
    await until(
      "both at the end of the edit, in the order picked",
      async () => (await starts()).join() === "0,2,1",
      15_000,
    );
    ctx.log("two clips dropped on an edit's row: at the end of its timeline, 2-3 then 1-2");
    await app.key("Escape");

    // The edit opened: on its timeline 0-1 (Start), 1-2 (2-3 s of the VOD), 2-3 (1-2 s).
    await app.js(`document.querySelector(arguments[0] + " .main").click()`, editRow);
    await until(
      "the edit's timeline",
      () => app.js(`return document.querySelectorAll(".drop-in [data-piece]").length === 3`),
      30_000,
    );
    // Its pieces at 1-2 and 0-1 picked, in that order; the first pulled off
    // the timeline and let go on the clip bar.
    await pickAt(1.5);
    await pickAt(0.5);
    await until(
      "two stretches picked",
      () => app.js(`return document.querySelectorAll(".scrubber .chosen").length === 2`),
      10_000,
    );
    const band = await app.js(`
      const bar = document.querySelector(".drop-in .scrubber");
      const box = bar.getBoundingClientRect();
      const x = box.left + ((1.5 - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
      return [...document.querySelectorAll(".drop-in [data-piece]")].find((b) => {
        const r = b.getBoundingClientRect();
        return r.left <= x && x < r.right;
      })?.dataset.piece;`);
    await press(`.drop-in [data-piece="${band}"]`, 120);
    await until(
      "the edit's clip bar, to drop on",
      () => app.js(`return !!document.querySelector(".clip-bar")`),
      10_000,
    );
    await moveOnto(".clip-bar");
    await until("the clip bar lit", () => app.js(`return !!document.querySelector(".clip-bar.over")`), 10_000);
    await ctx.checkpoint("stretches-over-the-clip-bar");
    await letGo();
    const barClips = () => app.invoke("edit_clips", { id: edit.id });
    const onBar = await until(
      "a clip of each on the edit's clip bar, in the order picked",
      async () => {
        const list = await barClips();
        return list.length === 2 && list.map((c) => Math.round(c.start)).join() === "2,0" && list;
      },
      15_000,
    );
    ctx.log("two of an edit's pieces dropped on its clip bar: a clip of each, in the order picked");
    await app.key("Escape");

    // Those two pills picked the other way round, one dragged onto the
    // timeline's start: in there, before the rest, in that order.
    await until(
      "the two pills",
      () => app.js(`return document.querySelectorAll(".clip-bar .chip").length === 2`),
      15_000,
    );
    await pickClip(onBar[1].id);
    await pickClip(onBar[0].id);
    await press(`.chip[data-clip="${onBar[0].id}"]`);
    await moveOnto(".drop-in .scrubber", 0.02);
    await until(
      "the marker where they'd go",
      () => app.js(`return !!document.querySelector(".drop-in .lane .insert")`),
      10_000,
    );
    await ctx.checkpoint("clips-over-the-timeline");
    await letGo();
    await until("both at the start, in the order picked", async () => (await starts()).join() === "0,2,0,2,1", 15_000);
    const pieces = (await app.invoke("get_edit", { id: edit.id })).pieces;
    if (!pieces[0].group || !pieces[1].group) throw new Error(`the clips aren't groups: ${JSON.stringify(pieces)}`);
    ctx.log("two clips of the clip bar dropped at the timeline's start: in there first, in the order picked");
    await app.key("Escape");
    await app.key("z", { cmd: true });
    await until("one undo takes both out", async () => (await starts()).join() === "0,2,1", 10_000);

    // Two of the edit's pills dragged out of the window with the system's
    // mouse: one drag with both files (seen as they come back over it).
    const why = os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux only, so far" : null);
    if (why) ctx.log(`no drag out of the window: ${why}`);
    else await dragOutBoth();
    async function dragOutBoth() {
      const ready = await until(
        "the edit's clips made",
        async () => {
          const list = await barClips();
          return list.every((c) => c.status === "ready" && c.file_path) && list;
        },
        MINUTE,
      );
      await pickClip(ready[0].id);
      await pickClip(ready[1].id);
      await app.js(
        `window.__dragsEnded = 0;
         window.__entered = [];
         const internals = window.__TAURI_INTERNALS__;
         const ended = internals.transformCallback(() => window.__dragsEnded++);
         await internals.invoke("plugin:event|listen", { event: "drag-ended", target: { kind: "Any" }, handler: ended });
         const entered = internals.transformCallback((e) => window.__entered.push(e.payload.paths ?? []));
         await internals.invoke("plugin:event|listen", { event: "tauri://drag-enter", target: { kind: "Any" }, handler: entered });
         performance.clearMarks();
         // What the page saw, for when the drag doesn't come: its pointer events and the carry's marks.
         const t0 = performance.now();
         const trace = (window.__trace = []);
         for (const type of ["pointerdown", "pointermove", "pointerup"])
           window.addEventListener(type, (e) => trace.push(\`\${Math.round(performance.now() - t0)} \${type} \${e.clientX},\${e.clientY} \${e.target?.className ?? ""}\`), true);`,
      );
      const at = await os.rectOf(app, `.chip[data-clip="${ready[0].id}"] .length`);
      const page = await os.windowRect(app);
      const [x, y] = [at.x + Math.round(at.w / 2), at.y + Math.round(at.h / 2)];
      const steps = [
        ["down", x, y],
        ["wait", 100],
      ];
      for (let i = 1; i <= 10; i++) steps.push(["move", Math.round(x + (page.x - x) * (i / 10)), y], ["wait", 50]);
      await os.gesture(steps);
      await until(
        "the system's drag",
        () => app.js(`return performance.getEntriesByName("drag-start").length > 0`),
        30_000,
      ).catch(async (e) => {
        const seen = await app.js(
          `return { trace: window.__trace.slice(-30), marks: performance.getEntriesByType("mark").map((m) => m.name), ghost: !!document.querySelector(".carry-ghost") }`,
        );
        ctx.log(`the page saw: ${JSON.stringify(seen)}`);
        throw e;
      });
      await os.gesture([
        ["move", page.x + 40, y],
        ["wait", 300],
        ["move", page.x + 80, y],
        ["wait", 500],
      ]);
      const paths = await until(
        "the drag, back over the window",
        () => app.js(`return window.__entered.find((p) => p.length) ?? false`),
        10_000,
      ).catch(() => null);
      await os.gesture([["key", "Escape"], ["wait", 300], ["up"]]);
      await until("the system's drag, over", () => app.js("return window.__dragsEnded"), 10_000);
      if (!paths) throw new Error("the drag never came back over the window with its files");
      const want = ready.map((c) => c.file_path);
      if (paths.length !== 2 || !want.every((p) => paths.includes(p)))
        throw new Error(`the drag held ${JSON.stringify(paths)}, not ${JSON.stringify(want)}`);
      ctx.log(`two pills dragged out of the window: one drag with both files`);
      await sleep(1500);
      await app.key("Escape");
    }

    // The library: two more files, picked; Add to edit… puts each whole on the edit's clip bar.
    const dir = path.join(ctx.out, "multi-drop");
    mkdirSync(dir, { recursive: true });
    const files = ["first.mp3", "second.mp3"].map((name) => {
      const file = path.join(dir, name);
      copyFileSync(ctx.fixtures.landed, file);
      return file;
    });
    const added = await app.addFiles(files);
    const [a, b] = added.map((v) => v.id);
    await until(
      "their lengths known",
      async () => (await app.invoke("list_vods")).filter((v) => [a, b].includes(v.id) && v.duration > 0).length === 2,
      MINUTE,
    );
    const row = (id) => `.list .vod[data-key="${id}"]`;
    await until("both rows", () => app.js(`return !!document.querySelector(arguments[0])`, row(b)), MINUTE);
    const click = (id, mods) =>
      app.js(
        `${TOGGLE}
         const mods = arguments[1] === "shift" ? { shiftKey: true } : toggle;
         document.querySelector(arguments[0] + " .main").dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...mods }));`,
        row(id),
        mods,
      );
    await click(a, "toggle");
    await click(b, "toggle");
    await until(
      "the list says two VODs",
      () => app.js(`return document.querySelector("aside .picked .count")?.textContent.trim() === "2 VODs"`),
      10_000,
    );
    const inPopover = (text) =>
      app.js(
        `const el = [...document.querySelectorAll("[role=dialog] .pick button")].find((b) => b.textContent.trim() === arguments[0]);
         if (!el) throw new Error("no " + arguments[0] + " in the menu");
         el.click();`,
        text,
      );
    await app.js(
      `[...document.querySelectorAll("aside .picked button")].find((b) => b.textContent.trim() === "Add to edit…").click()`,
    );
    await until(
      "the edits to pick from",
      () => app.js(`return !!document.querySelector("[role=dialog] .pick")`),
      10_000,
    );
    // New edit, then let go before saying where: no edit is made.
    const editsBefore = (await app.invoke("list_edits")).length;
    await inPopover("New edit");
    await until(
      "where in a new edit",
      () =>
        app.js(`return document.querySelector("[role=dialog] .pick .head")?.textContent.trim() === "Into a new edit:"`),
      10_000,
    );
    await app.key("Escape");
    await until("the menu closed", () => app.js(`return !document.querySelector("[role=dialog] .pick")`), 10_000);
    await sleep(500);
    const editsAfter = (await app.invoke("list_edits")).length;
    if (editsAfter !== editsBefore) throw new Error(`New edit let go made an edit: ${editsBefore} then ${editsAfter}`);
    await app.js(
      `[...document.querySelectorAll("aside .picked button")].find((b) => b.textContent.trim() === "Add to edit…").click()`,
    );
    await until(
      "the edits to pick from again",
      () => app.js(`return !!document.querySelector("[role=dialog] .pick")`),
      10_000,
    );
    await inPopover(edit.title);
    await until(
      "where in it",
      () =>
        app.js(
          `return [...document.querySelectorAll("[role=dialog] .pick button")].some((b) => b.textContent.trim() === "Clip bar")`,
        ),
      10_000,
    );
    await ctx.checkpoint("add-to-edit-where");
    await inPopover("Clip bar");
    const titles = Object.fromEntries((await app.invoke("list_vods")).map((v) => [v.id, v.title]));
    await until(
      "each VOD whole on the clip bar, in the order picked",
      async () => {
        const list = await barClips();
        return (
          list.length === 4 &&
          list[2].title === titles[a] &&
          list[3].title === titles[b] &&
          list.slice(2).every((c) => c.start === 0 && Math.abs(c.end - vod.duration) < 0.1)
        );
      },
      15_000,
    );
    ctx.log("Add to edit… put both VODs picked on the edit's clip bar, whole");

    // New edit › Clip bar puts what was picked when it was chosen, though
    // the edit is made after (held up here a second and a half) and the
    // picking changes meanwhile.
    const editIds = new Set((await app.invoke("list_edits")).map((e) => e.id));
    await app.js(`
      const real = window.fetch;
      window.__e2eSlowEdit = real;
      window.fetch = (url, init) =>
        decodeURIComponent(String(url)).split("/").pop() === "create_edit"
          ? new Promise((r) => setTimeout(r, 1500)).then(() => real(url, init))
          : real(url, init);`);
    await app.js(
      `[...document.querySelectorAll("aside .picked button")].find((b) => b.textContent.trim() === "Add to edit…").click()`,
    );
    await until(
      "the edits to pick from",
      () => app.js(`return !!document.querySelector("[role=dialog] .pick")`),
      10_000,
    );
    await inPopover("New edit");
    await until(
      "where in a new edit",
      () =>
        app.js(
          `return [...document.querySelectorAll("[role=dialog] .pick button")].some((b) => b.textContent.trim() === "Clip bar")`,
        ),
      10_000,
    );
    await inPopover("Clip bar");
    await click(b, "toggle");
    await until(
      "the list says one VOD",
      () => app.js(`return document.querySelector("aside .picked .count")?.textContent.trim() !== "2 VODs"`),
      10_000,
    );
    const newEdit = await until(
      "the new edit",
      async () => (await app.invoke("list_edits")).find((e) => !editIds.has(e.id)),
      15_000,
    );
    await app.js(`window.fetch = window.__e2eSlowEdit;`);
    await sleep(1000);
    const onNew = (await app.invoke("edit_clips", { id: newEdit.id })).map((c) => c.title);
    if (onNew.join() !== [titles[a], titles[b]].join())
      throw new Error(`New edit › Clip bar put what was picked once the edit was made: ${JSON.stringify(onNew)}`);
    ctx.log("New edit › Clip bar put both VODs picked when it was chosen, the picking changed while the edit was made");
    await click(b, "toggle");
    await until(
      "both picked again",
      () => app.js(`return document.querySelector("aside .picked .count")?.textContent.trim() === "2 VODs"`),
      10_000,
    );

    // Transcribe on one of them: pointed at, both light up; clicked, both are asked for.
    const transcribe = `${row(a)} .actions button[aria-label^="Transcribe all"]`;
    await until(
      "Transcribe for both",
      () => app.js(`return !!document.querySelector(arguments[0])`, transcribe),
      10_000,
    );
    await app.js(`document.querySelector(arguments[0]).dispatchEvent(new MouseEvent("mouseenter"))`, transcribe);
    await until(
      "both rows lit",
      () => app.js(`return document.querySelectorAll(".list .vod.lit").length === 2`),
      10_000,
    );
    await ctx.checkpoint("transcribe-all-lit");
    await app.js(`document.querySelector(arguments[0]).click()`, transcribe);
    const statuses = () =>
      app.invoke("list_vods").then((all) => all.filter((v) => [a, b].includes(v.id)).map((v) => v.status));
    await until("both asked for", async () => (await statuses()).every((s) => s !== "none"), 10_000);
    ctx.log(`Transcribe on one of two picked asked for both: ${(await statuses()).join(", ")}`);
    await until("both transcribed", async () => (await statuses()).every((s) => s === "done"), 10 * MINUTE);
  },
};
