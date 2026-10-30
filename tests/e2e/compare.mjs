// The same screens on two engines, compared: the app on WebKitGTK (the
// default build) and on Servo (the `servo` feature), each driven to the same
// states at the same page size, so each difference between them gets a name:
// a property one engine doesn't support, a value it computes differently, a
// box laid out elsewhere.
//
//   node tests/e2e/compare.mjs capture <the app> --engine webkit|servo --out <dir>
//                              [--size 1280x800] [--small 900x600] [--video <file>]
//   node tests/e2e/compare.mjs report <webkit dir> <servo dir> --out <dir>
//
// capture runs one engine (each runs where its driver does: WebKit's inside
// the e2e box, Servo's on the host; lib/drivers.mjs), on the data folder
// XDG_DATA_HOME names (emptied first, as run.mjs does: E2E_RESET_DATA=1),
// which must already hold the tools and the speech model (bin, models): the
// setup screen is shown by holding them aside for its one start, and the
// transcript comes from the model on disk, never a download. At each screen
// it saves the page's screenshot (shot.png) and dump.json: every element
// shown, by its path in the document (tag and place among its siblings,
// the same in both engines: one page bundle), its box, its scroll sizes and
// every computed property, and the fonts' state. Once, at the end: the
// page's style rules as each engine's CSS parser kept them, and whether the
// engine supports each declaration, selector and media query the app's
// stylesheets use (meta.json).
//
// report pairs the two captures' elements by path and writes
// <out>/report.html (with the pictures beside it, and the pixel diff from
// ImageMagick's `compare` on PATH or in MAGICK_BIN) and report.json: per
// screen, the properties whose computed values differ (by property, with
// counts and examples), the boxes more than 1 px apart (the first of each
// chain, whose parent agrees, apart), the properties Servo leaves at their
// initial value where WebKit doesn't, and those WebKit reports that Servo
// doesn't expose; and across screens, what each engine's parser dropped.

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { App, until } from "./lib/app.mjs";
import { dataDirs, resetData } from "./lib/data.mjs";
import * as drivers from "./lib/drivers.mjs";
import { cssUsed } from "./lib/css-used.mjs";
import { appPid } from "./lib/memory.mjs";
import * as os from "./lib/os.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SEARCH = 'input[placeholder="Search for a phrase…"]';
const DEFAULT_VIDEO = "/mnt/data/vodscribe/agents/test-video/vodscribe-test-video.mp4";

const { values: opts, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    engine: { type: "string" },
    out: { type: "string" },
    size: { type: "string", default: "1280x800" },
    small: { type: "string", default: "900x600" },
    video: { type: "string", default: DEFAULT_VIDEO },
    // The other engine's capture: its property names asked for too, so one this engine
    // doesn't list shows as "" ("not exposed") rather than not at all.
    ask: { type: "string" },
  },
});
// ---------------------------------------------------------------- capture

/** "1280x800" as { width, height }. */
function parseSize(text) {
  const [width, height] = text.split("x").map(Number);
  if (!(width > 0 && height > 0)) throw new Error(`a size is WxH, not ${text}`);
  return { width, height };
}

