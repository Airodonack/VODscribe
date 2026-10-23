// End-to-end tests of a VODscribe build: scenarios (scenarios/*.mjs), each
// on a fresh library, driving the app through its page.
//
//   node tests/e2e/run.mjs <the app> [--only a,b] [--driver webdriver|bridge] [--profile <dir>]
//
// The webdriver driver (the default on Linux) runs the installed app through
// tauri-driver, which must be on PATH (or TAURI_DRIVER), with the platform's
// native driver: WebKitWebDriver on Linux, msedgedriver on Windows
// (NATIVE_DRIVER; only a build with the `e2e` feature lets it in). macOS
// has no WebDriver for its webview, and a Windows release none either, so
// there (the default) the bridge driver runs a build made with the `e2e`
// feature, a .app or its binary, through the app's own test hook
// (src-tauri/src/e2e.rs).
//
// Each scenario empties the app's data folder first (lib/data.mjs), keeping
// the tools and speech model; set E2E_RESET_DATA=1 to allow it once there's
// a library. On failure: the page's errors and a screenshot in E2E_OUT
// (default tests/e2e/out). Every scenario runs; the exit code says if any failed.
//
// A scenario exports { name, before?(ctx), run(app, ctx), allowErrors?,
// needs? } (needs: ["network"] runs only with E2E_NETWORK=1): `before` sets up
// the data folder before the app starts; `run` gets an App (lib/app.mjs) and
// ctx: { fixtures, out, log, quit(), start(), restart(), skip(why),
// checkpoint(label) }: quit ends the app, start starts it again and returns
// the new App, restart does both; skip ends the scenario as skipped (it
// can't run here), not failed; checkpoint keeps a picture of the window
// for looking at (out/checkpoints, contact-sheet.mjs).
//
// --profile <dir> (Linux, webdriver): each scenario's app sampled by perf,
// with its children (lib/profile.sh, lib/profile.mjs): <dir>/<scenario>/
// gets the recordings, folded.txt and top.txt (README, "Profiling").

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

import { isUnplayable } from "../../apps/desktop/src/lib/media.ts";
import { App, watchWith } from "./lib/app.mjs";
import { resetData } from "./lib/data.mjs";
import * as drivers from "./lib/drivers.mjs";
import * as os from "./lib/os.mjs";
import { preflight } from "./lib/preflight.mjs";
import * as profile from "./lib/profile.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const { values: opts, positionals } = parseArgs({
  allowPositionals: true,
  options: { only: { type: "string" }, driver: { type: "string" }, profile: { type: "string" } },
});
if (!positionals[0]) {
  console.error("usage: node tests/e2e/run.mjs <the app> [--only a,b] [--driver webdriver|bridge] [--profile <dir>]");
  process.exit(2);
}
const driverName = opts.driver ?? (process.platform === "linux" ? "webdriver" : "bridge");
if (opts.profile && (process.platform !== "linux" || driverName !== "webdriver"))
  throw new Error("--profile is for Linux, through the webdriver driver");
// Profiled, the driver starts lib/profile.sh, which starts the app under perf.
const profileDir = opts.profile && path.resolve(opts.profile);
const app = profileDir ? profile.profiled(profileDir, positionals[0], process.env.PROFILE_CG) : positionals[0];
const startDriver = drivers[driverName];
if (!startDriver) throw new Error(`no driver ${driverName}`);

const out = process.env.E2E_OUT ?? path.join(here, "out");
const fixtures = { landed: path.join(here, "fixtures", "eagle-has-landed.mp3") };

// Setup first: it's where a new install starts, and it downloads what the others keep.
const order = ["setup"];
const names = readdirSync(path.join(here, "scenarios"))
  .filter((f) => f.endsWith(".mjs"))
  .map((f) => f.slice(0, -4))
  .sort((a, b) => (order.includes(b) ? 1 : 0) - (order.includes(a) ? 1 : 0) || a.localeCompare(b));
