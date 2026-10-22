// The time by frame under a site's VOD played in its own player (a release:
// no embed-takeover), from their player's reports (embed.ts, playerClock.ts):
// the owner's test VOD on Twitch (or YouTube, with E2E_SITE=youtube). While
// it plays, the frame number only goes up, a frame or two at a time: Twitch's
// reports, a quarter second off now and then, made it leap 9 frames on and 7
// back (the clock's own test replays them: playerClock.test.mjs). Samples the
// number every 50 ms for 10 s, beside what their player said and when, kept
// in site-frames/ for a failure's diagnosis.

import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const SITES = {
  youtube: "https://www.youtube.com/watch?v=IaIo3d6cUuo",
  twitch: "https://www.twitch.tv/videos/2901646294",
};
const SITE = process.env.E2E_SITE ?? "twitch";
const SAMPLE_MS = 50;
const FOR_MS = 10_000;
/** The most frames the number may move on between samples (about 1.5 at 30 fps). */
const MOST = 3;

export default {
  name: "site-frames",
  needs: ["network"],
  async run(app, ctx) {
    if (await app.invoke("embed_takeover")) ctx.skip("this build takes over the sites' players (site-embed)");
    await app.toLibrary();
    await app.type('input[aria-label="VOD link"]', SITES[SITE]);
    await app.submit('input[aria-label="VOD link"]');
    const vod = await until(
      `the ${SITE} VOD in the library`,
      async () => {
        const error = await app.js(`return document.querySelector(".add-error")?.textContent.trim() ?? ""`);
        if (error) throw new Error(`adding it: ${error}`);
        return (await app.invoke("list_vods")).find((v) => v.platform === SITE);
      },
      5 * MINUTE,
    );
    // What their player says, as it arrives: its time, and whether it plays.
    await app.js(`window.__said = [];
      window.addEventListener("message", (e) => {
        let d = e.data;
        if (e.origin === "https://player.twitch.tv") {
          d = d ?? {};
          window.__said.push({ t: performance.now(), name: d.namespace + "/" + d.eventName,
            time: d.params?.currentTime ?? null, playback: d.params?.playback ?? null,
            report: d.eventName === "UPDATE_STATE" });
        } else if (e.origin === "https://www.youtube-nocookie.com") {
          try { d = typeof d === "string" ? JSON.parse(d) : d; } catch { return; }
          const state = d?.info?.playerState;
          window.__said.push({ t: performance.now(), name: String(d?.event),
            time: d?.info?.currentTime ?? null,
            playback: typeof state === "number" ? (state === 1 ? "Playing" : "state " + state) : null,
            report: typeof d?.info?.currentTime === "number" });
        }
      });`);
    await app.openVod(vod.id);
    await until(
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    if (!os.unavailable()) {
      os.activate();
      const play = await os.rectOf(app, '.bar button[aria-label="Play"]');
      os.click(play.x + play.w / 2, play.y + play.h / 2, play.scale);
    } else {
      await app.click('.bar button[aria-label="Play"]');
    }
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    await until(
      "their player playing",
      async () => {
        const t = await app.shownTime();
        await wait(1500);
        return (await app.shownTime()) > t;
      },
      2 * MINUTE,
    );
    // Played on a while first, past the start's buffering.
    await wait(3000);
    // The time by frame.
    await app.js(
      `if (!document.querySelector(".bar button.time .stack")) document.querySelector(".bar button.time").click();`,
    );
    await until("the time by frame", () => app.js(`return !!document.querySelector(".bar button.time .stack")`), 5000);

    // Sampled in the page, so the driver's round trips don't space them.
    await app.js(`window.__frames = [];
      const each = () => {
        const text = document.querySelector(".bar button.time .stack .now:not(.clock) > span:not(.widest)")?.textContent ?? "";
        window.__frames.push({ t: performance.now(), frame: Number(text) });
        if (window.__frames.length < ${FOR_MS / SAMPLE_MS}) setTimeout(each, ${SAMPLE_MS});
      };
      each();`);
    await wait(FOR_MS + 1000);
    const { frames, said } = await app.js(`return { frames: window.__frames, said: window.__said }`);

    // Side by side, from the first sample: each message, and each sample,
    // with the frame's change since the one before.
    const t0 = frames[0].t;
    const rows = [
      ...said
        .filter((m) => m.t >= t0 - 2000)
        .map((m) => ({ t: m.t, line: `${SITE} ${m.name} ${m.playback ?? ""} currentTime ${m.time}` })),
      ...frames.map((f, i) => ({
        t: f.t,
        line: `frame ${f.frame}${i ? ` (${f.frame - frames[i - 1].frame >= 0 ? "+" : ""}${f.frame - frames[i - 1].frame})` : ""}`,
      })),
    ].sort((a, b) => a.t - b.t);
    const table = rows.map((r) => `${((r.t - t0) / 1000).toFixed(3).padStart(8)} ${r.line}`).join("\n");

    // Their player's own clock: its reported time against when it arrived.
    // Played at speed 1, the difference stays put unless the reports jitter.
    const reports = said.filter((m) => m.report && typeof m.time === "number" && m.t >= t0 - 2000);
    const offsets = reports.map((m) => ({
      t: m.t,
      time: m.time,
      off: m.time - (m.t - t0) / 1000,
      playback: m.playback,
    }));
    const reportLines = offsets
      .map((o, i) => {
        const gap = i ? `${(o.t - offsets[i - 1].t).toFixed(0)} ms after` : "first";
        return `${((o.t - t0) / 1000).toFixed(3).padStart(8)} ${o.playback} ${o.time.toFixed(3)} (time minus arrival ${o.off.toFixed(3)}; ${gap})`;
      })
      .join("\n");
    const out = path.join(process.env.E2E_OUT ?? ".", "site-frames");
    mkdirSync(out, { recursive: true });
    const run = `${SITE}-${Date.now()}`;
    writeFileSync(path.join(out, `${run}.txt`), `${table}\n\nTheir player's reports:\n${reportLines}\n`);
    writeFileSync(path.join(out, `${run}.json`), JSON.stringify({ frames, said }, null, 1));
    ctx.log(`kept: site-frames/${run}.txt (${frames.length} samples, ${said.length} messages from ${SITE})`);
    const offs = offsets.filter((o) => o.playback === "Playing").map((o) => o.off);
    if (offs.length)
      ctx.log(
        `${SITE}'s time minus its arrival, while Playing: ${Math.min(...offs).toFixed(3)} to ${Math.max(...offs).toFixed(3)} s over ${offs.length} reports`,
      );

    // Wrong while their player said it played: back, or ahead by more than a step.
    const playingAt = (t) => [...said].reverse().find((m) => m.t <= t && m.playback)?.playback === "Playing";
    const bad = [];
    for (let i = 1; i < frames.length; i++) {
      const step = frames[i].frame - frames[i - 1].frame;
      if (playingAt(frames[i].t) && (step < 0 || step > MOST))
        bad.push(`${((frames[i].t - t0) / 1000).toFixed(3)} s: ${frames[i - 1].frame} -> ${frames[i].frame}`);
    }
    ctx.log(`jumps while Playing: ${bad.length ? bad.join("; ") : "none"}`);
    if (bad.length)
      throw new Error(`the frame number jumped ${bad.length}× while ${SITE} played: ${bad.slice(0, 8).join("; ")}`);
  },
};
