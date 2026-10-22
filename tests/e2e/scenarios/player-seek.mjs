// A far seek while the app's own player plays: the time shown goes there
// and stays, never back to where it was. The player tells the page its time
// with every frame (`player-time` events) and in each command's reply, apart,
// and the page keeps the newest by `seq` (nativeDriver.ts); an event made
// just before the seek that arrived after the page had gone to the seek's
// time, but before the seek's reply, would take the time back (and say
// whether it plays as it was). Each seek is traced in the page: the events
// and replies as they arrive, which of them the page's rule takes, and the
// time on the bar every few milliseconds for a second after. (Not seen so
// far: 40 seeks on Linux, the replies in 1 to 15 ms; a guard.)

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { minute } from "../lib/fixtures.mjs";

/** Seeks traced: enough for an event in flight to meet one, if they can. */
const ATTEMPTS = 20;
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
      // The page's rule (ownStatus.ts): what arrives is taken if its seq is the newest yet.
      let newest = 0;
      const taken = [];
      for (const a of [...arrived].sort((x, y) => x.at - y.at)) {
        if (a.seq <= newest) continue;
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
      // The time on the bar. It shows the frame on screen (playerReading.svelte.ts):
      // put at the seek's time by the page at once, then back on the old frame
      // until the player draws the new one, then there. Once there for good,
      // it mustn't go back.
      const bar = [...samples, ...changes].sort((x, y) => x.at - y.at);
      const far = (s) => s.time >= Math.floor(target) - 1;
      const there = bar.findIndex(far);
      const left = there < 0 ? -1 : bar.findIndex((s, k) => k > there && !far(s));
      const settled = left < 0 ? there : bar.findIndex((s, k) => k > left && far(s));
      const back = settled < 0 ? [] : bar.slice(settled).filter((s) => !far(s));
      const bounce = left < 0 || settled < 0 ? 0 : bar[settled].at - bar[left].at;
      const flicked = bar.filter((s) => s.button === "Play").length;
      const line =
        `seek ${i + 1}: ${before} s → ${target.toFixed(2)} s; reply in ${reply ? (reply.at - reply.sent).toFixed(1) : "?"} ms, ` +
        `${between.length} event(s) arrived meanwhile; ${stale.length} stale taken; ` +
        `the bar on the old frame ${bounce.toFixed(0)} ms, back after ${back.length} time(s)` +
        (flicked ? `; the button said Play ${flicked} time(s)` : "");
      ctx.log(line);
      if (settled < 0) failures.push(`seek ${i + 1}: the bar never stayed at ${target.toFixed(2)} s`);
      if (stale.length || back.length)
        failures.push(
          `${line}: ${JSON.stringify({ stale, back: back.slice(0, 5), arrived: arrived.filter((a) => a.at >= start).slice(0, 12) })}`,
        );
    }
    ctx.log(`${ATTEMPTS} seeks: ${inFlight} event(s) arrived between a seek and its reply`);
    if (failures.length) throw new Error(`after a seek, the time went back:\n${failures.join("\n")}`);
  },
};
