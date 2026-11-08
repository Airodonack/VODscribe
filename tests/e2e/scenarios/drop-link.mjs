// A VOD link dragged in from another app and let go on the library's box
// adds it, as a pasted one does: a link as a browser offers one, then the
// same as selected text. One gesture each, as a person drags: pressed in
// the other app (lib/fileManager.mjs `openLinkGiver`, beside the window),
// carried over the window, let go on the box. Then a link of the page's
// own, dragged onto the box with the system's pointer, slowly and then in a
// flick: a drag that starts inside the app adds nothing.
//
// - Linux (X11): a GTK window offering text/uri-list, _NETSCAPE_URL and
//   text, then text alone.
// - macOS: a test app built with swiftc offering a URL and its text, as
//   Safari does, then text alone.
// - Windows: a WinForms form offering UniformResourceLocatorW and text, as
//   Edge does, then text alone.
//
// Where a shell script can stand in for yt-dlp (lib/siteVod.mjs: not on
// Windows), each link's VOD is added and nothing shows an error; on
// Windows the link is looked up and fails (it's on a name that never
// resolves), and what counts is that the page took it up (a row "Adding
// <link>").

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { noLinkGiver, openLinkGiver } from "../lib/fileManager.mjs";
import * as os from "../lib/os.mjs";
import { linkTo, withSiteVods } from "../lib/siteVod.mjs";

/** The add box. */
const BOX = 'input[aria-label="VOD link"]';

export default {
  name: "drop-link",
  async run(app, ctx) {
    const why = os.unavailable() ?? noLinkGiver();
    if (why) ctx.skip(why);
    const work = path.join(ctx.out, "drop-link");
    // The stand-in yt-dlp where there can be one, so each link's VOD is added.
    if (process.platform === "win32") return dropLinks(app, ctx, work, false);
    await withSiteVods(ctx, (app) => dropLinks(app, ctx, work, true));
  },
  // On Windows the links' lookups fail, and their error shows under the box;
  // elsewhere `dropLinks` checks that nothing shows one.
  allowErrors: true,
};

async function dropLinks(app, ctx, work, added) {
  await app.toLibrary();
  // Narrowed, for room beside it (the Mac's screen may be 1280 wide).
  if (process.platform === "darwin") await resizeInner(app, 660, await app.js("return innerHeight"));
  await watchAdding(app);
  for (const [n, textOnly] of [
    ["xe2edroplink", false],
    ["xe2edroptext", true],
  ]) {
    const link = linkTo(n);
    const giver = await openLinkGiver(app, ctx, path.join(work, n), link, { textOnly });
    try {
      await dragOnto(app, giver.from, BOX);
      // (Shown without its scheme: "Adding vods.example.invalid/<n>".)
      const shown = `Adding ${link.replace(/^https?:\/\//, "")}`;
      await until(`the page adding ${link}`, async () => (await adding(app)).includes(shown), 20_000).catch(
        async (e) => {
          ctx.log(`the giver said: ${giver.said().join(" | ")}`);
          offered(app, ctx);
          ctx.log(`the page was adding: ${JSON.stringify(await adding(app))}`);
          throw e;
        },
      );
      ctx.log(`dropped ${textOnly ? "as text" : "as a link"}: the page took up ${link}`);
      offered(app, ctx);
      if (added) {
        await until(
          `VOD ${n} in the library`,
          async () => (await app.invoke("list_vods")).some((v) => v.platform_id === n),
          MINUTE,
        );
      } else {
        // Its lookup over (it fails here): no row is left adding it.
        await until("no row left adding", () => app.js(`return !document.querySelector(".vod.pending")`), 2 * MINUTE);
      }
    } finally {
      giver.close();
    }
    await sleep(500);
  }
  await insideLink(app, ctx);
  if (added) {
    const errors = await app.errors();
    if (errors.length) throw new Error(`the page shows: ${errors.join(" | ")}`);
  }
}

/**
 * A link of the page's own, dragged onto the box with the system's pointer:
 * nothing is taken up. Carried as a person does (about a second), then
 * flicked there quickly (a fifth of a second), its window's native events
 * held back on their way to the page (`holdNative`) so that they all come
 * after the page's dragend, as they do for a drag that short on Windows.
 * The flick tests Windows and Linux, where the drop of a link of the page's
 * own reaches the page as a native drop with the link (Windows) or could
 * (Linux, through wry's uri-list). On macOS it can't fail either way: an
 * in-page drag goes to wry, whose drop gives file URLs only, so the link
 * never reaches the page natively; there it only shows the drag went.
 */
