// Every library row has the same actions, in a right-click menu (actions.ts,
// Menu.svelte). A file's row has Show in folder among its buttons, as an
// edit's row does once rendered. Its menu opens with the focus on its first
// item, the arrows move it, Escape closes it and gives the focus back; it
// has Show in folder, Add to edit… and Remove…, which asks as removing
// several does, saying the transcript is erased. An edit's row has a menu
// too: Add to edit… asks which edit, and Delete edit… asks before it goes.

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "row-menus",
  async run(app, { fixtures, log }) {
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

    const openMenu = (row) =>
      app.js(
        `const row = document.querySelector(arguments[0]);
         row.querySelector("button.main").focus();
         const box = row.getBoundingClientRect();
         row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2,
           clientX: box.left + 20, clientY: box.top + 10 }));`,
        row,
      );
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
    await openMenu(vodRow);
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
    await openMenu(vodRow);
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
    await openMenu(editRow(edit.id));
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
    await openMenu(editRow(edit.id));
    await menu("Edit menu");
    await pick("Edit menu", "Delete edit");
    await until("the edit gone", async () => !(await app.invoke("list_edits")).some((e) => e.id === edit.id), MINUTE);
    const deleted = JSON.parse(await app.js(`return String(window.__e2e.asked.at(-1))`)).message;
    if (!deleted.startsWith(`Delete the edit “${edit.title}”`)) throw new Error(`Delete edit asked: ${deleted}`);
    log(`an edit's menu: ${editItems.join(", ")}; Add to edit… asked which, Delete edit… asked "${deleted}"`);
  },
};