const only = opts.only?.split(",");
if (only) {
  const unknown = only.filter((n) => !names.includes(n));
  if (unknown.length) throw new Error(`no scenario ${unknown.join(", ")} (there are: ${names.join(", ")})`);
}

/** Thrown by ctx.skip: the scenario can't run here (said in the results, not a failure). */
class Skip extends Error {}

// The computer first: a gap it has is named once, not found as timeouts.
for (const warning of preflight()) console.log(`preflight: ${warning}`);

const started = Date.now();
const results = [];
// Each profiled scenario's summary, for perf.json.
const profiles = {};
// How many checkpoints taken, numbering their files in order.
let checkpoints = 0;
rmSync(path.join(out, "checkpoints"), { recursive: true, force: true });
for (const name of names.filter((n) => !only || only.includes(n))) {
  const { default: scenario } = await import(pathToFileURL(path.join(here, "scenarios", `${name}.mjs`)));
  const begun = Date.now();
  const log = (what) => console.log(`[${((Date.now() - started) / 1000).toFixed(0)} s] ${name}: ${what}`);
  // The app as it is now: `ctx.restart()` (for what must survive one) replaces it.
  let current;
  const launch = async () => {
    const driver = traced(await startDriver(app), log);
    current = { driver, app: new App(driver, log) };
    log("app started");
    await current.app.ready();
    return current.app;
  };
  const ctx = {
    fixtures,
    out,
    log,
    quit: async () => {
      const closing = current;
      current = null;
      await closing?.driver.close();
    },
    start: launch,
    skip: (why) => {
      throw new Skip(why);
    },
    checkpoint: (label) => checkpoint(current, name, label),
    restart: async () => {
      await ctx.quit();
      return launch();
    },
  };
  // A wait gives up at once when the page says a video can't be played.
  watchWith(
    scenario.allowErrors
      ? null
      : async () => {
          if (!current) return null;
          const shown = await current.driver.run(
            `return [...document.querySelectorAll(".error-text")].map((e) => e.textContent.trim()).join(" | ")`,
          );
          return isUnplayable(shown) ? `"${shown}"` : null;
        },
  );
  let error;
  let skipped;
  try {
    // Scenarios that reach the sites run only when asked: E2E_NETWORK=1.
    if (scenario.needs?.includes("network") && process.env.E2E_NETWORK !== "1")
      throw new Skip("needs the network: set E2E_NETWORK=1");
    resetData();
    if (profileDir) profile.recordingFor(profileDir, name);
    // What a scenario sets up in the data folder before the app starts.
    await scenario.before?.(ctx);
    await scenario.run(await launch(), ctx);
    // Whatever it did, the page shouldn't be showing an error at the end
    // (unless showing one is what it checks: allowErrors).
    const errors = current && !scenario.allowErrors ? await current.app.errors() : [];
    if (errors.length) throw new Error(`the page shows: ${errors.join(" | ")}`);
  } catch (e) {
    if (e instanceof Skip) {
      skipped = e.message;
    } else {
      error = e.message;
      console.error(`${name}: FAILED: ${error}`);
      if (current) await diagnose(current.driver, name);
    }
  } finally {
    await current?.driver.close();
  }
  if (profileDir) {
    const summary = await profile.finish(profileDir, name).catch((e) => {
      console.error(`${name}: its profile couldn't be read: ${e.message}`);
      return null;
    });
    if (summary) profiles[name] = summary;
  }
  results.push({ name, ok: !error, error, skipped, seconds: Math.round((Date.now() - begun) / 1000) });
  log(error ? "failed" : skipped ? `skipped: ${skipped}` : "passed");
}

mkdirSync(out, { recursive: true });
writeFileSync(path.join(out, "results.json"), JSON.stringify(results, null, 2));
if (profileDir) writeFileSync(path.join(profileDir, "profiles.json"), JSON.stringify(profiles, null, 2));
const failed = results.filter((r) => !r.ok);
console.log(`\ne2e: ${results.length - failed.length} of ${results.length} passed`);
for (const r of results)
  console.log(
    `  ${r.skipped ? "skip" : r.ok ? "ok  " : "FAIL"} ${r.name} (${r.seconds} s)${r.error ? `: ${r.error}` : r.skipped ? `: ${r.skipped}` : ""}`,
  );
