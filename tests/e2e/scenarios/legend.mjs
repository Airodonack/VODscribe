// The keyboard legend (the keyboard button in a player's header) is a cheat
// sheet under the controls, in four short columns (keymap.ts LEGEND_GROUPS):
// opening it leaves the picture where it was; its headings come in order,
// the same in a VOD and an edit (whose own keys join Cutting); it fits in
// the window under the controls; a key taken away in Settings goes from it;
// All controls opens Settings on Controls; full screen hides it; and left
// open, it's open again after a restart, in a VOD and in an edit. In a
// window too short for all of it (a small screen's: CI's runners clamp the
// window to 1024 by 768), the legend takes the room left and scrolls inside
// itself, still within the window, and the picture still doesn't move.

import path from "node:path";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

const DIALOG = '[role="dialog"][aria-label="Settings"]';
const BUTTON = 'button[aria-label="Keyboard shortcuts"]';
// The legend's columns, in order (keymap.ts LEGEND_GROUPS).
const GROUPS = ["Playing", "Cutting", "Marks", "More"];
/** The most lines a column has, so the legend fits under the controls. */
const MOST = 8;

/** Where the picture's stage is in the window, rounded to the pixel. */
const stageBox = (app) =>
  app.js(`const r = document.querySelector(".stage").getBoundingClientRect();
          return [r.x, r.y, r.width, r.height].map(Math.round).join(",");`);

/** The legend's group headings, in order, or false while there's none. */
const headings = (app) =>
  app.js(`const legend = document.querySelector(".legend");
          return !!legend && [...legend.querySelectorAll(".group .heading")].map((h) => h.textContent.trim());`);

/** The keys the legend shows for what it calls `does`, or "" if it shows none. */
const keysFor = (app, does) =>
  app.js(
    `const row = [...document.querySelectorAll(".legend .row")].find((r) => r.querySelector(".does")?.textContent.trim() === arguments[0]);
     return row?.querySelector(".keys").textContent.replace(/\\s+/g, " ").trim() ?? "";`,
    does,
  );

/** Checks the legend fits in the window under the controls, the page
 *  scrolled to the top, and that no column is longer than MOST lines. */
async function fits(app, label) {
  const { bottom, height, lines, shown } = await app.js(`
    for (let el = document.querySelector(".legend"); el; el = el.parentElement) el.scrollTop = 0;
    const legend = document.querySelector(".legend");
    return {
      bottom: Math.round(legend.getBoundingClientRect().bottom),
      height: window.innerHeight,
      lines: [...legend.querySelectorAll(".group")].map((g) => g.querySelectorAll(".row").length),
      shown: legend.clientHeight + " of " + legend.scrollHeight,
    };`);
  if (bottom > height) throw new Error(`${label}: the legend ends at ${bottom}, below the window's ${height}`);
  if (Math.max(...lines) > MOST) throw new Error(`${label}: a column has ${Math.max(...lines)} lines (${lines})`);
  return `ends at ${bottom} of ${height}, lines ${lines}, ${shown} shown`;
}

/** The window's page sizes the legend is checked at besides the window's
 *  own: those CI's Windows and macOS runners gave the 1200 by 800 window. */
const SHORT = [
  [1024, 749],
  [1024, 645],
];

/** The page's size now, as resizeInner gives it. */
const pageSize = (app) => app.js(`return { width: innerWidth, height: innerHeight }`);

/** Sizes the window's page to `width` by `height`, asking once more for what
 *  the window manager added or took (macOS counts the title bar in); returns
 *  the size it got, as "WxH". A screen too small for the size asked gives
 *  less (a 1024 by 768 runner's has no 749 px page under the menu bar and
 *  title bar): that's the size tested, and said. Only a page that didn't
 *  change at all, asked for another size, fails. */
