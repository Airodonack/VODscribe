// Ctrl+C and Ctrl+V (⌘ on a Mac: copyPaste.ts), copies, not references. On
// a transcribed file: Ctrl+A over the timeline selects all of it (not the
// clips), and Ctrl+C copies that, a line of text naming the file. Cuts at
// 1, 2 and 3 s; I at 1 s and O at 2 s, Ctrl+C writes one line with the
// stretch, and Ctrl+V over the timeline (no pills yet, so no clip bar
// shows; a VOD's timeline takes no pieces) makes a pill of it on the bar,
// picked, made; Ctrl+Z takes it off and Ctrl+Shift+Z brings it back. That pill
// clicked and copied, pasted on its own bar, is a second pill of the same
// stretch: ready at once, never cut, its file another of the same bytes.
// Ctrl+A over the bar then picks both. The first pill deleted, the copy
// stays, its file with it.
//
// In a new edit holding the stretch 1-2 s, the playhead half-way and the
// pointer on its timeline, Ctrl+V splits its piece there and puts the pill
// in between as a group, selected; one undo gives the one piece back. A
// stretch of the edit copied, the edit then trimmed, pasted on its clip bar
// is a clip of the stretch as it was copied. A second file's whole timeline
// copied and pasted in the edit: the edit holds both VODs; on the first
// VOD's own bar it's refused, nothing made. The edit's clip open in the
// editor takes a paste at its playhead, and is made again. Ctrl+X on a pill
// takes it off its bar, and Ctrl+V brings a copy back. Something else
// copied since (the clipboard's text not the copy's): Ctrl+V does nothing.
// In a text field Ctrl+C and Ctrl+V are the field's. A third file's
// stretch copied, the file then removed with nothing using it: pasted, the
// app says its VOD is gone and the edit stays as it was.
//
// The clipboard is the page's: its plugin's commands are answered in the
// page (the text written is kept there, and read back from there), so the
// system's clipboard is never touched, here or on a runner.

import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";

const sha = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
const near = (a, b, slack = 0.05) => Math.abs(a - b) <= slack;

