// Keyboard controls are the user's (Settings › Controls): P added to Play or
// pause and Space taken from it, P plays a VOD and Space no longer does, and
// the legend under the player says so; still so after a restart. While a row
// waits for its key, Escape cancels it and leaves Settings open; a key that
// another action has is named ("Used by …") and stays where it was on
// Cancel. Reset all controls… gives Space back.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

const DIALOG = '[role="dialog"][aria-label="Settings"]';
const PLAY = "play.toggle";

/** The key caps on action `id`'s row in Settings › Controls. */
const chips = (app, id) =>
  app.js(
    `return [...document.querySelectorAll('[data-action="' + arguments[0] + '"] .chip kbd')].map((k) => k.textContent.trim())`,
    id,
  );

/** Presses `key` on what has the focus (the row waiting for its key), as a person would. */
const press = (app, key) =>
  app.js(
    `document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: arguments[0], bubbles: true, cancelable: true }))`,
    key,
  );

/** Action `id`'s "+", then the row waiting for a key. */
const record = async (app, id) => {
  await app.click(`[data-add="${id}"]`);
  await until(
    `${id} waiting for a key`,
    () => app.js(`return document.activeElement?.classList.contains("recorder") ?? false`),
    10_000,
  );
};

const closeSettings = async (app) => {
  await app.click(`${DIALOG} button[aria-label="Close"]`);
  await until("Settings closed", () => app.js(`return !document.querySelector(arguments[0])`, DIALOG), 10_000);
};

const playing = (app) => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`);

/** In the open VOD: the legend's keys for play/pause, then P plays and Space doesn't. */
async function pPlays(app, ctx, label) {
  if (!(await app.js(`return !!document.querySelector(".legend")`)))
    await app.click('button[aria-label="Keyboard shortcuts"]');
  const legend = await until(
    "the legend",
    () => app.js(`return document.querySelector(".legend .keys")?.textContent.trim() ?? false`),
    10_000,
  );
  if (legend !== "K or P") throw new Error(`${label}: the legend's play/pause keys are "${legend}", not "K or P"`);
  await app.key("p");
  await until(`${label}: P to play`, () => playing(app), MINUTE);
  await app.pause();
  await app.key(" ");
  await sleep(1500);
  if (await playing(app)) throw new Error(`${label}: Space played it`);
  ctx.log(`${label}: P plays, Space doesn't; the legend says ${legend}`);
}

export default {
  name: "controls",
  async run(app, ctx) {
    await app.toLibrary();
    const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await app.transcribe(added.id);

    await app.openSettings("Controls");
    if ((await chips(app, PLAY)).join() !== "Space,K") throw new Error(`Play or pause has ${await chips(app, PLAY)}`);
    await record(app, PLAY);
    await ctx.checkpoint("controls-recording");
    await press(app, "p");
    await until("P on Play or pause", async () => (await chips(app, PLAY)).includes("P"), 10_000);
    await app.click('button[aria-label="Remove Space from Play or pause"]');
    await until("Space gone", async () => (await chips(app, PLAY)).join() === "K,P", 10_000);
    await ctx.checkpoint("controls-changed");
    await until(
      "the keys saved",
      async () =>
        JSON.stringify((await app.invoke("get_settings")).key_bindings) === JSON.stringify({ [PLAY]: ["k", "p"] }),
      10_000,
    );

    // Escape lets go of a row waiting for a key, and Settings stays open.
    await record(app, "cut.here");
    await press(app, "Escape");
    await until("the row let go", () => app.js(`return !document.querySelector(".recorder")`), 10_000);
    if (!(await app.js(`return !!document.querySelector(arguments[0])`, DIALOG)))
      throw new Error("Escape on a row waiting for a key closed Settings");

    // K is Play or pause's: asked for Cut here, it's named, and Cancel keeps it there.
    await record(app, "cut.here");
    await press(app, "k");
    const clash = await until(
      "the clash named",
      () => app.js(`return document.querySelector(".clash")?.textContent.replace(/\\s+/g, " ").trim() ?? false`),
      10_000,
    );
    if (!clash.includes("Used by Play or pause")) throw new Error(`the clash says "${clash}"`);
    await ctx.checkpoint("controls-clash");
    await app.click(".clash button:last-child");
    await until("the clash gone", () => app.js(`return !document.querySelector(".clash")`), 10_000);
    if ((await chips(app, "cut.here")).includes("K")) throw new Error("Cancel gave K to Cut here");
    if ((await chips(app, PLAY)).join() !== "K,P")
      throw new Error(`after Cancel, Play or pause has ${await chips(app, PLAY)}`);
    // The end of the list: Reset all controls… and the keys that are always the same.
    await app.js(
      `const pane = document.querySelector(arguments[0] + " .pane"); pane.scrollTop = pane.scrollHeight;`,
      DIALOG,
    );
    await ctx.checkpoint("controls-end");
    await closeSettings(app);

    await app.openVod(added.id);
    await pPlays(app, ctx, "changed");

    app = await ctx.restart();
    await app.toLibrary();
    await app.openVod(added.id);
    await pPlays(app, ctx, "after a restart");

    // Reset all controls… (answered Reset): Space is back, and plays.
    await app.openSettings("Controls");
    await app.answer("Reset");
    await app.clickText("Reset all controls…");
    await until("Space back", async () => (await chips(app, PLAY)).join() === "Space,K", 10_000);
    await until(
      "no keys of the user's saved",
      async () => Object.keys((await app.invoke("get_settings")).key_bindings).length === 0,
      10_000,
    );
    await closeSettings(app);
    await app.key(" ");
    await until("Space to play", () => playing(app), MINUTE);
    await app.pause();
    ctx.log("Reset all controls gave Space back");
  },
};