async function resize(app, log, width, height) {
  const was = await pageSize(app);
  let got = await resizeInner(app, width, height);
  if (got.width !== width || got.height !== height)
    got = await resizeInner(app, 2 * width - got.width, 2 * height - got.height);
  const asked = `${width}x${height}`;
  const size = `${got.width}x${got.height}`;
  if (Math.abs(got.width - width) <= 1 && Math.abs(got.height - height) <= 1) return asked;
  if (got.width === was.width && got.height === was.height)
    throw new Error(`the window's page stayed ${size}, asked for ${asked}`);
  log(`asked ${asked}, got ${size}`);
  return size;
}

/** Checks the legend, in a window too short for all of it, is a box that
 *  ends within the window and scrolls, its first line in view. */
async function scrolls(app, label) {
  const got = await app.js(`
    for (let el = document.querySelector(".legend"); el; el = el.parentElement) el.scrollTop = 0;
    const legend = document.querySelector(".legend");
    const box = legend.getBoundingClientRect();
    const first = legend.querySelector(".row .does").getBoundingClientRect();
    return {
      bottom: Math.round(box.bottom),
      height: innerHeight,
      scroll: legend.scrollHeight,
      client: legend.clientHeight,
      first: first.top >= box.top && first.bottom <= box.bottom,
    };`);
  if (got.bottom > got.height)
    throw new Error(`${label}: the legend ends at ${got.bottom}, below the window's ${got.height}`);
  if (!(got.scroll > got.client))
    throw new Error(`${label}: the legend doesn't scroll (${got.scroll} high, ${got.client} shown)`);
  if (!got.first) throw new Error(`${label}: the legend's first line isn't in view`);
  return `${got.client} of ${got.scroll} shown, ends at ${got.bottom} of ${got.height}`;
}

const same = (label, got, want) => {
  if (JSON.stringify(got) !== JSON.stringify(want))
    throw new Error(`${label}: ${JSON.stringify(got)}, not ${JSON.stringify(want)}`);
};

