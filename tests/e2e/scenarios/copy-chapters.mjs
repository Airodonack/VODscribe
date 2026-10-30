// Copy chapters, in the chapters pane's header as in a chapter's title box:
// both put the same text on the clipboard, each chapter's time and title a
// line, as a YouTube video's description lists them. What's copied is read
// from the clipboard command as the page sends it (as copy-moment.mjs does).

import { MINUTE, resizeInner, until } from "../lib/app.mjs";

export default {
  name: "copy-chapters",
  async run(app, { fixtures, log }) {
    await app.toLibrary();
    // Three columns need a page 1440 px wide: the pane beside the transcript.
    const opened = await app.js(`return { width: innerWidth, height: innerHeight }`);
    await resizeInner(app, 1600, opened.height);
    const vod = await app.addAndTranscribe(fixtures.landed);
    await app.openPaused(vod.id);
    // Two chapters (N), each its title written: at the start and at 2 s.
    const chapter = async (text) => {
      await app.key("n");
      const input = `[role=dialog][aria-label="Chapter title"] input`;
      await until("the title box", () => app.js(`return !!document.querySelector(arguments[0])`, input), 10_000);
      await app.type(input, text);
      await app.submit(input);
      await until("the box to close", () => app.js(`return !document.querySelector(arguments[0])`, input), 10_000);
      // Let go of it: the arrows then move the playhead, not the chapter.
      await app.key("Escape");
    };
    await app.toStart();
    await until("the start", async () => (await app.shownTime()) === 0, 10_000);
    await chapter("Approach");
    for (let i = 0; i < 2; i++) await app.key("ArrowRight", { shift: true });
    await until("2 s in", async () => (await app.shownTime()) === 2, 10_000);
    await chapter("Landing");
    const landing = await until(
      "the two chapters",
      async () => {
        const marks = await app.invoke("list_marks", { vodId: vod.id });
        return (
          marks.filter((m) => m.kind === "chapter" && m.text).length === 2 && marks.find((m) => m.text === "Landing")
        );
      },
      10_000,
    );
    await app.js(`
      window.__copied = [];
      const fetch = window.fetch;
      window.fetch = (url, init) => {
        const command = decodeURIComponent(String(url)).split("/").pop();
        if (command === "plugin:clipboard-manager|write_text") window.__copied.push(JSON.parse(init.body).text);
        return fetch(url, init);
      };`);
    const copied = (n) =>
      until(
        `copy ${n}`,
        () => app.js(`return window.__copied.length >= arguments[0] && window.__copied[arguments[0] - 1]`, n),
        10_000,
      );

    // The pane's header.
    const button = ".annotations-pane .copy-chapters";
    await until(
      "the pane's Copy chapters",
      () => app.js(`return !!document.querySelector(arguments[0])`, button),
      MINUTE,
    ).catch(() => {
      throw new Error("the chapters pane has no Copy chapters");
    });
    await app.js(`document.querySelector(arguments[0]).click()`, button);
    const fromPane = await copied(1);
    if (!/Approach/.test(fromPane) || !/Landing/.test(fromPane))
      throw new Error(`the pane copied ${JSON.stringify(fromPane)}`);
    await until(
      "the toast saying so",
      () => app.js(`return /^Copied/.test(document.querySelector(".toast .message")?.textContent ?? "")`),
      10_000,
    );

    // A chapter's title box (the chapter selected on the timeline, Enter), its Copy chapters.
    await app.js(
      `const band = document.querySelector('.chapters [data-chapter="' + arguments[0] + '"]');
       const box = band.getBoundingClientRect();
       const at = { bubbles: true, cancelable: true, clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, button: 0, isPrimary: true, pointerId: 1 };
       band.dispatchEvent(new PointerEvent("pointerdown", { ...at, buttons: 1 }));
       window.dispatchEvent(new PointerEvent("pointerup", { ...at, buttons: 0 }));`,
      landing.id,
    );
    await app.key("Enter");
    const boxCopy = `[role=dialog][aria-label="Chapter title"] button.link`;
    await until(
      "the title box's Copy chapters",
      () => app.js(`return !!document.querySelector(arguments[0])`, boxCopy),
      10_000,
    );
    await app.js(
      `[...document.querySelectorAll(arguments[0])].find((b) => b.textContent.includes("as YouTube chapters")).click()`,
      boxCopy,
    );
    const fromBox = await copied(2);
    if (fromBox !== fromPane)
      throw new Error(`the pane copied ${JSON.stringify(fromPane)}, the title box ${JSON.stringify(fromBox)}`);
    await app.key("Escape");
    log(`the pane and the title box copied the same: ${JSON.stringify(fromPane)}`);
  },
};
