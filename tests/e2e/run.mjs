// End-to-end tests of a VODscribe build: scenarios (scenarios/*.mjs), each
// on a fresh library, driving the app through its page.
//
//   node tests/e2e/run.mjs <the app> [--only a,b] [--driver webdriver|bridge] [--repeat N] [--profile <dir>]
//                          [--parallel N] [--shard i/N]
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
// Without it, E2E_OUT/perf/<scenario>/page.json still has what the page
// measured (App.pageMeasures).
//
// Timings: a scenario times the steps a person waits on (ctx.time,
// ctx.timing, and App's helpers), and the run its app's start; each
// result in results.json has them (`timings`, ms), and E2E_OUT/perf.json
// has them across runs (--repeat N runs the chosen scenarios N times, in
// turn: the median and spread) with what the run was on, for comparing
// releases (perf-compare.mjs), and the app's memory at each scenario's end
// (Linux: its processes' Pss, lib/memory.mjs). A profiled run's timings are
// marked so: perf slows what it watches.
//
// --parallel N (Linux): N apps at once, each a worker of its own: this
// script again, with its own data folders (XDG_DATA_HOME and the others,
// with "-<i>" added; the tools and model hard-linked in from the configured
// data folder), driver port (E2E_DRIVER_PORT + i), D-Bus session, X display
// (Xvfb with openbox: the OS layer's screenshots and clicks are of the
// whole screen, so two windows on one would see and click each other) and
// out folder (E2E_OUT/workers/<i>, merged into E2E_OUT at the end, in
// scenario order). Scenarios are dealt longest first by durations.json
// (lib/durations.mjs). --shard i/N runs the i-th of N such deals, for CI's
// machines to split the suite between them.
//
// The flake ledger (flaky.json: scenario, the reason, dated): a listed
// scenario that fails runs once more, and a pass then is reported "flaky",
// apart from the passes, with `attempts` and `flaky: true` in results.json.

import { execFileSync, spawn } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  renameSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import * as nodeOs from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

import { isUnplayable } from "../../apps/desktop/src/lib/media.ts";
import { App, clock, watchWith } from "./lib/app.mjs";
import { dataDirs, resetData } from "./lib/data.mjs";
import * as drivers from "./lib/drivers.mjs";
import { deal, parseShard, readDurations } from "./lib/durations.mjs";
import { pageDump } from "./lib/dump.mjs";
import { memoryOf } from "./lib/memory.mjs";
import * as os from "./lib/os.mjs";
import { preflight } from "./lib/preflight.mjs";
import * as profile from "./lib/profile.mjs";
import { startFileHost } from "./lib/fileHost.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const { values: opts, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    only: { type: "string" },
    driver: { type: "string" },
    repeat: { type: "string", default: "1" },
    profile: { type: "string" },
    parallel: { type: "string", default: "1" },
    shard: { type: "string" },
  },
});
if (!positionals[0]) {
  console.error(
    "usage: node tests/e2e/run.mjs <the app> [--only a,b] [--driver webdriver|bridge] [--repeat N] [--profile <dir>] [--parallel N] [--shard i/N]",
  );
  process.exit(2);
}
const repeat = Number(opts.repeat);
if (!Number.isInteger(repeat) || repeat < 1) throw new Error(`--repeat ${opts.repeat}: a whole number, 1 or more`);
// Timings come from unprofiled runs; a profile is of one.
if (repeat > 1 && opts.profile) throw new Error("--profile with --repeat: profile one run, time the others");
const driverName = opts.driver ?? (process.platform === "linux" ? "webdriver" : "bridge");
if (opts.profile && (process.platform !== "linux" || driverName !== "webdriver"))
  throw new Error("--profile is for Linux, through the webdriver driver");
// Profiled, the driver starts lib/profile.sh, which starts the app under perf.
const profileDir = opts.profile && path.resolve(opts.profile);
const app = profileDir ? profile.profiled(profileDir, positionals[0], process.env.PROFILE_CG) : positionals[0];
const startDriver = drivers[driverName];
if (!startDriver) throw new Error(`no driver ${driverName}`);
const parallel = Number(opts.parallel);
if (!Number.isInteger(parallel) || parallel < 1)
  throw new Error(`--parallel ${opts.parallel}: a whole number, 1 or more`);
