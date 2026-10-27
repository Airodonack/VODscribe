// The small choices the page remembers (stored.ts) outlast a restart: the
// speed and volume of the players, and the library hidden. An installed
// app's page comes from a port of its own each launch (app_server.rs), so
// what the web view keeps for a page (localStorage) was a new, empty one
// every time; the app keeps them in its data folder instead.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

const SPEED = ".bar button.speed";
const VOLUME = '.bar input[aria-label="Volume"]';
const TOGGLE = "button.library-edge";

/** What the page shows of the choices: the speed, the volume, the library shown. */
const shown = async (app) => {
  const [speed, volume, library] = await app.js(
    `return [
       document.querySelector(arguments[0])?.textContent.trim() ?? null,
       document.querySelector(arguments[1])?.value ?? null,
       document.querySelector(arguments[2])?.getAttribute("aria-expanded") ?? null,
     ]`,
    SPEED,
    VOLUME,
    TOGGLE,
  );
  // In this order, to compare as text (WebDriver's objects come back in any order).
  return { speed, volume, library };
};

export default {
  name: "remembered",
  async run(app, ctx) {
    await app.toLibrary();
    const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await until(
      "its length known",
      async () => (await app.invoke("list_vods")).find((v) => v.id === added.id)?.duration > 3,
      MINUTE,
    );
    await app.openVod(added.id);
    const before = await shown(app);
    if (before.speed !== "1×" || before.library !== "true")
      throw new Error(`a new library starts at ${JSON.stringify(before)}`);

    // Changed through the controls, as a person does.
    await app.click(SPEED);
    await app.js(
      `const input = document.querySelector(arguments[0]);
       input.value = "0.4";
       input.dispatchEvent(new Event("input", { bubbles: true }));`,
      VOLUME,
    );
    await app.click(TOGGLE);
    const want = { speed: "1.25×", volume: "0.4", library: "false" };
    await until(
      "the choices shown",
      async () => {
        const now = await shown(app);
        if (JSON.stringify(now) === JSON.stringify(want)) return true;
        throw new Error(`the page shows ${JSON.stringify(now)}`);
      },
      10_000,
    );
    ctx.log(`set: ${JSON.stringify(want)}`);
    // Written by the app one at a time after the page shows them (stored.ts):
    // the restart waits for the last, as a quit that soon would lose it.
    const kept = { "vodscribe.speed": "1.25", "vodscribe.volume": "0.4", "vodscribe.library": "hidden" };
    await until(
      "the choices kept",
      async () => {
        const all = await app.invoke("kept_choices");
        return Object.entries(kept).every(([key, value]) => all[key] === value);
      },
      10_000,
    );

    // Restarted: the same choices, in the same VOD's player.
    app = await ctx.restart();
    await app.toLibrary();
    await app.openVod(added.id);
    const after = await shown(app);
    if (JSON.stringify(after) !== JSON.stringify(want))
      throw new Error(`after a restart: ${JSON.stringify(after)}, not ${JSON.stringify(want)}`);
    ctx.log("the same after a restart");
  },
};
