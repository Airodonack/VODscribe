// A clip open in the editor with pieces from more VODs than its row of
// links fits, so the row scrolls sideways: every link in view is still
// what's under the pointer at its middle (the page's own hit test,
// elementFromPoint), at the row's start and scrolled to its end, as on a
// clip bar that scrolls (clip-bar-long).

import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";

/** The other VODs' files: copies of the landing, with long names, so their links are wide. */
const COPIES = 6;

export default {
  name: "clip-sources-long",
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    const dir = path.join(ctx.out, "clip-sources-long");
    mkdirSync(dir, { recursive: true });
    const files = Array.from({ length: COPIES }, (_, i) => {
      const file = path.join(dir, `the eagle has landed, a copy with a long name ${i + 1}.mp3`);
      copyFileSync(ctx.fixtures.landed, file);
      return file;
    });
    const others = (await app.addFiles(files)).map((v) => v.id);
    await until(
      "their lengths known",
      async () =>
        (await app.invoke("list_vods")).filter((v) => others.includes(v.id) && v.duration > 0).length === COPIES,
      MINUTE,
    );

    // A clip of the first, a second of each of the others put in it, opened
    // in the editor (its pill double-clicked): a link to each VOD on its row.
    await app.openPaused(vod.id);
    const clip = await app.clipOneToThree(vod.id);
    for (const id of others)
      await app.invoke("add_range_to_edit", { editId: clip.id, vodId: id, start: 0, end: 1, title: "a second" });
    await until("its pill, made", () => app.js(`return !!document.querySelector(".chip.ready .name")`), MINUTE);
    await app.js(
      `const el = document.querySelector('.chip.ready[data-clip="${clip.id}"] .time');
       for (const detail of [1, 2]) el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail }));
       el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, detail: 2 }));`,
    );
    await until(
      "the clip open in the editor",
      () => app.js(`return !!document.querySelector('section.edit-view input.title[aria-label="Clip name"]')`),
      MINUTE,
    );
    await until(
      "a link to each VOD",
      () => app.js(`return document.querySelectorAll(".sources .source").length === arguments[0]`, COPIES + 1),
      10_000,
    );
    const overflow = await app.js(`
      const row = document.querySelector(".sources");
      return [row.scrollWidth, row.clientWidth];`);
    if (overflow[0] <= overflow[1]) throw new Error(`the row of links doesn't scroll: ${overflow.join(" ≤ ")} px`);
    ctx.log(`the row of links is ${overflow[0]} px wide in ${overflow[1]} px: it scrolls`);

    // What's under each link's middle, of those in the row's view: the link, or a part of it.
    const allHit = async (where) => {
      const { seen, missed } = await app.js(`
        const row = document.querySelector(".sources");
        const view = row.getBoundingClientRect();
        const missed = [];
        let seen = 0;
        for (const link of row.querySelectorAll("button")) {
          const r = link.getBoundingClientRect();
          const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
          if (x < view.left || x > view.left + row.clientWidth) continue;
          seen++;
          const hit = document.elementFromPoint(x, y);
          if (!link.contains(hit))
            missed.push(link.textContent.trim().slice(0, 30) + ": " + (hit ? hit.tagName + "." + [...hit.classList].join(".") : "nothing"));
        }
        return { seen, missed };`);
      if (!seen) throw new Error(`no link in the row's view ${where}`);
      if (missed.length)
        throw new Error(`${where}, the pointer at a link's middle finds something else: ${missed.join(", ")}`);
      ctx.log(`${where}: each of the ${seen} links in view is what's under its middle`);
    };
    await allHit("at its start");
    await app.js(`const row = document.querySelector(".sources"); row.scrollLeft = row.scrollWidth;`);
    await allHit("scrolled to its end");
  },
};
