// A site's VOD in its own player, left as it is (a release: no embed-takeover;
// site-embed.mjs is the other way): the owner's test VOD on Twitch (or
// YouTube, with E2E_SITE=youtube), played, jumped, paused and resumed from
// the app's controls. Nothing of the app's runs in their frame, the app's own
// player stays shut, and the time under the player follows theirs. The bar's
// sound (and, for YouTube, speed) is theirs, set through their embed API
// and as their player reports it; Twitch's has no speed, so the bar has
// none. The volume is set and muted on the bar before Play too, their player
// starts at it, muted, and it's kept: after a restart their player starts at
// the last one set. Not transcribed, the captions button is greyed, with no
// strip. On YouTube, first, a VOD with a transcript shows captions once:
// with ours off, theirs show in their player; with ours on, in the strip
// under it, theirs are turned off.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { makeSiteVod } from "../lib/data.mjs";
import { videos } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";
import { TEST_VODS, addTestVod, pressPlay } from "../lib/siteVod.mjs";

const SITE = process.env.E2E_SITE ?? "twitch";
const SLIDER = '.bar input[aria-label="Volume"]';

/** Listens, in the page, for the volume their player reports (Twitch's
 *  state, YouTube's volume changes): `window.__theirVolume`; and when
 *  YouTube's said it loaded and was ready: `window.__theirSaid`. */
const hearTheirVolume = (app) =>
  app.js(`window.addEventListener("message", (e) => {
    let d = e.data;
    try { if (typeof d === "string") d = JSON.parse(d); } catch { return; }
    if (d?.eventName === "UPDATE_STATE" && typeof d.params?.volume === "number")
      window.__theirVolume = { volume: d.params.volume, muted: d.params.muted };
    if (d?.event === "onVolumeChange" && typeof d.info?.volume === "number")
      window.__theirVolume = { volume: d.info.volume / 100, muted: d.info.muted };
    // When YouTube's player said it had loaded, and that it takes commands:
    // the bar's mute is sent at the first (embed.ts), before the second.
    if (d?.event === "initialDelivery" || d?.event === "onReady") (window.__theirSaid ??= {})[d.event] ??= performance.now();
  });`);

/** Sets the bar's volume slider to `value`, as a drag does. */
const slide = (app, value) =>
  app.js(
    `const slider = document.querySelector(arguments[0]);
     slider.value = arguments[1];
     slider.dispatchEvent(new Event("input", { bubbles: true }));`,
    SLIDER,
    value,
  );

/** Waits for their player to report `volume`, unmuted (or `muted`). */
const theirsAt = (app, volume, muted = false) =>
  until(
    `their player at ${volume * 100}%${muted ? ", muted" : ""}`,
    async () => {
      const v = await app.js(`return window.__theirVolume ?? null`);
      return v && Math.abs(v.volume - volume) < 0.01 && v.muted === muted && v;
    },
    10_000,
  ).catch((e) =>
    app.js(`return window.__theirVolume ?? null`).then((v) => {
      throw new Error(`${e.message}: their player said ${JSON.stringify(v)}`);
    }),
  );

/** Whether YouTube's own captions show in their player, at any of a few
 *  looks over `ms`: their caption box (near black, under white text) in
 *  the lower part of their frame (the test video's there is lighter). They say
 *  nothing once turned off, so this is read from the screen. */
async function theirCaptionsShow(app, ms) {
  const frame = await os.rectOf(app, "iframe");
  const band = {
    x: Math.round(frame.x + frame.w * 0.15),
    y: Math.round(frame.y + frame.h * 0.72),
    w: Math.round(frame.w * 0.7),
    h: Math.round(frame.h * 0.25),
  };
  let most = 0;
  for (const end = Date.now() + ms; Date.now() < end;) {
    const px = os.pixels(os.screenshot(), band, 140, 30);
    let dark = 0;
    for (let i = 0; i < px.length; i += 3) if (Math.max(px[i], px[i + 1], px[i + 2]) < 60) dark++;
    most = Math.max(most, dark / (px.length / 3));
    if (most > 0.02) return { shown: true, most };
    await new Promise((r) => setTimeout(r, 300));
  }
  return { shown: false, most };
}

/** Waits for `value` to be the volume kept (playerPrefs.svelte.ts). */
const keptAt = (app, value) =>
  until(
    `${value} kept`,
    async () => {
      const kept = (await app.invoke("kept_choices"))["vodscribe.volume"];
      if (kept === value) return true;
      throw new Error(`the volume kept is ${JSON.stringify(kept)}`);
    },
    10_000,
  );

