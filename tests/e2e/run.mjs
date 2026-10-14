// End-to-end test of an installed VODscribe, driven over WebDriver through
// tauri-driver: first-run setup, adding a file, transcribing it, the
// transcript, searching, playing from a result, and removing it (answering
// "Keep" first, which must keep it).
//
//   node tests/e2e/run.mjs <the installed app>
//
// tauri-driver must be on PATH (or TAURI_DRIVER), with the platform's native
// driver: WebKitWebDriver on Linux, msedgedriver on Windows (NATIVE_DRIVER).
// macOS has no WebDriver for its webview. It runs the app with its usual data
// folder: on Linux, point XDG_DATA_HOME somewhere else to keep a library safe.
//
// The app's native dialogs can't be clicked over WebDriver, so the test
// answers them: the file picker returns the fixture, and a question the
// button the test chose. Tauri's invoke can't be replaced (it's defined
// read-only), so the answers go in at the fetch that carries each call.

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const app = process.argv[2];
if (!app) {
  console.error("usage: node tests/e2e/run.mjs <the installed app>");
  process.exit(2);
}
const FIXTURE = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "eagle-has-landed.mp3");
const PORT = 4444;
const DRIVER = `http://127.0.0.1:${PORT}`;
const MINUTE = 60_000;

const driverArgs = process.env.NATIVE_DRIVER ? ["--native-driver", process.env.NATIVE_DRIVER] : [];
const driver = spawn(process.env.TAURI_DRIVER ?? "tauri-driver", ["--port", String(PORT), ...driverArgs], {
  stdio: ["ignore", "inherit", "inherit"],
});

async function wd(method, url, body) {
  const res = await fetch(DRIVER + url, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body && JSON.stringify(body),
  });
  const json = await res.json();
  if (!res.ok) throw new Error(`${method} ${url}: ${JSON.stringify(json.value ?? json)}`);
  return json.value;
}

let session;
const started = Date.now();
const step = (what) => console.log(`[${((Date.now() - started) / 1000).toFixed(0)} s] ${what}`);

/** Runs `body` (a function body, with `args`) in the page and returns its awaited result. */
const js = (body, ...args) =>
  wd("POST", `/session/${session}/execute/sync`, {
    script: `return (async () => { ${body} })()`,
    args,
  });

/** Calls the app's command `cmd` from the page, as the interface does. */
const invoke = (cmd, args = {}) =>
  js(`return window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1])`, cmd, args);

