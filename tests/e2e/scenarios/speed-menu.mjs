// The speed button, with the OS's mouse (lib/os.mjs): a click steps to the
// next speed, as it always has; pressed and held, it opens a menu of the
// speeds beside it (above, where there's room), the current one marked, and let go over 1.5× that's the
// speed. Held and let go away from the menu, it closes with the speed as it
// was. The captions button right-clicked, with a stretch selected, opens the
// Transcribe menu, Transcribe selection first, and keeps the selection.
// Held, either menu shades the item under the pointer as a hovered one, and
// the Transcribe menu's lights up on the timeline what it would do.
// Linux (X11) only, so far.

import { until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const BUTTON = ".bar button.speed";
const MENU = '[role=dialog][aria-label="Playback speed"]';
const CAPTIONS = '.bar button[aria-label="Captions"]';
/** The captions button's Transcribe menu (TranscribeFrom.svelte). */
const FROM = '[role=dialog][aria-label="Transcribe"]';

export default {
  name: "speed-menu",
  async run(app, ctx) {
    const why = os.unavailable({ linuxOnly: "gestures" });
    if (why) ctx.skip(why);
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    const label = () => app.js(`return document.querySelector(arguments[0])?.textContent.trim()`, BUTTON);
    const speedIs = (text) =>
      until(
        `the speed at ${text}`,
        async () => {
          const now = await label();
          if (now === text) return true;
          throw new Error(`the button says ${now}`);
        },
        10_000,
      );
    const menuOpen = () => app.js(`return !!document.querySelector(arguments[0])`, MENU);
    /**
     * Runs gesture `steps`, ending in the release of `button` (2: the right
     * one, as PointerEvent numbers it), and waits till the page has seen that
     * release: what it does is done by then, the click after it included.
     */
    const released = async (steps, button) => {
      await app.js(
        `const seen = (window.__released = []);
         window.addEventListener("pointerup", (e) => seen.push(e.button), { capture: true });`,
      );
      await os.gesture(steps);
      await until("the release seen", () => app.js(`return window.__released.includes(arguments[0])`, button), 5_000);
    };
    /** The background of `over`, the menu item pointed at, and of `beside`, another of `menu`'s. */
    const pointedShade = (menu, over, beside) =>
      app.js(
        `const [a, b] = [arguments[1], arguments[2]].map((s) => document.querySelector(arguments[0] + " " + s));
         return { over: getComputedStyle(a).backgroundColor, beside: getComputedStyle(b).backgroundColor };`,
        menu,
        over,
        beside,
      );
    await speedIs("1×");
    const at = await os.rectOf(app, BUTTON);
    const [x, y] = [at.x + Math.round(at.w / 2), at.y + Math.round(at.h / 2)];

    // A click: the next speed, and no menu.
    os.click(x, y, at.scale);
    // The step is the click's, after its release: a hold's menu can't open
    // from that press any more.
    await speedIs("1.25×");
    if (await menuOpen()) throw new Error("a click opened the menu");
    ctx.log("a click steps 1× to 1.25×, no menu");

    // Held: the menu, 1.25× marked. Let go over 1.5×.
    await os.gesture([
      ["down", x, y],
      ["wait", 700],
    ]);
    try {
      await until("the menu, held open", menuOpen, 5_000);
      const marked = await app.js(
        `return [...document.querySelectorAll(arguments[0] + " [data-speed]")].filter((b) => b.classList.contains("current")).map((b) => b.dataset.speed)`,
        MENU,
      );
      if (marked.join() !== "1.25") throw new Error(`the menu marks ${marked.join()}, not 1.25`);
      // Above the button, or below it where the window has no room above
      // (an audio file's bar is near the top); never over it.
      const menu = await os.rectOf(app, MENU);
      const side = menu.y + menu.h <= at.y ? "above" : menu.y >= at.y + at.h ? "below" : null;
      if (!side) throw new Error(`the menu (${JSON.stringify(menu)}) covers the button (${JSON.stringify(at)})`);
      ctx.log(`the menu opened ${side} the button`);
      // Over 1.5×, still held: lit as a hovered item is, 2× beside it not.
      const item = await os.rectOf(app, `${MENU} [data-speed="1.5"]`);
      await os.gesture([
        ["move", item.x + Math.round(item.w / 2), item.y + Math.round(item.h / 2)],
        ["wait", 150],
      ]);
      const shade = await pointedShade(MENU, '[data-speed="1.5"]', '[data-speed="2"]');
      if (shade.over === shade.beside)
        throw new Error(`held over 1.5×, it's shaded as 2× beside it is: ${JSON.stringify(shade)}`);
      await os.gesture([["up"]]);
    } finally {
      // Never leave the button held for the scenarios after.
      await os.gesture([["up"]]).catch(() => {});
    }
    await speedIs("1.5×");
    await until("the menu closed", async () => !(await menuOpen()), 5_000);
    ctx.log("held, the menu opened with 1.25× marked; let go over 1.5×, it's 1.5×");

    // Held and let go over the time, away from the menu: closed, as it was.
    const time = await os.rectOf(app, "button.time");
    await os.gesture([
      ["down", x, y],
      ["wait", 700],
    ]);
    await until("the menu, held open", menuOpen, 5_000);
    await released(
      [["move", time.x + Math.round(time.w / 2), time.y + Math.round(time.h / 2)], ["wait", 150], ["up"]],
      0,
    );
    await until("the menu closed", async () => !(await menuOpen()), 5_000);
    // A speed picked would be the player's already: the release picks, and
    // LocalPlayer.svelte sets the media's rate as it's asked; the button
    // shows it a moment later.
    const rate = await app.js(`return document.querySelector("video, audio")?.playbackRate ?? null`);
    if (rate !== 1.5) throw new Error(`let go away from the menu, the speed is ${rate}`);
    await speedIs("1.5×");
    ctx.log("let go away from the menu: it closed, still 1.5×");

    // Right-clicked: the menu, open once the button's let go; a click on
    // 0.75× picks it.
    await released(
      [
        ["down", x, y, 3],
        ["wait", 100],
        ["up", 3],
      ],
      2,
    );
    // Its release seen already: open now is open for good.
    await until("the menu, right-clicked open, still open once let go", menuOpen, 5_000);
    const slower = await os.rectOf(app, `${MENU} [data-speed="0.75"]`);
    os.click(slower.x + Math.round(slower.w / 2), slower.y + Math.round(slower.h / 2), slower.scale);
    await speedIs("0.75×");
    await until("the menu closed", async () => !(await menuOpen()), 5_000);
    ctx.log("right-clicked, the menu opened and stayed; a click on 0.75× picked it");

    // The captions button right-clicked: the Transcribe menu, all of it
    // first (transcribed already, so again), then how much from the
    // playhead; Escape closes it with nothing asked, and the captions as they were.
    const captionsOn = () =>
      app.js(`return document.querySelector(arguments[0]).getAttribute("aria-pressed")`, CAPTIONS);
    const wasOn = await captionsOn();
    const selection = () => app.js(`return !!document.querySelector(".scrubber .selection")`);
    await app.toStart();
    await app.key("ArrowRight", { shift: true });
    await until("the player at 1 s", async () => (await app.shownTime()) === 1, 10_000);
    await app.key("o");
    await until("a selection", selection, 10_000);
    const transcribe = await os.rectOf(app, CAPTIONS);
    const [tx, ty] = [transcribe.x + Math.round(transcribe.w / 2), transcribe.y + Math.round(transcribe.h / 2)];
    const fromOpen = () => app.js(`return !!document.querySelector(arguments[0])`, FROM);
    await released(
      [
        ["down", tx, ty, 3],
        ["wait", 100],
        ["up", 3],
      ],
      2,
    );
    await until("the Transcribe menu, right-clicked open, still open once let go", fromOpen, 5_000);
    const choices = await app.js(
      `return [...document.querySelectorAll(arguments[0] + " [data-choice]")].map((b) => b.dataset.choice)`,
      FROM,
    );
    if (choices[0] !== "lead" || choices.length < 2)
      throw new Error(`the Transcribe menu, right-clicked open, has ${JSON.stringify(choices)}`);
    const lead = await app.js(
      `return document.querySelector(arguments[0] + ' [data-choice="lead"] span')?.textContent.trim()`,
      FROM,
    );
    if (lead !== "Transcribe selection" || !(await selection()))
      throw new Error(
        `with a selection, the menu's first entry says "${lead}", the selection ${(await selection()) ? "kept" : "gone"}`,
      );
    await os.gesture([["key", "Escape"]]);
    await until("the Transcribe menu closed", async () => !(await fromOpen()), 5_000);
    if ((await captionsOn()) !== wasOn) throw new Error("the captions button, right-clicked, turned the captions over");
    ctx.log(
      `the captions button right-clicked: the Transcribe menu opened (${choices.join(", ")}), "${lead}" first; Escape closed it`,
    );

    // Held, the captions button opens the menu too; over Transcribe
    // selection, still held, the entry is shaded as a hovered one and the
    // selection lights up on the timeline; let go over the time, away from
    // the menu, it closes with nothing asked.
    const lit = () => app.js(`return document.querySelectorAll(".scrubber .lit:not(.used):not(.removing)").length`);
    await os.gesture([
      ["down", tx, ty],
      ["wait", 700],
    ]);
    try {
      await until("the Transcribe menu, held open", fromOpen, 5_000);
      const entry = await os.rectOf(app, `${FROM} [data-choice="lead"]`);
      await os.gesture([
        ["move", entry.x + Math.round(entry.w / 2), entry.y + Math.round(entry.h / 2)],
        ["wait", 150],
      ]);
      const shade = await pointedShade(FROM, '[data-choice="lead"]', '[data-choice="end"]');
      const litNow = await lit();
      if (shade.over === shade.beside || litNow !== 1)
        throw new Error(
          `held over Transcribe selection, it's shaded ${JSON.stringify(shade)}, ${litNow} stretches lit on the timeline`,
        );
      const away = await os.rectOf(app, "button.time");
      await os.gesture([
        ["move", away.x + Math.round(away.w / 2), away.y + Math.round(away.h / 2)],
        ["wait", 150],
      ]);
    } finally {
      await os.gesture([["up"]]).catch(() => {});
    }
    await until("the Transcribe menu closed", async () => !(await fromOpen()), 5_000);
    const asked = (await app.invoke("transcript_stretches", { vodId: vod.id })).filter((s) => s.status !== "done");
    if (asked.length) throw new Error(`held and let go away from the menu, it asked for ${JSON.stringify(asked)}`);
    ctx.log("the captions button held: Transcribe selection shaded and lit while pointed at; let go away, closed");
  },
};