export default {
  name: "copy-paste",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "pasted-picked":
      "Under the player, the clip bar shows two pills of the same length side by side, the right one outlined as picked.",
  },
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    await app.pause();
    await app.toStart();

    // The clipboard plugin's commands answered in the page; the news of each
    // clip (its "clip" events) noted, to tell a copied file from one cut.
    await app.js(`
      window.__clip = { text: "", writes: [] };
      window.__clipNews = [];
      const answer = (value) => Promise.resolve(new Response(JSON.stringify(value), { headers: { "Content-Type": "application/json", "Tauri-Response": "ok" } }));
      const fetch = window.fetch;
      window.fetch = (url, init) => {
        const command = decodeURIComponent(String(url)).split("/").pop();
        if (command === "plugin:clipboard-manager|write_text") {
          window.__clip.text = JSON.parse(init.body).text;
          window.__clip.writes.push(window.__clip.text);
          return answer(null);
        }
        if (command === "plugin:clipboard-manager|read_text") return answer(window.__clip.text);
        return fetch(url, init);
      };
      const internals = window.__TAURI_INTERNALS__;
      const handler = internals.transformCallback((e) => window.__clipNews.push({ id: e.payload.id, status: e.payload.status }));
      await internals.invoke("plugin:event|listen", { event: "clip", target: { kind: "Any" }, handler });`);
    const written = () => app.js(`return window.__clip.writes.slice()`);
    /** Waits for one more text written than `before`; returns it. */
    const copiedSince = async (before, what) =>
      until(
        `${what}: the text copied`,
        async () => {
          const all = await written();
          return all.length > before && all.at(-1);
        },
        10_000,
      );
    const news = (id) =>
      app.js(`return window.__clipNews.filter((n) => n.id === arguments[0]).map((n) => n.status)`, id);

    // Where the pointer is, as the page goes by it (a pointermove over it).
    const pointAt = (selector) =>
      app.js(
        `const el = document.querySelector(arguments[0]);
         const b = el.getBoundingClientRect();
         el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: b.left + b.width / 2, clientY: b.top + b.height / 2, isPrimary: true, pointerId: 1 }));`,
        selector,
      );
    const copy = () => app.key("c", { cmd: true });
    const paste = () => app.key("v", { cmd: true });
    const undo = (redo = false) => app.key("z", { cmd: true, shift: redo });
    // The selection on the timeline, as drawn: its left and width, in percent of what shows.
    const selection = () =>
      app.js(`const s = document.querySelector(".scrubber .selection");
              return s && { left: parseFloat(s.style.left), width: parseFloat(s.style.width) };`);
    const pills = async () => (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => c.shown);
    const picked = () =>
      app.js(`return [...document.querySelectorAll(".clip-bar .chip.chosen")].map((c) => Number(c.dataset.clip))`);
    const ready = (find, what) =>
      until(
        what,
        async () => {
          const c = await find();
          if (c?.status === "error") throw new Error(`${what}: ${c.error}`);
          return c?.status === "ready" && c;
        },
        MINUTE,
      );

    // 1. Ctrl+A over the timeline: all of it selected; Ctrl+C copies all of it.
    const { duration } = (await app.invoke("list_vods")).find((v) => v.id === vod.id);
    await pointAt(".scrubber");
    await app.key("a", { cmd: true });
    const whole = await until("the whole timeline selected", selection, 10_000);
    if (whole.left > 0.5 || whole.width < 99.5)
      throw new Error(`Ctrl+A selected ${JSON.stringify(whole)} of the timeline`);
    if ((await picked()).length) throw new Error("Ctrl+A over the timeline picked clips");
    let before = (await written()).length;
    await copy();
    const all = await copiedSince(before, "the whole timeline");
    const end = Math.floor(duration);
    const span = `0:00–${Math.floor(end / 60)}:${String(end % 60).padStart(2, "0")}`;
    if (!all.includes(`(${span})`) || all.includes("\n") || all.includes(path.dirname(ctx.fixtures.landed)))
      throw new Error(`the whole timeline copied as "${all}", not one line with (${span}) and no path`);
    ctx.log(`Ctrl+A over the timeline, Ctrl+C: "${all}"`);
    await app.key("Escape");

    // 2. Cuts at 1, 2 and 3 s; I at 1 s, O at 2 s; Ctrl+C, then Ctrl+V over the bar.
    const to = (s) => until(`the player at ${s} s`, async () => (await app.shownTime()) === s, 10_000);
    for (const s of [1, 2, 3]) {
      await app.key("ArrowRight", { shift: true });
      await to(s);
      await app.key("b");
      await until("the cut, selected", () => app.js(`return !!document.querySelector(".cut.selected")`), 10_000);
      await app.key("Escape");
      await until("the cut let go", () => app.js(`return !document.querySelector(".cut.selected")`), 10_000);
    }
    await app.key("ArrowLeft", { shift: true });
    await app.key("ArrowLeft", { shift: true });
    await to(1);
    await app.key("i");
    await app.key("ArrowRight", { shift: true });
    await to(2);
    await app.key("o");
    await until("the selection 1-2 s", selection, 10_000);
    before = (await written()).length;
    await copy();
    const line = await copiedSince(before, "1-2 s");
    if (!line.includes("(0:01–0:02)") || line.includes("\n")) throw new Error(`1-2 s copied as "${line}"`);
    const said = await until(
      "the toast",
      async () => (await app.toastSays()).startsWith("Copied") && app.toastSays(),
      10_000,
    );
    ctx.log(`I, O, Ctrl+C: "${line}", the toast "${said}"`);
    // No pills yet, so no clip bar showing: over the timeline, a VOD's paste
    // goes on its bar (its timeline takes no pieces).
    await pointAt(".scrubber");
    await paste();
    const first = await until(
      "a pill of 1-2 s, picked",
      async () => {
        const [c] = await pills();
        return c && near(c.start, 1) && near(c.end, 2) && (await picked()).includes(c.id) && c;
      },
      10_000,
    );
    await ready(async () => (await pills()).find((c) => c.id === first.id), "the pasted pill made");
    await undo();
    await until("Ctrl+Z took it off", async () => (await pills()).length === 0, 10_000);
    await undo(true);
    const [again] = await until(
      "Ctrl+Shift+Z brought it back",
      async () => (await pills()).length === 1 && pills(),
      10_000,
    );
    const one = await ready(async () => (await pills()).find((c) => c.id === again.id), "the pill brought back, made");
    ctx.log(`Ctrl+V over the clip bar: pill ${first.id}, 1-2 s, picked; undone and redone (now ${one.id})`);

    // 3. The pill picked alone, copied, pasted on its own bar: a second one,
    // ready without being cut, its file a copy of the first's.
    await app.click(`.clip-bar .chip[data-clip="${one.id}"]`);
    await until("the pill picked", async () => (await picked()).join() === String(one.id), 10_000);
    before = (await written()).length;
    await copy();
    await copiedSince(before, "the pill");
    await pointAt(".clip-bar");
    await paste();
    const second = await until("a second pill", async () => (await pills()).find((c) => c.id !== one.id), 10_000);
    const copied = await ready(async () => (await pills()).find((c) => c.id === second.id), "the second pill made");
    if (!near(copied.start, one.start) || !near(copied.end, one.end))
      throw new Error(`the second pill runs ${copied.start}-${copied.end}, not ${one.start}-${one.end}`);
    const statuses = await news(copied.id);
    if (statuses.includes("cutting")) throw new Error(`the pasted pill was cut again: ${statuses.join(", ")}`);
    if (copied.file_path === one.file_path) throw new Error(`both pills have the one file ${one.file_path}`);
    if (sha(copied.file_path) !== sha(one.file_path))
      throw new Error(`the pasted pill's file ${copied.file_path} isn't the same as ${one.file_path}`);
    await until("the copy picked", async () => (await picked()).join() === String(copied.id), 10_000);
    await ctx.checkpoint("pasted-picked");
    ctx.log(`the pill pasted beside itself: ${copied.id}, ready (${statuses.join(", ")}), its file a copy`);
    // Ctrl+A over the bar: both pills.
    await pointAt(".clip-bar");
    await app.key("a", { cmd: true });
    await until("Ctrl+A over the bar: both pills", async () => (await picked()).length === 2, 10_000);
    await app.key("Escape");

    // 4. The first pill deleted: the copy stays, with its file.
    await app.click(`.clip-bar .chip[data-clip="${one.id}"]`);
    await until("the first pill picked", async () => (await picked()).join() === String(one.id), 10_000);
    await app.key("Delete");
    await until("the first pill gone", async () => (await pills()).every((c) => c.id !== one.id), 10_000);
    if (!(await pills()).some((c) => c.id === copied.id) || !existsSync(copied.file_path))
      throw new Error("deleting the first pill took its copy, or the copy's file");

    // 5. A new edit holding 1-2 s; its playhead half-way, Ctrl+V on its
    // timeline: its piece split there, the pill between, as a group.
    const created = await app.invoke("create_edit");
    await app.invoke("put_in_edit", {
      editId: created.id,
      items: [{ kind: "range", vodId: vod.id, start: 1, end: 2 }],
      bar: false,
    });
    const editTitle = 'section.edit-view input.title[aria-label="Edit name"]';
    const openEdit = async () => {
      await until(
        "the edit's row",
        () => app.js(`return !!document.querySelector('.vod.edit[data-key="-${created.id}"] button.main')`),
        10_000,
      );
      await app.click(`.vod.edit[data-key="-${created.id}"] button.main`);
      await until("the edit open", () => app.js(`return !!document.querySelector(arguments[0])`, editTitle), MINUTE);
      await until("its timeline", () => app.js(`return !!document.querySelector(".scrubber")`), MINUTE);
    };
    await openEdit();
    const pieces = async (id = created.id) => (await app.invoke("get_edit", { id })).pieces;
    /** Puts the open timeline's playhead at `fraction` of what shows, as a click there does; that moment. */
    const seekTo = (fraction) =>
      app.js(
        `const bar = document.querySelector(".scrubber");
         const box = bar.getBoundingClientRect();
         const at = { clientX: box.left + arguments[0] * box.width, clientY: box.top + box.height / 2, button: 0,
                      bubbles: true, pointerId: 1, pointerType: "mouse", isPrimary: true };
         document.elementFromPoint(at.clientX, at.clientY).dispatchEvent(new PointerEvent("pointerdown", at));
         window.dispatchEvent(new PointerEvent("pointerup", at));
         return Number(bar.dataset.viewStart) + arguments[0] * Number(bar.dataset.span);`,
        fraction,
      );
    const half = await seekTo(0.5);
    await pointAt(".scrubber");
    await paste();
    const split = await until("three pieces", async () => ((await pieces()).length === 3 ? pieces() : null), 10_000);
    const shape = split.map(
      (p) => `${p.start.toFixed(2)}-${p.end.toFixed(2)}${p.group ? ` in “${p.group.title}”` : ""}`,
    );
    const at = split[0].end - 1;
    if (
      split[0].group ||
      !split[1].group ||
      split[2].group ||
      !near(split[0].start, 1) ||
      !near(split[2].end, 2) ||
      !near(split[0].end, split[2].start, 0.001) ||
      !near(at, half, 0.15)
    )
      throw new Error(`pasted at ${half.toFixed(2)} s: ${shape.join(", ")}`);
    const pastedSpan = await until("the pasted stretch selected", selection, 10_000);
    if (!near(pastedSpan.width, 50, 3)) throw new Error(`the selection after the paste: ${JSON.stringify(pastedSpan)}`);
    ctx.log(`Ctrl+V on the edit's timeline at ${half.toFixed(2)} s: ${shape.join(", ")}`);
    await undo();
    await until("one undo: the one piece back", async () => (await pieces()).length === 1, 10_000);
    await app.key("Escape");

    // 6. A stretch of the edit copied, the edit then changed: pasted on its
    // clip bar, the stretch as it was copied.
    await seekTo(0.2);
    await app.key("i");
    await seekTo(0.8);
    await app.key("o");
    await until("the selection 0.2-0.8 s", selection, 10_000);
    before = (await written()).length;
    await copy();
    await copiedSince(before, "the edit's stretch");
    const [piece] = await pieces();
    await app.invoke("trim_piece", { id: piece.id, start: 1.5, end: 2 });
    await pointAt(".clip-bar");
    await paste();
    const barClips = async () => (await app.invoke("edit_clips", { id: created.id })).filter((c) => c.shown);
    const [barClip] = await until(
      "a clip on the edit's bar",
      async () => (await barClips()).length === 1 && barClips(),
      10_000,
    );
    const snapshot = await pieces(barClip.id);
    if (snapshot.length !== 1 || !near(snapshot[0].start, 1.2, 0.15) || !near(snapshot[0].end, 1.8, 0.15))
      throw new Error(`the edit's stretch pasted as ${JSON.stringify(snapshot.map((p) => [p.start, p.end]))}`);
    ctx.log(
      `the edit's stretch copied, the edit trimmed, pasted: ${snapshot[0].start.toFixed(2)}-${snapshot[0].end.toFixed(2)}`,
    );

    // 7. Another file's whole timeline copied: into the edit it goes; on the
    // first VOD's own bar it's refused.
    const dir = path.join(ctx.out, "copy-paste");
    mkdirSync(dir, { recursive: true });
    const files = ["second.mp3", "third.mp3"].map((name) => {
      const file = path.join(dir, name);
      copyFileSync(ctx.fixtures.landed, file);
      return file;
    });
    const [other, third] = (await app.addFiles(files)).sort((x, y) => x.id - y.id);
    const openTimeline = async (id) => {
      await app.openVod(id);
      await until("its timeline", () => app.js(`return !!document.querySelector(".scrubber")`), MINUTE);
      await until(
        "its length known",
        async () => (await app.invoke("list_vods")).find((v) => v.id === id)?.duration > 0,
        MINUTE,
      );
    };
    const copyWhole = async (what) => {
      await pointAt(".scrubber");
      await app.key("a", { cmd: true });
      await until(`${what}: all of it selected`, async () => (await selection())?.width > 99, 10_000);
      const count = (await written()).length;
      await copy();
      return copiedSince(count, what);
    };
    await openTimeline(other.id);
    ctx.log(`the second file's timeline copied: "${await copyWhole("the second file")}"`);
    await openEdit();
    await pointAt(".scrubber");
    await paste();
    await until(
      "the edit holding both VODs",
      async () => (await app.invoke("get_edit", { id: created.id })).edit.vod_ids.length === 2,
      10_000,
    );
    await app.openVod(vod.id);
    await until("the first VOD's clip bar", () => app.js(`return !!document.querySelector(".clip-bar")`), MINUTE);
    const had = (await pills()).length;
    await pointAt(".clip-bar");
    await paste();
    await until(
      "the paste refused",
      async () => (await app.toastSays()).includes("Clips of another VOD go in an edit"),
      10_000,
    );
    if ((await pills()).length !== had) throw new Error("another VOD's stretch was pasted on a VOD's own bar");

    // 8. The edit's clip open in the editor: a paste at its playhead goes in
    // it, and it's made again.
    await openEdit();
    await app.js(
      `const el = document.querySelector(arguments[0]);
       const box = el.getBoundingClientRect();
       const at = { bubbles: true, cancelable: true, clientX: box.left + 4, clientY: box.top + box.height / 2 };
       for (const detail of [1, 2]) el.dispatchEvent(new MouseEvent("click", { ...at, detail }));
       el.dispatchEvent(new MouseEvent("dblclick", { ...at, detail: 2 }));`,
      `section.edit-view .chip[data-clip="${barClip.id}"]`,
    );
    await until(
      "the clip open in the editor",
      () =>
        app.js(
          `return document.querySelector("section.edit-view input.title")?.getAttribute("aria-label") === "Clip name"`,
        ),
      10_000,
    );
    await until("its timeline", () => app.js(`return !!document.querySelector(".scrubber")`), MINUTE);
    const newsBefore = (await news(barClip.id)).length;
    await pointAt(".scrubber");
    await paste();
    await until(
      "the pasted stretch in the clip",
      async () => (await pieces(barClip.id)).some((p) => p.vod_id === other.id),
      10_000,
    );
    await app.click("section.edit-view .sources button.back");
    await until("the edit back", () => app.js(`return !!document.querySelector(arguments[0])`, editTitle), 10_000);
    await ready(async () => (await barClips()).find((c) => c.id === barClip.id), "the clip made again");
    const remade = (await news(barClip.id)).slice(newsBefore);
    if (!remade.includes("cutting")) throw new Error(`the clip pasted into wasn't made again: ${remade.join(", ")}`);

    // 9. Ctrl+X on a picked pill: off the bar; Ctrl+V: a copy back.
    // Back from the editor, its pill is picked already (a click would let it go).
    if ((await picked()).join() !== String(barClip.id))
      await app.click(`section.edit-view .clip-bar .chip[data-clip="${barClip.id}"]`);
    await until("the edit's pill picked", async () => (await picked()).join() === String(barClip.id), 10_000);
    before = (await written()).length;
    await app.key("x", { cmd: true });
    await copiedSince(before, "Ctrl+X");
    await until("Ctrl+X took it off", async () => (await barClips()).length === 0, 10_000);
    await pointAt(".clip-bar");
    await paste();
    const [back] = await until(
      "Ctrl+V brought a copy back",
      async () => (await barClips()).length === 1 && barClips(),
      10_000,
    );
    if (back.id === barClip.id || back.title !== barClip.title)
      throw new Error(`pasted back as ${back.id} "${back.title}", cut ${barClip.id} "${barClip.title}"`);

    // 10. Something else copied since: Ctrl+V pastes nothing, and says nothing.
    await app.invoke("plugin:clipboard-manager|write_text", { text: "hello" });
    await app.js(`document.querySelector(".toast button.close")?.click()`);
    await until("the toast dismissed", async () => (await app.toastSays()) === "", 10_000);
    await pointAt(".clip-bar");
    await paste();
    await new Promise((r) => setTimeout(r, 1000));
    if ((await barClips()).length !== 1) throw new Error("Ctrl+V pasted the old copy over what was copied since");
    if (await app.toastSays()) throw new Error(`Ctrl+V with something else copied said "${await app.toastSays()}"`);

    // 11. In a text field Ctrl+C and Ctrl+V are the field's.
    const field = 'input[aria-label="VOD link"]';
    await app.type(field, "https://example.com/a");
    const taken = await app.js(
      `const field = document.querySelector(arguments[0]);
       field.focus();
       field.select();
       const mac = /Mac/.test(navigator.platform);
       return ["c", "v"].map((key) => {
         const e = new KeyboardEvent("keydown", { key, ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true });
         field.dispatchEvent(e);
         return e.defaultPrevented;
       });`,
      field,
    );
    await new Promise((r) => setTimeout(r, 500));
    if (taken.some(Boolean)) throw new Error(`in a text field, the app took Ctrl+C or Ctrl+V: ${taken}`);
    if ((await written()).at(-1) !== "hello") throw new Error("Ctrl+C in a text field copied the app's selection");
    if ((await barClips()).length !== 1) throw new Error("Ctrl+V in a text field pasted on the clip bar");
    await app.type(field, "");
    await app.blur(field);

    // 12. The third file's stretch copied, the file then removed (nothing
    // uses it): pasted, the app says it's gone, and the edit stays as it was.
    await openTimeline(third.id);
    await copyWhole("the third file");
    await app.invoke("delete_vod", { id: third.id });
    await openEdit();
    const was = await pieces();
    await pointAt(".scrubber");
    await paste();
    await until(
      "the paste refused, the VOD gone",
      async () => (await app.toastSays()).includes("The VOD it was copied from is gone"),
      10_000,
    );
    if (JSON.stringify(await pieces()) !== JSON.stringify(was))
      throw new Error("a paste of a VOD gone changed the edit");
    ctx.log("pasted from a VOD gone: refused, the edit as it was");
    // Said and read: dismissed, so it isn't taken for an error left showing.
    await app.click(".toast button.close");
    await until("the toast dismissed", async () => (await app.toastSays()) === "", 10_000);
  },
};
