// Several things picked at once, as in a file manager (selection.svelte.ts,
// pick.ts). On a transcribed file: cuts at 1, 2 and 3 s (B), then
// Ctrl-click (⌘ on a Mac) on the stretch 2-3 s and then on 1-2 s: each shows
// its number in that order, the playhead stays where it was, and C makes a
// clip of each, in that order, as one change. An Alt-click (Option) on the
// timeline picks the stretch there alone, Ctrl+Alt-click adds one, and the
// playhead stays. The clip bar has no row of its own about what's picked:
// the keys and the drags are the same for several as for one. A plain click
// on a pill picks it alone, one on another picks that instead, and one on
// the pill picked alone lets it go. A pill's play button plays the VOD from
// the clip's start and picks nothing. One pill clicked and the other
// Ctrl-clicked picks both: Delete takes both off the clip bar, and one undo
// brings both back. A box dragged from empty space in the clip bar over both
// pills picks them; Escape lets them go.
//
// In the library, four more files. A plain click opens a row and picks it
// alone (nothing shows above the list for the open row alone); Shift-click
// then picks the run from it; Ctrl-click takes one out, and Ctrl+Shift-click
// adds the run from there to what's picked. A plain click on one of those
// picks it alone, and a Ctrl-click on another then picks both, the open one
// first; Escape lets go of all but the open one, so a Ctrl-click adds to it
// again. ↓ (or ↑) on a row opens the next one alone, and Shift with the other
// arrow picks the run back. The open one removed with its own Remove leaves
// the set (the other two numbered 1 and 2, the list saying 2 VODs);
// Remove… asks once, naming them, and both go (the first VOD and the fourth
// file stay). With only the open row picked, Delete isn't the library's;
// Escape lets go of a row picked beside it.
//
// One kind picked at a time: two pills picked, then a cut clicked, the pills
// are let go (the bar says nothing's picked) and Delete takes the cut only;
// the same with a library row picked. Ctrl+A (⌘A) picks by the side of the
// window the pointer is on, whatever has the focus: over the clip bar or the
// timeline, all the clips; over the library's pane, all its rows; over the
// main side with no clip bar, nothing, and not the page's text either. Last,
// the two pills picked, one of them dragged onto "Make new edit" in the
// library: both go in a new edit, in the order picked.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

/** A click's modifier that picks one: ⌘ on a Mac, Ctrl elsewhere (in the page). */
const TOGGLE = `const mac = /Mac/.test(navigator.platform);
  const toggle = { ctrlKey: !mac, metaKey: mac };`;

