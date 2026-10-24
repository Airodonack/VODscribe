// What scenarios do with the app: run code in its page, call its commands,
// click, wait, answer its dialogs. One `App` per scenario, on a driver
// (drivers.mjs).
//
// The app's native dialogs can't be clicked from the page, so the test
// answers them: the file picker returns `app.files`, and a question the
// button in `app.answer`. A restart the app asks for (plugin-process's
// relaunch) is noted, not done: the test restarts it itself (ctx.restart), as
// one the app started couldn't be driven. Tauri's invoke can't be replaced
// (it's defined read-only), so the answers go in at the fetch that carries
// each call.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { isUnplayable } from "../../../apps/desktop/src/lib/media.ts";
import { pageDump } from "./dump.mjs";
import * as os from "./os.mjs";

export const MINUTE = 60_000;

/** How long a click waits for what it clicks to be on the page. */
export const CLICK_WAIT = 10_000;

/**
 * Polls `check` until it returns something truthy, or fails after `timeout`
 * saying `what`; every half second, or `every` ms (a timed step's wait, for
 * a time not rounded up to the poll).
 */
export async function until(what, check, timeout, { every = 500 } = {}) {
  const start = Date.now();
  const end = start + timeout;
  let last;
  let told = start;
  let looked = start;
  while (Date.now() < end) {
    try {
      last = await check();
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    // Every few seconds: whether the page already says it can't (then waiting
    // on is pointless), and every ten, that it's still waiting and on what.
    if (watch && Date.now() - looked >= 3000) {
      looked = Date.now();
      const reason = await watch().catch(() => null);
      if (reason) throw new Error(`gave up waiting for ${what}: the page says ${reason}`);
    }
    if (Date.now() - told >= 10_000) {
      told = Date.now();
      console.log(
        `  … still waiting for ${what} (${Math.round((told - start) / 1000)} s; last: ${JSON.stringify(last)})`,
      );
    }
    await sleep(every);
  }
  throw new Error(`timed out waiting for ${what} (last: ${JSON.stringify(last)})`);
}

/** What `until` looks at as it waits (set per scenario by run.mjs): a reason
 *  the page gives that it can't go on, or null. */
let watch = null;
export function watchWith(check) {
  watch = check;
}

/** How often a timed step's wait looks (ms). */
export const TIMED_POLL = 50;

/** The monotonic clock, in seconds: perf's samples are stamped by it (lib/profile.sh). */
export const clock = () => Number(process.hrtime.bigint()) / 1e9;

export class App {
  constructor(driver, log) {
    this.driver = driver;
    this.log = log;
    // Times a step a helper takes (run.mjs sets it to ctx.time, which keeps
    // it with the scenario's timings); untimed by default.
    this.timed = (name, step) => step();
    // Keeps a time measured another way (run.mjs: ctx.timing).
    this.timing = () => {};
  }

  /**
   * How long from `act` (a function doing something to the app) to the app's
   * own player showing a frame (its `player-time` status says which:
   * `shown`): the one at `near` seconds (within 20 ms), or with no `near`,
   * any. Measured in the page, by when the status arrives; kept as the
   * timing `name` (unless `keep` is false: the caller keeps it its own
   * way). Returns the milliseconds.
   */
  async timeToFrame(name, act, near = null, { keep = true } = {}) {
    await this.js(`
      if (window.__e2eFrames) return;
      const frames = (window.__e2eFrames = []);
      const internals = window.__TAURI_INTERNALS__;
      const handler = internals.transformCallback((e) => {
        frames.push({ at: performance.now(), shown: e.payload.shown, time: e.payload.time });
        if (frames.length > 2000) frames.splice(0, 1000);
      });
      await internals.invoke("plugin:event|listen", { event: "player-time", target: { kind: "Any" }, handler });`);
    const from = clock();
    const since = await this.js(`return performance.now()`);
    await act();
    const at = await until(
      `${name}: the frame`,
      () =>
        this.js(
          `const [since, near] = arguments;
           const ok = (f) => f.shown !== null && (near === null || Math.abs(f.shown - near) <= 0.02);
           return window.__e2eFrames.find((f) => f.at >= since && ok(f))?.at ?? null`,
          since,
          near,
        ),
      30_000,
      { every: TIMED_POLL },
    );
    const ms = Math.round(at - since);
    if (keep) this.timing(name, ms, from, clock());
    return ms;
  }

  /** Runs `body` (a function body, with `arguments`) in the page and returns its awaited result. */
  js(body, ...args) {
    return this.driver.run(body, args);
  }

  /** Calls the app's command `cmd` from the page, as the interface does; a
   *  command's refusal comes back as a rejection with its words. (The app
   *  rejects with a string, which WebKit's WebDriver can't pass back: "Could
   *  not parse script result"; an Error it can.) */
  invoke(cmd, args = {}) {
    return this.js(
      `return window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]).catch((e) => {
         throw e instanceof Error ? e : new Error(typeof e === "string" ? e : JSON.stringify(e));
       })`,
      cmd,
      args,
    );
  }

  /** The page dump (lib/dump.mjs): what the page, its media and the app's player are at. */
  dump(options) {
    return pageDump(this.driver, options);
  }

  /** Logs `what` went wrong, with the dump (without the page's text) under it. */
  async logPage(what) {
    this.log(`${what}; the page:\n    ${(await this.dump({ text: false })).replaceAll("\n", "\n    ")}`);
  }

  /**
   * Finds what `find` (a function body: the element or null) gives, waiting
   * up to CLICK_WAIT for it in the page, then does `act` (a function body
   * with `el`) to it. A step a moment early (the page still drawing what the
   * last one did) waits rather than fails; one that never comes fails
   * saying what the page was at (the dump, without its text, in the log).
   */
  async onElement(what, find, act, ...args) {
    const done = await this.js(
      `const find = () => { ${find} };
       const end = performance.now() + arguments[0];
       let el;
       while (!(el = find()) && performance.now() < end) await new Promise((r) => setTimeout(r, 50));
       if (!el) return false;
       ${act}
       return true;`,
      CLICK_WAIT,
      ...args,
    );
    if (done) return;
    await this.logPage(`no ${what} after ${CLICK_WAIT / 1000} s`);
    throw new Error(`no ${what} on the page after ${CLICK_WAIT / 1000} s`);
  }

  /** Clicks `selector`, once it's on the page (onElement). */
  click(selector) {
    return this.onElement(selector, `return document.querySelector(arguments[1])`, `el.click();`, selector);
  }

  /** Double-clicks `selector`, as the page gets one: two clicks, then the double-click. */
  doubleClick(selector) {
    return this.onElement(
      selector,
      `return document.querySelector(arguments[1])`,
      `el.click();
       el.click();
       el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, detail: 2 }));`,
      selector,
    );
  }

  /**
   * Makes sure the page has the keyboard before keys or typing go to it.
   * Where it hasn't (a runner's window lost it to another: the hosted macOS
   * runner, once), the window is asked back: by the page, by the driver,
   * and by the OS (os.activate); failing that, the step fails at once,
   * "page not focused", rather than as a timeout further on. Not under
   * weston, where the window never has the keyboard (no seat) and focus and
   * blur are sent by hand.
   */
  async focused() {
    if (process.env.E2E_DISPLAY === "weston") return;
    if (await this.js(`return document.hasFocus()`)) return;
    const has = () => this.js(`return document.hasFocus()`);
    const ways = [
      ["the page", () => this.js(`window.focus()`)],
      ["the driver", () => this.driver.focus()],
      ["the OS", () => os.activate(this.driver.proc)],
    ];
    const tried = [];
    for (const [by, ask] of ways) {
      try {
        await ask();
      } catch (e) {
        tried.push(`${by}: ${e.message}`);
        continue;
      }
      if (await until(`the page focused by ${by}`, has, 1000, { every: 100 }).catch(() => false)) {
        this.log(`the page wasn't focused: ${by} focused it`);
        return;
      }
      tried.push(`${by}: no`);
    }
    await this.logPage("the page isn't focused");
    throw new Error(`page not focused (asked ${tried.join("; ")})`);
  }

  /**
   * Focuses the input `selector`, with its focus event. As with `blur`, a window without the
   * keyboard (weston) gets no focus event from it: there it's sent by hand.
   */
  async focus(selector) {
    await this.focused();
    return this.js(
      `const input = document.querySelector(arguments[0]);
       if (!input) throw new Error("no " + arguments[0]);
       // Focused already (a click that moved nothing), it's left first, as a person's click elsewhere would.
       if (document.activeElement === input) input.blur();
       input.focus();
       if (!document.hasFocus()) input.dispatchEvent(new FocusEvent("focus"));`,
      selector,
    );
  }

  /**
   * Opens Settings at `section`, a name in its list ("Appearance", "Storage", "Help"…): the gear
   * first if Settings isn't open, then the section, once it's the one shown.
   */
  async openSettings(section) {
    const dialog = '[role="dialog"][aria-label="Settings"]';
    if (!(await this.js(`return !!document.querySelector(arguments[0])`, dialog))) {
      await this.click('button[aria-label="Settings"]');
      await until(
        "Settings",
        () => this.js(`return !!document.querySelector(arguments[0] + " nav button")`, dialog),
        MINUTE,
      );
    }
    await this.js(
      `const b = [...document.querySelectorAll(arguments[0] + " nav button")].find((b) => b.textContent.trim() === arguments[1]);
       if (!b) throw new Error("no section " + arguments[1] + " in Settings");
       b.click();`,
      dialog,
      section,
    );
    await until(
      `Settings at ${section}`,
      () =>
        this.js(
          `return document.querySelector(arguments[0] + ' nav button[aria-current="true"]')?.textContent.trim() === arguments[1]`,
          dialog,
          section,
        ),
      10_000,
    );
  }

  /** Clicks the button whose text is `text`, once it's on the page (onElement). */
  clickText(text) {
    return this.onElement(
      `button ${text}`,
      `return [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === arguments[1])`,
      `el.click();`,
      text,
    );
  }

  /** Types `value` into the input `selector`, as a person would (an input event). */
  async type(selector, value) {
    await this.focused();
    return this.js(
      `const input = document.querySelector(arguments[0]);
       if (!input) throw new Error("no " + arguments[0]);
       input.value = arguments[1];
       input.dispatchEvent(new Event("input", { bubbles: true }));`,
      selector,
      value,
    );
  }

  /**
   * Leaves the input `selector` (its blur event, as moving focus away fires).
   * WebKit fires focus and blur only in a page that has the keyboard, and a
   * headless Wayland compositor with no seat (weston) never gives the window
   * the keyboard: there the event is sent by hand.
   */
  blur(selector) {
    return this.js(
      `const input = document.querySelector(arguments[0]);
       if (!input) throw new Error("no " + arguments[0]);
       if (document.hasFocus()) input.blur();
       else input.dispatchEvent(new FocusEvent("blur"));`,
      selector,
    );
  }

  /** Submits the form `selector` is in, as Enter would (macOS 12's WebKit has no requestSubmit). */
  submit(selector) {
    return this.js(
      `const form = document.querySelector(arguments[0])?.closest("form");
       if (!form) throw new Error("no form for " + arguments[0]);
       if (form.requestSubmit) form.requestSubmit();
       else form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));`,
      selector,
    );
  }

  /** Whether the app was built with the e2e feature (the test-only commands
   *  and fixtures): CI's Linux job tests the release package, which wasn't. */
  hasE2e() {
    return this.invoke("e2e_player_state").then(
      () => true,
      () => false,
    );
  }

  /** Presses `key` (a KeyboardEvent key), with Shift if `shift`, and with
   *  `cmd` the command modifier of the computer the app runs on: ⌘ on a Mac,
   *  Ctrl elsewhere (the app's undo is ⌘Z there, not Ctrl+Z), as the window
   *  gets it. */
  async key(key, { shift = false, cmd = false } = {}) {
    await this.focused();
    return this.js(
      `const mac = arguments[2] && /Mac/.test(navigator.platform);
       document.body.dispatchEvent(new KeyboardEvent("keydown",
         { key: arguments[0], shiftKey: arguments[1], ctrlKey: arguments[2] && !mac, metaKey: mac, bubbles: true }))`,
      key,
      shift,
      cmd,
    );
  }

  /** Pauses the open VOD with the bar's button, if it's playing. */
  async pause() {
    if (await this.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`))
      await this.click('.bar button[aria-label="Pause"]');
    await until(
      "the player to pause",
      () => this.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      10_000,
    );
  }

  /**
   * The contrast ratio (WCAG, 1 to 21) of `selector`'s text against the
   * first solid background behind it, as the page computes their colours.
   */
  contrast(selector) {
    return this.js(
      `const el = document.querySelector(arguments[0]);
       if (!el) throw new Error("no " + arguments[0]);
       const rgba = (c) => c.match(/[\\d.]+/g).map(Number);
       let bg = null;
       for (let e = el; e && !bg; e = e.parentElement) {
         const c = rgba(getComputedStyle(e).backgroundColor);
         if (c.length < 4 || c[3] > 0.95) bg = c;
       }
       bg ??= [255, 255, 255];
       const luminance = ([r, g, b]) => {
         const lin = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
         return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
       };
       const [a, b] = [luminance(rgba(getComputedStyle(el).color)), luminance(bg)].sort((x, y) => y - x);
       return (a + 0.05) / (b + 0.05);`,
      selector,
    );
  }

  /** Error text the page shows, if any. */
  errors() {
    return this.js(
      `return [...document.querySelectorAll(".error-text")].map((e) => e.textContent.trim()).filter(Boolean)`,
    );
  }

  /** Waits for the page, then puts in the dialog answers. */
  async ready() {
    await this.loaded();
    // A start once left the page empty for about two minutes (WebKitGTK and
    // macOS's WebKit, 4 starts in 10 at worst): the app's page server, then
    // tiny_http, left one of the page's script preloads unanswered
    // (app_server.rs). Should a start stall again, one reload, said in the
    // log, gets past it without hiding it.
    const shown = () => this.js(`return document.body.innerText.trim() !== ""`);
    const begun = Date.now();
    await until(
      "the first screen",
      async () => {
        if (await shown()) return true;
        if (Date.now() - begun > 15_000 && !this.reloaded) {
          this.reloaded = true;
          // What it's waiting on: the first call to the app (+page.svelte
          // marks it), and what it has fetched.
          const waiting = await this.js(
            `const at = (name) => performance.getEntriesByName(name)[0]?.startTime.toFixed(0) ?? "never";
             const fetched = performance.getEntriesByType("resource").map((r) =>
               r.name.slice(0, 60) + " " + r.startTime.toFixed(0) + "+" + r.duration.toFixed(0) + "ms");
             return "library asked at " + at("library-asked") + " ms, answered " + at("library-answered") +
               "; " + document.readyState + "; fetched: " + JSON.stringify(fetched.slice(-8)) +
               "; console: " + JSON.stringify(window.__e2e?.console ?? []);`,
          ).catch((e) => `(couldn't ask: ${e.message})`);
          this.log(`the page is still empty after 15 s (${waiting}): reloading it`);
          await this.js(`setTimeout(() => location.reload(), 0)`);
          await sleep(1000);
          await this.loaded();
        }
        return false;
      },
      3 * MINUTE,
    );
  }

  /** Waits for the page to load, then puts in the dialog answers. */
  async loaded() {
    await until(
      "the page",
      () => this.js(`return !!window.__TAURI_INTERNALS__ && document.readyState === "complete"`),
      MINUTE,
    );
    await this.js(`
      const real = window.fetch;
      window.__e2e = { files: [], answer: null, asked: [], restarts: 0, console: [] };
      // The page's errors, for a failure's diagnosis (run.mjs).
      const error = console.error.bind(console);
      console.error = (...args) => {
        window.__e2e.console.push(args.map(String).join(" ").slice(0, 300));
        if (window.__e2e.console.length > 20) window.__e2e.console.shift();
        error(...args);
      };
      window.addEventListener("error", (e) => window.__e2e.console.push("uncaught: " + e.message));
      window.addEventListener("unhandledrejection", (e) => window.__e2e.console.push("unhandled: " + e.reason));
      const answer = (value) =>
        new Response(JSON.stringify(value), { headers: { "Content-Type": "application/json", "Tauri-Response": "ok" } });
      window.fetch = (url, init) => {
        const cmd = decodeURIComponent(String(url)).split("/").pop();
        if (cmd === "plugin:dialog|open") return Promise.resolve(answer(window.__e2e.files));
        if (cmd === "plugin:dialog|message") {
          window.__e2e.asked.push(init?.body);
          return Promise.resolve(answer(window.__e2e.answer));
        }
        if (cmd === "plugin:process|restart") {
          window.__e2e.restarts++;
          return Promise.resolve(answer(null));
        }
        return real(url, init);
      };`);
    await this.watchPerformance();
  }

  /**
   * What the page's own measures can't say afterwards, watched from its
   * load: input events whose handling took 16 ms or more (WebKit has the
   * Event Timing API, not long tasks), and gaps between animation frames
   * over 50 ms, the main thread busy (or the page not drawn) that long.
   * Read with pageMeasures().
   */
  watchPerformance() {
    return this.js(`
      if (window.__e2ePerf) return;
      const perf = (window.__e2ePerf = { slowEvents: [], frameGaps: [] });
      const keep = (list, item) => list.length < 500 && list.push(item);
      // An element by its label, else its tag and first class.
      const describe = (el) => {
        if (!el) return null;
        const label = el.getAttribute?.("aria-label");
        if (label) return label;
        const [first] = typeof el.className === "string" ? el.className.trim().split(" ") : [];
        return el.tagName.toLowerCase() + (first ? "." + first : "");
      };
      try {
        new PerformanceObserver((list) => {
          for (const e of list.getEntries())
            keep(perf.slowEvents, { name: e.name, duration: Math.round(e.duration), at: Math.round(e.startTime), target: describe(e.target) });
        }).observe({ type: "event", durationThreshold: 16, buffered: true });
      } catch {}
      let last = performance.now();
      const frame = (now) => {
        if (now - last > 50) keep(perf.frameGaps, { ms: Math.round(now - last), at: Math.round(last) });
        last = now;
        requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);`);
  }

  /**
   * The page's measures since it loaded: its load (navigation, first paint,
   * largest paint), the app's marks and measures, what it fetched, and
   * what watchPerformance saw.
   */
  pageMeasures() {
    return this.js(`
      const round = (n) => Math.round(n);
      const [nav] = performance.getEntriesByType("navigation");
      const resources = performance.getEntriesByType("resource");
      const lcp = (() => {
        try {
          return PerformanceObserver.supportedEntryTypes.includes("largest-contentful-paint")
            ? performance.getEntriesByType("largest-contentful-paint").at(-1)?.startTime ?? null
            : null;
        } catch {
          return null;
        }
      })();
      return {
        url: location.pathname,
        load: nav ? { domContentLoaded: round(nav.domContentLoadedEventEnd), load: round(nav.loadEventEnd) } : null,
        paint: Object.fromEntries(performance.getEntriesByType("paint").map((e) => [e.name, round(e.startTime)])),
        largestPaint: lcp === null ? null : round(lcp),
        marks: performance.getEntriesByType("mark").map((e) => ({ name: e.name, at: round(e.startTime) })),
        measures: performance.getEntriesByType("measure").map((e) => ({ name: e.name, at: round(e.startTime), ms: round(e.duration) })),
        resources: {
          count: resources.length,
          slowest: [...resources].sort((a, b) => b.duration - a.duration).slice(0, 5)
            .map((r) => ({ name: r.name.slice(0, 100), at: round(r.startTime), ms: round(r.duration) })),
        },
        slowEvents: window.__e2ePerf?.slowEvents ?? [],
        frameGaps: window.__e2ePerf?.frameGaps ?? [],
        now: round(performance.now()),
      };`);
  }

  /** Files the next file picker returns. */
  pickFiles(files) {
    return this.js(`window.__e2e.files = arguments[0]`, files);
  }

  /** The button the next question is answered with. */
  answer(button) {
    return this.js(`window.__e2e.answer = arguments[0]`, button);
  }

  /** How many times the app has asked to restart. */
  restarts() {
    return this.js(`return window.__e2e.restarts`);
  }

  /** How many questions the app has asked. */
  asked() {
    return this.js(`return window.__e2e.asked.length`);
  }

  /**
   * Gets to the library: through first-run setup if the app asks for it
   * (the tools and the speech model, about 1 GB unless they're there).
   */
  async toLibrary() {
    let lastShown = "";
    const library = () => this.js(`return !!document.querySelector('input[placeholder="Search for a phrase…"]')`);
    const setup = () =>
      this.js(`return [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Download")`);
    const screen = await until(
      "setup or the library",
      async () =>
        ((await library()) && "library") ||
        ((await setup()) && "setup") ||
        // If it never comes, the error says what was showing instead.
        this.js(`return document.body.innerText.trim().slice(0, 200) || "(a blank page)"`).then((shown) => {
          lastShown = shown;
          return false;
        }),
      // Once in a while on CI, WebKitGTK takes over a minute to show the first screen.
      3 * MINUTE,
    ).catch((e) => {
      throw new Error(`${e.message}; the page showed: ${lastShown}`);
    });
    if (screen === "setup") {
      await this.clickText("Download");
      this.log("setup downloading");
      await until("setup to finish", library, 30 * MINUTE);
    }
    return screen;
  }

  /** Opens VOD `id` from the library list and waits for its player's controls. */
  async openVod(id) {
    // Rows are keyed by VOD id (edits by minus theirs: Library.svelte). One
    // open already (an add opens what it added) closes on a click.
    if (!(await this.js(`return !!document.querySelector('.vod.selected[data-key="${id}"]')`)))
      await this.click(`.vod[data-key="${id}"]`);
    await until("the player", () => this.js(`return !!document.querySelector("button.time")`), MINUTE);
  }

  /** The time the player bar shows, in seconds (whole ones, as it shows them). */
  shownTime() {
    return this.js(`
      const text = document.querySelector("button.time .now > span:not(.widest)")?.textContent ?? "";
      return text.split(":").reduce((total, part) => total * 60 + Number(part), 0);`);
  }

  /**
   * Checks the time by the play button is the frame the app's own player
   * shows: by frame, the number of the frame it says is on screen, at its
   * frame rate; with no picture (sound alone), its time. Exactly, paused; within `slack` seconds of frames, playing
   * (the two are read a moment apart). The time shown as a clock is switched to frames to
   * read, and back. Returns what it saw.
   */
  async timeIsPicture(label, { playing = false, slack = 0.1 } = {}) {
    const { text, status } = await this.js(`
      const status = await window.__TAURI_INTERNALS__.invoke("player_set_speed", { speed: 1 });
      const button = document.querySelector(".bar button.time");
      const byFrame = !!button.querySelector(".stack");
      const frames = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      if (!byFrame) { button.click(); await frames(); }
      const text = button.querySelector(".now:not(.clock) > span:not(.widest)")?.textContent ?? "";
      if (!byFrame) { button.click(); await frames(); }
      return { text, status };`);
    if (!status) throw new Error(`${label}: the app's player isn't open`);
    const fps = status.fps ?? 30;
    const at = status.shown ?? status.time;
    const want = Math.floor(at * fps + 1e-3);
    const got = Number(text);
    const what = status.shown === null ? "no picture: the time is" : "the picture is";
    const seen = `${label}: the time shows frame ${text}, ${what} frame ${want} (${at.toFixed(3)} s at ${fps} fps)`;
    if (!Number.isInteger(got) || Math.abs(got - want) > (playing ? Math.ceil(slack * fps) : 0)) throw new Error(seen);
    return seen;
  }

  /**
   * Plays the open VOD with the bar's Play button, whichever player it has
   * (the app's own draws outside the page, with no <video>), and checks the
   * time it shows runs on; returns the time it got to.
   */
  async plays() {
    const from = await this.shownTime();
    await this.click('.bar button[aria-label="Play"]');
    return until(
      "the time to run on",
      async () => {
        const error = await this.js(
          `return [...document.querySelectorAll(".error-text")].map((e) => e.textContent).join(" ")`,
        );
        if (isUnplayable(error)) throw new Error(error);
        const now = await this.shownTime();
        return now >= from + 2 && now;
      },
      MINUTE,
    );
  }

  /**
   * Back to the start with the keys: ← twice (5 s each, stopping at 0), so a
   * test file up to 10 s long is at exactly 0, wherever it was (one ← from
   * near the end of a 5.4 s file leaves it at 0.4, which shows as 0:00).
   */
  async toStart() {
    await this.key("ArrowLeft");
    await this.key("ArrowLeft");
    await until("the player at the start", async () => (await this.shownTime()) === 0, 10_000);
  }

  /**
   * Opens VOD `id` from the library, paused near its start: played (either
   * player is made on the first play), paused once it plays. Not from a
   * search hit: WebKitGTK stalls an mp3 started in its last second (TODO.md).
   */
  async openPaused(id) {
    await this.openVod(id);
    await this.click('.bar button[aria-label="Play"]');
    await until(
      "it to play",
      () => this.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    await this.pause();
  }

  /** Searches for `phrase` and plays from the first result. */
  async playFromSearch(phrase) {
    await this.type('input[placeholder="Search for a phrase…"]', phrase);
    // The results' rows, not the transcript's (both are button.row): the
    // transcript stays up until the search answers, a moment on a slow computer.
    await until("a search result", () => this.js(`return !!document.querySelector(".results button.row")`), MINUTE);
    await this.click(".results button.row");
    await until("the player", () => this.js(`return !!document.querySelector("button.time")`), MINUTE);
  }

  /**
   * In the open VOD (5 s or longer): cuts at 1 and 3 s (B), then Clip (E) with
   * the playhead between them; returns the clip once its file is made.
   *
   * Moved as a person would, with the keys (Player.svelte): ← back 5 s,
   * Shift+→ on a second. The media element's own time isn't the player's:
   * set behind its back, the player's clock (which places cuts) may not
   * follow, as on macOS once it has ended. B makes a cut and, once it's
   * recorded, selects it; the arrows would then move it, so Escape lets it
   * go, after it's selected (a quicker Escape finds nothing to let go).
   */
  async clipOneToThree(vodId) {
    const to = (seconds) =>
      until(`the player at ${seconds} s`, async () => (await this.shownTime()) === seconds, 10_000);
    const cut = async () => {
      await this.key("b");
      await until("the cut, selected", () => this.js(`return !!document.querySelector(".cut.selected")`), 10_000);
      await this.key("Escape");
      await until("the cut let go", () => this.js(`return !document.querySelector(".cut.selected")`), 10_000);
    };
    await this.pause();
    await this.toStart();
    await this.key("ArrowRight", { shift: true });
    await to(1);
    await cut();
    await this.key("ArrowRight", { shift: true });
    await this.key("ArrowRight", { shift: true });
    await to(3);
    await cut();
    const marks = await until(
      "two cuts",
      async () => {
        const marks = await this.invoke("list_marks", { vodId });
        if (marks.length === 2) return marks;
        throw new Error(`cuts at ${JSON.stringify(marks.map((m) => m.start))}`);
      },
      10_000,
    );
    const at = marks.map((m) => m.start).sort((a, b) => a - b);
    if (Math.abs(at[0] - 1) > 0.05 || Math.abs(at[1] - 3) > 0.05) throw new Error(`cuts at ${at}, not 1 and 3`);
    await this.key("ArrowLeft", { shift: true });
    await to(2);
    const clip = await this.timed("clip made (2 s)", async () => {
      await this.click("button.action.clip");
      return until(
        "the clip's file",
        async () => {
          const [c] = await this.invoke("list_clips", { vodId });
          if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
          return c?.status === "ready" && c;
        },
        MINUTE,
        { every: TIMED_POLL },
      );
    });
    if (Math.abs(clip.start - 1) > 0.05 || Math.abs(clip.end - 3) > 0.05)
      throw new Error(`the clip runs ${clip.start} to ${clip.end}, not 1 to 3`);
    return clip;
  }

  /** Adds `files` through the file picker; returns their VODs once all are in the library (not transcribed). */
  async addFiles(files) {
    const before = new Set((await this.invoke("list_vods")).map((v) => v.id));
    await this.pickFiles(files);
    await this.clickText("add video files…");
    const added = await until(
      "the files in the library",
      async () => {
        const added = (await this.invoke("list_vods")).filter((v) => !before.has(v.id));
        return added.length >= files.length && added;
      },
      MINUTE,
    );
    this.log(`added VOD${added.length > 1 ? "s" : ""} ${added.map((v) => v.id).join(", ")}`);
    return added;
  }

  /**
   * Asks for all of VOD `id` to be transcribed, with its row's Transcribe
   * button (`click`) or the command, and waits till it is; returns the VOD.
   */
  async transcribe(id, { click = false, label = String(id) } = {}) {
    if (click) await this.click(`.vod[data-key="${id}"] .actions button[aria-label="Transcribe"]`);
    else await this.invoke("transcribe_stretch", { vodId: id, start: 0, end: null });
    // From asked to done: reading its sound and transcribing it, the worker's time.
    return this.timed(`transcribed (${label})`, () =>
      until(
        "the transcription",
        async () => {
          const v = (await this.invoke("list_vods")).find((v) => v.id === id);
          if (v.status === "error") throw new Error(`transcription failed: ${v.error}`);
          return v.status === "done" && v;
        },
        15 * MINUTE,
        { every: TIMED_POLL },
      ),
    );
  }

  /** Adds `file` through the file picker, then transcribes it with its row's button; returns the VOD. */
  async addAndTranscribe(file) {
    const [vod] = await this.addFiles([file]);
    return this.transcribe(vod.id, { click: true, label: path.basename(file) });
  }
}

/**
 * Makes the app's page `width` by `height` (CSS pixels), as a small screen
 * would (the e2e build's e2e_set_inner_size); returns the size the page got
 * once it holds still, which the window manager or the screen may make a
 * little different.
 */
export async function resizeInner(app, width, height) {
  const size = () => app.js(`return innerWidth + "x" + innerHeight`);
  const before = await size();
  await app.invoke("e2e_set_inner_size", { width, height });
  // Changed (or already so), then the same over two looks.
  let last = before;
  // (A window manager that keeps the size as it was gives no change to wait
  // for: after a while, what the page has is the answer.)
  const got = await until(
    `the page to be ${width}x${height}`,
    async () => {
      const seen = await size();
      const settled = seen === last && (seen !== before || seen === `${width}x${height}`);
      last = seen;
      return settled && seen;
    },
    5_000,
  ).catch(() => last);
  const [w, h] = got.split("x").map(Number);
  return { width: w, height: h };
}
