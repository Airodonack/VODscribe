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

import { setTimeout as sleep } from "node:timers/promises";

export const MINUTE = 60_000;

/** Polls `check` until it returns something truthy, or fails after `timeout` saying `what`. */
export async function until(what, check, timeout) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      last = await check();
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    await sleep(500);
  }
  throw new Error(`timed out waiting for ${what} (last: ${JSON.stringify(last)})`);
}

export class App {
  constructor(driver, log) {
    this.driver = driver;
    this.log = log;
  }

  /** Runs `body` (a function body, with `arguments`) in the page and returns its awaited result. */
  js(body, ...args) {
    return this.driver.run(body, args);
  }

  /** Calls the app's command `cmd` from the page, as the interface does. */
  invoke(cmd, args = {}) {
    return this.js(`return window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1])`, cmd, args);
  }

  click(selector) {
    return this.js(
      `const el = document.querySelector(arguments[0]);
       if (!el) throw new Error("no " + arguments[0]);
       el.click();`,
      selector,
    );
  }

  /** Clicks the button whose text is `text`. */
  clickText(text) {
    return this.js(
      `const el = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === arguments[0]);
       if (!el) throw new Error("no button " + arguments[0]);
       el.click();`,
      text,
    );
  }

  /** Types `value` into the input `selector`, as a person would (an input event). */
  type(selector, value) {
    return this.js(
      `const input = document.querySelector(arguments[0]);
       if (!input) throw new Error("no " + arguments[0]);
       input.value = arguments[1];
       input.dispatchEvent(new Event("input", { bubbles: true }));`,
      selector,
      value,
    );
  }

  /** Presses `key` (a KeyboardEvent key), with Shift if `shift`, as the window gets it. */
  key(key, { shift = false } = {}) {
    return this.js(
      `document.body.dispatchEvent(new KeyboardEvent("keydown", { key: arguments[0], shiftKey: arguments[1], bubbles: true }))`,
      key,
      shift,
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

  /** Error text the page shows, if any. */
  errors() {
    return this.js(
      `return [...document.querySelectorAll(".error-text")].map((e) => e.textContent.trim()).filter(Boolean)`,
    );
  }

  /** Waits for the page, then puts in the dialog answers. */
  async ready() {
    await until(
      "the page",
      () => this.js(`return !!window.__TAURI_INTERNALS__ && document.readyState === "complete"`),
      MINUTE,
    );
    await this.js(`
      const real = window.fetch;
      window.__e2e = { files: [], answer: null, asked: [], restarts: 0 };
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
    // On WebKitGTK 2.54 (Debian 13), a driven start sometimes leaves the page
    // empty for about two minutes; undriven, it never has (TODO.md). One
    // reload, said in the log, gets past it without hiding it.
    const begun = Date.now();
    let reloaded = false;
    const stuck = async () => {
      if (reloaded || Date.now() - begun < 15_000) return false;
      if (await this.js(`return document.body.innerText.trim() !== ""`)) return false;
      reloaded = true;
      this.log("the page is still empty after 15 s: reloading it");
      await this.js(`setTimeout(() => location.reload(), 0)`);
      await sleep(1000);
      await this.ready();
      return false;
    };
    const screen = await until(
      "setup or the library",
      async () =>
        ((await library()) && "library") ||
        ((await setup()) && "setup") ||
        (await stuck()) ||
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
    // Rows are keyed by VOD id (edits by minus theirs: Library.svelte).
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
        if (/couldn't be played|can't be played/.test(error)) throw new Error(error);
        const now = await this.shownTime();
        return now >= from + 2 && now;
      },
      MINUTE,
    );
  }

  /** Adds `file` through the file picker and waits for it to be transcribed; returns the VOD. */
  async addAndTranscribe(file) {
    await this.pickFiles([file]);
    await this.clickText("add video files…");
    const [vod] = await until(
      "the VOD in the library",
      async () => {
        const vods = await this.invoke("list_vods");
        return vods.length === 1 && vods;
      },
      MINUTE,
    );
    this.log(`added VOD ${vod.id}`);
    return until(
      "the transcription",
      async () => {
        const v = (await this.invoke("list_vods")).find((v) => v.id === vod.id);
        if (v.status === "error") throw new Error(`transcription failed: ${v.error}`);
        return v.status === "done" && v;
      },
      15 * MINUTE,
    );
  }
}