export default {
  name: "legend",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "legend-vod":
      "A VOD's player: under its timeline and bar of buttons, a cheat sheet of keyboard keys in four columns of similar length, under small uppercase headings Playing, Cutting, Marks and More, each line a key cap or two and a short phrase such as play/pause or cut here, and a link All controls › at the end of More; all of it within the window. The picture sits above the controls, not pushed down.",
    "legend-edit":
      "An edit's player: under its timeline and buttons, the same four columns (Playing, Cutting, Marks, More), Cutting with the edit's own keys (move the piece, remove the stretch or piece, render), and All controls › at the end of More.",
  },
  async run(app, ctx) {
    await app.toLibrary();
    const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await until(
      "its length known",
      async () => (await app.invoke("list_vods")).find((v) => v.id === added.id)?.duration > 3,
      MINUTE,
    );
    await app.openVod(added.id);
    if (await app.js(`return !!document.querySelector(".legend")`))
      throw new Error("the legend is open before it was ever opened");

    // Opened: the picture stays where it was.
    const before = await stageBox(app);
    await app.click(BUTTON);
    await until("the legend", () => app.js(`return !!document.querySelector(".legend")`), 10_000);
    same("the stage with the legend open", await stageBox(app), before);
    same("the VOD's headings", await headings(app), GROUPS);
    const vodFit = await fits(app, "the VOD");
    const play = await keysFor(app, "play/pause");
    if (play !== "Space or K") throw new Error(`the legend's play/pause keys are "${play}"`);
    await ctx.checkpoint("legend-vod");
    await until("the legend's being open saved", async () => (await app.invoke("get_settings")).show_keys, 10_000);
    ctx.log(`open: the stage stayed at ${before}; groups ${GROUPS.join(", ")}; ${vodFit}`);

    // Shorter windows: the legend scrolls within what's left, and opening or
    // closing it still leaves the picture where it was.
    const { width, height } = await pageSize(app);
    for (const [w, h] of SHORT) {
      const at = await resize(app, ctx.log, w, h);
      const fit = await scrolls(app, `the VOD at ${at}`);
      const open = await stageBox(app);
      await app.click(BUTTON);
      await until("the legend closed", () => app.js(`return !document.querySelector(".legend")`), 10_000);
      same(`the stage at ${at} with the legend closed`, await stageBox(app), open);
      await app.click(BUTTON);
      await until("the legend", () => app.js(`return !!document.querySelector(".legend")`), 10_000);
      same(`the stage at ${at} with the legend open again`, await stageBox(app), open);
      ctx.log(`at ${at}: the legend ${fit}; the stage stayed at ${open}`);
    }
    await resize(app, ctx.log, width, height);
    await until("the legend's being open saved", async () => (await app.invoke("get_settings")).show_keys, 10_000);

    // All controls › opens Settings on Controls; B taken from Cut here, the
    // legend shows the key left (Ctrl+B, ⌘B).
    await app.clickText("All controls ›");
    await until(
      "Settings on Controls",
      () =>
        app.js(
          `return document.querySelector(arguments[0] + ' nav button[aria-current="true"]')?.textContent.trim() === "Controls"`,
          DIALOG,
        ),
      10_000,
    );
    await app.click('button[aria-label="Remove B from Cut here"]');
    await app.click(`${DIALOG} button[aria-label="Close"]`);
    await until("Settings closed", () => app.js(`return !document.querySelector(arguments[0])`, DIALOG), 10_000);
    const cut = process.platform === "darwin" ? "⌘B" : "Ctrl+B";
    await until("the rebound key in the legend", async () => (await keysFor(app, "cut here")) === cut, 10_000);
    ctx.log(`All controls opened Settings › Controls; cut here shows ${cut}`);

    // Full screen hides it; out of full screen, it's back.
    const shown = () => app.js(`const l = document.querySelector(".legend"); return !!l && l.offsetParent !== null`);
    await app.key("f");
    await until("full screen", () => app.js(`return !!document.querySelector("section.fullscreen")`), 10_000);
    if (await shown()) throw new Error("the legend shows in full screen");
    await app.key("Escape");
    await until("out of full screen", () => app.js(`return !document.querySelector("section.fullscreen")`), 10_000);
    await until("the legend back", shown, 10_000);

    // Left open: open again after a restart, in the VOD and in an edit.
    const edit = await app.invoke("create_edit");
    await app.invoke("add_range_to_edit", { editId: edit.id, vodId: added.id, start: 1, end: 3, title: "A piece" });
    app = await ctx.restart();
    await app.toLibrary();
    await app.openVod(added.id);
    same("the VOD's headings after a restart", await until("the legend", () => headings(app), 10_000), GROUPS);
    await app.click(`.vod.edit[data-key="-${edit.id}"] button.main`);
    await until("the edit open", () => app.js(`return !!document.querySelector("section.edit-view")`), MINUTE);
    same("the edit's headings", await until("the legend", () => headings(app), 10_000), GROUPS);
    const render = await keysFor(app, "render");
    if (render !== "E") throw new Error(`the edit's legend has "${render}" for render`);
    const editFit = await fits(app, "the edit");
    ctx.log(`the edit's legend ${editFit}`);
    const { width: editWidth, height: editHeight } = await pageSize(app);
    for (const [w, h] of SHORT) {
      const at = await resize(app, ctx.log, w, h);
      ctx.log(`at ${at}: the edit's legend ${await scrolls(app, `the edit at ${at}`)}`);
    }
    await resize(app, ctx.log, editWidth, editHeight);
    await ctx.checkpoint("legend-edit");

    // Closed, it's saved closed.
    const editBefore = await stageBox(app);
    await app.click(`section.edit-view ${BUTTON}`);
    await until("the legend closed", () => app.js(`return !document.querySelector(".legend")`), 10_000);
    same("the edit's stage with the legend closed", await stageBox(app), editBefore);
    await until(
      "the legend's being closed saved",
      async () => (await app.invoke("get_settings")).show_keys === false,
      10_000,
    );
    ctx.log("open again after a restart, in the VOD and the edit; closed, saved closed");
  },
};