const shard = opts.shard ? parseShard(opts.shard) : null;
// Set in a worker of a --parallel run: its number, from 1.
const worker = process.env.E2E_WORKER;

const out = process.env.E2E_OUT ?? path.join(here, "out");
const fixtures = { landed: path.join(here, "fixtures", "eagle-has-landed.mp3") };
// The file host the app asks for annotations, before any app starts (lib/fileHost.mjs).
await startFileHost();

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

// The flake ledger: scenarios allowed one more run on a failure, with why.
const ledger = JSON.parse(readFileSync(path.join(here, "flaky.json"), "utf8"));

// The computer first: a gap it has is named once, not found as timeouts
// (by the run, not again by each of its workers).
if (!worker) for (const warning of preflight()) console.log(`preflight: ${warning}`);

const started = Date.now();
const results = [];
// How many apps ran at once (--parallel: fewer if there were fewer scenarios).
let workers = 1;
// Where each scenario's measures go (<scenario>/page.json): with the profile, if there's one.
const perfOut = profileDir ?? path.join(out, "perf");
if (!profileDir) rmSync(perfOut, { recursive: true, force: true });
// Each profiled scenario's summary, for perf.json.
const profiles = {};
// How many checkpoints taken, numbering their files in order.
let checkpoints = 0;
rmSync(path.join(out, "checkpoints"), { recursive: true, force: true });
// The scenarios chosen, and of them this shard's (in the same order).
const durations = readDurations(path.join(here, "durations.json"));
let chosen = names.filter((n) => !only || only.includes(n));
if (shard) {
  // Every shard starts from setup too: a shard dealt without it began cold,
  // and its first scenario's fixtures found no ffmpeg (CI, 2026-10-30).
  const mine = deal(chosen, durations, shard.count)[shard.index - 1].names;
  chosen = chosen.filter((n) => mine.includes(n) || order.includes(n));
}
if (parallel > 1) {
  const merged = await runWorkers(chosen);
  results.push(...merged);
} else {
  // Each run of each scenario, in turn (setup once: it's what the others start from).
  const turns = Array.from({ length: repeat }, (_, i) =>
    chosen.filter((n) => i === 0 || !order.includes(n)).map((name) => ({ name, run: i + 1 })),
  ).flat();
  for (const turn of turns) {
    const taken = checkpoints;
    const first = await runTurn(turn);
    if (first.ok || !ledger[turn.name]) {
      results.push(first);
      continue;
    }
    // In the ledger: once more, the first attempt's evidence kept apart.
    console.log(`${turn.name}: in the flake ledger (${ledger[turn.name]}): once more`);
    keepFirstAttempt(turn.name, taken);
    const again = await runTurn(turn);
    results.push({ ...again, attempts: 2, firstError: first.error, ...(again.ok && { flaky: true }) });
  }
}

mkdirSync(out, { recursive: true });
writeFileSync(path.join(out, "results.json"), JSON.stringify(results, null, 2));
if (parallel === 1) writeFileSync(path.join(out, "perf.json"), JSON.stringify(perfSummary(), null, 2));
const failed = results.filter((r) => !r.ok);
// A worker's summary is the whole run's, printed by the run.
if (!worker) summarize();
process.exit(failed.length ? 1 : 0);

