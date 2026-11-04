// Settings are kept: a theme, a video quality and a clip format chosen in
// Settings are still chosen after a restart; Reset settings puts them back.
// So is a note's switch: the note before downloading from a site, stopped
// as 0.7.0 kept it, shows its switch off; turned on, the note comes back on
// a site VOD's download button; turned off, it stays off after a restart,
// and Reset settings turns it on again.
//
// Offline: the VOD is a file added here, made Dailymotion's (no player the
// app can show, so its row downloads its video); nothing is downloaded.

import { writeFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, makeSiteVod } from "../lib/data.mjs";

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

/** The switch of the note before downloading, in Settings › Storage. */
const ASK = "Ask before downloading from a site";
/** Whether that switch is on, or null when it isn't shown. */
const askSwitch = (app) =>
  app.js(
    `const l = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label.toggle')]
       .find((l) => l.textContent.trim() === arguments[0]);
     return l ? l.querySelector("input").checked : null`,
    ASK,
  );
/** Turns that switch to `on`, and waits till Settings shows it and it's saved. */
async function setAsk(app, on) {
  await app.openSettings("Storage");
  await app.js(
    `const l = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label.toggle')]
       .find((l) => l.textContent.trim() === arguments[0]);
     if (!l) throw new Error("no switch " + arguments[0]);
     if (l.querySelector("input").checked !== arguments[1]) l.querySelector("input").click();`,
    ASK,
    on,
  );
  await until(
    `"${ASK}" ${on ? "on" : "off"} and saved`,
    async () => (await askSwitch(app)) === on && (await app.invoke("get_settings")).ask_before_downloading === on,
    10_000,
  );
}
const same = (a, b) => Object.keys(b).every((k) => a[k] === b[k]);

export default {
  name: "settings",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    settings:
      "A Settings dialog over the library: down its left a list of sections (Transcription, Appearance, Controls, Storage, Updates, Privacy, Help) with one of them highlighted, and to the right that section's heading and its options, each with a line of explanation.",
    "settings-help":
      'Settings, in a dark theme, shows its Help section: a link to report a problem at the top, then, set apart under a "Reset and repair" heading like the other subsections, two red-outlined buttons, Reset settings… and Repair VODscribe…, each with a line of explanation under it.',
  },
  async run(app, ctx) {
    await app.toLibrary();
    const { vods } = await app.invoke("add_files", { paths: [ctx.fixtures.landed] });
    const id = vods[0]?.id;
    if (!id) throw new Error("the file wasn't added");
    await ctx.quit();
    makeSiteVod(id, "dailymotion", "xe2easks", { duration: 60 });
    // The note stopped as 0.7.0 kept it (crates/core choices.rs), brought into the settings at start.
    writeFileSync(path.join(dataDirs()[0], "choices.json"), JSON.stringify({ "vodscribe.clipNoteHidden": "true" }));
    app = await ctx.start();
    await app.toLibrary();

    await app.openSettings("Storage");
    await until(
      `"${ASK}" off, as it was stopped`,
      async () =>
        (await askSwitch(app)) === false && (await app.invoke("get_settings")).ask_before_downloading === false,
      10_000,
    );
    // Turned on, the note shows again before the download; Cancel downloads nothing.
    await setAsk(app, true);
    await app.click('[role="dialog"][aria-label="Settings"] button[aria-label="Close"]');
    await app.click(`.vod[data-key="${id}"] button.keep`);
    const note = '[role="dialog"][aria-label="Before downloading the video"]';
    await until(
      "the note before downloading",
      () => app.js(`return !!document.querySelector(arguments[0])`, note),
      10_000,
    );
    await app.js(
      `[...document.querySelectorAll(arguments[0] + " button")].find((b) => b.textContent.trim() === "Cancel").click()`,
      note,
    );
    await until("the note closed", () => app.js(`return !document.querySelector(arguments[0])`, note), 10_000);
    ctx.log(`"${ASK}" came in off, from 0.7.0's choice; turned on, the note showed again`);
    await setAsk(app, false);

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
    await app.openSettings("Storage");
    if ((await askSwitch(app)) !== false) throw new Error(`after a restart, "${ASK}" isn't off`);

    // Reset settings… (Help), answered Reset: every choice as it was at the start.
    await app.answer("Reset");
    await app.openSettings("Help");
    await ctx.checkpoint("settings-help");
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
    if (
      saved.theme !== "system" ||
      saved.video_height !== null ||
      saved.clip_format !== "mp4" ||
      saved.ask_before_downloading !== true
    )
      throw new Error(`after a reset the settings are ${JSON.stringify(saved)}`);
    await app.openSettings("Storage");
    await until(`"${ASK}" on after the reset`, async () => (await askSwitch(app)) === true, 10_000);
    ctx.log("Reset settings put them back, the note's switch on");
  },
};
