// Adding a file, transcribing it, searching, and playing from the result in
// the app's own player: a waveform read from the file, the transcript as captions.

import { isUnplayable } from "../../../apps/desktop/src/lib/media.ts";
import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "transcribe-search-play",
  async run(app, { fixtures, checkpoint }) {
    await app.toLibrary();
    const done = await app.addAndTranscribe(fixtures.landed);
    const text = (await app.invoke("transcript", { vodId: done.id })).map((l) => l.text).join(" ");
    if (!/landed/i.test(text)) throw new Error(`the transcript doesn't have "landed": ${text}`);
    app.log(`transcribed: "${text}"`);

    await app.type('input[placeholder="Search for a phrase…"]', "landed");
    await until("a search result", () => app.js(`return !!document.querySelector("button.row")`), MINUTE);
    await app.click("button.row");
    await until(
      "the player",
      async () => {
        const { error, playing } = await app.js(`
          const video = document.querySelector("video, audio");
          return {
            error: [...document.querySelectorAll(".error-text")].map((e) => e.textContent).join(" "),
            playing: !!video && video.readyState >= 1 && video.currentTime > 0,
          };`);
        if (isUnplayable(error)) throw new Error(error);
        return playing;
      },
      MINUTE,
    );
    app.log("searched and played");
    await checkpoint("search-results-playing");
    // "landed" is in the file's last second, where WebKitGTK (2.50) can stall
    // a file for good: it plays to the end all the same (LocalPlayer.svelte).
    await until(
      "it to play to the end",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      15_000,
    );
    app.log("played to the end");

    const levels = await app.js(
      `return window.__TAURI_INTERNALS__.invoke("peaks", { id: arguments[0] }).then((b) => new Uint8Array(b).length)`,
      done.id,
    );
    if (levels < 20) throw new Error(`the waveform has ${levels} levels, for a file of ${done.duration} s`);
    const [cue] = await app.invoke("captions", { vodId: done.id });
    const middle = (cue.start + cue.end) / 2;
    await app.js(`const v = document.querySelector(".player video"); v.pause(); v.currentTime = arguments[0];`, middle);
    const caption = await until(
      "a caption",
      () => app.js(`return document.querySelector(".player .caption")?.textContent.trim()`),
      MINUTE,
    );
    if (caption !== cue.text) throw new Error(`the caption shows "${caption}", not "${cue.text}"`);
    // Readable on what's behind it, in each theme (a sound-only file's
    // caption sits on the panel, not on a video's dark stage).
    for (const theme of ["Light", "Dark"]) {
      await app.click('button[aria-label="Settings"]');
      await until("Settings", () => app.js(`return !!document.querySelector('[aria-label="Theme"]')`), MINUTE);
      await app.js(
        `[...document.querySelectorAll('[role="radiogroup"][aria-label="Theme"] button')].find((b) => b.textContent.trim() === arguments[0]).click()`,
        theme,
      );
      await app.key("Escape");
      await until(
        `the ${theme.toLowerCase()} theme`,
        async () => (await app.invoke("get_settings")).theme === theme.toLowerCase(),
        10_000,
      );
      const contrast = await until("the caption", () => app.contrast(".player .caption"), 10_000);
      app.log(`${theme}: the caption's contrast is ${contrast.toFixed(1)}:1`);
      if (contrast < 4.5)
        throw new Error(
          `in the ${theme.toLowerCase()} theme the caption's contrast is ${contrast.toFixed(2)}:1, under 4.5:1`,
        );
    }
    app.log(`waveform and captions: "${caption}"`);
    await checkpoint("player-caption");
  },
};