async function insideLink(app, ctx) {
  for (const [n, quick] of [
    ["xe2einsidelink", false],
    ["xe2einsidequick", true],
  ]) {
    await app.js(
      `const a = document.createElement("a");
       a.id = "e2e-inside-link"; a.href = arguments[0]; a.textContent = "A link of the page's own";
       a.style.cssText = "position: fixed; left: 45%; top: 55%; z-index: 99999; padding: 16px; background: #fd0; color: #000; font-size: 18px";
       document.body.append(a);`,
      linkTo(n),
    );
    try {
      const at = await os.rectOf(app, "#e2e-inside-link");
      await os.toFront(app);
      const from = { x: Math.round(at.x + at.w / 2), y: Math.round(at.y + at.h / 2) };
      if (quick) await holdNative(app, 1500);
      await (quick ? flickOnto : dragOnto)(app, from, BOX);
      await sleep(quick ? 4000 : 3000);
      if (quick) {
        const heard = await app.js(`return window.__native`);
        // Else the flick started no drag, and the step would show nothing. On
        // Windows the window's own drop target takes the drop; elsewhere the
        // web view keeps a drop from inside it, and the window hears it leave.
        const needed = process.platform === "win32" ? "tauri://drag-drop" : "tauri://drag-enter";
        if (!heard.includes(needed))
          throw new Error(`the flick never reached the window as a drag (it heard: ${heard.join(", ") || "nothing"})`);
        ctx.log(`the flick's native events, held back: ${heard.join(", ")}`);
      }
      const taken = (await adding(app)).filter((l) => l.includes(n));
      const how = quick ? "flicked" : "dragged";
      if (taken.length) throw new Error(`a link ${how} inside the app was taken up: ${taken.join(", ")}`);
      if ((await app.invoke("list_vods")).some((v) => v.platform_id === n))
        throw new Error(`a link ${how} inside the app was added`);
      ctx.log(`a link of the page's own, ${how} onto the box: nothing taken up`);
    } finally {
      // (Let through again whatever happened: held, they'd delay every later drag.)
      if (quick) await holdNative(app, 0);
      await app.js(`document.getElementById("e2e-inside-link")?.remove()`);
    }
  }
}

/** Keeps, in window.__adding, the label of every row the page shows adding something ("Adding <link>"), as it shows. */
function watchAdding(app) {
  return app.js(
    `window.__adding = [];
     const seen = () => {
       for (const row of document.querySelectorAll(".vod.pending")) {
         const label = row.getAttribute("aria-label");
         if (label && !window.__adding.includes(label)) window.__adding.push(label);
       }
     };
     window.__addingSeen?.disconnect();
     window.__addingSeen = new MutationObserver(seen);
     window.__addingSeen.observe(document.body, { childList: true, subtree: true, attributes: true });`,
  );
}

const adding = (app) => app.js(`return window.__adding`);

/** Pressed at `from`, moved past the drag's threshold, carried over the window, let go on `selector`'s middle. */
async function dragOnto(app, from, selector) {
  const box = await os.rectOf(app, selector);
  const on = { x: Math.round(box.x + box.w / 2), y: Math.round(box.y + box.h / 2) };
  await os.gesture([
    ["down", from.x, from.y],
    ["wait", 150],
    ["move", from.x - 10, from.y + 4],
    ["wait", 100],
    ["move", from.x - 40, from.y + 10],
    ["wait", 200],
    ...[0.25, 0.5, 0.75, 1].flatMap((f) => [
      ["move", Math.round(from.x + (on.x - from.x) * f), Math.round(from.y + (on.y - from.y) * f)],
      ["wait", 120],
    ]),
    ["move", on.x + 2, on.y],
    ["wait", 400],
    ["letgo", on.x + 2, on.y],
  ]);
}

/**
 * Holds back, by `ms`, the window's native drag events (Tauri's
 * "tauri://drag-*") on their way to the page's listeners, as an emit that
 * comes late; each is kept, by name, in window.__native. 0 lets them through
 * again. Its listeners are Tauri's registered callbacks, run by id
 * (`__TAURI_INTERNALS__.callbacks`, a Map), each wrapped in place.
 */
function holdNative(app, ms) {
  return app.js(
    `const callbacks = window.__TAURI_INTERNALS__.callbacks;
     const delay = arguments[0];
     if (delay === 0) {
       for (const [id, f] of callbacks) if (f.__held) callbacks.set(id, f.__held);
       return;
     }
     window.__native = [];
     for (const [id, f] of callbacks) {
       if (f.__held) continue;
       const held = (data) => {
         const name = data && data.event;
         if (typeof name !== "string" || !name.startsWith("tauri://drag")) return f(data);
         window.__native.push(name);
         setTimeout(() => f(data), delay);
       };
       held.__held = f;
       callbacks.set(id, held);
     }`,
    ms,
  );
}

/** Pressed at `from`, past the drag's threshold at once, and let go on `selector`'s middle, all in about a fifth of a second. */
async function flickOnto(app, from, selector) {
  const box = await os.rectOf(app, selector);
  const on = { x: Math.round(box.x + box.w / 2), y: Math.round(box.y + box.h / 2) };
  await os.gesture([
    ["down", from.x, from.y],
    ["wait", 40],
    ["move", from.x - 12, from.y + 4],
    ["wait", 40],
    ["move", on.x, on.y],
    ["wait", 40],
    ["move", on.x + 2, on.y],
    ["wait", 40],
    ["letgo", on.x + 2, on.y],
  ]);
}

/** What the app said each drag offered, where it says (its builds for the tests, through the bridge). */
function offered(app, ctx) {
  const said = (app.driver.proc?.stderr ?? "").split("\n").filter((l) => /^drop: (a drag|let go)/.test(l));
  for (const line of said.slice(-4)) ctx.log(`the app: ${line.slice("drop: ".length)}`);
}