async function capture(binary) {
  const out = path.resolve(opts.out);
  const size = parseSize(opts.size);
  const small = parseSize(opts.small);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  const log = (what) => console.log(`[${opts.engine}] ${what}`);
  const screens = [];
  // The driver of the engine: the webdriver one (WebKitWebDriver) or Servo's.
  const start = async () => {
    const driver = await drivers[opts.engine === "servo" ? "servo" : "webdriver"](binary);
    // Servo's driver has no focus() or proc: App.focused asks the OS (xdotool) by the app's pid.
    driver.focus ??= async () => {};
    if (!driver.proc) {
      const pid = appPid(binary);
      driver.proc = { exe: binary, pid: pid && Number(pid) };
    }
    const app = new App(driver, log);
    await app.ready();
    return app;
  };
  const shoot = async (app, name, note = "") => {
    await settle(app);
    const dir = path.join(out, name);
    mkdirSync(dir, { recursive: true });
    const dump = await dumpPage(app);
    writeFileSync(path.join(dir, "dump.json"), JSON.stringify({ ...dump, note }));
    writeFileSync(path.join(dir, "page.png"), await app.driver.screenshot());
    // The window as the screen shows it (as run.mjs's checkpoints take it): WebKitWebDriver's
    // screenshot comes back black on Xvfb, and the app's own player draws outside the page.
    try {
      writeFileSync(path.join(dir, "shot.png"), os.crop(os.screenshot(), await os.windowRect(app)));
    } catch (e) {
      log(`${name}: no picture from the screen (${e.message}): the driver's instead`);
      copyFileSync(path.join(dir, "page.png"), path.join(dir, "shot.png"));
    }
    screens.push({
      name,
      note,
      page: `${dump.page.innerWidth}x${dump.page.innerHeight}`,
      elements: dump.elements.length,
    });
    log(`${name}: ${dump.elements.length} elements at ${dump.page.innerWidth}x${dump.page.innerHeight}`);
  };
  const failed = [];
  // A screen that can't be reached is said in meta.json and the report, not fatal to the rest.
  const step = async (name, fn) => {
    try {
      await fn();
    } catch (e) {
      failed.push({ name, error: e.message });
      log(`${name}: FAILED: ${e.message}`);
    }
  };

  resetData();
  const data = dataDirs()[0];
  for (const d of ["bin", "models"])
    if (!existsSync(path.join(data, d))) throw new Error(`${data} has no ${d}: seed it with the tools and the model`);

  // The setup screen: the tools and the model held aside for one start (beside the data folder, not in it:
  // resetData would delete anything else in it), so the app asks for them as a new install does.
  const held = `${data}.held`;
  rmSync(held, { recursive: true, force: true });
  mkdirSync(held, { recursive: true });
  const kept = ["bin", "models", "gpu"].filter((d) => existsSync(path.join(data, d)));
  for (const d of kept) renameSync(path.join(data, d), path.join(held, d));
  let app;
  try {
    app = await start();
    await step("01-setup", async () => {
      await until(
        "the setup screen",
        () =>
          app.js(`return [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Download")`),
        3 * 60_000,
      );
      await setSize(app, size);
      await shoot(app, "01-setup", "First start, no tools or model: the setup screen (nothing downloaded).");
    });
    await app.driver.close();
  } finally {
    for (const d of kept) renameSync(path.join(held, d), path.join(data, d));
    rmSync(held, { recursive: true, force: true });
  }
  // What that start left (its settings, an empty library) gone: the rest starts as the first start with tools would.
  resetData();

  app = await start();
  let probes = null;
  // The names this engine lists itself, for the other capture's --ask.
  let own = [];
  try {
    await app.toLibrary();
    await setSize(app, size);
    await shoot(app, "02-library-empty", "The library, empty.");
    own = JSON.parse(readFileSync(path.join(out, "02-library-empty", "dump.json"), "utf8")).own;

    const [vod] = await app.addFiles([opts.video]);
    // An add opens what it added: transcribed first (the model on disk), then closed, for the library.
    await app.transcribe(vod.id);
    await closePlayer(app);
    await waitImages(app);
    await shoot(app, "03-library-video", "The library with the test video added and transcribed, nothing open.");

    await step("04-search", async () => {
      const lines = await app.invoke("transcript", { vodId: vod.id });
      // The longest word of the first line with one: the same transcript, so the same query on both.
      const word =
        lines
          .flatMap((l) => l.text.split(/[^\p{L}']+/u))
          .filter((w) => w.length >= 5)
          .sort((a, b) => b.length - a.length)[0] ?? "the";
      await app.type(SEARCH, word.toLowerCase());
      await until("a search result", () => app.js(`return !!document.querySelector(".results button.row")`), 60_000);
      await shoot(app, "04-search", `A search for "${word.toLowerCase()}" with its results.`);
    });

    await step("05-dialog", async () => {
      await app.click("button.filter");
      await until(
        "the search filter",
        () => app.js(`return !!document.querySelector('[role="dialog"][aria-label="Search filter"]')`),
        10_000,
      );
      await shoot(app, "05-dialog", "The search filter's popover (a dialog), open over the results.");
      await app.click("button.filter");
      await until(
        "the search filter closed",
        () => app.js(`return !document.querySelector('[role="dialog"][aria-label="Search filter"]')`),
        10_000,
      );
    });
    await app.type(SEARCH, "");
    await app.blur(SEARCH);

    await step("06-player", async () => {
      await app.openPaused(vod.id);
      await app.toStart();
      // A second at a time, each landed before the next (as clipOneToThree steps).
      for (let i = 1; i <= 5; i++) {
        await app.key("ArrowRight", { shift: true });
        await until(`the player at ${i} s`, async () => (await app.shownTime()) === i, 10_000);
      }
      await shoot(app, "06-player", "The player, paused at 5 s, its transcript beside it.");
    });

    await step("07-captions", async () => {
      const clicked = await app.js(
        `const b = [...document.querySelectorAll(".bar button")].find((b) => /caption/i.test(b.getAttribute("aria-label") ?? ""));
         if (!b || b.disabled) return null;
         const label = b.getAttribute("aria-label");
         b.click();
         return label;`,
      );
      if (!clicked) throw new Error("no captions button that can be pressed");
      await sleep(500);
      await shoot(app, "07-captions", `Captions on (the bar's "${clicked}" pressed), at 5 s.`);
      await app.js(
        `[...document.querySelectorAll(".bar button")].find((b) => /caption/i.test(b.getAttribute("aria-label") ?? ""))?.click()`,
      );
    });

    await step("08-clip-bar", async () => {
      await app.clipOneToThree(vod.id);
      await until("the chip to show it made", () => app.js(`return !!document.querySelector(".chip.ready")`), 10_000);
      await shoot(app, "08-clip-bar", "One clip, 1 to 3 s, in the clips bar; paused at 2 s.");
    });

    await step("09-small-player", async () => {
      await setSize(app, small);
      await shoot(app, `09-player-${opts.small}`, `The player at ${opts.small}.`);
    });
    await step("10-small-library", async () => {
      await closePlayer(app);
      await shoot(app, `10-library-${opts.small}`, `The library at ${opts.small}, nothing open.`);
    });
    await setSize(app, size);

    await step("11-settings", async () => {
      await app.openSettings("Appearance");
      await shoot(app, "11-settings", "Settings, at Appearance.");
      await app.openSettings("Transcription");
      await shoot(app, "12-settings-transcription", "Settings, at Transcription.");
    });
    probes = await probePage(app);
  } finally {
    await app.driver.close();
  }
  writeFileSync(
    path.join(out, "meta.json"),
    JSON.stringify(
      { engine: opts.engine, binary, size: opts.size, small: opts.small, own, screens, failed, ...probes },
      null,
      2,
    ),
  );
  log(
    `captured ${screens.length} screens${failed.length ? `; ${failed.length} failed: ${failed.map((f) => f.name).join(", ")}` : ""}`,
  );
  if (failed.length) process.exitCode = 1;
}

/** Closes the open VOD (a click on its row, as a person would), back to the library. */
async function closePlayer(app) {
  // (As controls.mjs does: the row's main part, not its buttons; the library's home screen then.)
  if (!(await app.js(`return !!document.querySelector(".home")`))) await app.click(".list .vod.selected .main");
  await until("the library, nothing open", () => app.js(`return !!document.querySelector(".home")`), 10_000);
}

/** Waits for the page's images to have loaded (the library's thumbnails). */
function waitImages(app) {
  return until("the images loaded", () => app.js(`return [...document.images].every((i) => i.complete)`), 20_000).catch(
    () => {},
  );
}

/**
 * Makes the page `size` CSS px with the e2e build's own command (both
 * builds have it), corrected once by what the window adds around the page,
 * and waits for it to hold still; says what it got if it isn't that.
 */
async function setSize(app, { width, height }) {
  const inner = () => app.js(`return [innerWidth, innerHeight]`);
  let ask = [width, height];
  for (let i = 0; i < 3; i++) {
    await app.invoke("e2e_set_inner_size", { width: ask[0], height: ask[1] });
    let last = null;
    const got = await until(
      "the page to hold its size",
      async () => {
        await sleep(300);
        const now = await inner();
        const same = last && now[0] === last[0] && now[1] === last[1];
        last = now;
        return same && now;
      },
      10_000,
    ).catch(() => last);
    if (got[0] === width && got[1] === height) return;
    ask = [ask[0] + width - got[0], ask[1] + height - got[1]];
  }
  app.log(`the page is ${(await inner()).join("x")}, not ${width}x${height}`);
}

/** Before a picture: the fonts loaded, two frames drawn, and a moment for transitions to end. */
async function settle(app) {
  await app.js(`
    if (document.fonts?.ready) await Promise.race([document.fonts.ready, new Promise((r) => setTimeout(r, 3000))]);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));`);
  await sleep(800);
}

/** Collects the elements in the page (kept in it, window.__compare) and says how many. */
const COLLECT = `
  const skip = new Set(["script", "style", "link", "meta", "head", "title", "noscript", "template", "base"]);
  // Svelte's scoped classes: the same in both engines, but noise in a label.
  const label = (el) => {
    let s = el.localName;
    if (el.id) s += "#" + el.id;
    const cls = [...el.classList].filter((c) => !/^svelte-[a-z0-9]+$/i.test(c));
    if (cls.length) s += "." + cls.join(".");
    return s;
  };
  const list = [];
  const walk = (el, key, readable) => {
    const cs = getComputedStyle(el);
    if (cs.display === "none") return;
    list.push({ el, key, label: readable });
    // An icon's insides are its picture, not layout.
    if (el.localName === "svg") return;
    const seen = {};
    for (const child of el.children) {
      const tag = child.localName;
      if (skip.has(tag)) continue;
      seen[tag] = (seen[tag] ?? 0) + 1;
      walk(child, key + ">" + tag + ":" + seen[tag], readable + " > " + label(child));
    }
  };
  walk(document.documentElement, "html", "html");
  const rootStyle = getComputedStyle(document.documentElement);
  const names = Array.from({ length: rootStyle.length }, (_, i) => rootStyle.item(i));
  window.__compare = { list, names };
  return { count: list.length, names };
`;

/** Elements [from, to) of the collected list, as JSON: values interned in `values`. */
const DUMP_RANGE = `
  const [from, to, names] = arguments;
  const { list } = window.__compare;
  const values = [];
  const index = new Map();
  const intern = (v) => {
    let i = index.get(v);
    if (i === undefined) { i = values.length; values.push(v); index.set(v, i); }
    return i;
  };
  const round = (n) => Math.round(n * 100) / 100;
  const styleOf = (cs) => names.map((n) => intern(cs.getPropertyValue(n)));
  const out = [];
  for (const { el, key, label } of list.slice(from, to)) {
    const r = el.getBoundingClientRect();
    const own = [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join("").trim();
    const entry = {
      key, label,
      text: own.slice(0, 40),
      rect: [round(r.x), round(r.y), round(r.width), round(r.height)],
      boxes: [el.clientWidth, el.clientHeight, el.offsetWidth ?? null, el.offsetHeight ?? null, el.scrollWidth, el.scrollHeight, el.scrollTop, el.scrollLeft],
      style: styleOf(getComputedStyle(el)),
    };
    for (const pseudo of ["::before", "::after"]) {
      try {
        const ps = getComputedStyle(el, pseudo);
        const content = ps.getPropertyValue("content");
        if (content && content !== "none" && content !== "normal") (entry.pseudo ??= {})[pseudo] = styleOf(ps);
      } catch {}
    }
    out.push(entry);
  }
  return JSON.stringify({ values, out });
`;

/** The page's state for the dump: its size, its fonts, and every property's initial value. */
const PAGE = `
  const [names] = arguments;
  const probe = document.createElement("div");
  probe.style.cssText = "all: initial";
  document.body.append(probe);
  const cs = getComputedStyle(probe);
  const initial = Object.fromEntries(names.map((n) => [n, cs.getPropertyValue(n)]));
  probe.remove();
  // Each part apart: an engine may have document.fonts without all of it (Servo: no check()).
  const tryIt = (f) => { try { return f(); } catch (e) { return "error: " + e.message; } };
  const fonts = {
    status: tryIt(() => document.fonts.status),
    faces: tryIt(() => [...document.fonts].map((f) => ({ family: f.family, weight: f.weight, style: f.style, status: f.status }))),
    check: Object.fromEntries(["14px ui-sans-serif", "14px system-ui", "14px sans-serif", "14px ui-monospace", "14px monospace"]
      .map((f) => [f, tryIt(() => document.fonts.check(f))])),
  };
  return JSON.stringify({
    page: { innerWidth, innerHeight, devicePixelRatio, userAgent: navigator.userAgent, scrollX, scrollY,
      docWidth: document.documentElement.scrollWidth, docHeight: document.documentElement.scrollHeight },
    fonts, initial,
  });
`;

/** The names --ask's capture's engine lists (its meta.json's `own`), or none. */
const askAlso = opts.ask ? (JSON.parse(readFileSync(path.join(opts.ask, "meta.json"), "utf8")).own ?? []) : [];

/** One screen's dump: the page, and each element shown with its box and computed style. */
async function dumpPage(app) {
  const { count, names: own } = await app.js(COLLECT);
  // Every property either engine lists (the other's names are asked for too, to tell
  // "not exposed" from "a different value"), in one order.
  const names = [...new Set([...own, ...ALSO_ASK, ...askAlso])].sort();
  const page = JSON.parse(await app.js(PAGE, names));
  const values = [];
  const index = new Map();
  const elements = [];
  // In slices: a whole page's computed styles in one script can outlast WebDriver's script timeout.
  for (let from = 0; from < count; from += 80) {
    const part = JSON.parse(await app.js(DUMP_RANGE, from, from + 80, names));
    const remap = part.values.map((v) => {
      let i = index.get(v);
      if (i === undefined) {
        i = values.length;
        values.push(v);
        index.set(v, i);
      }
      return i;
    });
    for (const e of part.out) {
      e.style = e.style.map((i) => remap[i]);
      if (e.pseudo) for (const p of Object.keys(e.pseudo)) e.pseudo[p] = e.pseudo[p].map((i) => remap[i]);
      elements.push(e);
    }
  }
  await app.js(`delete window.__compare`);
  return { ...page, own, names, values, elements };
}

/**
 * Properties asked for whether or not the engine lists them: the CSS
 * longhands either engine may leave out of its getComputedStyle list
 * (WebKitGTK 2.50's and Servo's lists differ), so a missing one shows as ""
 * rather than not at all.
 */
const ALSO_ASK = [
  "accent-color",
  "align-content",
  "align-items",
  "align-self",
  "animation-name",
  "appearance",
  "aspect-ratio",
  "backdrop-filter",
  "background-clip",
  "background-color",
  "background-image",
  "border-bottom-left-radius",
  "border-collapse",
  "box-shadow",
  "box-sizing",
  "caret-color",
  "clip-path",
  "color",
  "color-scheme",
  "column-gap",
  "contain",
  "container-type",
  "content",
  "cursor",
  "display",
  "filter",
  "flex-basis",
  "flex-direction",
  "flex-grow",
  "flex-shrink",
  "flex-wrap",
  "font-family",
  "font-feature-settings",
  "font-size",
  "font-variant-numeric",
  "font-weight",
  "gap",
  "grid-template-columns",
  "grid-template-rows",
  "height",
  "inset",
  "isolation",
  "justify-content",
  "letter-spacing",
  "line-height",
  "mask-image",
  "max-width",
  "min-width",
  "mix-blend-mode",
  "object-fit",
  "opacity",
  "outline-color",
  "outline-offset",
  "outline-style",
  "outline-width",
  "overflow-anchor",
  "overflow-x",
  "overflow-y",
  "overscroll-behavior",
  "place-items",
  "pointer-events",
  "position",
  "resize",
  "rotate",
  "row-gap",
  "scale",
  "scroll-behavior",
  "scrollbar-color",
  "scrollbar-gutter",
  "scrollbar-width",
  "text-align",
  "text-decoration-line",
  "text-overflow",
  "text-transform",
  "text-underline-offset",
  "text-wrap",
  "text-wrap-mode",
  "touch-action",
  "transform",
  "transition-property",
  "translate",
  "user-select",
  "-webkit-user-select",
  "vertical-align",
  "visibility",
  "white-space",
  "white-space-collapse",
  "width",
  "will-change",
  "word-break",
  "z-index",
];

/** Run in the page: the style rules as the engine's parser kept them. */
const RULES = `
  const rules = [];
  const head = (r) => {
    const name = r.constructor?.name ?? "CSSRule";
    const cond = r.conditionText ?? r.media?.mediaText ?? r.name ?? "";
    return "@" + name.replace(/^CSS|Rule$/g, "").toLowerCase() + " " + cond;
  };
  const walk = (list, context) => {
    for (const r of list) {
      if (r.selectorText !== undefined || r.style) {
        const decl = {};
        const s = r.style;
        if (s) for (let i = 0; i < s.length; i++) {
          const p = s[i];
          decl[p] = s.getPropertyValue(p) + (s.getPropertyPriority(p) ? " !important" : "");
        }
        rules.push({ context, selector: r.selectorText ?? head(r), decl });
      }
      let inner = null;
      try { inner = r.cssRules; } catch {}
      if (inner && inner.length) walk(inner, context + (r.selectorText ?? head(r)) + " | ");
    }
  };
  const sheets = [];
  for (const sheet of document.styleSheets) {
    let list = null;
    try { list = sheet.cssRules; } catch (e) { sheets.push({ href: sheet.href, error: String(e) }); continue; }
    sheets.push({ href: sheet.href, rules: list.length });
    walk(list, "");
  }
  return JSON.stringify({ sheets, rules });
`;

/** Run in the page: the app's stylesheets' text (each sheet's file, or its <style>). */
const SHEET_TEXT = `
  const texts = [];
  for (const node of document.querySelectorAll('link[rel="stylesheet"], style')) {
    if (node.localName === "style") texts.push({ from: "style", text: node.textContent });
    else texts.push({ from: node.href, text: await fetch(node.href).then((r) => r.text(), (e) => "") });
  }
  return JSON.stringify(texts);
`;

/** Run in the page: which of the app's declarations, selectors and media queries the engine takes. */
const SUPPORTS = `
  const [decls, selectors, medias] = arguments;
  const root = getComputedStyle(document.documentElement);
  // var() is resolved by the root's custom properties (or its fallback), as
  // supports() would otherwise say yes to anything with var() in it.
  const resolve = (v, depth = 0) =>
    depth > 5 ? v : v.replace(/var\\(\\s*(--[\\w-]+)\\s*(?:,\\s*([^()]*(?:\\([^()]*\\))*[^()]*))?\\)/g, (_, name, fallback) => {
      const got = root.getPropertyValue(name).trim();
      return resolve(got || (fallback ?? "").trim() || "initial", depth + 1);
    });
  const el = document.createElement("div");
  const decl = decls.map(([p, v]) => {
    const value = v.replace(/\\s*!important\\s*$/i, "");
    const resolved = /var\\(/.test(value) ? resolve(value) : value;
    el.style.cssText = "";
    let parsed = false;
    try { el.style.setProperty(p, resolved); parsed = el.style.getPropertyValue(p) !== ""; } catch {}
    let supports = null;
    try { supports = CSS.supports(p, resolved); } catch {}
    return [supports, parsed, resolved === value ? null : resolved];
  });
  const style = document.createElement("style");
  document.head.append(style);
  const sel = selectors.map((s) => {
    try { style.sheet.insertRule(s + " {}", 0); style.sheet.deleteRule(0); return true; } catch { return false; }
  });
  style.remove();
  const media = medias.map((m) => { try { return matchMedia(m).media !== "not all"; } catch { return false; } });
  const has = (o, k) => { try { return k in o; } catch { return false; } };
  const api = {
    "HTMLElement.inert": has(HTMLElement.prototype, "inert"),
    "HTMLElement.popover": has(HTMLElement.prototype, "popover"),
    "Element.animate": has(Element.prototype, "animate"),
    "Animation.cancel": typeof Animation !== "undefined" && has(Animation.prototype, "cancel"),
    "Element.checkVisibility": has(Element.prototype, "checkVisibility"),
    "HTMLDialogElement.showModal": typeof HTMLDialogElement !== "undefined" && has(HTMLDialogElement.prototype, "showModal"),
    OffscreenCanvas: typeof OffscreenCanvas !== "undefined",
    ResizeObserver: typeof ResizeObserver !== "undefined",
    IntersectionObserver: typeof IntersectionObserver !== "undefined",
    "document.fonts": !!document.fonts,
    "CSS.registerProperty": typeof CSS !== "undefined" && typeof CSS.registerProperty === "function",
    requestIdleCallback: typeof requestIdleCallback === "function",
    "Element.scrollIntoView": has(Element.prototype, "scrollIntoView"),
    PointerEvent: typeof PointerEvent !== "undefined",
  };
  return JSON.stringify({ decl, sel, media, api });
`;

/** Once, at the end of a capture: the parsed rules, and what the engine supports of the app's CSS. */
async function probePage(app) {
  const rules = JSON.parse(await app.js(RULES));
  const texts = JSON.parse(await app.js(SHEET_TEXT));
  // A sheet the page couldn't fetch: the built page's own files instead (the bundle the binary was built from).
  if (!texts.some((t) => t.text)) {
    const assets = path.join(here, "..", "..", "apps", "desktop", "build", "_app", "immutable", "assets");
    for (const f of existsSync(assets) ? readdirSync(assets).filter((f) => f.endsWith(".css")) : [])
      texts.push({ from: `build/${f}`, text: readFileSync(path.join(assets, f), "utf8") });
  }
  const used = cssUsed(texts.map((t) => t.text).join("\n"));
  const got = JSON.parse(await app.js(SUPPORTS, used.decls, used.selectors, used.medias));
  // Keyed by what was tested, so two captures pair whatever each could read.
  const supports = {
    decl: Object.fromEntries(used.decls.map(([p, v], i) => [`${p}: ${v}`, got.decl[i]])),
    sel: Object.fromEntries(used.selectors.map((s, i) => [s, got.sel[i]])),
    media: Object.fromEntries(used.medias.map((m, i) => [m, got.media[i]])),
    api: got.api,
  };
  return {
    rules,
    sheets: texts.map((t) => ({ from: t.from, length: t.text.length })),
    used,
    supports,
  };
}

// ---------------------------------------------------------------- report

/** A capture folder read back: its meta and its screens' dumps, by name. */
function readCapture(dir) {
  const meta = JSON.parse(readFileSync(path.join(dir, "meta.json"), "utf8"));
  const screens = new Map();
  for (const name of readdirSync(dir).sort()) {
    const file = path.join(dir, name, "dump.json");
    if (existsSync(file)) screens.set(name, JSON.parse(readFileSync(file, "utf8")));
  }
  return { dir, meta, screens };
}

/** One element's style as { property: value }. */
const styleOf = (dump, style) => Object.fromEntries(dump.names.map((n, i) => [n, dump.values[style[i]]]));

/**
 * Two values the same but for how they're written (spaces, 0 vs 0px, rgb vs
 * rgba with alpha 1): a serialization difference, kept apart from real ones.
 */
function normalize(v) {
  return (
    String(v)
      .replace(/\s+/g, " ")
      .replace(/rgba\((\d+), (\d+), (\d+), 1\)/g, "rgb($1, $2, $3)")
      .replace(/["']/g, "")
      // Numbers to a tenth: 177.4375px and 177.43333px are one length as drawn (WebKit keeps 1/64 px,
      // Servo 1/60), and color(srgb 0.098039) the same as 0.0980392.
      .replace(/-?\d*\.?\d+(?:e-?\d+)?/g, (n) => String(Math.round(Number(n) * 10) / 10))
      .replace(/\b0px\b/g, "0")
      .trim()
  );
}

/** The region an element is in, for aggregating its box: its path to depth 6, readable. */
function regionOf(label) {
  return label.split(" > ").slice(0, 6).join(" > ");
}

function compareScreen(a, b) {
  // Each element, and its ::before and ::after where it has content (no box of their own to compare).
  const byKey = (dump) =>
    new Map(
      dump.elements.flatMap((e) => [
        [e.key, e],
        ...Object.entries(e.pseudo ?? {}).map(([p, style]) => [
          e.key + p,
          { ...e, key: e.key + p, label: e.label + p, style, rect: null },
        ]),
      ]),
    );
  const A = byKey(a);
  const B = byKey(b);
  const both = [...A.keys()].filter((k) => B.has(k));
  const onlyA = [...A.keys()].filter((k) => !B.has(k));
  const onlyB = [...B.keys()].filter((k) => !A.has(k));
  const props = new Map();
  const initialOnly = new Map();
  const notExposed = new Map();
  const rects = [];
  const moved = new Set();
  const note = (map, prop, pair, example) => {
    const p = map.get(prop) ?? { count: 0, pairs: new Map(), examples: [] };
    p.count++;
    const k = JSON.stringify(pair);
    p.pairs.set(k, (p.pairs.get(k) ?? 0) + 1);
    if (p.examples.length < 3) p.examples.push(example);
    map.set(prop, p);
  };
  for (const key of both) {
    const ea = A.get(key);
    const eb = B.get(key);
    const sa = styleOf(a, ea.style);
    const sb = styleOf(b, eb.style);
    const example = (prop) => ({ label: ea.label, text: ea.text, webkit: sa[prop], servo: sb[prop] });
    for (const prop of new Set([...a.names, ...b.names])) {
      const va = sa[prop];
      const vb = sb[prop];
      if (va === undefined || vb === undefined) continue;
      if (va !== "" && vb === "") {
        note(notExposed, prop, [va, ""], example(prop));
        continue;
      }
      if (va === vb) continue;
      // Not drawn apart: written otherwise, or (Servo) a border-box element's width or height
      // given as its content box's while its box on the screen is the same.
      const sameBox =
        ea.rect && eb.rect && Math.abs(ea.rect[2] - eb.rect[2]) <= 1 && Math.abs(ea.rect[3] - eb.rect[3]) <= 1;
      const formatOnly =
        normalize(va) === normalize(vb)
          ? "format only"
          : sameBox && /^(width|height|inline-size|block-size)$/.test(prop)
            ? "same box"
            : false;
      note(props, prop, [va, vb, formatOnly], example(prop));
      if (vb === b.initial[prop] && va !== a.initial[prop]) note(initialOnly, prop, [va, vb], example(prop));
    }
    const d = ea.rect && eb.rect ? ea.rect.map((v, i) => Math.abs(v - eb.rect[i])) : [0];
    if (Math.max(...d) > 1) {
      moved.add(key);
      rects.push({ key, label: ea.label, text: ea.text, webkit: ea.rect, servo: eb.rect, delta: Math.max(...d) });
    }
  }
  // The first of each chain: a box apart whose parent's box agrees, where a difference starts.
  const parent = (key) => key.slice(0, key.lastIndexOf(">"));
  for (const r of rects) r.root = !moved.has(parent(r.key));
  const regions = new Map();
  for (const r of rects) {
    const region = regionOf(r.label);
    const g = regions.get(region) ?? { count: 0, roots: 0, max: 0, example: r };
    g.count++;
    if (r.root) g.roots++;
    g.max = Math.max(g.max, r.delta);
    regions.set(region, g);
  }
  const flat = (map) =>
    [...map]
      .map(([prop, p]) => ({
        prop,
        count: p.count,
        pairs: [...p.pairs].map(([k, n]) => ({ pair: JSON.parse(k), n })).sort((x, y) => y.n - x.n),
        examples: p.examples,
      }))
      .sort((x, y) => y.count - x.count);
  return {
    paired: both.length,
    onlyWebkit: onlyA.map((k) => A.get(k).label),
    onlyServo: onlyB.map((k) => B.get(k).label),
    props: flat(props),
    initialOnly: flat(initialOnly),
    notExposed: flat(notExposed),
    rects: rects.sort((x, y) => Number(y.root) - Number(x.root) || y.delta - x.delta),
    regions: [...regions].map(([region, g]) => ({ region, ...g })).sort((x, y) => y.count - x.count),
    page: { webkit: a.page, servo: b.page },
    fonts: { webkit: a.fonts, servo: b.fonts },
  };
}

/** The pixel diff of two shots: the count of pixels apart (2 % fuzz) and a heatmap, by ImageMagick. */
function pixelDiff(a, b, diffFile) {
  const magick = (tool, args) =>
    execFileSync(process.env.MAGICK_BIN ? path.join(process.env.MAGICK_BIN, tool) : tool, args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  const sizeOf = (f) => magick("identify", ["-format", "%w %h", f]).trim().split(" ").map(Number);
  const [wa, ha] = sizeOf(a);
  const [wb, hb] = sizeOf(b);
  const w = Math.max(wa, wb);
  const h = Math.max(ha, hb);
  let A = a;
  let B = b;
  // Shots of different sizes: padded to one, so compare can still say where.
  if (wa !== wb || ha !== hb) {
    A = `${diffFile}.a.png`;
    B = `${diffFile}.b.png`;
    for (const [from, to] of [
      [a, A],
      [b, B],
    ])
      magick("convert", [from, "-background", "magenta", "-extent", `${w}x${h}`, to]);
  }
  let out;
  try {
    out = magick("compare", ["-metric", "AE", "-fuzz", "2%", A, B, diffFile]);
  } catch (e) {
    // compare exits 1 when the pictures differ, its count on stderr.
    out = e.stderr ?? "";
    if (e.status !== 1) throw new Error(`compare: ${out || e.message}`);
  }
  for (const f of [A, B]) if (f !== a && f !== b) rmSync(f, { force: true });
  const pixels = Number(String(out).trim().split(/\s/)[0]);
  return { pixels, total: w * h, sizes: [`${wa}x${ha}`, `${wb}x${hb}`] };
}

/** The rules each engine's parser kept, compared: declarations one keeps and the other drops, rules one drops. */
function compareRules(a, b) {
  const keyed = (rules) => {
    const map = new Map();
    const seen = new Map();
    for (const r of rules) {
      const base = `${r.context}${r.selector}`;
      const n = (seen.get(base) ?? 0) + 1;
      seen.set(base, n);
      map.set(`${base} #${n}`, r);
    }
    return map;
  };
  const A = keyed(a.rules.rules);
  const B = keyed(b.rules.rules);
  const droppedDecls = new Map();
  const keptOnlyByServo = new Map();
  const rulesOnlyWebkit = [];
  const rulesOnlyServo = [];
  const add = (map, prop, value, selector) => {
    const d = map.get(prop) ?? { count: 0, examples: [] };
    d.count++;
    if (d.examples.length < 3) d.examples.push({ selector, value });
    map.set(prop, d);
  };
  for (const [k, ra] of A) {
    const rb = B.get(k);
    if (!rb) {
      rulesOnlyWebkit.push(k);
      continue;
    }
    for (const [p, v] of Object.entries(ra.decl)) if (!(p in rb.decl)) add(droppedDecls, p, v, ra.selector);
    for (const [p, v] of Object.entries(rb.decl)) if (!(p in ra.decl)) add(keptOnlyByServo, p, v, rb.selector);
  }
  for (const k of B.keys()) if (!A.has(k)) rulesOnlyServo.push(k);
  const flat = (map) => [...map].map(([prop, d]) => ({ prop, ...d })).sort((x, y) => y.count - x.count);
  return {
    counts: { webkit: A.size, servo: B.size },
    droppedByServo: flat(droppedDecls),
    droppedByWebkit: flat(keptOnlyByServo),
    rulesOnlyWebkit,
    rulesOnlyServo,
  };
}

/** What the app's CSS uses that one engine supports and the other doesn't. */
function compareSupport(a, b) {
  const used = a.used;
  const rows = [];
  // A declaration counts as taken if CSS.supports says so or an inline style keeps it.
  const taken = (r) => (r ? (r[0] ?? r[1]) || r[1] : "not tested");
  for (const [prop, value] of used.decls) {
    const what = `${prop}: ${value}`;
    const wa = taken(a.supports.decl[what]);
    const wb = taken(b.supports.decl[what]);
    if (wa !== wb)
      rows.push({ kind: "declaration", what, prop, webkit: wa, servo: wb, resolved: b.supports.decl[what]?.[2] });
  }
  for (const s of used.selectors)
    if (a.supports.sel[s] !== b.supports.sel[s])
      rows.push({ kind: "selector", what: s, webkit: a.supports.sel[s], servo: b.supports.sel[s] ?? "not tested" });
  for (const m of used.medias)
    if (a.supports.media[m] !== b.supports.media[m])
      rows.push({ kind: "media", what: m, webkit: a.supports.media[m], servo: b.supports.media[m] ?? "not tested" });
  const api = Object.keys(a.supports.api).map((k) => ({ api: k, webkit: a.supports.api[k], servo: b.supports.api[k] }));
  // By property: one row each for the declarations Servo refuses.
  const byProp = new Map();
  for (const r of rows.filter((r) => r.kind === "declaration")) {
    const p = byProp.get(r.prop) ?? { prop: r.prop, servoNo: 0, webkitNo: 0, examples: [] };
    if (r.servo !== true) p.servoNo++;
    if (r.webkit !== true) p.webkitNo++;
    if (p.examples.length < 3) p.examples.push(r.what);
    byProp.set(r.prop, p);
  }
  return {
    usedCounts: { decls: used.decls.length, selectors: used.selectors.length, medias: used.medias.length },
    rows,
    byProp: [...byProp.values()].sort((x, y) => y.servoNo + y.webkitNo - (x.servoNo + x.webkitNo)),
    api,
  };
}

function report(webkitDir, servoDir, outDir) {
  const a = readCapture(webkitDir);
  const b = readCapture(servoDir);
  const out = path.resolve(outDir);
  mkdirSync(out, { recursive: true });
  const screens = [];
  for (const [name, da] of a.screens) {
    const db = b.screens.get(name);
    if (!db) {
      screens.push({ name, missing: "servo" });
      continue;
    }
    const pics = { webkit: `${name}-webkit.png`, servo: `${name}-servo.png`, diff: `${name}-diff.png` };
    copyFileSync(path.join(a.dir, name, "shot.png"), path.join(out, pics.webkit));
    copyFileSync(path.join(b.dir, name, "shot.png"), path.join(out, pics.servo));
    for (const [cap, engine] of [
      [a, "webkit"],
      [b, "servo"],
    ])
      if (existsSync(path.join(cap.dir, name, "page.png")))
        copyFileSync(path.join(cap.dir, name, "page.png"), path.join(out, `${name}-${engine}-page.png`));
    let pixels;
    try {
      pixels = pixelDiff(path.join(out, pics.webkit), path.join(out, pics.servo), path.join(out, pics.diff));
    } catch (e) {
      pixels = { error: e.message };
    }
    screens.push({ name, note: da.note, pics, pixels, ...compareScreen(da, db) });
  }
  for (const name of b.screens.keys()) if (!a.screens.has(name)) screens.push({ name, missing: "webkit" });
  const rules = compareRules(a.meta, b.meta);
  const support = compareSupport(a.meta, b.meta);
  // Across screens: each property's differing elements, and on which screens.
  const across = new Map();
  for (const s of screens.filter((s) => s.props))
    for (const p of s.props) {
      const x = across.get(p.prop) ?? { prop: p.prop, count: 0, screens: [], formatOnly: true, top: p.pairs[0].pair };
      x.count += p.count;
      x.screens.push(s.name);
      if (p.pairs.some((q) => !q.pair[2])) x.formatOnly = false;
      across.set(p.prop, x);
    }
  const summary = {
    webkit: { binary: a.meta.binary, failed: a.meta.failed },
    servo: { binary: b.meta.binary, failed: b.meta.failed },
    screens: screens.length,
    compared: screens.filter((s) => s.props).length,
    properties: [...across.values()].sort((x, y) => y.count - x.count),
  };
  writeFileSync(path.join(out, "report.json"), JSON.stringify({ summary, screens, rules, support }, null, 2));
  writeFileSync(path.join(out, "report.html"), html({ summary, screens, rules, support }));
  console.log(`report: ${summary.compared} of ${summary.screens} screens compared: ${path.join(out, "report.html")}`);
  for (const s of screens)
    console.log(
      s.missing
        ? `  ${s.name}: missing on ${s.missing}`
        : `  ${s.name}: ${s.pixels.error ? `pixels: ${s.pixels.error}` : `${((100 * s.pixels.pixels) / s.pixels.total).toFixed(1)} % pixels apart`}, ${s.paired} elements paired (${s.onlyWebkit.length} WebKit only, ${s.onlyServo.length} Servo only), ${s.props.length} properties differ, ${s.rects.filter((r) => r.root).length} boxes start a layout difference`,
    );
}

// ---------------------------------------------------------------- HTML

const esc = (s) =>
  String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const cut = (s, n = 90) => (String(s).length > n ? `${String(s).slice(0, n)}…` : String(s));
const table = (head, rows) =>
  `<table><thead><tr>${head.map((h) => `<th>${esc(h)}</th>`).join("")}</tr></thead><tbody>${rows
    .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join("")}</tr>`)
    .join("")}</tbody></table>`;
const yes = (v) => (v === true ? "yes" : v === false ? '<b class="no">no</b>' : esc(v));

function html({ summary, screens, rules, support }) {
  const pct = (s) => (s.pixels?.total ? `${((100 * s.pixels.pixels) / s.pixels.total).toFixed(1)} %` : "–");
  const examples = (list) =>
    list
      .map((e) => `<div class="ex">${esc(cut(e.label, 140))}${e.text ? ` <i>“${esc(e.text)}”</i>` : ""}</div>`)
      .join("");
  const pairs = (p) =>
    p.pairs
      .slice(0, 3)
      .map(
        (q) =>
          `<div><code>${esc(cut(q.pair[0]))}</code> → <code>${esc(cut(q.pair[1]))}</code> ×${q.n}${q.pair[2] ? ` <span class="fmt">${esc(q.pair[2])}</span>` : ""}</div>`,
      )
      .join("");
  const propTable = (list, limit = 60) =>
    table(
      ["property", "elements", "WebKit → Servo (most common)", "examples"],
      list.slice(0, limit).map((p) => [`<code>${esc(p.prop)}</code>`, p.count, pairs(p), examples(p.examples)]),
    );
  const screenHtml = (s) => {
    if (s.missing)
      return `<section><h2 id="${esc(s.name)}">${esc(s.name)}</h2><p>Not captured on ${esc(s.missing)}.</p></section>`;
    const real = s.props.filter((p) => p.pairs.some((q) => !q.pair[2]));
    return `<section><h2 id="${esc(s.name)}">${esc(s.name)}</h2><p>${esc(s.note)}</p>
      <p>Page: WebKit ${s.page.webkit.innerWidth}×${s.page.webkit.innerHeight} @${s.page.webkit.devicePixelRatio},
      Servo ${s.page.servo.innerWidth}×${s.page.servo.innerHeight} @${s.page.servo.devicePixelRatio};
      shots ${esc(s.pixels.sizes?.join(" / ") ?? "")}; ${pct(s)} of pixels apart${s.pixels.error ? ` (${esc(s.pixels.error)})` : ""}.
      ${s.paired} elements paired; ${s.onlyWebkit.length} only in WebKit's page, ${s.onlyServo.length} only in Servo's.</p>
      <div class="shots">${["webkit", "servo", "diff"]
        .map(
          (k) =>
            `<figure><a href="${s.pics[k]}"><img src="${s.pics[k]}" loading="lazy"></a><figcaption>${k}</figcaption></figure>`,
        )
        .join("")}</div>
      <p>The window from the screen (above); the drivers' own page screenshots: <a href="${esc(s.name)}-webkit-page.png">WebKit</a>, <a href="${esc(s.name)}-servo-page.png">Servo</a>.</p>
      <h3>Computed values that differ (${real.length} properties, ${s.props.length - real.length} more only as written or with the same box)</h3>
      ${propTable(real)}
      <h3>Boxes more than 1 px apart: where they start (${s.rects.filter((r) => r.root).length} of ${s.rects.length})</h3>
      ${table(
        ["element", "WebKit x y w h", "Servo x y w h"],
        s.rects
          .filter((r) => r.root)
          .slice(0, 40)
          .map((r) => [
            esc(cut(r.label, 160)) + (r.text ? ` <i>“${esc(r.text)}”</i>` : ""),
            r.webkit.join(" "),
            r.servo.join(" "),
          ]),
      )}
      <h3>By region</h3>
      ${table(
        ["region", "boxes apart", "where they start", "most"],
        s.regions.slice(0, 25).map((g) => [esc(cut(g.region, 160)), g.count, g.roots, `${g.max.toFixed(1)} px`]),
      )}
      <h3>Servo at the initial value where WebKit isn't (candidate unsupported)</h3>
      ${propTable(s.initialOnly, 40)}
      <h3>Reported by WebKit, not exposed by Servo's getComputedStyle</h3>
      ${table(
        ["property", "elements", "WebKit's value"],
        s.notExposed.map((p) => [
          `<code>${esc(p.prop)}</code>`,
          p.count,
          `<code>${esc(cut(p.pairs[0].pair[0]))}</code>`,
        ]),
      )}
      <details><summary>Elements only one engine shows</summary>
        <p>WebKit only:</p><ul>${s.onlyWebkit
          .slice(0, 50)
          .map((l) => `<li>${esc(cut(l, 200))}</li>`)
          .join("")}</ul>
        <p>Servo only:</p><ul>${s.onlyServo
          .slice(0, 50)
          .map((l) => `<li>${esc(cut(l, 200))}</li>`)
          .join("")}</ul>
      </details>
      <details><summary>Fonts</summary><pre>${esc(JSON.stringify(s.fonts, null, 1))}</pre></details>
      </section>`;
  };
  return `<!doctype html><meta charset="utf-8"><title>WebKit and Servo, screen by screen</title>
<style>
body { font: 14px/1.45 system-ui, sans-serif; margin: 24px; color: #222; }
table { border-collapse: collapse; margin: 8px 0 16px; font-size: 12.5px; }
th, td { border: 1px solid #ddd; padding: 3px 6px; vertical-align: top; text-align: left; }
th { background: #f4f4f4; }
code { font: 12px ui-monospace, monospace; background: #f6f6f6; padding: 0 2px; word-break: break-all; }
.shots { display: flex; gap: 8px; }
.shots figure { margin: 0; flex: 1; min-width: 0; }
.shots img { width: 100%; border: 1px solid #ccc; }
figcaption { text-align: center; color: #666; }
.ex { color: #555; font-size: 11.5px; word-break: break-all; }
.fmt { color: #888; font-size: 11px; }
.no { color: #b00; }
section { border-top: 2px solid #333; margin-top: 28px; }
</style>
<h1>WebKit and Servo, screen by screen</h1>
<p>WebKit: <code>${esc(summary.webkit.binary)}</code>; Servo: <code>${esc(summary.servo.binary)}</code>.
${summary.compared} of ${summary.screens} screens compared.
${[
  ...summary.webkit.failed.map((f) => `WebKit missed ${f.name}: ${f.error}`),
  ...summary.servo.failed.map((f) => `Servo missed ${f.name}: ${f.error}`),
]
  .map((t) => `<br><b>${esc(t)}</b>`)
  .join("")}</p>
<h2>Screens</h2>
${table(
  ["screen", "pixels apart", "paired", "WebKit only", "Servo only", "properties apart (real)", "boxes apart (starts)"],
  screens.map((s) =>
    s.missing
      ? [`<a href="#${esc(s.name)}">${esc(s.name)}</a>`, `missing on ${esc(s.missing)}`, "", "", "", "", ""]
      : [
          `<a href="#${esc(s.name)}">${esc(s.name)}</a>`,
          pct(s),
          s.paired,
          s.onlyWebkit.length,
          s.onlyServo.length,
          s.props.filter((p) => p.pairs.some((q) => !q.pair[2])).length,
          `${s.rects.length} (${s.rects.filter((r) => r.root).length})`,
        ],
  ),
)}
<h2>Properties apart, across screens</h2>
${table(
  ["property", "elements", "screens", "most common, WebKit → Servo"],
  summary.properties
    .filter((p) => !p.formatOnly)
    .slice(0, 80)
    .map((p) => [
      `<code>${esc(p.prop)}</code>`,
      p.count,
      p.screens.length,
      `<code>${esc(cut(p.top[0]))}</code> → <code>${esc(cut(p.top[1]))}</code>`,
    ]),
)}
<h2>The app's CSS that one engine supports and the other doesn't</h2>
<p>Of ${support.usedCounts.decls} declarations, ${support.usedCounts.selectors} selectors and ${support.usedCounts.medias} media queries in the app's stylesheets
(declarations tested with CSS.supports and an inline style, var() resolved from :root; selectors by inserting a rule).</p>
${table(
  ["property", "Servo refuses", "WebKit refuses", "examples"],
  support.byProp.map((p) => [
    `<code>${esc(p.prop)}</code>`,
    p.servoNo,
    p.webkitNo,
    p.examples.map((e) => `<div><code>${esc(cut(e, 120))}</code></div>`).join(""),
  ]),
)}
${table(
  ["kind", "what", "WebKit", "Servo"],
  support.rows
    .filter((r) => r.kind !== "declaration")
    .map((r) => [r.kind, `<code>${esc(cut(r.what, 160))}</code>`, yes(r.webkit), yes(r.servo)]),
)}
<h3>APIs</h3>
${table(
  ["API", "WebKit", "Servo"],
  support.api.map((r) => [esc(r.api), yes(r.webkit), yes(r.servo)]),
)}
<h2>Style rules as each parser kept them</h2>
<p>Rules: WebKit ${rules.counts.webkit}, Servo ${rules.counts.servo}.</p>
<h3>Declarations WebKit kept that Servo's parser dropped</h3>
${table(
  ["property", "rules", "examples"],
  rules.droppedByServo.map((d) => [
    `<code>${esc(d.prop)}</code>`,
    d.count,
    d.examples.map((e) => `<div><code>${esc(cut(e.selector, 80))}</code> { ${esc(cut(e.value, 60))} }</div>`).join(""),
  ]),
)}
<h3>Declarations Servo kept that WebKit's parser dropped (or expanded otherwise)</h3>
${table(
  ["property", "rules", "examples"],
  rules.droppedByWebkit.map((d) => [
    `<code>${esc(d.prop)}</code>`,
    d.count,
    d.examples.map((e) => `<div><code>${esc(cut(e.selector, 80))}</code> { ${esc(cut(e.value, 60))} }</div>`).join(""),
  ]),
)}
<details><summary>Rules only WebKit kept (${rules.rulesOnlyWebkit.length}) / only Servo kept (${rules.rulesOnlyServo.length})</summary>
<ul>${rules.rulesOnlyWebkit
    .slice(0, 200)
    .map((k) => `<li>WebKit: <code>${esc(cut(k, 200))}</code></li>`)
    .join("")}
${rules.rulesOnlyServo
  .slice(0, 200)
  .map((k) => `<li>Servo: <code>${esc(cut(k, 200))}</code></li>`)
  .join("")}</ul></details>
${screens.map(screenHtml).join("\n")}
`;
}

// Last, once everything above is defined (the page scripts are consts).
const [command, ...rest] = positionals;
if (command === "capture" && rest[0] && opts.out && ["webkit", "servo"].includes(opts.engine)) {
  await capture(rest[0]);
  // The driver's tauri-driver would keep node alive (as run.mjs, exit at the end; its exit handler ends it).
  process.exit(process.exitCode ?? 0);
} else if (command === "report" && rest.length === 2 && opts.out) {
  report(rest[0], rest[1], opts.out);
} else {
  console.error(
    "usage: node tests/e2e/compare.mjs capture <the app> --engine webkit|servo --out <dir> [--size WxH] [--small WxH] [--video <file>]\n" +
      "       node tests/e2e/compare.mjs report <webkit dir> <servo dir> --out <dir>",
  );
  process.exit(2);
}
