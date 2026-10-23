// Settings are kept: a theme, a video quality and a clip format chosen in
// Settings are still chosen after a restart; Reset settings puts them back.

import { MINUTE, until } from "../lib/app.mjs";

/** The section of Settings each radio group is in. */
const SECTION = { Theme: "Appearance", "Video quality": "Storage", "Clip format": "Storage" };

/** The chosen option of each radio group named in `groups`, as Settings shows them, each in its section. */
const chosen = async (app, groups) => {
  const shown = {};
  for (const g of groups) {
    await app.openSettings(SECTION[g]);
    shown[g] = await app.js(
      `return document.querySelector('[role="radiogroup"][aria-label="' + arguments[0] + '"] [aria-checked="true"]')?.textContent.trim()`,
      g,
    );
  }
  return shown;
};

const openSettings = async (app) => {
  await app.openSettings("Appearance");
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
    await ctx.checkpoint("settings");
    for (const [group, option] of Object.entries(CHOICES)) {
      if (before[group] === option) throw new Error(`${group} is ${option} already: pick another for the test`);
      await app.openSettings(SECTION[group]);
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
    // Shown at once, saved a moment later: restart once they're saved (the
    // saves go one after another; quitting at once could beat the last).
    await until(
      "the choices saved",
      async () => {
        const s = await app.invoke("get_settings");
        return s.theme === "dark" && s.video_height === 480 && s.clip_format === "mov";
      },
      10_000,
    );

    app = await ctx.restart();
    await app.toLibrary();
    await openSettings(app);
    const after = await chosen(app, Object.keys(CHOICES));
    await ctx.checkpoint("settings-dark");
    if (!same(after, CHOICES)) throw new Error(`after a restart, Settings shows ${JSON.stringify(after)}`);

    // Reset settings… (Help), answered Reset: every choice as it was at the start.
    await app.answer("Reset");
    await app.openSettings("Help");
    await app.clickText("Reset settings…");
    await until(
      "the choices back as they started",
      async () => {
        const now = await chosen(app, Object.keys(CHOICES));
        if (same(now, before)) return true;
        throw new Error(`Settings shows ${JSON.stringify(now)}, not ${JSON.stringify(before)}`);
      },
      10_000,
    );
    const saved = await app.invoke("get_settings");
    if (saved.theme !== "system" || saved.video_height !== null || saved.clip_format !== "mp4")
      throw new Error(`after a reset the settings are ${JSON.stringify(saved)}`);
    ctx.log("Reset settings put them back");
  },
};
