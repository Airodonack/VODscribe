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
// strip.

import { MINUTE, until } from "../lib/app.mjs";
import { addTestVod, pressPlay } from "../lib/siteVod.mjs";

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
  async run(app, ctx) {
    if (await app.invoke("embed_takeover")) ctx.skip("this build takes over the sites' players (site-embed)");
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
      captions: document.querySelector('.bar button[aria-label="Captions"]')?.disabled ?? null,
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