export default {
  name: "site-plain",
  needs: ["network"],
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "youtube-captions-once":
      "A YouTube VOD playing in YouTube's own player (the test video's clock and Segment text): no caption text inside their player, and the bar's CC button shows on.",
  },
  async run(app, ctx) {
    if (await app.invoke("embed_takeover")) ctx.skip("this build takes over the sites' players (site-embed)");
    if (SITE === "youtube") app = await captionsOnce(app, ctx);
    const vod = await addTestVod(app, SITE);
    // Anything the app's frame script (embed_shim.js) says, from any frame:
    // injected, it says so even in a frame left plain ({ kind: "plain" }).
    await app.js(`window.__fromFrames = [];
      window.addEventListener("message", (e) => e.data?.vodscribeShim && window.__fromFrames.push(e.data.vodscribeShim.kind));`);
    await hearTheirVolume(app);
    await app.openVod(vod.id);
    await until(
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    // Before Play, nothing of theirs loaded: the volume is on the bar, set
    // there and kept, and muted there, and their player starts at it, muted.
    if (!(await app.js(`return !!document.querySelector(arguments[0])`, SLIDER)))
      throw new Error("before Play, the bar has no volume");
    await slide(app, "0.6");
    await keptAt(app, "0.6");
    await app.click('.bar button[aria-label="Mute"]');
    // Playing: the time under the player runs on, from their player's reports.
    const started = await app.plays({ press: () => pressPlay(app), ms: 2 * MINUTE });
    await theirsAt(app, 0.6, true);
    // Still muted a moment later: not unmuted as their player settles.
    await new Promise((r) => setTimeout(r, 3000));
    await theirsAt(app, 0.6, true);
    if (!(await app.js(`return !!document.querySelector('.bar button[aria-label="Unmute"]')`)))
      throw new Error("muted before Play, the bar doesn't show Unmute once playing");
    await app.click('.bar button[aria-label="Unmute"]');
    await theirsAt(app, 0.6);
    const loaded = await app.js(`return window.__theirSaid ?? {}`);
    const gap = loaded.onReady - loaded.initialDelivery;
    ctx.log(
      `set to 60% and muted before Play: their player started at it, muted${Number.isFinite(gap) ? ` (muted at their loading, ${Math.round(gap)} ms before they were ready)` : ""}`,
    );
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    const ours = () => app.invoke("player_set_speed", { speed: 1 }).catch(() => null);
    const label = () => app.js(`return document.querySelector(".bar button.control")?.getAttribute("aria-label")`);
    ctx.log(`playing: the time at ${started} s`);
    // Their frame as it is: nothing of the app's ran in it (its script says
    // so wherever it runs), and the app's own player never opened.
    const said = await app.js(`return window.__fromFrames`);
    if (said.length) throw new Error(`the app's script ran in their frame: it said ${said.join(", ")}`);
    if (await ours()) throw new Error("the app's own player opened for a site's VOD");
    if ((await app.js(`return document.querySelector("iframe")?.name ?? ""`)) !== "vodscribe-plain")
      throw new Error("their frame isn't the plain one");

    // The bar: sound, and speed where their API has one; captions greyed, no strip.
    const bar = await app.js(`return {
      mute: !!document.querySelector('.bar button[aria-label="Mute"], .bar button[aria-label="Unmute"]'),
      volume: !!document.querySelector('.bar input[aria-label="Volume"]'),
      speed: !!document.querySelector(".bar button.speed"),
      captions: document.querySelector('.bar button[aria-label="Captions"]')?.getAttribute("aria-disabled") === "true",
      strip: !!document.querySelector(".caption-strip"),
    }`);
    const want = { mute: true, volume: true, speed: SITE === "youtube", captions: true, strip: false };
    if (Object.keys(want).some((k) => bar[k] !== want[k]))
      throw new Error(`the bar has ${JSON.stringify(bar)}, not ${JSON.stringify(want)}`);
    // The volume set on the bar: their player says it's at it, the bar stays
    // there, and it's kept, as every player's volume is.
    await slide(app, "0.3");
    const theirs = await theirsAt(app, 0.3);
    await wait(1500);
    const shown = await app.js(`return document.querySelector(arguments[0]).value`, SLIDER);
    if (shown !== "0.3") throw new Error(`their player said ${JSON.stringify(theirs)}, and the bar shows ${shown}`);
    await keptAt(app, "0.3");
    // Muted from the bar: theirs is, and unmuted, back at 30%.
    await app.click('.bar button[aria-label="Mute"]');
    await until("their player muted", async () => (await app.js(`return window.__theirVolume`)).muted, 10_000);
    await app.click('.bar button[aria-label="Unmute"]');
    await until("their player unmuted", async () => !(await app.js(`return window.__theirVolume`)).muted, 10_000);
    ctx.log(`the bar: ${JSON.stringify(bar)}; their player went to 30%, muted and back`);

    // A jump (L: 10 s each): their player goes there, and the time with it.
    const before = await app.shownTime();
    for (let i = 0; i < 3; i++) await app.key("l");
    const jumped = await until(
      "the time past the jump",
      async () => {
        const t = await app.shownTime();
        return t >= before + 25 && t;
      },
      MINUTE,
    );
    ctx.log(`jumped: from ${before} s to ${jumped} s`);

    // Paused from the app's controls: the time stands, and the button says so.
    await app.key(" ");
    await until("Play shown", async () => (await label()) === "Play", 10_000);
    await wait(1500);
    const a = await app.shownTime();
    await wait(2500);
    const b = await app.shownTime();
    ctx.log(`paused: ${a} s, then ${b} s`);
    if (a !== b) throw new Error(`paused, the time went from ${a} s to ${b} s`);

    // Resumed: on from where it stood, not back.
    await app.key(" ");
    const resumed = await until(
      "the time running again",
      async () => {
        const t = await app.shownTime();
        return t > b && t;
      },
      MINUTE,
    );
    ctx.log(`resumed: at ${resumed} s`);
    if ((await label()) !== "Pause") throw new Error("playing again, the button doesn't say Pause");

    // Restarted: the bar shows the volume kept, and their player starts at it.
    app = await ctx.restart();
    await app.toLibrary();
    await hearTheirVolume(app);
    await app.openVod(vod.id);
    await until(
      "the volume on the bar",
      () => app.js(`return document.querySelector(arguments[0])?.value`, SLIDER),
      MINUTE,
    );
    const barVolume = await app.js(`return document.querySelector(arguments[0]).value`, SLIDER);
    if (barVolume !== "0.3") throw new Error(`after a restart, the bar's volume is ${barVolume}`);
    await app.plays({ press: () => pressPlay(app), ms: 2 * MINUTE });
    await theirsAt(app, 0.3);
    ctx.log("after a restart: their player started at 30%");
  },
};