process.exit(failed.length ? 1 : 0);

/**
 * A picture of the app's window at a named moment, for looking at
 * (contact-sheet.mjs): from the screen where the OS layer works, so the
 * app's own player's picture is in it, else the driver's (the page only).
 * Never fails a scenario: a checkpoint that can't be taken is noted.
 */
async function checkpoint(current, scenario, label) {
  if (!current) return;
  const dir = path.join(out, "checkpoints");
  mkdirSync(dir, { recursive: true });
  // A moment for what was just done to be drawn.
  await new Promise((r) => setTimeout(r, 400));
  let png = null;
  let from = "screen";
  try {
    if (os.unavailable()) throw new Error(os.unavailable());
    png = os.crop(os.screenshot(), await os.windowRect(current.app));
  } catch {
    from = "page";
    png = await current.driver.screenshot().catch(() => null);
  }
  if (!png) return console.error(`  checkpoint ${label}: no picture`);
  checkpoints += 1;
  const file = `${String(checkpoints).padStart(2, "0")}-${scenario}-${label}.png`;
  writeFileSync(path.join(dir, file), png);
  const index = path.join(dir, "index.json");
  const list = existsSync(index) ? JSON.parse(readFileSync(index, "utf8")) : [];
  list.push({ file, scenario, label, from, platform: process.platform });
  writeFileSync(index, JSON.stringify(list, null, 2));
}

/** With E2E_TRACE=1, logs each call into the page and how long it took. */
function traced(driver, log) {
  if (process.env.E2E_TRACE !== "1") return driver;
  const run = driver.run;
  return {
    ...driver,
    run: async (body, args, waitMs) => {
      const begun = Date.now();
      const what = body.replace(/\s+/g, " ").trim().slice(0, 80);
      try {
        return await run(body, args, waitMs);
      } finally {
        log(`${Date.now() - begun} ms: ${what}`);
      }
    },
  };
}

/** On a failure: what the page shows as errors, and a screenshot. */
async function diagnose(driver, name) {
  const errors = await driver
    .run(`return [...document.querySelectorAll(".error-text")].map((e) => e.textContent.trim()).filter(Boolean)`)
    .catch((e) => [`(couldn't read the page: ${e.message})`]);
  if (errors.length) console.error(`  the page shows: ${errors.join(" | ")}`);
  // What it showed, as text: where there's no screenshot (macOS over SSH), it's all there is.
  const shown = await driver
    .run(
      `const media = document.querySelector("video, audio");
       return [
         "player: " + (media ? JSON.stringify({ src: media.currentSrc, readyState: media.readyState,
           networkState: media.networkState, paused: media.paused, currentTime: media.currentTime,
           error: media.error && (media.error.code + " " + media.error.message) }) : "no <video> or <audio>"),
         "time shown: " + (document.querySelector("button.time")?.textContent.trim() ?? "none"),
         "page: " + document.visibilityState + (document.hasFocus() ? ", focused" : ", not focused"),
         "console errors: " + JSON.stringify(window.__e2e?.console ?? []),
         "",
         document.body.innerText,
       ].join("\\n")`,
    )
    .catch((e) => `(couldn't read the page: ${e.message})`);
  mkdirSync(out, { recursive: true });
  writeFileSync(path.join(out, `${name}.txt`), shown);
  console.error(`  page: ${path.join(out, `${name}.txt`)}`);
  const png = await driver.screenshot().catch(() => null);
  if (png) {
    mkdirSync(out, { recursive: true });
    const file = path.join(out, `${name}.png`);
    writeFileSync(file, png);
    console.error(`  screenshot: ${file}`);
  }
}
