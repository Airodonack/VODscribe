// Asking the file host about the VODs added from sites (lib/fileHost.mjs),
// and the settings that stop the offer and the asking. A VOD it has nothing
// for: asked once, then nothing offered and no gift button. One it has
// something for: offered, and "No, and don't ask again" turns the offer's
// switch off, and only it, the dialog then saying where it's turned back on.
// The next VOD published is still asked about, and not offered; its gift
// button offers it. With asking off in Settings › Privacy, a VOD added is
// never asked about, and its menu's "Check for a VODscription" says asking is
// turned off; turned back on in Settings › Privacy, the same item asks, and
// says when there's nothing. A VOD from another site (Dailymotion) is never
// asked about, the setting on: the file host has only Twitch's, YouTube's and
// Kick's, and its menu's item says so.
//
// Offline: the VODs are the stand-in yt-dlp's (lib/siteVod.mjs).

import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { notFilter, serveFiles } from "../lib/fileHost.mjs";
import { addSiteVod, OFFER, offerShown, withSiteVods } from "../lib/siteVod.mjs";

const SETTING = "Enable VODscription annotations for VODs";
const gift = (id) => `.vod[data-key="${id}"] button.gift`;

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
      'A small dialog titled "VODscription" says VODscribe won\'t ask about the VODs you add any more, and in gray that it can be turned back on in Settings › Privacy, with "Open Settings" and a highlighted "OK".',
    "lookup-setting":
      'Settings at Privacy: under the paragraphs about privacy, an unticked box "Enable VODscription annotations for VODs" with a gray line under it saying it sends the VOD\'s site and its id, nothing else, then an unticked box "Ask to use a VODscription when one is found" with its own gray line.',
  },
  async run(app, ctx) {
    const host = serveFiles((method, at) =>
      at === "/annotations/youtube/xe2eask.json" || at === "/annotations/youtube/xe2equiet.json"
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
        if (await app.js(`return !!document.querySelector(arguments[0])`, gift(none.id)))
          wrong.push("a VOD with nothing published has the gift button");
        // The filter the app asks for at its start (none published here) aside.
        const asked = host.requests.filter(notFilter).map((r) => `${r.method} ${r.path}`);
        if (asked.join() !== "HEAD /annotations/youtube/xe2enone.json")
          wrong.push(`for the VOD with none, the file host was asked: ${asked.join(", ")}`);

        // Published: offered; "No, and don't ask again" turns the offer off, and only it.
        await addSiteVod(app, "xe2eask");
        await until("the offer", () => offerShown(app), MINUTE);
        await app.clickText("No, and don't ask again");
        const said = await until(
          "where it's turned back on",
          () =>
            app.js(
              `const t = document.querySelector(arguments[0] + " .offer")?.textContent ?? "";
              return t.includes("Settings › Privacy") && t.replace(/\\s+/g, " ").trim()`,
              OFFER,
            ),
          10_000,
        );
        await ctx.checkpoint("offer-stopped");
        if (!said.includes("won't ask about the VODs you add any more"))
          wrong.push(`after "No, and don't ask again": "${said}"`);
        const stopped = await app.invoke("get_settings");
        if (stopped.ask_to_use_vodscription !== false) wrong.push("No, and don't ask again left the offer on");
        if (stopped.ask_for_annotations !== true) wrong.push("No, and don't ask again turned asking the file host off");
        await app.clickText("OK");
        await until("the dialog closed", async () => !(await offerShown(app)), 10_000);

        // The offer off: the next VOD published is still asked about, its gift
        // button shows, nothing is offered; the gift button offers it.
        const quiet = await addSiteVod(app, "xe2equiet");
        await until(
          "its gift button",
          () => app.js(`return !!document.querySelector(arguments[0])`, gift(quiet.id)),
          MINUTE,
        );
        await sleep(1500);
        if (await offerShown(app)) wrong.push("offered after an add with the offer turned off");
        await app.click(gift(quiet.id));
        await until("the offer from the gift button", () => offerShown(app), 10_000).catch(() =>
          wrong.push("the gift button offered nothing with the offer turned off"),
        );
        await app.clickText("Not now");
        await until("the dialog closed", async () => !(await offerShown(app)), 10_000);

        // Asking turned off in Settings › Privacy.
        if (!(await settingTicked(app))) wrong.push("Settings shows asking off after No, and don't ask again");
        await app.js(
          `const label = [...document.querySelectorAll('[role="dialog"][aria-label="Settings"] label')].find((l) => l.textContent.trim() === arguments[0]);
           label.querySelector("input").click();`,
          SETTING,
        );
        await until(
          "asking off, saved",
          async () => (await app.invoke("get_settings")).ask_for_annotations === false,
          10_000,
        );
        await app.key("Escape");

        // Off: a VOD added isn't asked about; its menu says asking is off.
        const before = host.requests.length;
        const off = await addSiteVod(app, "xe2eoff");
        await sleep(3000);
        await app.rowMenu(off.id, "Check for a VODscription");
        const toast = await until("the menu's toast", () => app.toastSays(), 10_000).catch(() => "");
        if (toast !== "VODscription is turned off in Settings › Privacy.") wrong.push(`the menu's toast: "${toast}"`);
        if (host.requests.length !== before)
          wrong.push(`with the setting off, the file host was asked: ${JSON.stringify(host.requests.slice(before))}`);
        if (await settingTicked(app)) wrong.push("Settings shows asking on after it was turned off");
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
        await app.rowMenu(off.id, "Check for a VODscription");
        const nothing = await until(
          "the menu's answer",
          async () => (await app.toastSays()).startsWith("There's no VODscription") && app.toastSays(),
          MINUTE,
        ).catch(async () => `(${await app.toastSays()})`);
        if (nothing !== "There's no VODscription for this VOD yet.") wrong.push(`the menu's check says "${nothing}"`);
        if (!host.requests.some((r) => r.method === "HEAD" && r.path === "/annotations/youtube/xe2eoff.json"))
          wrong.push("the menu's check, with the setting on, didn't ask the file host");
        await until("the toast gone", async () => !(await app.toastSays()), MINUTE).catch(() => {});

        // Another site's VOD: never asked about, and its menu says why.
        const since = host.requests.length;
        const other = await addSiteVod(app, "xe2eother", "dailymotion");
        if (other.platform !== "dailymotion") wrong.push(`the other site's VOD is ${other.platform}'s`);
        await sleep(3000);
        await app.rowMenu(other.id, "Check for a VODscription");
        const why = await until("the menu's toast", () => app.toastSays(), 10_000).catch(() => "");
        if (why !== "VODscription is only for Twitch, YouTube and Kick VODs.")
          wrong.push(`for another site's VOD, the menu's toast: "${why}"`);
        if (host.requests.length !== since)
          wrong.push(`for another site's VOD, the file host was asked: ${JSON.stringify(host.requests.slice(since))}`);
        if (wrong.length) throw new Error(wrong.join("; "));
      });
    } finally {
      host.stop();
    }
  },
};
