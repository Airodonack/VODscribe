// Adding a file, transcribing it, searching, and playing from the result in
// the app's own player: a waveform read from the file, the transcript as
// captions. Then a transcript long enough to scroll, followed as it plays.

import path from "node:path";

import { isUnplayable } from "../../../apps/desktop/src/lib/media.ts";
import { MINUTE, until } from "../lib/app.mjs";
import { longSpeech } from "../lib/fixtures.mjs";

export default {
  name: "transcribe-search-play",
  async run(app, { fixtures, out, checkpoint }) {
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

    // Following along: the file's transcript is a line or two, so ten minutes
    // of the landing, over and over, gives one to scroll.
    await app.type('input[placeholder="Search for a phrase…"]', "");
    await app.pickFiles([longSpeech(path.join(out, "fixtures"))]);
    await app.clickText("add video files…");
    const long = await until(
      "the ten minutes transcribed",
      async () => {
        const v = (await app.invoke("list_vods")).find((v) => v.id !== done.id);
        if (v?.status === "error") throw new Error(`transcription failed: ${v.error}`);
        return v?.status === "done" && v;
      },
      10 * MINUTE,
    );
    await app.openVod(long.id);
    await until(
      "the long transcript",
      () => app.js(`return document.querySelectorAll("#transcript .line button.row").length > 10`),
      MINUTE,
    );
    // Where the line playing is, next to the view below the transcript's header, and the follow button.
    const following = () =>
      app.js(`
        const body = document.querySelector(".body");
        const head = body.querySelector(".layout > .head");
        const view = body.getBoundingClientRect();
        const top = view.top + (head?.offsetHeight ?? 0);
        const line = document.querySelector("#transcript .line.now");
        const box = line?.getBoundingClientRect();
        return {
          at: line?.querySelector(".row .time")?.textContent ?? null,
          inView: !!box && box.bottom > top + 4 && box.top < view.bottom - 4,
          button: document.querySelector("#transcript button.follow")?.textContent.trim() ?? null,
        };`);
    // Playing from the first line, then a click halfway along the timeline:
    // the transcript follows the jump.
    await app.click("#transcript .line button.row");
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    await app.js(
      `const box = document.querySelector(".scrubber canvas").getBoundingClientRect();
       const at = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1 };
       const el = document.elementFromPoint(at.clientX, at.clientY);
       el.dispatchEvent(new PointerEvent("pointerdown", at));
       el.dispatchEvent(new PointerEvent("pointerup", at));`,
    );
    const halfway = await until(
      "the line playing halfway in view",
      async () => {
        const f = await following();
        const [m] = (f.at ?? "0:00").split(":").map(Number);
        return f.inView && m >= 4 && !f.button && f;
      },
      15_000,
    );
    app.log(`following: the line at ${halfway.at} is in view`);
    // Scrolled away (by script: no wheel or keys), it follows no more, and says so.
    await app.js(`document.querySelector(".body").scrollTop = 0`);
    const away = await until(
      "the follow button",
      async () => {
        const f = await following();
        return f.button && f;
      },
      10_000,
    );
    if (away.inView) throw new Error("scrolled to the top, the line playing is still in view");
    if (away.button !== "Follow along") throw new Error(`the follow button says "${away.button}"`);
    await checkpoint("transcript-not-following");
    await app.click("#transcript button.follow");
    const back = await until(
      "following again",
      async () => {
        const f = await following();
        return f.inView && !f.button && f;
      },
      10_000,
    );
    app.log(`scrolled away, then Follow along: the line at ${back.at} is in view again`);
    await checkpoint("transcript-following");
  },
};
