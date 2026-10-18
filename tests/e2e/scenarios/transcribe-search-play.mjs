// Adding a file, transcribing it, searching, and playing from the result in
// the app's own player: a waveform read from the file, the transcript as captions.

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
      () =>
        app.js(`
          const video = document.querySelector("video, audio");
          const error = [...document.querySelectorAll(".error-text")].map((e) => e.textContent).join(" ");
          if (/couldn't be played|can't be played/.test(error)) throw new Error(error);
          return !!video && video.readyState >= 1 && video.currentTime > 0;`),
      MINUTE,
    );
    app.log("searched and played");
    await checkpoint("search-results-playing");

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
    app.log(`waveform and captions: "${caption}"`);
    await checkpoint("player-caption");
  },
};
