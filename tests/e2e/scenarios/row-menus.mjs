// Every library row has the same actions, in a right-click menu (actions.ts,
// Menu.svelte). A file's row has Show in folder among its buttons, as an
// edit's row does once rendered. Its menu opens with the focus on its first
// item, the arrows move it, Escape closes it and gives the focus back; it
// has Show in folder, Add to edit… and Remove…, which asks as removing
// several does, saying the transcript is erased. An edit's row has a menu
// too: Add to edit… asks which edit, and Delete edit… asks before it goes.
// A site's VOD whose video was downloaded has Show in folder too, on its
// row and in its menu, beside Open on its site and Copy link; the menu is
// as wide as its longest item, and a click on the row closes it without
// opening or picking the row. So has one
// whose download is paused part way, for the part file as it is; with no
// part file, neither, and the app says there's no file.

import { copyFileSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, libraryPath, makeSiteVod } from "../lib/data.mjs";
import * as os from "../lib/os.mjs";

export default {
  name: "row-menus",
  async run(app, ctx) {
    const { fixtures, log } = ctx;
    await app.toLibrary();
    const [vod] = await app.addFiles([fixtures.landed]);
    const vodRow = `.list .vod[data-key="${vod.id}"]`;
    const editRow = (id) => `.list .vod.edit[data-key="-${id}"]`;

    // A file's row: Show in folder among its buttons.
    await until(
      "the file's Show in folder on its row",
      () => app.js(`return !!document.querySelector(arguments[0] + ' button[aria-label="Show in folder"]')`, vodRow),
      10_000,
    ).catch(() => {
      throw new Error("a file's row has no Show in folder");
    });

    const menu = (label) =>
      until(
        `the ${label}`,
        () =>
          app.js(
            `const m = document.querySelector('[role=dialog][aria-label="' + arguments[0] + '"] [role=menu]');
             return m && [...m.querySelectorAll("[role=menuitem]")].map((b) => b.textContent.trim())`,
            label,
          ),
        10_000,
      );
    const focused = () => app.js(`return document.activeElement?.textContent.trim() ?? ""`);
    const pick = (label, item) =>
      app.js(
        `const m = document.querySelector('[role=dialog][aria-label="' + arguments[0] + '"] [role=menu]');
         [...m.querySelectorAll("[role=menuitem]")].find((b) => b.textContent.trim().startsWith(arguments[1])).click();`,
        label,
        item,
      );

    // Its menu: the focus on the first item, ↓ to the next, Escape back to the row.
    await app.rowMenu(vod.id);
    const items = await menu("VOD menu");
    for (const want of ["Show in folder", "Add to edit…", "Remove…"])
      if (!items.some((i) => i.startsWith(want)))
        throw new Error(`the VOD's menu has ${JSON.stringify(items)}, no ${want}`);
    await until("the focus on its first item", async () => (await focused()) === items[0], 10_000);
    await app.js(
      `document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }))`,
    );
    await until("the focus on its second item", async () => (await focused()) === items[1], 10_000);
    await app.js(
      `document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }))`,
    );
    await until(
      "the menu closed, the focus back on the row",
      () =>
        app.js(
          `return !document.querySelector('[aria-label="VOD menu"]') && !!document.activeElement?.closest(arguments[0])`,
          vodRow,
        ),
      10_000,
    );
    log(`a VOD's menu: ${items.join(", ")}; ↓ and Escape`);

    // Remove… asks as for several, with what's erased; Keep keeps it.
    const asked = await app.asked();
    await app.answer("Keep");
    await app.rowMenu(vod.id);
    await menu("VOD menu");
    await pick("VOD menu", "Remove");
    await until("Remove to ask", async () => (await app.asked()) === asked + 1, 10_000);
    const question = JSON.parse(await app.js(`return String(window.__e2e.asked.at(-1))`)).message;
    if (
      !question.includes(`Remove “${vod.title}”?`) ||
      !question.includes("transcript and any downloaded audio or video are erased")
    )
      throw new Error(`Remove asked: ${question}`);
    if (!(await app.invoke("list_vods")).some((v) => v.id === vod.id)) throw new Error("Keep removed the VOD");

    // An edit's row: its menu, Add to edit… asking which edit, Delete edit… asking first.
    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: edit.id, vodId: vod.id, start: 0, end: 2 });
    await until(
      "the edit's row",
      () => app.js(`return !!document.querySelector(arguments[0])`, editRow(edit.id)),
      10_000,
    );
    await app.rowMenu(`-${edit.id}`);
    const editItems = await menu("Edit menu");
    for (const want of ["Add to edit…", "Delete edit…"])
      if (!editItems.some((i) => i.startsWith(want)))
        throw new Error(`the edit's menu has ${JSON.stringify(editItems)}, no ${want}`);
    await pick("Edit menu", "Add to edit");
    await until(
      "the edits to pick from",
      () => app.js(`return !!document.querySelector("[role=dialog] .pick")`),
      10_000,
    );
    await app.js(
      `document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }))`,
    );
    await until("it closed", () => app.js(`return !document.querySelector("[role=dialog] .pick")`), 10_000);
    await app.answer("Delete");
    await app.rowMenu(`-${edit.id}`);
    await menu("Edit menu");
    await pick("Edit menu", "Delete edit");
    await until("the edit gone", async () => !(await app.invoke("list_edits")).some((e) => e.id === edit.id), MINUTE);
    const deleted = JSON.parse(await app.js(`return String(window.__e2e.asked.at(-1))`)).message;
    if (!deleted.startsWith(`Delete the edit “${edit.title}”`)) throw new Error(`Delete edit asked: ${deleted}`);
    log(`an edit's menu: ${editItems.join(", ")}; Add to edit… asked which, Delete edit… asked "${deleted}"`);

    // A site's VOD (Dailymotion's, whose video is kept to play here) with its
    // video downloaded: Show in folder on its row and in its menu, as a file's.
    await ctx.quit();
    const kept = path.join(ctx.out, "row-menus", "kept.mp3");
    mkdirSync(path.dirname(kept), { recursive: true });
    copyFileSync(fixtures.landed, kept);
    makeSiteVod(vod.id, "dailymotion", "xe2erow", { mediaPath: kept });
    app = await ctx.start();
    await app.toLibrary();
    await until(
      "the downloaded video's Show in folder on its row",
      () => app.js(`return !!document.querySelector(arguments[0] + ' button[aria-label="Show in folder"]')`, vodRow),
      10_000,
    ).catch(() => {
      throw new Error("a site's VOD with its video downloaded has no Show in folder on its row");
    });
    await app.rowMenu(vod.id);
    const siteItems = await menu("VOD menu");
    for (const want of ["Show in folder", "Open on", "Copy link"])
      if (!siteItems.some((i) => i.startsWith(want)))
        throw new Error(`the downloaded VOD's menu has ${JSON.stringify(siteItems)}, no ${want}`);
    // Its longest item (Check all VODs for VODscriptions) fits: nothing to
    // scroll sideways. A click on the row itself closes the menu, as a press
    // anywhere outside it does, and does nothing else: the row isn't opened
    // or picked. With the OS's mouse where there is one, else as the page
    // gets a click. On macOS the app restarted behind the runner's own
    // window, so this is the click that brings it to the front, which the
    // page never gets: the menu was opened in the back, as a right click
    // there does.
    const sideways = await app.js(
      `const p = document.querySelector('[role=dialog][aria-label="VOD menu"]');
       return p.scrollWidth - p.clientWidth`,
    );
    // Both said, whichever is wrong.
    const wrong = sideways > 0 ? [`the VOD's menu scrolls sideways by ${sideways} px`] : [];
    const rowState = () =>
      app.js(
        `const row = document.querySelector(arguments[0]);
         return (row.classList.contains("selected") ? "open" : "closed") + ", " + (row.getAttribute("aria-selected") === "true" ? "picked" : "not picked")`,
        vodRow,
      );
    const before = await rowState();
    if (before !== "closed, not picked") throw new Error(`before its click, the row is ${before}`);
    if (!os.unavailable()) {
      const r = await os.rectOf(app, `${vodRow} .meta`);
      os.click(r.x + r.w / 2, r.y + r.h / 2, r.scale);
    } else
      await app.js(
        `const row = document.querySelector(arguments[0] + " .meta");
         const box = row.getBoundingClientRect();
         const at = { bubbles: true, cancelable: true, composed: true, button: 0, clientX: box.left + box.width / 2, clientY: box.top + box.height / 2 };
         row.dispatchEvent(new PointerEvent("pointerdown", { ...at, buttons: 1, isPrimary: true }));
         row.dispatchEvent(new MouseEvent("mousedown", { ...at, buttons: 1 }));
         row.dispatchEvent(new PointerEvent("pointerup", { ...at, isPrimary: true }));
         row.dispatchEvent(new MouseEvent("mouseup", at));
         row.dispatchEvent(new MouseEvent("click", at));`,
        vodRow,
      );
    const closed = await until(
      "a click on the row to close its menu",
      () => app.js(`return !document.querySelector('[aria-label="VOD menu"]')`),
      5_000,
    ).catch(() => false);
    if (!closed) wrong.push("a click on the row left its menu open");
    await new Promise((r) => setTimeout(r, 500));
    const after = await rowState();
    if (after !== before) wrong.push(`the click on the row with its menu open left the row ${after}`);
    if (wrong.length) throw new Error(wrong.join("; "));
    log(
      `a downloaded site VOD's menu: ${siteItems.join(", ")}; as wide as its items, closed by a click on the row, which stays ${after}`,
    );

    // A YouTube VOD whose transcription is paused while its audio downloads.
    // With no part file yet: no Show in folder, and the app says so if asked.
    // The reveal itself isn't asked for: it would open the system's file
    // manager on the test's desktop.
    await ctx.quit();
    makeSiteVod(vod.id, "youtube", "noa29qsTc4g");
    const db = new DatabaseSync(libraryPath());
    try {
      db.prepare("DELETE FROM stretches WHERE vod_id = ?").run(vod.id);
      db.prepare("INSERT INTO stretches (vod_id, start, end, status, progress) VALUES (?, 0, NULL, 'paused', 0.3)").run(
        vod.id,
      );
    } finally {
      db.close();
    }
    const hasButton = () =>
      app.js(`return !!document.querySelector(arguments[0] + ' button[aria-label="Show in folder"]')`, vodRow);
    app = await ctx.start();
    await app.toLibrary();
    await until(
      "the paused VOD's row",
      async () => (await app.invoke("list_vods")).find((v) => v.id === vod.id)?.status === "paused",
      10_000,
    );
    await until("its row", () => app.js(`return !!document.querySelector(arguments[0])`, vodRow), 10_000);
    if (await hasButton()) throw new Error("a paused download with no part file has Show in folder on its row");
    await app.rowMenu(vod.id);
    const noPart = await menu("VOD menu");
    await app.key("Escape");
    if (noPart.some((i) => i.startsWith("Show in folder")))
      throw new Error(`a paused download with no part file has Show in folder in its menu: ${JSON.stringify(noPart)}`);
    const refused = await app.invoke("show_file", { id: vod.id }).then(
      () => "shown",
      (e) => e.message,
    );
    if (!refused.includes("That VOD has no file on this computer."))
      throw new Error(`Show in folder with no file answered: ${refused}`);

    // Its part files (yt-dlp's, at the audio's stem in the work folder):
    // Show in folder on its row and in its menu.
    await ctx.quit();
    const work = path.join(dataDirs().at(-1), "work");
    mkdirSync(work, { recursive: true });
    writeFileSync(path.join(work, `${vod.id}.f140.m4a.part`), Buffer.alloc(1000));
    writeFileSync(path.join(work, `${vod.id}.f251.webm.part`), Buffer.alloc(5000));
    app = await ctx.start();
    await app.toLibrary();
    await until("the partial download's Show in folder on its row", hasButton, 10_000).catch(() => {
      throw new Error("a paused download with a part file has no Show in folder on its row");
    });
    await app.rowMenu(vod.id);
    const partItems = await menu("VOD menu");
    await app.key("Escape");
    if (!partItems.some((i) => i.startsWith("Show in folder")))
      throw new Error(`a paused download's menu has ${JSON.stringify(partItems)}, no Show in folder`);
    log("a paused download: no Show in folder without a part file (the app says it has none), and with one");
  },
};
