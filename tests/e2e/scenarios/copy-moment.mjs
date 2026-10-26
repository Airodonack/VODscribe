// The timestamp in the player's header copies a link to where the playhead
// is (Player.svelte `copyLink`), on a site's VOD: the owner's test VOD on
// Twitch (or YouTube or Kick, with E2E_SITE=youtube or kick). Opened at its
// start and played, the stamp comes up with the time playing; after a click
// further on the timeline, it follows. What's copied is read from the
// clipboard command as the page sends it (the app's IPC), not back from the
// system's clipboard, which the app may only write.

import { MINUTE, until } from "../lib/app.mjs";

const SITES = {
  youtube: "https://www.youtube.com/watch?v=IaIo3d6cUuo",
  twitch: "https://www.twitch.tv/videos/2901646294",
  kick: "https://kick.com/airodonack/videos/01a169a1-9aa8-7653-8a9e-390e694cc11a",
};
const SITE = process.env.E2E_SITE || "twitch";

/** The seconds in a copied link's time: YouTube's and Kick's `t=75s` or `t=75`, Twitch's `t=0h1m15s`. */
function linkTime(url) {
  const t = new URL(url).searchParams.get("t") ?? "";
  const parts = t.match(/^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s?)?$/);
  if (!t || !parts) throw new Error(`no time in the copied link ${url}`);
  return (Number(parts[1] ?? 0) * 60 + Number(parts[2] ?? 0)) * 60 + Number(parts[3] ?? 0);
}

export default {
  name: "copy-moment",
  needs: ["network"],
  async run(app, ctx) {
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
    // What the page asks the clipboard to hold, noted as the command goes
    // out (to the app's IPC, as player-seek.mjs traces the player's).
    await app.js(`
      window.__copied = [];
      const fetch = window.fetch;
      window.fetch = (url, init) => {
        if (decodeURIComponent(String(url)).split("/").pop() === "plugin:clipboard-manager|write_text")
          window.__copied.push(JSON.parse(init.body).text);
        return fetch(url, init);
      };`);

    /** Clicks the stamp: what it showed, and the link copied, checked against the time playing. */
    const copy = async (what) => {
      const { shown, link, played } = await until(
        `the stamp (${what})`,
        () =>
          app.js(`
            const stamp = document.querySelector("button.stamp");
            if (!stamp) return null;
            const seconds = (text) => text.split(":").reduce((total, part) => total * 60 + Number(part), 0);
            const shown = seconds(stamp.querySelector(".time").textContent);
            const played = seconds(document.querySelector("button.time .now > span:not(.widest)")?.textContent ?? "");
            const before = window.__copied.length;
            stamp.click();
            await new Promise((r) => setTimeout(r, 300));
            return window.__copied.length > before && { shown, link: window.__copied.at(-1), played };`),
        MINUTE,
      );
      const copied = linkTime(link);
      ctx.log(`${what}: the bar at ${played} s, the stamp at ${shown} s, copied ${link}`);
      if (Math.abs(shown - played) > 2 || Math.abs(copied - shown) > 1)
        throw new Error(`${what}: the bar was at ${played} s, the stamp showed ${shown} s and copied ${link}`);
      const feedback = await app.js(`return document.querySelector("button.stamp .copied-text")?.textContent ?? ""`);
      if (feedback !== "Copied") throw new Error(`${what}: the stamp said "${feedback}", not Copied`);
      return copied;
    };

    // 1. Opened at its start and played: the stamp is where it plays.
    await app.click('.bar button[aria-label="Play"]');
    await until("it to play a few seconds in", async () => (await app.shownTime()) >= 4, 2 * MINUTE);
    const first = await copy("playing from the start");

    // 2. A click further on the timeline: the stamp follows the playhead there.
    const target = await app.js(`
      const bar = document.querySelector(".scrubber");
      const box = bar.getBoundingClientRect();
      const at = { clientX: box.left + 0.6 * box.width, clientY: box.top + box.height / 2, button: 0,
                   bubbles: true, pointerId: 1, pointerType: "mouse", isPrimary: true };
      document.elementFromPoint(at.clientX, at.clientY).dispatchEvent(new PointerEvent("pointerdown", at));
      window.dispatchEvent(new PointerEvent("pointerup", at));
      return Number(bar.dataset.viewStart) + 0.6 * Number(bar.dataset.span);`);
    await until("the playhead at the click", async () => Math.abs((await app.shownTime()) - target) < 3, MINUTE);
    const second = await copy(`after a click at ${target.toFixed(1)} s`);
    if (second <= first) throw new Error(`the link after the click (${second} s) isn't past the first (${first} s)`);
  },
};