/** Polls `check` until it returns something truthy, or fails after `timeout` saying `what`. */
async function until(what, check, timeout) {
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

const click = (selector) =>
  js(
    `const el = document.querySelector(arguments[0]);
     if (!el) throw new Error("no " + arguments[0]);
     el.click();`,
    selector,
  );

/** Clicks the button whose text is `text`. */
const clickText = (text) =>
  js(
    `const el = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === arguments[0]);
     if (!el) throw new Error("no button " + arguments[0]);
     el.click();`,
    text,
  );

async function main() {
  await until("tauri-driver", () => fetch(`${DRIVER}/status`).then((r) => r.ok), MINUTE);
  session = (
    await wd("POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
    })
  ).sessionId;
  step("app started");
  await until(
    "the page",
    () => js(`return !!window.__TAURI_INTERNALS__ && document.readyState === "complete"`),
    MINUTE,
  );
  await js(`
    const real = window.fetch;
    window.__e2e = { files: [], answer: null, asked: [] };
    const answer = (value) =>
      new Response(JSON.stringify(value), { headers: { "Content-Type": "application/json", "Tauri-Response": "ok" } });
    window.fetch = (url, init) => {
      const cmd = decodeURIComponent(String(url)).split("/").pop();
      if (cmd === "plugin:dialog|open") return Promise.resolve(answer(window.__e2e.files));
      if (cmd === "plugin:dialog|message") {
        window.__e2e.asked.push(init?.body);
        return Promise.resolve(answer(window.__e2e.answer));
      }
      return real(url, init);
    };`);

  // First-run setup, if the app asks for it: the tools and the speech model
  // (about 1 GB, unless it's there already). What it needs is the app's call.
  const library = () => js(`return !!document.querySelector('input[placeholder="Search for a phrase…"]')`);
  const setup = () =>
    js(`return [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Download")`);
  const screen = await until(
    "setup or the library",
    async () => ((await library()) && "library") || ((await setup()) && "setup"),
    MINUTE,
  );
  if (screen === "setup") {
    await clickText("Download");
    step("setup downloading");
    await until("setup to finish", library, 30 * MINUTE);
  }
  step("set up");

  // Adding a file, through the file picker.
  await js(`window.__e2e.files = [arguments[0]]`, FIXTURE);
  await clickText("add video files…");
  const [vod] = await until(
    "the VOD in the library",
    async () => {
      const vods = await invoke("list_vods");
      return vods.length === 1 && vods;
    },
    MINUTE,
  );
  step(`added VOD ${vod.id}`);

  const done = await until(
    "the transcription",
    async () => {
      const v = (await invoke("list_vods"))[0];
      if (v.status === "error") throw new Error(`transcription failed: ${v.error}`);
      return v.status === "done" && v;
    },
    15 * MINUTE,
  );
  const text = (await invoke("transcript", { vodId: done.id })).map((l) => l.text).join(" ");
  if (!/landed/i.test(text)) throw new Error(`the transcript doesn't have "landed": ${text}`);
  step(`transcribed: "${text}"`);

  // Searching, and playing from the result.
  await js(
    `const input = document.querySelector('input[placeholder="Search for a phrase…"]');
     input.value = "landed";
     input.dispatchEvent(new Event("input", { bubbles: true }));`,
  );
  await until("a search result", () => js(`return !!document.querySelector("button.row")`), MINUTE);
  await click("button.row");
  await until(
    "the player",
    () =>
      js(`
        const video = document.querySelector("video");
        const error = [...document.querySelectorAll(".error-text")].map((e) => e.textContent).join(" ");
        if (/couldn't be played|can't be played/.test(error)) throw new Error(error);
        return !!video && video.readyState >= 1 && video.currentTime > 0;`),
    MINUTE,
  );
  step("searched and played");

  // Removing: "Keep" keeps it, "Remove" removes it.
  await js(`window.__e2e.answer = "Keep"`);
  await click('button[aria-label="Remove"]');
  await sleep(2000);
  const asked = await js(`return window.__e2e.asked.length`);
  if (asked !== 1) throw new Error(`Remove asked ${asked} times, not once`);
  if ((await invoke("list_vods")).length !== 1) throw new Error(`answering "Keep" removed the VOD`);
  await js(`window.__e2e.answer = "Remove"`);
  await click('button[aria-label="Remove"]');
  await until("the VOD to be removed", async () => (await invoke("list_vods")).length === 0, MINUTE);
  step("removed, after asking");
}

/** On a failure: what the page shows as errors, and a screenshot in E2E_OUT (default tests/e2e/out). */
async function diagnose() {
  if (!session) return;
  const errors = await js(
    `return [...document.querySelectorAll(".error-text")].map((e) => e.textContent.trim()).filter(Boolean)`,
  ).catch((e) => [`(couldn't read the page: ${e.message})`]);
  if (errors.length) console.error(`the page shows: ${errors.join(" | ")}`);
  const out = process.env.E2E_OUT ?? path.join(path.dirname(fileURLToPath(import.meta.url)), "out");
  const png = await wd("GET", `/session/${session}/screenshot`).catch(() => null);
  if (png) {
    mkdirSync(out, { recursive: true });
    writeFileSync(path.join(out, "failure.png"), Buffer.from(png, "base64"));
    console.error(`screenshot: ${path.join(out, "failure.png")}`);
  }
}

let failed = false;
try {
  await main();
  console.log("e2e: passed");
} catch (e) {
  failed = true;
  console.error(`e2e: FAILED: ${e.message}`);
  await diagnose();
} finally {
  if (session) await wd("DELETE", `/session/${session}`).catch(() => {});
  driver.kill();
}
process.exit(failed ? 1 : 0);
