// Video files of several kinds, added together: each is transcribed (or,
// with nothing said or no sound, done with an empty transcript) and plays.
// The first, before its first Play (when its player is made), already has
// the bar's sound, speed and captions: the volume and speed set then are
// what the player starts with, and none of them leaves the bar meanwhile.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

export default {
  name: "video-files",
  async run(app, { out, log, checkpoint }) {
    const files = videos(path.join(out, "fixtures"));
    await app.toLibrary();
    for (const vod of await app.addFiles(Object.values(files)))
      await app.invoke("transcribe_stretch", { vodId: vod.id, start: 0, end: null });
    const byFile = await until(
      "every file in the library, finished",
      async () => {
        const vods = await app.invoke("list_vods");
        if (vods.length < Object.keys(files).length || vods.some((v) => !["done", "error"].includes(v.status)))
          throw new Error(vods.map((v) => `${v.title}: ${v.status}`).join(", ") || "no VODs");
        return Object.fromEntries(
          Object.entries(files).map(([kind, file]) => [
            kind,
            vods.find((v) => v.title === path.basename(file, path.extname(file))),
          ]),
        );
      },
      15 * MINUTE,
    );
    for (const [kind, vod] of Object.entries(byFile)) {
      if (!vod) throw new Error(`no VOD titled after ${path.basename(files[kind])}`);
      log(`${kind}: ${vod.status}${vod.error ? ` (${vod.error})` : ""}`);
    }
    for (const [kind, vod] of Object.entries(byFile)) {
      if (vod.status !== "done") throw new Error(`${kind}: ${vod.status}: ${vod.error}`);
      const text = (await app.invoke("transcript", { vodId: vod.id })).map((l) => l.text).join(" ");
      const speaks = !["silent", "mute"].includes(kind);
      if (speaks !== /landed/i.test(text)) throw new Error(`${kind}: the transcript is "${text}"`);
      await app.openVod(vod.id);
      if (kind === "video") await setBeforePlay(app, log);
      const at = await app.plays();
      if (kind === "video") await appliedOncePlaying(app, log);
      log(`${kind}: transcribed${speaks ? "" : " (nothing said)"}, played to ${at} s`);
      if (kind === "video" || kind === "wide") await checkpoint(`playing-${kind}`);
    }
  },
};

/** The bar's controls of the VOD just opened, its player not made yet: set
 *  to 40% and 1.25×. What the page asks of the player from now is kept
 *  (window.__asked: its commands as they go out, as player-seek.mjs traces
 *  them), the speed it says it plays at (window.__speeds), and whether each
 *  control is there, every frame (window.__missing: the frames one wasn't). */
