// A site's VOD in its own player, left as it is (a release: no embed-takeover;
// site-embed.mjs is the other way): the owner's test VOD on Twitch (or
// YouTube, with E2E_SITE=youtube), played, jumped, paused and resumed from
// the app's controls. Nothing of the app's runs in their frame, the app's own
// player stays shut, and the time under the player follows theirs. The bar's
// sound (and, for YouTube, speed) is theirs, set through their embed API
// and as their player reports it; Twitch's has no speed, so the bar has
// none. Not transcribed, the captions button is greyed, with no strip.

import { MINUTE, until } from "../lib/app.mjs";
import { addTestVod, pressPlay } from "../lib/siteVod.mjs";

const SITE = process.env.E2E_SITE ?? "twitch";

export default {
  name: "site-plain",
  needs: ["network"],
  async run(app, ctx) {
    if (await app.invoke("embed_takeover")) ctx.skip("this build takes over the sites' players (site-embed)");
    const vod = await addTestVod(app, SITE);
    // Anything the app's frame script (embed_shim.js) says, from any frame:
    // injected, it says so even in a frame left plain ({ kind: "plain" }).
    // And the volume their player last reported (Twitch's state, YouTube's
    // volume changes).
    await app.js(`window.__fromFrames = [];
      window.addEventListener("message", (e) => e.data?.vodscribeShim && window.__fromFrames.push(e.data.vodscribeShim.kind));
      window.addEventListener("message", (e) => {
        let d = e.data;
        try { if (typeof d === "string") d = JSON.parse(d); } catch { return; }
        if (d?.eventName === "UPDATE_STATE" && typeof d.params?.volume === "number")
          window.__theirVolume = { volume: d.params.volume, muted: d.params.muted };
        if (d?.event === "onVolumeChange" && typeof d.info?.volume === "number")
          window.__theirVolume = { volume: d.info.volume / 100, muted: d.info.muted };
      });`);
    await app.openVod(vod.id);
    await until(
      "the Play button",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      MINUTE,
    );
    // Playing: the time under the player runs on, from their player's reports.
    const started = await app.plays({ press: () => pressPlay(app), ms: 2 * MINUTE });
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
    // The volume set on the bar: their player says it's at it, and the bar stays there.
    await app.js(`const slider = document.querySelector('.bar input[aria-label="Volume"]');
      slider.value = "0.3";
      slider.dispatchEvent(new Event("input", { bubbles: true }));`);
    const theirs = await until(
      "their player at 30%",
      async () => {
        const v = await app.js(`return window.__theirVolume ?? null`);
        return v && Math.abs(v.volume - 0.3) < 0.01 && !v.muted && v;
      },
      10_000,
    );
    await wait(1500);
    const shown = await app.js(`return document.querySelector('.bar input[aria-label="Volume"]').value`);
    if (shown !== "0.3") throw new Error(`their player said ${JSON.stringify(theirs)}, and the bar shows ${shown}`);
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
  },
};
