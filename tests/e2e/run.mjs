// End-to-end tests of a VODscribe build: scenarios (scenarios/*.mjs), each
// on a fresh library, driving the app through its page.
//
//   node tests/e2e/run.mjs <the app> [--only a,b] [--driver webdriver|bridge]
//
// The webdriver driver (the default on Linux and Windows) runs the installed
// app through tauri-driver, which must be on PATH (or TAURI_DRIVER), with the
// platform's native driver: WebKitWebDriver on Linux, msedgedriver on Windows
// (NATIVE_DRIVER). macOS has no WebDriver for its webview, so there (the
// default) the bridge driver runs a build made with the `e2e` feature, a .app
// or its binary, through the app's own test hook (src-tauri/src/e2e.rs).
//
// Each scenario empties the app's data folder first (lib/data.mjs), keeping
// the tools and speech model; set E2E_RESET_DATA=1 to allow it once there's
// a library. On failure: the page's errors and a screenshot in E2E_OUT
// (default tests/e2e/out). Every scenario runs; the exit code says if any failed.

import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

import { App } from "./lib/app.mjs";
import { resetData } from "./lib/data.mjs";
import * as drivers from "./lib/drivers.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const { values: opts, positionals } = parseArgs({
  allowPositionals: true,
  options: { only: { type: "string" }, driver: { type: "string" } },
});
const app = positionals[0];
if (!app) {
  console.error("usage: node tests/e2e/run.mjs <the app> [--only a,b] [--driver webdriver|bridge]");
  process.exit(2);
}
const driverName = opts.driver ?? (process.platform === "darwin" ? "bridge" : "webdriver");
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

const started = Date.now();
const results = [];
for (const name of names.filter((n) => !only || only.includes(n))) {
  const { default: scenario } = await import(pathToFileURL(path.join(here, "scenarios", `${name}.mjs`)));
  const begun = Date.now();
  const log = (what) => console.log(`[${((Date.now() - started) / 1000).toFixed(0)} s] ${name}: ${what}`);
  let driver;
  let error;
  try {
    resetData();
    driver = traced(await startDriver(app), log);
    log("app started");
    const instance = new App(driver, log);
    await instance.ready();
    await scenario.run(instance, { fixtures });
    // Whatever it did, the page shouldn't be showing an error at the end.
    const errors = await instance.errors();
    if (errors.length) throw new Error(`the page shows: ${errors.join(" | ")}`);
  } catch (e) {
    error = e.message;
    console.error(`${name}: FAILED: ${error}`);
    if (driver) await diagnose(driver, name);
  } finally {
    await driver?.close();
  }
  results.push({ name, ok: !error, error, seconds: Math.round((Date.now() - begun) / 1000) });
  log(error ? "failed" : "passed");
}

mkdirSync(out, { recursive: true });
writeFileSync(path.join(out, "results.json"), JSON.stringify(results, null, 2));
const failed = results.filter((r) => !r.ok);
console.log(`\ne2e: ${results.length - failed.length} of ${results.length} passed`);
for (const r of results)
  console.log(`  ${r.ok ? "ok  " : "FAIL"} ${r.name} (${r.seconds} s)${r.ok ? "" : `: ${r.error}`}`);
process.exit(failed.length ? 1 : 0);

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
  const png = await driver.screenshot().catch(() => null);
  if (png) {
    mkdirSync(out, { recursive: true });
    const file = path.join(out, `${name}.png`);
    writeFileSync(file, png);
    console.error(`  screenshot: ${file}`);
  }
}