async function setBeforePlay(app, log) {
  const bar = await until(
    "the bar's controls before Play",
    async () => {
      const bar = await app.js(`return {
        player: !!document.querySelector(".player"),
        mute: !!document.querySelector('.bar button[aria-label="Mute"]'),
        volume: !!document.querySelector('.bar input[aria-label="Volume"]'),
        speed: document.querySelector(".bar button.speed")?.textContent.trim() ?? null,
        captions: document.querySelector('.bar button[aria-label="Captions"]')?.disabled ?? null,
      }`);
      // The captions button is greyed until the transcript's lines are in.
      return bar.captions === false && bar;
    },
    10_000,
  ).catch((e) => {
    throw new Error(`before Play, the captions button isn't there or stays greyed (${e.message})`);
  });
  if (bar.player) throw new Error("the player was made before Play");
  if (!bar.mute || !bar.volume || bar.speed === null)
    throw new Error(`before Play, the bar has ${JSON.stringify(bar)}`);
  await app.js(`
    window.__asked = [];
    window.__speeds = [];
    const internals = window.__TAURI_INTERNALS__;
    const handler = internals.transformCallback((e) => window.__speeds.push(e.payload.speed));
    await internals.invoke("plugin:event|listen", { event: "player-time", target: { kind: "Any" }, handler });
    const fetch = window.fetch;
    window.fetch = (url, init) => {
      const cmd = decodeURIComponent(String(url)).split("/").pop();
      if (cmd.startsWith("player_")) {
        let args = null;
        try { args = JSON.parse(init?.body); } catch {}
        window.__asked.push([cmd, args]);
      }
      return fetch(url, init);
    };
    const slider = document.querySelector('.bar input[aria-label="Volume"]');
    slider.value = "0.4";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    window.__missing = [];
    (function each() {
      const gone = ['.bar button[aria-label="Mute"], .bar button[aria-label="Unmute"]', '.bar input[aria-label="Volume"]',
        ".bar button.speed", '.bar button[aria-label="Captions"]'].filter((s) => !document.querySelector(s));
      if (gone.length) window.__missing.push(gone.join(" "));
      if (window.__missing.length < 50 && !window.__stopWatch) requestAnimationFrame(each);
    })();`);
  await app.click(".bar button.speed");
  await until(
    "the bar at 1.25× and 40%",
    () =>
      app.js(`return document.querySelector(".bar button.speed").textContent.trim() === "1.25×" &&
        document.querySelector('.bar input[aria-label="Volume"]').value === "0.4"`),
    10_000,
  );
  log("before Play: the bar has mute, volume, speed and captions; set to 40%, 1.25×");
}

/** Playing: the player started at what was set, and the bar never lost a control. */
async function appliedOncePlaying(app, log) {
  const seen = await app.js(`
    window.__stopWatch = true;
    const video = document.querySelector(".player video");
    return {
      web: video ? { volume: video.volume, rate: video.playbackRate } : null,
      asked: window.__asked,
      speeds: window.__speeds,
      missing: window.__missing,
      speed: document.querySelector(".bar button.speed")?.textContent.trim(),
      volume: document.querySelector('.bar input[aria-label="Volume"]')?.value,
    }`);
  if (seen.missing.length) throw new Error(`while the player was made, the bar lacked: ${seen.missing[0]}`);
  if (seen.speed !== "1.25×" || seen.volume !== "0.4")
    throw new Error(`playing, the bar shows ${seen.speed} and ${seen.volume}`);
  if (seen.web) {
    if (Math.abs(seen.web.volume - 0.4) > 1e-6 || seen.web.rate !== 1.25)
      throw new Error(`the web view's player plays at ${seen.web.rate}×, volume ${seen.web.volume}`);
    log("playing in the web view's player: at 1.25×, volume 0.4");
  } else {
    // The app's own player: told the volume and the speed once open (nativeDriver.ts `opened`).
    const asked = seen.asked.map(([cmd, args]) => `${cmd} ${JSON.stringify(args)}`);
    const opened = asked.findIndex((a) => a.startsWith("player_open "));
    const after = asked.slice(opened + 1);
    if (
      opened < 0 ||
      !after.includes('player_set_volume {"volume":0.4}') ||
      !after.includes('player_set_speed {"speed":1.25}')
    )
      throw new Error(`the player was asked: ${asked.join("; ")}`);
    const said = seen.speeds.at(-1);
    if (said !== 1.25) throw new Error(`the app's player says it plays at ${said}×`);
    log(`playing in the app's own player: asked ${after.filter((a) => /volume|speed/.test(a)).join(", ")} once open`);
  }
  // Back to as it was, for the other files (the volume and speed are remembered).
  await app.js(`const slider = document.querySelector('.bar input[aria-label="Volume"]');
    slider.value = "1";
    slider.dispatchEvent(new Event("input", { bubbles: true }));`);
  // A click steps to the next speed: 1.25× is five from 1× again.
  const speed = () => app.js(`return document.querySelector(".bar button.speed").textContent.trim()`);
  for (let i = 0; i < 5 && (await speed()) !== "1×"; i++) await app.click(".bar button.speed");
}