/** One run of a scenario: its result, for results.json. */
async function runTurn({ name, run }) {
  const { default: scenario } = await import(pathToFileURL(path.join(here, "scenarios", `${name}.mjs`)));
  const begun = Date.now();
  // This run's folder for its measures: <scenario>, then <scenario>-2, …
  const measures = path.join(perfOut, run === 1 ? name : `${name}-${run}`);
  const log = (what) => console.log(`[${((Date.now() - started) / 1000).toFixed(0)} s] ${name}: ${what}`);
  // The app as it is now: `ctx.restart()` (for what must survive one) replaces it.
  let current;
  // What the page measured, from each start of the app (App.pageMeasures),
  // read before it closes.
  const pages = [];
  const measurePage = async () => {
    const measured = await current?.app.pageMeasures().catch(() => null);
    if (measured) pages.push(measured);
  };
  // The steps timed: { name, ms, from, to } (from and to on the monotonic
  // clock, in seconds, as perf's samples are stamped: lib/profile.mjs).
  const steps = [];
  const timing = (step, ms, from, to = clock()) => steps.push({ name: step, ms, from, to });
  const time = async (step, fn) => {
    const from = clock();
    const result = await fn();
    const to = clock();
    timing(step, Math.round((to - from) * 1000), from, to);
    return result;
  };
  const launch = async () => {
    const from = clock();
    const driver = traced(await startDriver(app), log);
    current = { driver, app: new App(driver, log) };
    Object.assign(current.app, { timed: time, timing });
    log("app started");
    await current.app.ready();
    // From asking for the app to its first screen: the app's start, the
    // page's load and its first call (WebDriver's session is part of it).
    // The first start apart from later ones (ctx.restart): the library is
    // new then; and one that took App.ready's reload apart from both, not
    // to hide that stall in a median.
    const first = !steps.some((s) => /^start to/.test(s.name));
    const step = `${first ? "start" : "restart"} to first screen${current.app.reloaded ? " (reloaded)" : ""}`;
    timing(step, Math.round((clock() - from) * 1000), from);
    return current.app;
  };
  const ctx = {
    fixtures,
    out,
    log,
    time,
    timing,
    clock,
    quit: async () => {
      await measurePage();
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
  let memory = null;
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
    await measurePage();
    // The app's memory at the end, with its children's (Linux; lib/memory.mjs).
    memory = current ? memoryOf(positionals[0]) : null;
    await current?.driver.close();
    // A button or key a failed gesture left down would spoil the next scenario's.
    try {
      os.letGoOfAll();
    } catch (e) {
      console.error(`${name}: couldn't let go of what its gestures held: ${e.message}`);
    }
  }
  const page = pages.length
    ? { pages, slowEvents: pages.flatMap((p) => p.slowEvents), frameGaps: pages.flatMap((p) => p.frameGaps) }
    : null;
  if (page) {
    mkdirSync(measures, { recursive: true });
    writeFileSync(path.join(measures, "page.json"), JSON.stringify(page, null, 2));
  }
  if (profileDir) {
    const summary = await profile.finish(profileDir, name, steps, page).catch((e) => {
      console.error(`${name}: its profile couldn't be read: ${e.message}`);
      return null;
    });
    if (summary) profiles[name] = summary;
  }
  log(error ? "failed" : skipped ? `skipped: ${skipped}` : "passed");
  return {
    name,
    ...(repeat > 1 && { run }),
    ok: !error,
    error,
    skipped,
    seconds: Math.round((Date.now() - begun) / 1000),
    timings: byStep(steps),
    ...(memory && { memory }),
  };
}

/**
 * The run's summary: how many passed (a flaky pass counted apart), a line
 * for each scenario, the ledger's size, and the wall time on how many workers.
 */
function summarize() {
  const flaky = results.filter((r) => r.flaky);
  const passed = results.length - failed.length - flaky.length;
  console.log(`\ne2e: ${passed} of ${results.length} passed${flaky.length ? `, ${flaky.length} flaky` : ""}`);
  for (const r of results) {
    const mark = r.flaky ? "flaky" : r.skipped ? "skip" : r.ok ? "ok  " : "FAIL";
    const why = r.error ?? r.skipped ?? (r.flaky ? `failed first: ${r.firstError}` : null);
    const again = !r.flaky && r.attempts > 1 ? " (twice)" : "";
    console.log(`  ${mark} ${r.name} (${r.seconds} s)${again}${why ? `: ${why}` : ""}`);
  }
  const ledgered = Object.keys(ledger).length;
  console.log(`  ledger: ${ledgered} scenario${ledgered === 1 ? "" : "s"} allowed one retry (flaky.json)`);
  const minutes = ((Date.now() - started) / 60_000).toFixed(1);
  console.log(
    `  wall: ${minutes} min, ${workers} worker${workers === 1 ? "" : "s"}${shard ? `, shard ${shard.index}/${shard.count}` : ""}`,
  );
}

/**
 * Before a ledger scenario's second attempt: its first attempt's page and
 * screenshot renamed <scenario>.first.*, and its checkpoints labelled so,
 * so the second's don't overwrite them or read as the same picture.
 */
function keepFirstAttempt(name, taken) {
  for (const ext of ["txt", "png"]) {
    const file = path.join(out, `${name}.${ext}`);
    if (existsSync(file)) renameSync(file, path.join(out, `${name}.first.${ext}`));
  }
  const index = path.join(out, "checkpoints", "index.json");
  if (!existsSync(index)) return;
  // Those taken since the attempt began (the index lists them in order).
  const list = JSON.parse(readFileSync(index, "utf8")).map((c, i) =>
    i >= taken ? { ...c, label: `${c.label} (first attempt)`, attempt: 1 } : c,
  );
  writeFileSync(index, JSON.stringify(list, null, 2));
}

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

/** A run's timings by step, in ms: a step timed more than once, its median. */
function byStep(steps) {
  const times = new Map();
  for (const s of steps) times.set(s.name, [...(times.get(s.name) ?? []), s.ms]);
  return Object.fromEntries([...times].map(([step, ms]) => [step, median(ms)]));
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length / 2;
  return sorted.length % 2 ? sorted[Math.floor(mid)] : Math.round((sorted[mid - 1] + sorted[mid]) / 2);
}

/**
 * perf.json: what the run was on, and for each scenario its timings across
 * runs (each step's median, least and most, and every run's) and, profiled,
 * where its time went (lib/profile.mjs).
 */
function perfSummary() {
  const quiet = (cmd, args) => {
    try {
      return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim() || null;
    } catch {
      return null;
    }
  };
  const scenarios = {};
  for (const r of results.filter((r) => !r.skipped)) {
    const s = (scenarios[r.name] ??= { runs: 0, failed: 0, seconds: [], timings: {} });
    s.runs += 1;
    if (!r.ok) s.failed += 1;
    s.seconds.push(r.seconds);
    // A failed run's numbers stop wherever it failed: they'd pull the medians.
    if (!r.ok) continue;
    for (const [step, ms] of Object.entries(r.timings)) (s.timings[step] ??= []).push(ms);
    if (r.memory) (s.memory ??= []).push(r.memory);
  }
  for (const [name, s] of Object.entries(scenarios)) {
    s.timings = Object.fromEntries(
      Object.entries(s.timings).map(([step, runs]) => [
        step,
        { median: median(runs), least: Math.min(...runs), most: Math.max(...runs), runs },
      ]),
    );
    // Memory (MiB, Pss) across runs, and by process in the last.
    if (s.memory) {
      const totals = s.memory.map((m) => m.pss);
      s.memory = {
        median: median(totals),
        least: Math.min(...totals),
        most: Math.max(...totals),
        runs: totals,
        processes: s.memory.at(-1).processes.map(({ name, pss, rss }) => ({ name, pss, rss })),
      };
    }
    if (profiles[name]) s.profile = profiles[name];
  }
  return {
    date: new Date().toISOString(),
    commit: process.env.E2E_COMMIT ?? quiet("git", ["-C", here, "describe", "--tags", "--always", "--dirty"]),
    app: positionals[0],
    profiled: !!profileDir,
    repeat,
    on: {
      platform: `${process.platform} ${nodeOs.release()}`,
      cpu: `${nodeOs.cpus()[0]?.model ?? "?"} (${nodeOs.cpus().length} threads)`,
      memory: `${Math.round(nodeOs.totalmem() / 2 ** 30)} GiB`,
      webkit:
        process.platform === "linux"
          ? (quiet("dpkg-query", ["-W", "-f", "${Version}", "libwebkit2gtk-4.1-0"]) ??
            quiet("rpm", ["-q", "--qf", "%{VERSION}", "webkit2gtk4.1"]))
          : null,
      display: process.env.E2E_DISPLAY ?? (process.platform === "linux" ? "x11" : process.platform),
    },
    scenarios,
  };
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
  // What it showed, as text, with the media's, the app's player's and the
  // app's state (lib/dump.mjs): where there's no screenshot (macOS over
  // SSH), it's all there is.
  const shown = await pageDump(driver);
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

/**
 * --parallel: the chosen scenarios dealt between workers (this script
 * again, E2E_WORKER set), each with its own data folders, driver port,
 * D-Bus session, X display and out folder, all at once; then their results,
 * checkpoints, measures and files merged into `out` in scenario order, as
 * one run would have left them. Returns the merged results.
 */
async function runWorkers(chosen) {
  if (process.platform !== "linux")
    throw new Error("--parallel is for Linux: elsewhere the app's data folder can't be moved per worker");
  if (profileDir) throw new Error("--profile with --parallel: profile a run of its own, the computer to itself");
  if (process.env.WAYLAND_DISPLAY && process.env.GDK_BACKEND !== "x11")
    throw new Error("--parallel runs each worker on an X display of its own (Xvfb): not under Wayland");
  if (!process.env.XDG_DATA_HOME)
    throw new Error("--parallel needs XDG_DATA_HOME: each worker's data folder is made beside it");
  const lists = deal(chosen, durations, parallel).filter((l) => l.names.length);
  workers = lists.length;
  const base = Number(process.env.E2E_DRIVER_PORT ?? 4444);
  const workRoot = path.join(out, "workers");
  rmSync(workRoot, { recursive: true, force: true });
  // The tools and model every worker starts with, as setup left them in the configured data folder.
  const seed = dataDirs().at(-1);
  const kept = ["bin", "models", "gpu"].filter((d) => existsSync(path.join(seed, d)));
  if (!kept.includes("models"))
    console.log(`no tools or model in ${seed} to give the workers: each one's first scenario downloads them`);
  const openbox = which("openbox");
  const displays = freeDisplays(lists.length);
  // Each worker in a process group of its own, ended with the run: its
  // app, driver, display and bus with it.
  const children = new Set();
  process.on("exit", () =>
    children.forEach((c) => {
      try {
        process.kill(-c.pid, "SIGTERM");
      } catch {
        // Gone already.
      }
    }),
  );
  for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => process.exit(130));
  console.log(
    `${lists.length} workers: ${lists.map((l, i) => `${i + 1}: ${l.names.length} scenarios, ~${l.seconds} s`).join("; ")}`,
  );
  const runs = lists.map(async (list, at) => {
    const i = at + 1;
    const dataHome = `${process.env.XDG_DATA_HOME}-${i}`;
    const data = path.join(dataHome, path.basename(seed));
    // A fresh copy of the seed each run (hard links: no time or disk), in case it changed.
    for (const d of kept) {
      rmSync(path.join(data, d), { recursive: true, force: true });
      linkTree(path.join(seed, d), path.join(data, d));
    }
    const runtime = mkdtempSync(path.join(nodeOs.tmpdir(), `vodscribe-e2e-runtime-${i}-`));
    const workerOut = path.join(workRoot, String(i));
    mkdirSync(workerOut, { recursive: true });
    const env = {
      ...process.env,
      E2E_WORKER: String(i),
      E2E_OUT: workerOut,
      E2E_DRIVER_PORT: String(base + i),
      XDG_DATA_HOME: dataHome,
      XDG_CONFIG_HOME: process.env.XDG_CONFIG_HOME
        ? `${process.env.XDG_CONFIG_HOME}-${i}`
        : path.join(dataHome, ".config"),
      XDG_CACHE_HOME: process.env.XDG_CACHE_HOME ? `${process.env.XDG_CACHE_HOME}-${i}` : path.join(dataHome, ".cache"),
      XDG_RUNTIME_DIR: runtime,
    };
    delete env.DBUS_SESSION_BUS_ADDRESS;
    const args = [path.join(here, "run.mjs"), positionals[0], "--only", list.names.join(","), "--driver", driverName];
    if (repeat > 1) args.push("--repeat", String(repeat));
    // Its own session bus (the single-instance handoff goes over it) and X
    // display, with openbox as the window manager where it's installed.
    const shell = `${openbox ? "openbox >/dev/null 2>&1 & sleep 1; " : ""}exec "$0" "$@"`;
    const child = spawn(
      "dbus-run-session",
      [
        "--",
        "xvfb-run",
        "-n",
        String(displays[at]),
        "-s",
        "-screen 0 1400x900x24",
        "sh",
        "-c",
        shell,
        process.execPath,
        ...args,
      ],
      { env, stdio: ["ignore", "pipe", "pipe"], detached: true },
    );
    children.add(child);
    prefixed(child.stdout, process.stdout, `[w${i}] `);
    prefixed(child.stderr, process.stderr, `[w${i}] `);
    const code = await new Promise((resolve) => child.on("close", (c, signal) => resolve(signal ?? c)));
    children.delete(child);
    rmSync(runtime, { recursive: true, force: true });
    return { i, list, code, dir: workerOut };
  });
  const done = await Promise.all(runs);

  // Merged in scenario order: by run (--repeat), then as `names` has them.
  const rank = (r) => (r.run ?? 1) * names.length + names.indexOf(r.name);
  const merged = [];
  const pictures = [];
  const perfs = [];
  for (const { i, list, code, dir } of done) {
    const file = path.join(dir, "results.json");
    const theirs = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : [];
    merged.push(...theirs);
    // A worker that ended without results (it crashed, or never started): its scenarios failed.
    for (const name of list.names)
      if (!theirs.some((r) => r.name === name))
        merged.push({ name, ok: false, error: `worker ${i} left no result for it (exit ${code})`, seconds: 0 });
    const index = path.join(dir, "checkpoints", "index.json");
    if (existsSync(index))
      pictures.push(...JSON.parse(readFileSync(index, "utf8")).map((c, at) => ({ ...c, dir, at })));
    if (existsSync(path.join(dir, "perf.json")))
      perfs.push(JSON.parse(readFileSync(path.join(dir, "perf.json"), "utf8")));
    // Everything else they wrote (failures' pages and screenshots, perf/, scenarios' own files) as it is.
    for (const entry of readdirSync(dir))
      if (!["results.json", "perf.json", "checkpoints"].includes(entry))
        cpSync(path.join(dir, entry), path.join(out, entry), { recursive: true, force: true });
  }
  merged.sort((a, b) => rank(a) - rank(b));

  // The checkpoints numbered again, in scenario order (each worker's own order within one).
  pictures.sort((a, b) => names.indexOf(a.scenario) - names.indexOf(b.scenario) || a.at - b.at);
  const pictureDir = path.join(out, "checkpoints");
  mkdirSync(pictureDir, { recursive: true });
  const index = pictures.map(({ dir, at, ...c }, n) => {
    const file = c.file.replace(/^\d+/, String(n + 1).padStart(2, "0"));
    copyFileSync(path.join(dir, "checkpoints", c.file), path.join(pictureDir, file));
    return { ...c, file };
  });
  if (index.length) writeFileSync(path.join(pictureDir, "index.json"), JSON.stringify(index, null, 2));

  // perf.json: what the first worker was on, every worker's scenarios.
  if (perfs.length) {
    const scenarios = Object.assign({}, ...perfs.map((p) => p.scenarios));
    const sorted = Object.fromEntries(
      Object.keys(scenarios)
        .sort((a, b) => names.indexOf(a) - names.indexOf(b))
        .map((k) => [k, scenarios[k]]),
    );
    writeFileSync(
      path.join(out, "perf.json"),
      JSON.stringify({ ...perfs[0], parallel: lists.length, scenarios: sorted }, null, 2),
    );
  }
  rmSync(workRoot, { recursive: true, force: true });
  return merged;
}

/** Copies the folder `from` to `to` as hard links (copies where they can't be: another file system). */
function linkTree(from, to) {
  mkdirSync(to, { recursive: true });
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    const a = path.join(from, entry.name);
    const b = path.join(to, entry.name);
    if (entry.isDirectory()) linkTree(a, b);
    else if (entry.isSymbolicLink()) symlinkSync(readlinkSync(a), b);
    else
      try {
        linkSync(a, b);
      } catch (e) {
        if (!["EXDEV", "EPERM"].includes(e.code)) throw e;
        copyFileSync(a, b);
      }
  }
}

/** `count` X display numbers no server holds (from :90 up), for xvfb-run -n. */
function freeDisplays(count) {
  const free = [];
  for (let n = 90; free.length < count; n++)
    if (!existsSync(`/tmp/.X${n}-lock`) && !existsSync(`/tmp/.X11-unix/X${n}`)) free.push(n);
  return free;
}

/** The path of `cmd` on PATH, or null. */
function which(cmd) {
  try {
    return execFileSync("sh", ["-c", `command -v ${cmd}`], { encoding: "utf8" }).trim() || null;
  } catch {
    return null;
  }
}

/** Copies a worker's output to ours line by line, each line marked as its. */
function prefixed(from, to, mark) {
  let rest = "";
  from.setEncoding("utf8");
  from.on("data", (chunk) => {
    const lines = (rest + chunk).split("\n");
    rest = lines.pop();
    for (const line of lines) to.write(`${mark}${line}\n`);
  });
  from.on("end", () => rest && to.write(`${mark}${rest}\n`));
}
