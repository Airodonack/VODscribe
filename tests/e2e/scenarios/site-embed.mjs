// A site's VOD in its own player, with the app's picture under it (embed.ts,
// embed_shim.js, embed.rs): the owner's test VOD on Twitch (or YouTube, with
// E2E_SITE=youtube) played from the app's controls. The app's player runs in
// step with it, follows a jump and a pause, draws captions, and when it
// steps aside (as for an ad in the stream) their player plays on alone.
// E2E_SHOTS: the whole screen at each step, native picture and all (an X
// server: xwd; headless weston: weston-screenshooter; macOS: screencapture).

import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, renameSync } from "node:fs";
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
    const shot = (name) => {
      if (!process.env.E2E_SHOTS) return;
      const file = path.join(shots, name);
      if (process.env.E2E_GNOME_SHELL) {
        // GNOME Shell's own screenshots (the test session's Shell allows them: unsafe mode).
        execFileSync("gdbus", [
          "call",
          "--session",
          "--dest",
          "org.gnome.Shell.Screenshot",
          "--object-path",
          "/org/gnome/Shell/Screenshot",
          "--method",
          "org.gnome.Shell.Screenshot.Screenshot",
          "false",
          "false",
          `${file}.png`,
        ]);
      } else if (process.env.WAYLAND_DISPLAY) {
        const before = new Set(readdirSync(shots));
        execFileSync("weston-screenshooter", [], { cwd: shots });
        const made = readdirSync(shots).find((f) => f.startsWith("wayland-screenshot") && !before.has(f));
        if (made) renameSync(path.join(shots, made), `${file}.png`);
      } else if (process.platform === "darwin") execFileSync("screencapture", ["-x", `${file}.png`]);
      else if (process.platform === "linux") execFileSync("xwd", ["-root", "-silent", "-out", `${file}.xwd`]);
    };

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
    shot("playing");

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
    shot("jumped");

    // Paused from the app's controls: it stands.
    await app.key(" ");
    await wait(2_000);
    const a = await ours();
    await wait(3_000);
    const b = await ours();
    ctx.log(`paused: ${a.time.toFixed(3)} s, then ${b.time.toFixed(3)} s`);
    if (a.playing || b.playing || Math.abs(a.time - b.time) > 0.05) throw new Error("it didn't stay paused");
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
