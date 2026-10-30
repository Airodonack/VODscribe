// Adding a file, transcribing it, searching, and playing from the result in
// the app's own player: a waveform read from the file, the transcript as
// captions. Then a transcript long enough to scroll, following the playhead
// from when it opens, paused or playing, and its lines' times marked on the
// timeline as the pointer goes over them.

import path from "node:path";

import { isUnplayable } from "../../../apps/desktop/src/lib/media.ts";
import { MINUTE, until } from "../lib/app.mjs";
import { longSpeech } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

export default {
  name: "transcribe-search-play",
  async run(app, { fixtures, out, checkpoint }) {
    await app.toLibrary();
    const done = await app.addAndTranscribe(fixtures.landed);
    const text = (await app.invoke("transcript", { vodId: done.id })).map((l) => l.text).join(" ");
    if (!/landed/i.test(text)) throw new Error(`the transcript doesn't have "landed": ${text}`);
    app.log(`transcribed: "${text}"`);

    await app.type('input[placeholder="Search for a phrase…"]', "landed");
    // The results' rows, not the transcript's (both are button.row): the
    // transcript stays up until the search answers, a moment on a slow computer.
    await until("a search result", () => app.js(`return !!document.querySelector(".results button.row")`), MINUTE);
    await app.click(".results button.row");
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
      await app.openSettings("Appearance");
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
    const [added] = await app.addFiles([longSpeech(path.join(out, "fixtures"))]);
    const long = await app.transcribe(added.id, { label: "the ten minutes" });
    await app.openVod(long.id);
    await until(
      "the long transcript",
      () => app.js(`return document.querySelectorAll("#transcript .line button.row").length > 10`),
      MINUTE,
    );
    // Where the line at the playhead is, next to the view below the transcript's header, and the follow button.
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
          whole: !!box && box.top >= top - 1 && box.bottom <= view.bottom + 1,
          button: document.querySelector("#transcript button.follow")?.textContent.trim() ?? null,
          paused: !!document.querySelector('.bar button[aria-label="Play"]'),
        };`);
    const minutes = (at) => (at ?? "0:00").split(":").reduce((total, part) => total * 60 + Number(part), 0) / 60;
    // Opened, it follows already, before anything plays: the line at the
    // playhead (the start) lit and in view.
    const opened = await until(
      "the first line lit and in view",
      async () => {
        const f = await following();
        return f.whole && !f.button && f;
      },
      15_000,
    );
    if (!opened.paused) throw new Error("the VOD plays as it opens");
    if (minutes(opened.at) >= 1) throw new Error(`opened at the start, the line lit is the one at ${opened.at}`);
    app.log(`opened: following, the line at ${opened.at} lit and in view`);
    // Paused, a click halfway along the timeline: the transcript follows the jump.
    await app.js(
      `const box = document.querySelector(".scrubber canvas").getBoundingClientRect();
       const at = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1 };
       const el = document.elementFromPoint(at.clientX, at.clientY);
       el.dispatchEvent(new PointerEvent("pointerdown", at));
       el.dispatchEvent(new PointerEvent("pointerup", at));`,
    );
    const halfway = await until(
      "the line halfway lit and in view",
      async () => {
        const f = await following();
        return f.whole && minutes(f.at) >= 4 && !f.button && f;
      },
      15_000,
    );
    if (!halfway.paused) throw new Error("a click on the timeline while paused started it playing");
    app.log(`paused, the timeline clicked halfway: the line at ${halfway.at} is lit and in view`);
    // Scrolled away with the wheel, it follows no more, and says so.
    await app.js(`
      const body = document.querySelector(".body");
      body.dispatchEvent(new WheelEvent("wheel", { deltaY: -body.scrollTop, bubbles: true }));
      body.scrollTop = 0;`);
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
        return f.whole && !f.button && f;
      },
      10_000,
    );
    if (back.at !== halfway.at) throw new Error(`Follow along went to the line at ${back.at}, not ${halfway.at}`);
    app.log(`scrolled away, then Follow along: the line at ${back.at} is in view again`);
    await checkpoint("transcript-following");
    // A search match far from there (near 1:30), double-clicked (its clicks
    // play it): the transcript at its line, still following.
    await app.type('input[placeholder="Search for a phrase…"]', "landed");
    await until(
      "search results",
      () => app.js(`return document.querySelectorAll(".results button.row").length > 2`),
      MINUTE,
    );
    const hit = await app.js(
      `const minutes = (at) => at.split(":").reduce((total, part) => total * 60 + Number(part), 0) / 60;
       const off = (row) => Math.abs(minutes(row.querySelector(".time").textContent) - 1.5);
       const row = [...document.querySelectorAll(".results button.row")].reduce((a, b) => (off(b) < off(a) ? b : a));
       row.click();
       row.click();
       row.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, detail: 2 }));
       return row.querySelector(".time").textContent;`,
    );
    if (Math.abs(minutes(hit) - minutes(halfway.at)) < 1) throw new Error(`the search match near 1:30 is at ${hit}`);
    // The match's time is its word's; its line starts a little before, and
    // as it plays on, the next lines take over.
    const near = (at) => {
      const d = (minutes(at) - minutes(hit)) * 60;
      return d > -10 && d < 20;
    };
    const shown = await until(
      "the search match's line lit and in view",
      async () => {
        const f = await following();
        return near(f.at) && f.whole && !f.button && f;
      },
      15_000,
    );
    // Once the transcript is done placing it (two seconds), it still follows.
    await new Promise((r) => setTimeout(r, 2500));
    const after = await following();
    if (after.button) throw new Error("after a search match was shown, the transcript stopped following");
    if (!after.whole) throw new Error(`after a search match was shown, the line at ${after.at} is out of view`);
    app.log(`a search match at ${shown.at} double-clicked: its line lit and in view, following`);
    await checkpoint("transcript-search-match");
    // Paused, the search let go of: the transcript opens again (after the
    // results, a moment), at the playhead's line from the start.
    await app.pause();
    const paused = await following();
    await app.type('input[placeholder="Search for a phrase…"]', "");
    await until(
      "the transcript again, at the playhead's line",
      async () => {
        const f = await following();
        return f.at === paused.at && f.whole && !f.button && f;
      },
      15_000,
    );
    // And it stays there as the rows around it are measured.
    await new Promise((r) => setTimeout(r, 1000));
    const settled = await following();
    if (!settled.whole || settled.button)
      throw new Error(`the transcript opened again moved the line at ${settled.at} out of view`);
    app.log(`the search let go of: the transcript again, the line at ${paused.at} lit and in view`);

    // Pressing on a line and dragging across two more selects the three.
    await app.pause();
    const dragged = await app.js(`
      const body = document.querySelector(".body").getBoundingClientRect();
      const rows = [...document.querySelectorAll("#transcript .line button.row")];
      const i = rows.findIndex((b) => b.getBoundingClientRect().top > body.top + 120);
      const [first, last] = [rows[i], rows[i + 2]];
      if (!first || !last) return null;
      // The button held from the press to the release, as a real pointer has it.
      const at = (el, buttons) => {
        const r = el.getBoundingClientRect();
        return { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0, buttons, bubbles: true, pointerId: 1 };
      };
      first.dispatchEvent(new PointerEvent("pointerdown", at(first, 1)));
      window.dispatchEvent(new PointerEvent("pointermove", at(last, 1)));
      window.dispatchEvent(new PointerEvent("pointerup", at(last, 0)));
      return [Number(first.dataset.time), Number(last.dataset.time)];`);
    if (!dragged) throw new Error("no three lines in the transcript's view to drag across");
    const picked = await until(
      "the lines dragged across selected",
      async () => {
        const times = await app.js(
          `return [...document.querySelectorAll("#transcript .line.picked button.row")].map((b) => Number(b.dataset.time))`,
        );
        return times.length >= 3 && times;
      },
      10_000,
    );
    if (picked.length !== 3 || picked[0] !== dragged[0] || picked[2] !== dragged[1])
      throw new Error(`dragged across the lines at ${dragged.join(" to ")} s, selected those at ${picked.join(", ")}`);
    app.log(`dragged across three lines: ${picked.join(", ")} s selected`);
    // A click on a line lets go of the selection.
    await app.click("#transcript .line button.row");
    await until(
      "the selection let go of",
      () => app.js(`return !document.querySelector("#transcript .line.picked")`),
      10_000,
    );

    // The pointer over a line marks its time on the timeline, and over the
    // timeline, its moment on the transcript's minimap (moving the OS's
    // pointer is Linux-only, so far: hover needs a real pointer).
    if (process.platform !== "linux" || os.unavailable()) {
      app.log(`hover marks not checked: ${os.unavailable() ?? "the OS pointer moves on Linux only, so far"}`);
      return;
    }
    await app.pause();
    // A line well inside the view, under no button.
    const line = await app.js(`
      const body = document.querySelector(".body").getBoundingClientRect();
      const row = [...document.querySelectorAll("#transcript .line button.row")].find((b) => {
        const r = b.getBoundingClientRect();
        return r.top > body.top + 120 && r.bottom < body.bottom - 80;
      });
      return row && { index: row.closest(".line").dataset.index, time: Number(row.dataset.time) };`);
    if (!line) throw new Error("no line well inside the transcript's view");
    const row = await os.rectOf(app, `#transcript .line[data-index="${line.index}"] button.row`);
    os.activate();
    await os.gesture([["move", row.x + Math.round(row.w / 2), row.y + Math.round(row.h / 2)]]);
    const marked = await until(
      "the line's time marked on the timeline",
      () =>
        app.js(
          `
          const bar = document.querySelector(".scrubber");
          const mark = bar.querySelector(".pointed");
          if (!mark) return false;
          const box = bar.getBoundingClientRect();
          const r = mark.getBoundingClientRect();
          const want = ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
          return { x: r.left + r.width / 2 - box.left, want };`,
          line.time,
        ),
      10_000,
    );
    if (Math.abs(marked.x - marked.want) > 3)
      throw new Error(
        `the line at ${line.time} s is marked at ${marked.x.toFixed(1)} px, not ${marked.want.toFixed(1)}`,
      );
    app.log(`a line hovered: marked on the timeline at ${marked.x.toFixed(1)} px (${line.time} s)`);
    await checkpoint("line-marked-on-timeline");
    // Over the timeline, a quarter of the way along: the minimap marks that moment.
    const bar = await os.rectOf(app, ".scrubber canvas");
    await os.gesture([["move", bar.x + Math.round(bar.w / 4), bar.y + Math.round(bar.h / 2)]]);
    const onMap = await until(
      "the timeline's moment marked on the minimap",
      () =>
        app.js(
          `
          if (document.querySelector(".scrubber .pointed")) return false;
          const map = document.querySelector(".minimap");
          const mark = map?.querySelector(".pointed");
          const tip = document.querySelector(".timeline .tip")?.textContent.trim();
          if (!mark || !tip) return false;
          const at = tip.split(":").reduce((total, part) => total * 60 + Number(part), 0);
          const box = map.getBoundingClientRect();
          const y = mark.getBoundingClientRect().top - box.top;
          return { y, want: (at / arguments[0]) * box.height, at, slack: box.height / arguments[0] };`,
          long.duration,
        ),
      10_000,
    );
    // Within a few pixels, and the tip's whole second.
    if (Math.abs(onMap.y - onMap.want) > 3 + onMap.slack)
      throw new Error(
        `the timeline at ${onMap.at} s is marked on the minimap at ${onMap.y.toFixed(1)} px, not ${onMap.want.toFixed(1)}`,
      );
    app.log(`the timeline hovered: marked on the minimap at ${onMap.y.toFixed(1)} px (${onMap.at} s)`);
    // Gone as the pointer leaves.
    const list = await os.rectOf(app, ".toolbar");
    await os.gesture([["move", list.x + 4, list.y + Math.round(list.h / 2)]]);
    await until(
      "the marks gone",
      () => app.js(`return !document.querySelector(".scrubber .pointed, .minimap .pointed")`),
      10_000,
    );
  },
};