/** YouTube's VOD with a transcript (a file's, transcribed here, made the
 *  test VOD's library entry, removed after): with ours off their captions
 *  show in their player, and with ours on, in the strip under it, theirs go
 *  off. The app, started again. */
async function captionsOnce(app, ctx) {
  if (os.cannotSee()) {
    ctx.log("their captions not looked at: the screen can't be read here");
    return app;
  }
  await app.toLibrary();
  const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
  const vod = await app.transcribe(added.id);
  await ctx.quit();
  makeSiteVod(vod.id, "youtube", TEST_VODS.youtube.id);
  app = await ctx.start();
  await app.toLibrary();
  // When their captions module says it loaded, with a track: theirs are on.
  await app.js(`window.addEventListener("message", (e) => {
    let d = e.data;
    try { if (typeof d === "string") d = JSON.parse(d); } catch { return; }
    if (d?.event === "apiInfoDelivery" && d.info?.captions?.track?.languageCode) window.__theirTrack = d.info.captions.track.languageCode;
  });`);
  await app.openVod(vod.id);
  await until(
    "the Play button",
    () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
    MINUTE,
  );
  const strip = () => app.js(`return !!document.querySelector(".caption-strip")`);
  // Ours off (captions are a choice kept: on, as the app starts out).
  if (await strip()) await app.key("C", { shift: true });
  await until("no strip, with ours off", async () => !(await strip()), 10_000);
  await app.plays({ press: () => pressPlay(app), ms: 2 * MINUTE });
  const track = await until("their captions loaded", () => app.js(`return window.__theirTrack ?? null`), 30_000);
  const off = await theirCaptionsShow(app, 8000);
  if (!off.shown) throw new Error(`with ours off, their captions (${track}) don't show in their player`);
  // Ours on: theirs go.
  await app.key("C", { shift: true });
  await until("the strip, with ours on", strip, 10_000);
  await new Promise((r) => setTimeout(r, 1500));
  const on = await theirCaptionsShow(app, 6000);
  if (on.shown) throw new Error("with ours on, their captions still show in their player");
  await ctx.checkpoint("youtube-captions-once");
  const pct = (f) => `${Math.round(f * 1000) / 10}%`;
  ctx.log(
    `with ours off their captions (${track}) showed in their player; with ours on, none (${pct(on.most)} dark, against ${pct(off.most)})`,
  );
  await app.invoke("delete_vod", { id: vod.id });
  return app;
}
