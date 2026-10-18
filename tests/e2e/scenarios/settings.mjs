// Settings are kept: a theme, a video quality and a clip format chosen in
// Settings are still chosen after a restart.

import { MINUTE, until } from "../lib/app.mjs";

/** The chosen option of each radio group named in `groups`, as Settings shows them. */
const chosen = (app, groups) =>
  app.js(
    `return Object.fromEntries(arguments[0].map((g) => [g,
       document.querySelector('[role="radiogroup"][aria-label="' + g + '"] [aria-checked="true"]')?.textContent.trim()]))`,
    groups,
  );

const openSettings = async (app) => {
  await app.click('button[aria-label="Settings"]');
  await until("Settings", () => app.js(`return !!document.querySelector('[aria-label="Theme"]')`), MINUTE);
};

const CHOICES = { Theme: "Dark", "Video quality": "480p", "Clip format": "MOV" };
const same = (a, b) => Object.keys(b).every((k) => a[k] === b[k]);

export default {
  name: "settings",
  async run(app, ctx) {
    await app.toLibrary();
    await openSettings(app);
    const before = await chosen(app, Object.keys(CHOICES));
    for (const [group, option] of Object.entries(CHOICES)) {
      if (before[group] === option) throw new Error(`${group} is ${option} already: pick another for the test`);
      await app.js(
        `const b = [...document.querySelectorAll('[role="radiogroup"][aria-label="' + arguments[0] + '"] button')]
           .find((b) => b.textContent.trim() === arguments[1]);
         if (!b) throw new Error("no " + arguments[1] + " in " + arguments[0]);
         b.click();`,
        group,
        option,
      );
    }
    await until(
      "the choices to show",
      async () => {
        const now = await chosen(app, Object.keys(CHOICES));
        if (same(now, CHOICES)) return true;
        throw new Error(`Settings shows ${JSON.stringify(now)}`);
      },
      10_000,
    );
    ctx.log(`chose ${JSON.stringify(CHOICES)} (were ${JSON.stringify(before)})`);

    app = await ctx.restart();
    await app.toLibrary();
    await openSettings(app);
    const after = await chosen(app, Object.keys(CHOICES));
    if (!same(after, CHOICES)) throw new Error(`after a restart, Settings shows ${JSON.stringify(after)}`);
  },
};
