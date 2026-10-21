// A site's VOD in its own player, with the app's picture under it (embed.ts,
// embed_shim.js, embed.rs): the owner's test VOD on Twitch (or YouTube, with
// E2E_SITE=youtube) played from the app's controls. The app's player runs in
// step with it, follows a jump and a pause, draws captions, and when it
// steps aside (as for an ad in the stream) their player plays on alone.
// Where the screen can be seen (lib/os.mjs), it checks the picture shows
// through their frame; E2E_SHOTS keeps the whole screen at each step.
// E2E_SCHEME (light, dark): the colour scheme the run is in, checked.

import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

const SITES = {
  youtube: "https://www.youtube.com/watch?v=IaIo3d6cUuo",
  twitch: "https://www.twitch.tv/videos/2901646294",
};
const SITE = process.env.E2E_SITE ?? "twitch";

export default {
  name: "site-embed",
  needs: ["network"],
  async run(app, ctx) {
    if (!(await app.invoke("player_available"))) ctx.skip("the app's own player doesn't draw here");
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
    await app.openVod(vod.id);
    await until(
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    // The colour scheme asked for (E2E_SCHEME), as the page sees it: a run in
    // the wrong one would prove nothing about it.
    if (process.env.E2E_SCHEME) {
      const dark = await app.js(`return matchMedia("(prefers-color-scheme: dark)").matches`);
      if (dark !== (process.env.E2E_SCHEME === "dark"))
        throw new Error(
          `asked for the ${process.env.E2E_SCHEME} scheme; the page has the ${dark ? "dark" : "light"} one`,
        );
    }
    // Clicked by the system's pointer where there is one: their player may
    // only start for a real click.
    if (!os.unavailable()) {
      os.activate();
      const play = await os.rectOf(app, '.bar button[aria-label="Play"]');
      os.click(play.x + play.w / 2, play.y + play.h / 2, play.scale);
    } else {
      await app.click('.bar button[aria-label="Play"]');
    }
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    /** The app's player: where it is (null: none open). Its status comes
     *  back from any command; setting the speed it plays at (1, as the test
     *  never changes it) changes nothing. */
    const ours = () => app.invoke("player_set_speed", { speed: 1 }).catch(() => null);
    const reports = () => app.js(`return window.__vodscribeEmbed?.reports ?? []`);
    const shots = path.join(process.env.E2E_OUT ?? ".", "site-embed");
    mkdirSync(shots, { recursive: true });
    /** The whole screen into `name`.png (E2E_SHOTS), where it can be seen. */
    const shot = (name) => {
      if (process.env.E2E_SHOTS && !os.cannotSee()) writeFileSync(path.join(shots, `${name}.png`), os.screenshot());
    };

    // From the play press: the playhead never goes back, nor runs on ahead of
    // the app's player while that waits for their player to start (it ran
    // ~4 s ahead, then jumped back to 0, on a desktop).
    const pressed = Date.now();
    let furthest = 0;
    const seen = [];
    while (Date.now() - pressed < 8_000) {
      const [bar, s] = [await app.shownTime(), await ours()];
      seen.push(
        `+${((Date.now() - pressed) / 1000).toFixed(1)} s: bar ${bar}, ours ${s ? `${s.time.toFixed(2)}${s.running ? " running" : ""}` : "none"}`,
      );
      const fail = (why) => new Error(`${why}; from the press:\n    ${seen.join("\n    ")}`);
      if (bar < furthest - 0.5) throw fail(`the playhead went back, from ${furthest} s to ${bar} s`);
      furthest = Math.max(furthest, bar);
      if (s && !s.running && bar > s.time + 1.5)
        throw fail(`the playhead ran on to ${bar} s, the app's player not playing yet (at ${s.time} s)`);
      await wait(200);
    }

    // Playing: the app's player runs, in step with the playhead.
    const playing = await until(
      "the app's player playing what their player fetched",
      async () => {
        const s = await ours();
        return s?.running && s.time > 5 && s;
      },
      2 * MINUTE,
    );
    const bar = await app.shownTime();
    ctx.log(`playing: ours at ${playing.time.toFixed(2)} s (frame ${playing.shown?.toFixed(3)}), the bar at ${bar} s`);
    if (Math.abs(playing.time - bar) > 2) throw new Error(`ours at ${playing.time} s, the playhead at ${bar} s`);
    // On the screen, their frame shows the picture: the test video's colour
    // (each segment has its own) at a spot clear of its text and their
    // overlays, not grey (their frame drawn opaque over it, as it was in a
    // dark scheme).
    if (!os.cannotSee()) {
      const png = os.screenshot();
      const frame = await os.rectOf(app, "iframe");
      const spot = { x: Math.round(frame.x + frame.w * 0.1), y: Math.round(frame.y + frame.h * 0.55), w: 12, h: 12 };
      const [r, g, b] = os.colourIn(png, spot);
      ctx.log(`the picture on the screen: rgb(${r}, ${g}, ${b})`);
      if (Math.max(r, g, b) - Math.min(r, g, b) < 40) {
        writeFileSync(path.join(shots, "grey.png"), png);
        throw new Error(`their frame shows grey, rgb(${r}, ${g}, ${b}), not the picture (site-embed/grey.png)`);
      }
    }
    shot("playing");
    // The time by the button is our picture's frame, not their player's word.
    ctx.log(await app.timeIsPicture("playing", { playing: true }));

    // Captions from the transcript: drawn over the picture by the screen, or
    // in the page where the page draws the picture (X11); seen only there.
    const cues = await app.invoke("captions", { vodId: vod.id });
    const pageDraws = !(await app.invoke("screen_under"));
    ctx.log(`captions: ${cues.length} cues${pageDraws ? " (the page draws the picture)" : ""}`);
    // Where the page draws the picture, it draws the caption too, once there's one now.
    if (pageDraws && cues.some((c) => c.start <= playing.time + 1 && playing.time < c.end - 1))
      await until(
        "the page's caption",
        () => app.js(`return !!document.querySelector(".screen .over canvas")`),
        10_000,
      );

    // A jump from the app's controls (L: 10 s each): their player seeks, ours follows.
    const before = (await ours()).time;
    for (let i = 0; i < 3; i++) await app.key("l");
    const jumped = await until(
      "the app's player past the jump",
      async () => {
        const s = await ours();
        return s?.running && s.time > before + 25 && s;
      },
      MINUTE,
    );
    ctx.log(`jumped: from ${before.toFixed(2)} s to ${jumped.time.toFixed(2)} s`);
    ctx.log(await app.timeIsPicture("after the jump", { playing: true }));
    shot("jumped");

    // Paused from the app's controls: it stands.
    await app.key(" ");
    await wait(2_000);
    const a = await ours();
    await wait(3_000);
    const b = await ours();
    ctx.log(`paused: ${a.time.toFixed(3)} s, then ${b.time.toFixed(3)} s`);
    if (a.playing || b.playing || Math.abs(a.time - b.time) > 0.05) throw new Error("it didn't stay paused");
    ctx.log(await app.timeIsPicture("paused"));
    shot("paused");

    /** What went wrong, as the frame and the page reported it. */
    const failures = async () => {
      const bad = (await reports()).filter((m) => ["appendError", "error", "mismatch", "fallback"].includes(m.kind));
      if (bad.length) throw new Error(`the frame reported: ${JSON.stringify(bad).slice(0, 400)}`);
    };
    await failures();

    // Played again: on from where it stood (Twitch's player has been seen to
    // seek itself to the start as it resumes; the page sends it back).
    await app.key(" ");
    await wait(3_000);
    const resumed = await ours();
    const restored = (await reports()).filter((m) => m.kind === "restored");
    ctx.log(
      `resumed: ours at ${resumed.time.toFixed(2)} s${restored.length ? `; sent back from the start ${restored.length}×` : ""}`,
    );
    if (!resumed.playing || resumed.time < b.time - 0.5)
      throw new Error(`resumed at ${resumed.time} s, paused at ${b.time} s`);
    await failures();

    // Stepped aside (as for an ad in the stream): ours closes, theirs plays on from there alone.
    const from = await app.shownTime();
    if (!(await app.js(`return typeof window.__vodscribeFallback === "function"`)))
      throw new Error("no fallback to ask for (embed.ts's test hook)");
    await app.js(`window.__vodscribeFallback()`);
    await until("the app's player closed", async () => (await ours()) === null, 30_000);
    const t1 = await until(
      "their player playing alone",
      async () => {
        const t = await app.shownTime();
        await wait(2_000);
        const t2 = await app.shownTime();
        return t2 > t && t2;
      },
      MINUTE,
    );
    ctx.log(
      `fell back at ${from} s: their player alone at ${t1} s (${await app.js(`return document.querySelector("iframe")?.src ?? ""`)})`,
    );
    if ((await app.js(`return document.querySelector("iframe")?.name ?? ""`)) !== "vodscribe-plain")
      throw new Error("their frame wasn't loaded plain");
    if (t1 < from - 2) throw new Error(`their player went back to ${t1} s from ${from} s`);
    shot("alone");
  },
};
