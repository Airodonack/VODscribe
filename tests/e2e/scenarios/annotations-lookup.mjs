// Asking the file host about the VODs added from sites (lib/fileHost.mjs),
// and the setting that stops it. A VOD it has nothing for: asked once,
// then nothing offered and no gift button. One it has something for:
// offered, and Don't ask again turns the setting off, the dialog then
// saying where it's turned back on. With the setting off, a VOD added is
// never asked about, and its menu's "Check for annotations" says asking is
// turned off; turned back on in Settings › Privacy, the same item asks, and
// says when there's nothing.
//
// Offline: the VODs are the stand-in yt-dlp's (lib/siteVod.mjs).

import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { serveFiles } from "../lib/fileHost.mjs";
import { addSiteVod, withSiteVods } from "../lib/siteVod.mjs";

const DIALOG = '[role="dialog"][aria-label="Transcript and annotations"]';
const SETTING = "Ask the file host whether annotations exist for a VOD you add";

const offerShown = (app) => app.js(`return !!document.querySelector(arguments[0])`, DIALOG);
const toastSays = (app) => app.js(`return document.querySelector(".toast .message")?.textContent.trim() ?? ""`);

/** Opens VOD `id`'s menu (a right click on its row) and picks its "Check for annotations". */
async function checkFromMenu(app, id) {
  await app.js(
    `const row = document.querySelector('.vod[data-key="' + arguments[0] + '"]');
     const box = row.getBoundingClientRect();
     row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2,
       clientX: box.left + 20, clientY: box.top + 10 }));`,
    id,
  );
  await app.clickText("Check for annotations");
}

/** The setting's box in Settings › Privacy: whether it's ticked. */
async function settingTicked(app) {
  await app.openSettings("Privacy");
  return app.js(
    `const label = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label')].find((l) => l.textContent.trim() === arguments[0]);
     if (!label) throw new Error("no setting " + arguments[0]);
     return label.querySelector("input").checked;`,
    SETTING,
  );
}

export default {
  name: "annotations-lookup",
  expects: {
    "offer-stopped":
      'A small dialog titled "Transcript and annotations" says VODscribe won\'t ask about the VODs you add any more, and in gray that it can be turned back on in Settings › Privacy, with "Open Settings" and a highlighted "OK".',
    "lookup-setting":
      'Settings at Privacy: under the paragraphs about privacy, an unticked box "Ask the file host whether annotations exist for a VOD you add" with a gray line under it saying it sends the VOD\'s site and its id, nothing else.',
  },
  async run(app, ctx) {
    const host = serveFiles((method, at) =>
      at === "/annotations/dailymotion/xe2eask.json"
        ? { status: 200, headers: { ETag: '"v1"' }, body: "{}" }
        : { status: 404 },
    );
    try {
      await withSiteVods(ctx, async (app) => {
        const wrong = [];

        // Nothing published: asked once, nothing offered, no gift button.
        const none = await addSiteVod(app, "xe2enone");
        const known = await until(
          "the answer kept",
          async () => (await app.invoke("list_vods")).find((v) => v.id === none.id)?.annotations === "none",
          MINUTE,
        ).catch(() => false);
        if (!known) wrong.push("the VOD with nothing published isn't known to have none");
        await sleep(1500);
        if (await offerShown(app)) wrong.push("annotations were offered for a VOD with none");
        if (await app.js(`return !!document.querySelector('.vod[data-key="${none.id}"] button.gift')`))
          wrong.push("a VOD with nothing published has the gift button");
        const asked = host.requests.map((r) => `${r.method} ${r.path}`);
        if (asked.join() !== "HEAD /annotations/dailymotion/xe2enone.json")
          wrong.push(`for the VOD with none, the file host was asked: ${asked.join(", ")}`);

        // Published: offered; Don't ask again turns the setting off.
        await addSiteVod(app, "xe2eask");
        await until("the offer", () => offerShown(app), MINUTE);
        await app.clickText("Don't ask again");
        const said = await until(
          "where it's turned back on",
          () =>
            app.js(
              `const t = document.querySelector(arguments[0] + " .offer")?.textContent ?? "";
              return t.includes("Settings › Privacy") && t.replace(/\\s+/g, " ").trim()`,
              DIALOG,
            ),
          10_000,
        );
        await ctx.checkpoint("offer-stopped");
        if (!said.includes("won't ask about the VODs you add any more")) wrong.push(`after Don't ask again: "${said}"`);
        if ((await app.invoke("get_settings")).ask_for_annotations !== false)
          wrong.push("Don't ask again left the setting on");
        await app.clickText("OK");
        await until("the dialog closed", async () => !(await offerShown(app)), 10_000);

        // Off: a VOD added isn't asked about; its menu says asking is off.
        const before = host.requests.length;
        const off = await addSiteVod(app, "xe2eoff");
        await sleep(3000);
        await checkFromMenu(app, off.id);
        const toast = await until("the menu's toast", () => toastSays(app), 10_000).catch(() => "");
        if (toast !== "Looking for annotations is turned off in Settings › Privacy.")
          wrong.push(`the menu's toast: "${toast}"`);
        if (host.requests.length !== before)
          wrong.push(`with the setting off, the file host was asked: ${JSON.stringify(host.requests.slice(before))}`);
        if (await settingTicked(app)) wrong.push("Settings shows the setting on after Don't ask again");
        await ctx.checkpoint("lookup-setting");

        // Back on, from Settings: the menu asks, and says there's nothing.
        await app.js(
          `const label = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label')].find((l) => l.textContent.trim() === arguments[0]);
           label.querySelector("input").click();`,
          SETTING,
        );
        await until(
          "the setting saved",
          async () => (await app.invoke("get_settings")).ask_for_annotations === true,
          10_000,
        );
        await app.key("Escape");
        await checkFromMenu(app, off.id);
        const nothing = await until(
          "the menu's answer",
          async () => (await toastSays(app)).startsWith("No annotations") && toastSays(app),
          MINUTE,
        ).catch(async () => `(${await toastSays(app)})`);
        if (nothing !== "No annotations are published for this VOD yet.")
          wrong.push(`the menu's check says "${nothing}"`);
        if (!host.requests.some((r) => r.method === "HEAD" && r.path === "/annotations/dailymotion/xe2eoff.json"))
          wrong.push("the menu's check, with the setting on, didn't ask the file host");
        if (wrong.length) throw new Error(wrong.join("; "));
      });
    } finally {
      host.stop();
    }
  },
};