export default {
  name: "multi-select",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "stretches-numbered":
      "On the timeline two neighbouring stretches, about 0:01 to 0:02 and 0:02 to 0:03, are outlined and shaded, each with a small round number: 2 on the left one, 1 on the right one. The clip bar under the timeline has no row about them (no count, no Clip or Add to edit… buttons there).",
  },
  async run(app, ctx) {
    await app.toLibrary();

    // Where the pointer is, for Ctrl+A: the OS's own pointer where it can be
    // moved (X11), else a pointermove sent to the page. The key after it is
    // the page's own event, sent by a script, not the OS's: so the page must
    // have seen the pointer get there (a pointermove over the element, what
    // the app goes by) before it's pressed. If it never does, the failure
    // says where X has the pointer, not which side Ctrl+A then picked.
    const pointAt = async (selector) => {
      if (process.platform === "linux" && os.unavailable() === null) {
        const r = await os.rectOf(app, selector);
        await app.js(
          `window.__pointerSeen = null;
           if (!window.__pointerWatched) {
             window.__pointerWatched = true;
             addEventListener("pointermove", (e) => {
               const want = window.__pointerWant;
               window.__pointerSeen = { x: e.clientX, y: e.clientY, over: !!e.target.closest?.(want) };
             }, true);
           }
           window.__pointerWant = arguments[0];`,
          selector,
        );
        await os.gesture([["move", Math.round(r.x + r.w / 2), Math.round(r.y + r.h / 2)]]);
        await until(
          `the page to see the pointer over ${selector}`,
          () => app.js(`return !!window.__pointerSeen?.over`),
          10_000,
          { every: 50 },
        ).catch(async (e) => {
          const x = execFileSync("xdotool", ["getmouselocation"], { encoding: "utf8" }).trim();
          const seen = await app.js(`return window.__pointerSeen`);
          throw new Error(
            `${e.message}: X has the pointer at ${x}; the page's last pointermove: ${JSON.stringify(seen)}`,
          );
        });
      } else
        await app.js(
          `const el = document.querySelector(arguments[0]);
           const b = el.getBoundingClientRect();
           el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: b.left + b.width / 2, clientY: b.top + b.height / 2, isPrimary: true, pointerId: 1 }));`,
          selector,
        );
    };

    // Ctrl+A over the main side with no clip bar (the library's page): nothing
    // to pick, and the page's text isn't selected either. The OS's own keys
    // where they can be pressed (a synthetic press has no default to run);
    // anywhere, the press is the app's (its default prevented).
    await pointAt("main.main");
    await app.js(`document.activeElement?.blur()`);
    await app.focused();
    if (process.platform === "linux" && os.unavailable() === null) {
      await os.gesture([
        ["key", "ctrl+a"],
        ["wait", 300],
      ]);
      const selected = await app.js(`return window.getSelection()?.toString() ?? ""`);
      if (selected) throw new Error(`Ctrl+A with nothing to pick selected the page's text: "${selected.slice(0, 80)}"`);
    }
    const pressed = await app.js(
      `const mac = /Mac/.test(navigator.platform);
       const e = new KeyboardEvent("keydown", { key: "a", ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true });
       document.body.dispatchEvent(e);
       return e.defaultPrevented;`,
    );
    if (!pressed) throw new Error("Ctrl+A with nothing to pick was left to the page (its select-all)");

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
    // The clip bar says nothing of what's picked: no row there with a count
    // or buttons of its own (C, Clip under the player, Delete and the
    // pills' drags do it).
    const barRow = () =>
      app.js(`
        const bar = document.querySelector(".clip-bar");
        const own = [...bar.children].filter((el) => !el.matches(".chip, .drop, .error-text, .added"));
        return own.map((el) => el.textContent.trim()).join(" | ");`);
    const noRow = async (when) => {
      const text = await barRow();
      if (text) throw new Error(`the clip bar has a row "${text}" ${when}`);
    };
    await noRow("with two stretches picked");
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

    // An Alt-click (Option on a Mac) on the timeline, not dragged, picks the
    // stretch there alone; with Ctrl (⌘) too, adds one; the playhead stays.
    const altAt = (time, adding = false) =>
      app.js(
        `${TOGGLE}
         const bar = document.querySelector(".scrubber");
         const box = bar.getBoundingClientRect();
         const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
         const at = { bubbles: true, cancelable: true, clientX: x, clientY: box.top + box.height / 2, button: 0, buttons: 1, isPrimary: true, pointerId: 1, altKey: true };
         const mods = arguments[1] ? toggle : {};
         bar.dispatchEvent(new PointerEvent("pointerdown", { ...at, ...mods }));
         window.dispatchEvent(new PointerEvent("pointerup", { ...at, ...mods, buttons: 0 }));`,
        time,
        adding,
      );
    const stretchesPicked = () =>
      app.js(`return [...document.querySelectorAll(".scrubber .chosen")].map((p) => p.dataset.segment).sort().join()`);
    const stretchesAre = (want, what) =>
      until(what, async () => (await stretchesPicked()) === want, 10_000).catch(async (e) => {
        throw new Error(
          `${e.message}: picked "${await stretchesPicked()}", the ghost ${await app.js(`return !!document.querySelector(".carry-ghost")`)}`,
        );
      });
    const playheadBefore = await app.shownTime();
    await altAt(1.5);
    await stretchesAre("1 2", "an Alt-click picked the stretch 1-2 s alone");
    await altAt(2.5, true);
    await stretchesAre("1 2,2 3", "a Ctrl+Alt-click added 2-3 s");
    await altAt(2.5);
    await stretchesAre("2 3", "an Alt-click picked 2-3 s alone");
    if ((await app.shownTime()) !== playheadBefore)
      throw new Error(`an Alt-click moved the playhead from ${playheadBefore} s to ${await app.shownTime()} s`);
    await app.key("Escape");
    await stretchesAre("", "Escape let it go");
    ctx.log("an Alt-click picks a stretch alone, Ctrl+Alt-click adds one, the playhead staying");

    // A plain click on a pill picks it alone, as in a file manager; one on
    // another picks that one instead; one on the pill picked alone lets it go.
    await until("two pills", () => app.js(`return document.querySelectorAll(".chip.ready").length === 2`), 10_000);
    const plainClick = (id) =>
      app.js(
        `document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
           .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }));`,
        id,
      );
    const chosen = () =>
      app.js(`return [...document.querySelectorAll(".chip.chosen")].map((c) => Number(c.dataset.clip))`);
    const pickedAlone = (id, what) =>
      until(what, async () => (await chosen()).join() === String(id), 10_000).catch(async (e) => {
        throw new Error(`${e.message}: picked ${JSON.stringify(await chosen())}`);
      });
    await plainClick(made[0].id);
    await pickedAlone(made[0].id, "a plain click picked the pill alone");
    await noRow("with one pill picked");
    await plainClick(made[1].id);
    await pickedAlone(made[1].id, "a plain click on another picked it instead");
    await plainClick(made[1].id);
    await until("a plain click on the pill picked alone let it go", async () => !(await chosen()).length, 10_000);
    ctx.log("a plain click picks a pill alone, another instead, and lets go of the one picked alone");

    // The play button on the 1-2 s pill (the playhead at 3 s, after the
    // cuts): the VOD plays from 1 s, and the pill isn't picked.
    const first = made.find((c) => Math.round(c.start) === 1);
    if ((await app.shownTime()) < 3) throw new Error(`the playhead at ${await app.shownTime()} s before Play, not 3 s`);
    await app.click(`.chip[data-clip="${first.id}"] button.play`);
    await until("playing", () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`), MINUTE);
    const at = await app.shownTime();
    if (at < 1 || at >= 3) throw new Error(`the play button played from ${at} s, not the clip's start (1 s)`);
    await app.pause();
    if ((await chosen()).length) throw new Error(`the play button picked ${JSON.stringify(await chosen())}`);
    ctx.log(`a pill's play button played from the clip's start (at ${at} s once playing), picking nothing`);

    // One pill clicked, the other Ctrl-clicked: both picked, in that order,
    // still no row.
    await plainClick(made[0].id);
    await pickedAlone(made[0].id, "a plain click picked the pill alone");
    await app.js(
      `${TOGGLE}
       document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
         .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
      made[1].id,
    );
    await until("two pills picked", async () => (await chosen()).length === 2, 10_000).catch(async (e) => {
      throw new Error(`${e.message} (one clicked, the other Ctrl-clicked): picked ${JSON.stringify(await chosen())}`);
    });
    await noRow("with two pills picked");
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

    // Two pills picked, then a cut clicked: the cut is selected and the
    // pills are let go, so Delete takes the cut and only it.
    const pickPills = async () => {
      for (const clip of made)
        await app.js(
          `${TOGGLE}
           document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
             .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
          clip.id,
        );
      await until("two pills picked", async () => (await chosen()).length === 2, 10_000);
    };
    const marks = async () => (await app.invoke("list_marks", { vodId: vod.id })).length;
    const clickCut = () =>
      app.js(`
        const flag = document.querySelector(".scrubber .cut[data-mark]");
        const box = flag.getBoundingClientRect();
        const at = { bubbles: true, cancelable: true, clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, isPrimary: true, pointerId: 1 };
        flag.dispatchEvent(new PointerEvent("pointerdown", { ...at, buttons: 1 }));
        window.dispatchEvent(new PointerEvent("pointerup", { ...at, buttons: 0 }));`);
    await pickPills();
    await clickCut();
    await until("the cut selected", () => app.js(`return !!document.querySelector(".cut.selected")`), 10_000);
    await until("the pills let go", () => app.js(`return !document.querySelector(".chip.chosen")`), 10_000).catch(
      async (e) => {
        throw new Error(`${e.message}: picked ${JSON.stringify(await chosen())}`);
      },
    );
    await app.key("Delete");
    await until("the cut gone", async () => (await marks()) === 2, 10_000);
    if ((await shown()).length !== 2)
      throw new Error(`Delete on the cut took clips too: ${(await shown()).length} left`);

    // The library: four more files; three of them, side by side in the list,
    // picked in every way a file manager has.
    const dir = path.join(ctx.out, "multi-select");
    mkdirSync(dir, { recursive: true });
    const files = ["first.mp3", "second.mp3", "third.mp3", "fourth.mp3"].map((name) => {
      const file = path.join(dir, name);
      copyFileSync(ctx.fixtures.landed, file);
      return file;
    });
    const added = await app.addFiles(files);
    const [a, b, c, d] = added.map((v) => v.id);
    const row = (id) => `.list .vod[data-key="${id}"] .main`;
    await until(
      "the four rows",
      () => app.js(`return arguments[0].every((r) => !!document.querySelector(r))`, [a, b, c, d].map(row)),
      MINUTE,
    );

    // Ctrl+A by where the pointer is, the focus elsewhere.
    // The first VOD open again (an add opened what it added), with its pills.
    await app.openVod(vod.id);
    await until("its two pills", () => app.js(`return document.querySelectorAll(".chip.ready").length === 2`), MINUTE);
    const focusOn = (selector) => app.js(`document.querySelector(arguments[0]).focus()`, selector);
    const listCount = () => app.js(`return document.querySelector("aside .picked .count")?.textContent.trim() ?? ""`);
    for (const over of [".clip-bar", ".scrubber"]) {
      await focusOn(row(a));
      await pointAt(over);
      await app.key("a", { cmd: true });
      await until(`Ctrl+A over ${over}: the clips`, async () => (await chosen()).length === 2, 10_000).catch(
        async (e) => {
          throw new Error(`${e.message}: picked ${JSON.stringify(await chosen())}, the list "${await listCount()}"`);
        },
      );
      if (await listCount()) throw new Error(`Ctrl+A over ${over} picked rows too: "${await listCount()}"`);
      await app.key("Escape");
      await until("Escape let them go", () => app.js(`return !document.querySelector(".chip.chosen")`), 10_000);
    }
    await focusOn(`.chip[data-clip="${made[0].id}"] .name`);
    await pointAt("aside .brand");
    await app.key("a", { cmd: true });
    await until("Ctrl+A over the library: its rows", async () => (await listCount()) === "5 VODs", 10_000).catch(
      async (e) => {
        throw new Error(`${e.message}: the list says "${await listCount()}", picked ${JSON.stringify(await chosen())}`);
      },
    );
    if ((await chosen()).length) throw new Error(`Ctrl+A over the library picked clips too: ${await chosen()}`);
    await app.key("Escape");
    await until("Escape let them go", () => app.js(`return !document.querySelector(".list .vod.chosen")`), 10_000);

    // A click on a row: "plain", "toggle" (Ctrl, ⌘ on a Mac), "shift", or "both" (Ctrl+Shift).
    const click = (id, mods) =>
      app.js(
        `${TOGGLE}
         const mods = { plain: {}, toggle, shift: { shiftKey: true }, both: { ...toggle, shiftKey: true } }[arguments[1]];
         document.querySelector(arguments[0]).dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...mods }));`,
        row(id),
        mods,
      );
    const listHead = () => app.js(`return document.querySelector("aside .picked .count")?.textContent.trim() ?? ""`);
    const openRow = () => app.js(`return Number(document.querySelector(".list .vod.selected")?.dataset.key ?? -1)`);
    // The rows picked with others (outlined), by key, with their numbers.
    const numbered = () =>
      app.js(
        `return Object.fromEntries([...document.querySelectorAll(".list .vod.chosen")].map((r) => [r.dataset.key, r.querySelector(".row-order")?.textContent ?? null]))`,
      );
    /** Waits for the list to say `want` ("" for nothing picked beside the open row), naming `what` was done. */
    const says = (want, what) =>
      until(`the list says "${want}"`, async () => (await listHead()) === want, 10_000).catch(async (e) => {
        throw new Error(
          `${e.message} (${what}): it says "${await listHead()}", outlined ${JSON.stringify(await numbered())}, open ${await openRow()}`,
        );
      });
    const pickedAre = async (want, what) => {
      const got = await numbered();
      const keys = Object.keys(want);
      if (Object.keys(got).length !== keys.length || keys.some((k) => got[k] !== want[k]))
        throw new Error(`${what}: outlined ${JSON.stringify(got)}, not ${JSON.stringify(want)}`);
    };

    // A plain click opens a row and picks it; a Shift-click then picks the run from it.
    await click(a, "plain");
    await until("the clicked row open", async () => (await openRow()) === a, 10_000);
    await says("", "one row clicked: the open one alone, nothing shown above the list");
    await click(c, "shift");
    await says("3 VODs", "one opened, another Shift-clicked");
    // Ctrl-click takes the middle one out; Ctrl+Shift-click adds the run from it, keeping the rest.
    await click(b, "toggle");
    await says("2 VODs", "the middle one Ctrl-clicked out");
    await click(c, "both");
    await says("3 VODs", "Ctrl+Shift-click from the one Ctrl-clicked: its run added");
    await pickedAre({ [a]: "1", [c]: "2", [b]: "3" }, "Ctrl+Shift-click added the run after what was picked");
    if ((await openRow()) !== a) throw new Error(`picking opened ${await openRow()}, not left ${a} open`);

    // A plain click on one picked: it alone, opened; the others let go.
    await click(b, "plain");
    await says("", "a plain click on one of three picked");
    await until("the clicked row open", async () => (await openRow()) === b, 10_000);
    // The owner's case: one clicked (opened), another Ctrl-clicked: both, the open one first.
    await click(c, "toggle");
    await says("2 VODs", "one opened, another Ctrl-clicked");
    await pickedAre({ [b]: "1", [c]: "2" }, "one opened, another Ctrl-clicked");
    // Escape lets go of the other: the open one stays picked, so a Ctrl-click adds to it again.
    await app.key("Escape");
    await says("", "Escape");
    if ((await openRow()) !== b) throw new Error(`Escape closed the open row: ${await openRow()}`);
    await click(a, "toggle");
    await says("2 VODs", "Escape, then another Ctrl-clicked");
    await pickedAre({ [b]: "1", [a]: "2" }, "Escape, then another Ctrl-clicked");
    await app.key("Escape");
    await says("", "Escape");
    ctx.log("rows picked as in a file manager: click, Ctrl-click, Shift-click, Ctrl+Shift-click, Escape");

    // The keys: ↑ or ↓ opens the row before or after, alone; with Shift, picks the run to it.
    const keys = await app.js(`return [...document.querySelectorAll(".list .vod")].map((r) => Number(r.dataset.key))`);
    const toward = keys.indexOf(c) > keys.indexOf(b) ? "ArrowDown" : "ArrowUp";
    const back = toward === "ArrowDown" ? "ArrowUp" : "ArrowDown";
    // Pressed on the row with the focus (app.key presses on the page's body).
    const keyOn = (id, key, shift = false) =>
      app.js(
        `const el = document.querySelector(arguments[0]);
         el.focus();
         el.dispatchEvent(new KeyboardEvent("keydown", { key: arguments[1], shiftKey: arguments[2], bubbles: true, cancelable: true }));`,
        row(id),
        key,
        shift,
      );
    await keyOn(b, toward);
    await until("the next row open", async () => (await openRow()) === c, 10_000).catch(async (e) => {
      throw new Error(`${e.message} (${toward} on ${b}'s row): open ${await openRow()}`);
    });
    await says("", `${toward}: the row opened alone`);
    await keyOn(c, back, true);
    await says("2 VODs", `Shift+${back} back`);
    // A run is in on-screen order.
    const [upper, lower] = toward === "ArrowDown" ? [b, c] : [c, b];
    await pickedAre({ [upper]: "1", [lower]: "2" }, `Shift+${back} back`);
    if ((await openRow()) !== c) throw new Error(`Shift+${back} opened ${await openRow()}, not left ${c} open`);
    ctx.log(`${toward} opened the next row; Shift+${back} picked the run back`);

    // A real click anywhere on a row, outside its title (its status, the
    // space around it), gives the row the keyboard: the system's ↓ (or ↑)
    // then opens the next, as after a click on its title.
    if (process.platform === "linux" && os.unavailable() === null) {
      await app.js(`document.activeElement?.blur()`);
      // A point of the row's own, under no button of it.
      const spot = await app.js(
        `const row = document.querySelector(arguments[0]);
         const box = row.getBoundingClientRect();
         for (let y = box.bottom - 2; y > box.top; y -= 2)
           for (let x = box.right - 2; x > box.left; x -= 4) {
             const at = document.elementFromPoint(x, y);
             if (at && at.closest(".vod") === row && !at.closest("button")) return { x: x - box.left, y: y - box.top };
           }
         return null;`,
        `.list .vod[data-key="${b}"]`,
      );
      if (!spot) throw new Error("no place on the row outside its buttons to click");
      await app.focused();
      const box = await os.rectOf(app, `.list .vod[data-key="${b}"]`);
      os.click(box.x + spot.x * box.scale, box.y + spot.y * box.scale, box.scale);
      await until("the row clicked open", async () => (await openRow()) === b, 10_000);
      await os.gesture([
        ["key", toward === "ArrowDown" ? "Down" : "Up"],
        ["wait", 300],
      ]);
      await until("the next row open", async () => (await openRow()) === c, 10_000).catch(async (e) => {
        const focus = await app.js(`return document.activeElement?.outerHTML.slice(0, 80) ?? null`);
        throw new Error(
          `${e.message}: a click on the row at ${JSON.stringify(spot)}, then ${toward}, left ${await openRow()} open (the focus on ${focus})`,
        );
      });
      ctx.log(`a click on the row outside its title, then the system's ${toward}: the next row opened`);
      // The run picked back again, as before this.
      await keyOn(c, back, true);
      await says("2 VODs", `Shift+${back} back again`);
    }

    // The open one removed with its own Remove: it
    // leaves the set, the others numbered 1 and 2.
    await click(a, "toggle");
    await says("3 VODs", "a third Ctrl-clicked");
    await pickedAre({ [upper]: "1", [lower]: "2", [a]: "3" }, "a third Ctrl-clicked");
    await app.answer("Remove");
    await app.click(`.list .vod[data-key="${c}"] button[aria-label="Remove"]`);
    await until("its row gone", () => app.js(`return !document.querySelector(arguments[0])`, row(c)), MINUTE);
    await says("2 VODs", "the open one removed by its own Remove");
    const left = await until(
      "the two left numbered 1 and 2",
      async () => {
        const n = await numbered();
        return Object.values(n).sort().join() === "1,2" && n;
      },
      10_000,
    ).catch(async (e) => {
      throw new Error(`${e.message}: ${JSON.stringify(await numbered())}`);
    });
    if (Object.keys(left).sort().join() !== [a, b].sort().join())
      throw new Error(`picked rows ${JSON.stringify(left)}, not ${a} and ${b}`);
    await ctx.checkpoint("library-picked");

    // Remove… above the list: one ask, both go.
    const before = await app.asked();
    await app.answer("Remove");
    await app.clickText("Remove…");
    await until(
      "both removed",
      async () => {
        const ids = (await app.invoke("list_vods")).map((v) => v.id);
        return !ids.includes(a) && !ids.includes(b) && ids.includes(vod.id) && ids.includes(d);
      },
      MINUTE,
    );
    const asked = await app.js(`return window.__e2e.asked.slice(arguments[0])`, before);
    if (asked.length !== 1 || !String(asked[0]).includes("Remove 2 VODs?"))
      throw new Error(`Remove… asked ${JSON.stringify(asked)}`);

    // The first VOD open again, picked as the open row: Delete there isn't
    // the library's (nothing's asked, nothing removed).
    await app.openVod(vod.id);
    await until("its two pills", () => app.js(`return document.querySelectorAll(".chip.ready").length === 2`), MINUTE);
    await says("", "the first VOD opened");
    const askedOpen = await app.asked();
    await app.key("Delete");
    await sleep(500);
    const askedThen = await app.js(`return window.__e2e.asked.slice(arguments[0])`, askedOpen);
    if (askedThen.length) throw new Error(`Delete with only the open row picked asked ${JSON.stringify(askedThen)}`);

    // Escape lets go of a row picked with the open one.
    await click(d, "toggle");
    await says("2 VODs", "another Ctrl-clicked beside the open one");
    await app.key("Escape");
    await until(
      "Escape let it go",
      () => app.js(`return !document.querySelector(".list .vod.chosen, aside .picked")`),
      10_000,
    );

    // A row picked, then a cut clicked: the row is let go, and Delete takes
    // the cut, asking nothing.
    await click(d, "toggle");
    await says("2 VODs", "another Ctrl-clicked beside the open one");
    await clickCut();
    await until("the cut selected", () => app.js(`return !!document.querySelector(".cut.selected")`), 10_000);
    await until(
      "the row let go",
      () => app.js(`return !document.querySelector(".list .vod.chosen, aside .picked")`),
      10_000,
    ).catch(async (e) => {
      throw new Error(`${e.message}: the list says "${await listHead()}"`);
    });
    const askedBefore = await app.asked();
    await app.key("Delete");
    await until("the cut gone", async () => (await marks()) === 1, 10_000);
    const askedNow = await app.js(`return window.__e2e.asked.slice(arguments[0])`, askedBefore);
    if (askedNow.length) throw new Error(`Delete on the cut asked ${JSON.stringify(askedNow)}`);
    if (!(await app.invoke("list_vods")).some((v) => v.id === vod.id))
      throw new Error("Delete on the cut removed the VOD");

    // The two pills picked, 2-3 s first, and the second dragged (in the
    // page's events) onto "Make new edit": both go in a new edit, in the
    // order picked.
    const editsBefore = new Set((await app.invoke("list_edits")).map((e) => e.id));
    const plainCount = (await shown()).length;
    // Made first, so picked first: 2-3 s, then 1-2 s.
    const inOrder = [...made].sort((x, y) => x.id - y.id);
    for (const clip of inOrder)
      await app.js(
        `${TOGGLE}
         document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
           .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
        clip.id,
      );
    await until("two pills picked", async () => (await chosen()).length === 2, 10_000);
    const AT = `const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });`;
    await app.js(
      `${AT}
       const el = document.querySelector('.chip[data-clip="' + arguments[0] + '"] .time');
       const r = el.getBoundingClientRect();
       const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
       el.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
       window.dispatchEvent(new PointerEvent("pointermove", at(x + 10, y + 10, 1)));
       window.__drag = [x + 10, y + 10];`,
      inOrder[1].id,
    );
    await until(
      '"Make new edit", to drop on',
      () => app.js(`return !!document.querySelector("[data-new-edit]")`),
      10_000,
    );
    await app.js(
      `${AT}
       const r = document.querySelector("[data-new-edit]").getBoundingClientRect();
       const [x0, y0] = window.__drag;
       const [x1, y1] = [r.left + r.width / 2, r.top + r.height / 2];
       for (let i = 1; i <= 5; i++) window.dispatchEvent(new PointerEvent("pointermove", at(x0 + ((x1 - x0) * i) / 5, y0 + ((y1 - y0) * i) / 5, 1)));
       window.dispatchEvent(new PointerEvent("pointerup", at(x1, y1, 0)));`,
    );
    const edit = await until(
      "the new edit",
      async () => (await app.invoke("list_edits")).find((e) => !editsBefore.has(e.id)),
      15_000,
    );
    const pieces = async () => (await app.invoke("get_edit", { id: edit.id })).pieces.map((p) => Math.round(p.start));
    await until("both in the edit, in the order picked", async () => (await pieces()).join() === "2,1", 15_000).catch(
      async (e) => {
        throw new Error(`${e.message}: the edit has ${JSON.stringify(await pieces())}`);
      },
    );
    if ((await shown()).length !== plainCount) throw new Error("the drop on Make new edit changed the clip bar");
    ctx.log("a pill picked with another, dragged onto Make new edit, put both in a new edit, 2-3 then 1-2");
  },
};
