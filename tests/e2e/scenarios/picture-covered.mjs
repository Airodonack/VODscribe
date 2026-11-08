// An edit's picture under what opens over it, after its clips were opened
// and left: a clip of the edit's clip bar opened in the editor (double-click
// its pill) and the way back taken, several times and at varied speeds. Each
// time the picture is in the player's box, and Settings, then the search
// filter, opened over it shows on the screen, not the picture over them.
// The app's own player draws outside the page, so only a screenshot shows
// which is on top.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { colourNamed, colours, NEAR } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** Double-clicks clip `id`'s pill on the open edit's clip bar, as the page gets one. */
const openPill = (app, id) =>
  app.js(
    `const el = document.querySelector(arguments[0]);
     const box = el.getBoundingClientRect();
     const at = { bubbles: true, cancelable: true, clientX: box.left + 4, clientY: box.top + box.height / 2 };
     for (const detail of [1, 2]) el.dispatchEvent(new MouseEvent("click", { ...at, detail }));
     el.dispatchEvent(new MouseEvent("dblclick", { ...at, detail: 2 }));`,
    `section.edit-view .chip[data-clip="${id}"]`,
  );

/** The head's name field, saying whether an edit or a clip is open. */
const opened = (app, label) =>
  until(
    `the ${label.split(" ")[0].toLowerCase()} open`,
    () =>
      app.js(`return !!document.querySelector(arguments[0])`, `section.edit-view input.title[aria-label="${label}"]`),
    MINUTE,
  );

export default {
  name: "picture-covered",
  async run(app, ctx) {
    const why = os.unavailable();
    if (why) ctx.skip(why);
    try {
      os.screenshot();
    } catch (e) {
      ctx.skip(`no screenshot here: ${e.message.split("\n")[0]}`);
    }
    if (!(await app.invoke("player_available"))) ctx.skip("no screen for the app's own player here");
    await app.toLibrary();
    const [vod] = await app.addFiles([colours(path.join(ctx.out, "fixtures"))]);
    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 0, end: 6, title: "Colours" });
    const row = `.vod.edit[data-key="-${edit.id}"] button.main`;
    await until("the edit's row", () => app.js(`return !!document.querySelector(arguments[0])`, row), MINUTE);
    await app.click(row);
    await opened(app, "Edit name");

    // A clip on the edit's clip bar (its Clip button: the piece at the playhead).
    await app.click("section.edit-view .bar button.clip");
    const clip = await until(
      "the edit's clip, made",
      async () => {
        const [c] = await app.invoke("edit_clips", { id: edit.id });
        if (c?.status === "error") throw new Error(`it failed: ${c.error}`);
        return c?.status === "ready" && c;
      },
      MINUTE,
    );
    const pill = `section.edit-view .clip-bar .chip[data-clip="${clip.id}"]`;
    await until("its pill", () => app.js(`return !!document.querySelector(arguments[0])`, pill), 10_000);

    /** The picture's colour, in the middle of the player's box. */
    const frame = "section.edit-view .frame";
    const picture = async () => os.colourIn(os.screenshot(), os.middle(await os.rectOf(app, frame)));
    /** Where the page's player screens are, for a failure: in the frame, or left out of the page. */
    const boxes = () =>
      app.js(
        `const all = [...document.querySelectorAll(".screen")];
         return JSON.stringify({ inFrame: !!document.querySelector(arguments[0] + " .screen"), screens: all.length })`,
        frame,
      );
    /** Opens `panel` with `open`, and checks that where it overlaps the
     *  picture, the screen doesn't show the picture's colour `shown` (what's
     *  in the panel varies; the picture is one flat colour). */
    const covers = async (what, open, close, panel, shown) => {
      await open();
      await until(`${what} open`, () => app.js(`return !!document.querySelector(arguments[0])`, panel), 10_000);
      await sleep(500);
      const [p, v] = [await os.rectOf(app, panel), await os.rectOf(app, frame)];
      const x = Math.max(p.x, v.x);
      const y = Math.max(p.y, v.y);
      const over = { x, y, w: Math.min(p.x + p.w, v.x + v.w) - x, h: Math.min(p.y + p.h, v.y + v.h) - y };
      if (over.w < 20 || over.h < 20) throw new Error(`${what} doesn't overlap the picture in this window`);
      await until(
        `${what} on the screen where it overlaps the picture`,
        async () => {
          const there = os.colourIn(os.screenshot(), over);
          if (os.distance(there, shown) > NEAR) return true;
          throw new Error(`the screen shows the picture (${colourNamed(there)}) there, not ${what} (${await boxes()})`);
        },
        5_000,
      );
      await close();
      await until(`${what} closed`, () => app.js(`return !document.querySelector(arguments[0])`, panel), 10_000);
    };
    const check = async (where) => {
      const shown = await until(
        `the picture in the player's box (${where})`,
        async () => {
          const seen = await picture();
          const name = colourNamed(seen);
          if (name.startsWith("none") || name === "black") throw new Error(`it shows ${name} (${await boxes()})`);
          return seen;
        },
        10_000,
      );
      const settings = '[role=dialog][aria-label="Settings"]';
      await covers(
        "Settings",
        () => app.click('button[aria-label="Settings"]'),
        () => app.click(`${settings} button[aria-label="Close"]`),
        settings,
        shown,
      );
      const filter = () => app.click("button.filter");
      await covers("the search filter", filter, filter, '[role=dialog][aria-label="Search filter"]', shown);
      ctx.log(`${where}: Settings and the search filter show over the picture`);
    };

    await check("the edit");
    // Into the clip and back, waiting each time for the view, then at once.
    for (const [i, pause] of [500, 0, 2000, 0, 100].entries()) {
      await openPill(app, clip.id);
      await opened(app, "Clip name");
      await sleep(pause);
      if (i === 0) await check("the clip");
      await app.click("section.edit-view button.source.back");
      await opened(app, "Edit name");
      await sleep(pause);
    }
    await check("the edit, after its clip five times");
  },
};
