// A far seek while the app's own player plays: the time shown goes there
// and stays, never back to where it was, and the button goes on saying
// Pause (a seek doesn't pause; nativeDriver.ts). The player tells the page its time
// with every frame (`player-time` events) and in each command's reply, apart,
// and the page keeps the newest by `seq` (nativeDriver.ts); an event made
// just before the seek that arrived after the page had gone to the seek's
// time, but before the seek's reply, would take the time back (and say
// whether it plays as it was). Each seek is traced in the page: the events
// and replies as they arrive, which of them the page's rule takes, and the
// time on the bar every few milliseconds for a second after. (Not seen so
// far: 40 seeks on Linux, the replies in 1 to 15 ms; a guard.)

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { minute } from "../lib/fixtures.mjs";

/** Seeks traced: enough for an event in flight to meet one, if they can,
 *  and for a frame to be drawn in the few milliseconds before a seek's
 *  status comes (it was, in about one seek in twelve, before the button
 *  kept saying Pause). */
const ATTEMPTS = 40;
/** Where on the timeline it plays from, and where it seeks to (fractions of its width). */
const FROM = 0.1;
const TO = 0.85;

export default {
  name: "player-seek",
  async run(app, ctx) {
    const file = minute(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    const vod = await app.addAndTranscribe(file);
    await app.openVod(vod.id);
    await app.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    const native = await until(
      "a player",
      () =>
        app.js(
          `return document.querySelector(".player .picture") ? "native" : document.querySelector(".player video") ? "web" : null`,
        ),
      MINUTE,
    );
    if (native !== "native") ctx.skip("it plays in the web view here, not the app's own player");
    // The time as a clock (whole seconds), which is what's read below; by
    // frame (a setting kept from before), it's switched.
    await until(
      "the time shown as a clock",
      () =>
        app.js(`const button = document.querySelector(".bar button.time");
                if (!button.querySelector(".stack")) return true;
                button.click();
                return false;`),
      10_000,
    );

    // The trace: every `player-time` event and every player command's reply,
    // stamped as they arrive (the page's own listener gets the same events
    // at the same moment).
    await app.js(`
      const trace = (window.__seekTrace = { arrived: [] });
      const internals = window.__TAURI_INTERNALS__;
      const handler = internals.transformCallback((e) =>
        trace.arrived.push({ kind: "event", at: performance.now(), ...pick(e.payload) }));
      const pick = (s) => ({ seq: s.seq, time: s.time, playing: s.playing });
      await internals.invoke("plugin:event|listen", { event: "player-time", target: { kind: "Any" }, handler });
      const fetch = window.fetch;
      window.fetch = (url, init) => {
        const cmd = decodeURIComponent(String(url)).split("/").pop();
        const sent = performance.now();
        const reply = fetch(url, init);
        if (cmd.startsWith("player_"))
          reply.then((r) => r.clone().json()).then(
            (s) => trace.arrived.push({ kind: cmd, sent, at: performance.now(), ...pick(s) }),
            () => {},
          );
        return reply;
      };`);

    const failures = [];
    let inFlight = 0;
    for (let i = 0; i < ATTEMPTS; i++) {
      const seen = await app.js(
        `const [from, to] = arguments;
         const trace = window.__seekTrace;
         const bar = document.querySelector(".scrubber");
         const click = (fraction) => {
           const box = bar.querySelector("canvas").getBoundingClientRect();
           const at = { clientX: box.left + box.width * fraction, clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1 };
           const el = document.elementFromPoint(at.clientX, at.clientY);
           el.dispatchEvent(new PointerEvent("pointerdown", at));
           el.dispatchEvent(new PointerEvent("pointerup", at));
         };
         const wait = (ms) => new Promise((r) => setTimeout(r, ms));
         const shown = () => {
           const text = document.querySelector("button.time .now > span:not(.widest)")?.textContent ?? "";
           return text.split(":").reduce((total, part) => total * 60 + Number(part), 0);
         };
         const button = () => document.querySelector(".bar button[aria-label='Pause'], .bar button[aria-label='Play']")?.getAttribute("aria-label");
         // Back near the start, playing on for a moment: its frames' events flow.
         click(from);
         await wait(700);
         if (button() !== "Pause") return { error: "not playing before the seek: " + button() };
         const target = Number(bar.dataset.viewStart) + Number(bar.dataset.span) * to;
         const before = shown();
         // What came before stays, for the rule's newest seq; a second's worth is plenty.
         const mark = Math.max(0, trace.arrived.length - 60);
         // Every change to the bar, as it's made (a frame's worth may never be painted).
         const changes = [];
         const watch = new MutationObserver(() => changes.push({ at: performance.now(), time: shown(), button: button() }));
         watch.observe(document.querySelector(".bar"), { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["aria-label"] });
         const start = performance.now();
         click(to);
         const samples = [];
         while (performance.now() - start < 1000) {
           samples.push({ at: performance.now(), time: shown(), button: button() });
           await wait(3);
         }
         await wait(0);
         watch.disconnect();
         return { start, target, before, samples, changes, arrived: trace.arrived.slice(mark) };`,
        FROM,
        TO,
      );
      if (seen.error) throw new Error(`seek ${i + 1}: ${seen.error}`);
      const { start, target, before, samples, changes, arrived } = seen;
      // The page's rule (nativeDriver.ts, ownStatus.ts): what arrives is taken
      // if its seq is the newest yet, but for events while a command is out.
      const replies = arrived.filter((a) => a.kind !== "event");
      const out = (a) => a.kind === "event" && replies.some((r) => r.sent <= a.at && a.at < r.at);
      let newest = 0;
      const taken = [];
      for (const a of [...arrived].sort((x, y) => x.at - y.at)) {
        if (a.seq <= newest || out(a)) continue;
        newest = a.seq;
        taken.push(a);
      }
      const reply = arrived.find((a) => a.kind === "player_seek" && a.sent >= start);
      // Made before the seek, taken after the page went to the seek's time:
      // back past it, or saying it doesn't play when it does.
      const stale = taken.filter(
        (a) =>
          a.at >= start &&
          (!reply || a.at <= reply.at) &&
          (a.time < target - 1 || (reply?.playing && !a.playing && a.kind === "event")),
      );
      const between = arrived.filter((a) => a.kind === "event" && reply && a.at > reply.sent && a.at < reply.at);
      inFlight += between.length;
      // The time on the bar, once it has shown the seek's time, never
      // earlier (but for the frame the player lands on, up to a frame before
      // the time asked; the bar shows whole seconds).
      const bar = [...samples, ...changes].sort((x, y) => x.at - y.at);
      const floor = Math.floor(target - 1 / 30);
      const there = bar.findIndex((s) => s.time >= floor);
      const back = there < 0 ? [] : bar.slice(there).filter((s) => s.time < floor);
      const flicked = bar.filter((s) => s.button === "Play").length;
      const line =
        `seek ${i + 1}: ${before} s → ${target.toFixed(2)} s; reply in ${reply ? (reply.at - reply.sent).toFixed(1) : "?"} ms, ` +
        `${between.length} event(s) arrived meanwhile; ${stale.length} stale taken; ` +
        `the bar back ${back.length} time(s)` +
        (flicked ? `; the button said Play ${flicked} time(s)` : "");
      ctx.log(line);
      if (there < 0) failures.push(`seek ${i + 1}: the bar never showed ${target.toFixed(2)} s`);
      // Playing, and no pause asked: the button says so throughout.
      if (flicked) failures.push(`${line}: the button said Play while it played`);
      if (stale.length || back.length)
        failures.push(
          `${line}: ${JSON.stringify({ stale, back: back.slice(0, 5), arrived: arrived.filter((a) => a.at >= start).slice(0, 12) })}`,
        );
    }
    ctx.log(`${ATTEMPTS} seeks: ${inFlight} event(s) arrived between a seek and its reply`);
    if (failures.length) throw new Error(`after a seek:\n${failures.join("\n")}`);

    // Paused, a seek's time shows until its frame is drawn, then the frame:
    // once the reply has come and the frame with it, the time is the
    // picture's, to the frame.
    await app.pause();
    const sent = await app.js(
      `const box = document.querySelector(".scrubber canvas").getBoundingClientRect();
       const at = { clientX: box.left + box.width * 0.3, clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1 };
       const el = document.elementFromPoint(at.clientX, at.clientY);
       const sent = performance.now();
       el.dispatchEvent(new PointerEvent("pointerdown", at));
       el.dispatchEvent(new PointerEvent("pointerup", at));
       return sent;`,
    );
    await until(
      "the paused seek's reply",
      () =>
        app.js(
          `return window.__seekTrace.arrived.some((a) => a.kind === "player_seek" && a.sent >= arguments[0])`,
          sent,
        ),
      10_000,
    );
    await sleep(600);
    ctx.log(await app.timeIsPicture("after a paused seek"));
  },
};
