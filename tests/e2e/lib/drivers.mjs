// The two ways into the app's page, behind one interface: each starts the app
// and returns { run(body, args, waitMs), screenshot(), focus(), resize(width,
// height), close(), proc }, where `run` does what WebDriver's "execute script"
// does (a function body, its `arguments`, the awaited result), `focus` asks
// for the window to be given the keyboard, `resize` asks for the window's page
// to be `width` by `height` CSS pixels (without waiting for it to get there),
// and `proc` is the app's process as far as it's known:
// { exe, pid, closedAt, exitedAt, exit } (times in ms since the epoch; null
// where not known or not yet), for the page dump (lib/dump.mjs).
//
// - webdriver: tauri-driver and the platform's driver (WebKitWebDriver on
//   Linux, msedgedriver on Windows: NATIVE_DRIVER), for the installed app
//   (on Windows, only a build with the `e2e` feature lets it in).
// - bridge: the app's own test hook (src-tauri/src/e2e.rs), in a build made
//   with the `e2e` feature, for macOS, whose webview has no WebDriver, and
//   Windows.

import { spawn, execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { readFileSync, readlinkSync, realpathSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { Script } from "node:vm";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { appPid, appPids, processesUnder } from "./memory.mjs";
import { watchExit } from "./procs.mjs";

const MINUTE = 60_000;

async function waitFor(what, check, timeout) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (await check().catch(() => false)) return;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}

/** A free port on 127.0.0.1. */
function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
    server.on("error", reject);
  });
}

/**
 * Thrown by a driver when the app's page is gone for good: its WebDriver
 * session ended under it, or tauri-driver isn't there. A wait gives up on it
 * at once (lib/app.mjs's `until`) rather than poll a page that can't come
 * back, and the next scenario starts with a new tauri-driver.
 */
export class SessionLost extends Error {}

/**
 * Why a failed WebDriver call means the session is gone, or null when only
 * that call failed: `failure` is the driver's error answer ({ error, message
 * }) or, when none came, the fetch's error. WebKitWebDriver answers "invalid
 * session id" to every command once the app's web process has died (CI,
 * 2026-10-25: WebKit aborted on an assertion in Skia, and a wait polled the
 * dead session for ten minutes); a refused connection is tauri-driver gone.
 */
export function sessionGone(failure) {
  if (failure?.error === "invalid session id")
    return `the WebDriver session is gone (the app's web process ended?)${failure.message ? `: ${failure.message}` : ""}`;
  if (failure?.cause?.code === "ECONNREFUSED") return "tauri-driver doesn't answer: it isn't running";
  return null;
}

/**
 * How long a new session may take: tauri-driver starting the app and the
 * page answering. Without a limit, fetch's own (undici's headersTimeout, 300
 * s) is what ended a start that never came.
 */
const SESSION_START = 2 * MINUTE;

/** The tauri-driver process, shared by every scenario's session. */
let tauriDriver;
/** Why tauri-driver must be started again before the next session, or null. */
let spoiled = null;
/** Whether tauri-driver is ended with this process (once set up). */
let stopOnExit = false;
/** The apps this tauri-driver's sessions started, by pid, with their binary: ended with it. */
const sessionApps = new Map();

/** How long an app may take to exit once its session is closed, before it's killed. */
const EXIT_WAIT = 5000;
/**
 * How long a session's start may go without an app of its own while one of
 * an earlier session's still runs: the new copy hands over to it and quits
 * at once (often before it can be seen), and the session never comes.
 */
const HANDOVER_WAIT = 5000;

/** Whether process `pid` (a number) is still the app of binary `exe`. */
function isApp(pid, exe) {
  try {
    return readlinkSync(`/proc/${pid}/exe`) === realpathSync(exe);
  } catch {
    return false;
  }
}

/**
 * Ends tauri-driver with WebKitWebDriver and any app it started. An app left
 * running takes the next one's start: the app is single-instance, so the new
 * copy hands over to the old and quits, and the session never comes (CI,
 * 2026-10-25: every later scenario on the shard waited 300 s for it).
 */
async function stopTauriDriver() {
  const driver = tauriDriver;
  tauriDriver = null;
  // (Found through /proc: elsewhere, tauri-driver alone.) A tauri-driver that
  // died has none under it any more: its apps were handed to another parent,
  // so those its sessions found are ended by their pids.
  for (const pid of process.platform === "linux" ? processesUnder(driver.pid) : []) {
    try {
      process.kill(Number(pid), "SIGKILL");
    } catch {}
  }
  for (const [pid, exe] of sessionApps) {
    if (!isApp(pid, exe)) continue;
    console.log(`tauri-driver: its app (pid ${pid}) outlived it: killed`);
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
  sessionApps.clear();
  if (driver.exitCode === null && driver.signalCode === null) {
    const ended = new Promise((resolve) => driver.once("exit", resolve));
    driver.kill("SIGKILL");
    await ended;
  }
}

/** Ends tauri-driver, if it runs, with whatever it started (as this process's exit does). */
export async function stopWebDriver() {
  if (tauriDriver) await stopTauriDriver();
}

export async function webdriver(app) {
  // Another port for runs side by side on one computer: E2E_DRIVER_PORT.
  const port = Number(process.env.E2E_DRIVER_PORT ?? 4444);
  const base = `http://127.0.0.1:${port}`;
  if (tauriDriver && (spoiled || tauriDriver.exitCode !== null || tauriDriver.signalCode !== null)) {
    console.log(`tauri-driver: started again (${spoiled ?? "it had ended"})`);
    await stopTauriDriver();
  }
  if (!tauriDriver) {
    spoiled = null;
    const native = process.env.NATIVE_DRIVER ? ["--native-driver", process.env.NATIVE_DRIVER] : [];
    // The native driver's port too (tauri-driver's default is 4445 for
    // every run): two runs side by side would otherwise share one
    // WebKitWebDriver, each sending its sessions to the other's.
    native.push("--native-port", String(await freePort()));
    tauriDriver = spawn(process.env.TAURI_DRIVER ?? "tauri-driver", ["--port", String(port), ...native], {
      stdio: ["ignore", "inherit", "inherit"],
    });
    if (!stopOnExit) {
      stopOnExit = true;
      process.on("exit", () => tauriDriver?.kill());
    }
    await waitFor("tauri-driver", () => fetch(`${base}/status`).then((r) => r.ok), MINUTE);
  }
  // Why this session is gone, once it is: every call after says so at once.
  let lost = null;
  const lose = (why) => {
    lost ??= why;
    spoiled ??= why;
    return new SessionLost(why);
  };
  const wd = async (method, url, body, signal) => {
    if (lost) throw new SessionLost(lost);
    let res;
    try {
      res = await fetch(base + url, {
        method,
        headers: { "Content-Type": "application/json" },
        body: body && JSON.stringify(body),
        signal,
      });
    } catch (e) {
      const why = sessionGone(e);
      if (why) throw lose(`${why} (${method} ${url})`);
      throw new Error(`${method} ${url}: ${e.message}${e.cause ? ` (${e.cause.code ?? e.cause.message})` : ""}`);
    }
    const json = await res.json();
    if (!res.ok) {
      const value = json.value ?? json;
      const said = `${method} ${url}: ${JSON.stringify(value)}`;
      const why = sessionGone(value);
      if (why) throw lose(`${why} (${said})`);
      throw new Error(said);
    }
    return json.value;
  };
  const limit = AbortSignal.timeout(SESSION_START);
  // The start fails at once when its app has gone without a session (handed
  // over to another copy still running, or ended on its own): the driver
  // would wait out SESSION_START for it.
  const early = new AbortController();
  const earlier = new Set(appPids(app));
  const asked = Date.now();
  const seen = new Set();
  const watch = setInterval(() => {
    const now = appPids(app);
    const mine = now.filter((pid) => !earlier.has(pid));
    for (const pid of mine) seen.add(pid);
    const others = now.filter((pid) => earlier.has(pid));
    const after = ((Date.now() - asked) / 1000).toFixed(1);
    const still = others.length
      ? `another instance still running? pid ${others.join(", ")} of this run's is`
      : "no other copy of it is running: see its output above";
    if (seen.size && !mine.length)
      early.abort(
        `the app (pid ${[...seen].join(", ")}) exited ${after} s after its start, before its session started: ${still}`,
      );
    else if (!seen.size && others.length && Date.now() - asked > HANDOVER_WAIT)
      early.abort(`no app of its own ${after} s after its start, before its session started: ${still}`);
  }, 100);
  let session;
  try {
    const caps = { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } };
    session = (await wd("POST", "/session", caps, AbortSignal.any([limit, early.signal]))).sessionId;
  } catch (e) {
    // The driver may be left waiting on that app: a new one for the next.
    spoiled ??= "the last session didn't start";
    if (early.signal.aborted) throw new Error(early.signal.reason);
    if (!limit.aborted) throw e;
    const older = appPid(app);
    const running = older ? ` (an app of this run, pid ${older}, is still running)` : "";
    throw new Error(`no WebDriver session in ${SESSION_START / 1000} s${running}`);
  } finally {
    clearInterval(watch);
  }
  // The app runs under the native driver, which doesn't say when it ends:
  // found by its binary under this process (Linux), and watched.
  const pid = appPids(app).find((pid) => !earlier.has(pid)) ?? appPid(app);
  const proc = { exe: app, pid: pid && Number(pid), closedAt: null, exitedAt: null, exit: null };
  if (proc.pid) {
    watchExit(proc);
    sessionApps.set(proc.pid, app);
  }
  return {
    run: (body, args = []) =>
      wd("POST", `/session/${session}/execute/sync`, { script: `return (async () => { ${body} })()`, args }),
    screenshot: async () => Buffer.from(await wd("GET", `/session/${session}/screenshot`), "base64"),
    // WebDriver's Switch To Window, to the window it's on. WebKitWebDriver
    // doesn't give the window the keyboard with it (openbox, 2026-10-23:
    // the OS does, os.activate); msedgedriver's not tried yet.
    focus: async () => {
      const handle = await wd("GET", `/session/${session}/window`);
      await wd("POST", `/session/${session}/window`, { handle });
    },
    // WebDriver's Set Window Rect, which sizes the whole window: the page is
    // asked for plus what the window has around its page now. (The installed
    // app has no e2e_set_inner_size: that's only in builds with the feature.)
    resize: async (width, height) => {
      const rect = await wd("GET", `/session/${session}/window/rect`);
      const page = await wd("POST", `/session/${session}/execute/sync`, {
        script: "return { width: innerWidth, height: innerHeight }",
        args: [],
      });
      await wd("POST", `/session/${session}/window/rect`, {
        width: width + rect.width - page.width,
        height: height + rect.height - page.height,
      });
    },
    close: async () => {
      proc.closedAt = Date.now();
      await wd("DELETE", `/session/${session}`).catch(() => {});
      if (!proc.pid) return;
      // The driver answers before its app is gone: one still running takes
      // the next session's start (stopTauriDriver), so it's waited for. A lost
      // session can't end its app at all.
      if (!lost) await waitFor("the app to exit", async () => proc.exitedAt !== null, EXIT_WAIT).catch(() => {});
      if (proc.exitedAt === null) {
        if (!lost)
          console.log(`the app (pid ${proc.pid}) still ran ${EXIT_WAIT / 1000} s after its session closed: killed`);
        try {
          process.kill(proc.pid, "SIGKILL");
        } catch {}
        await waitFor("the app killed", async () => proc.exitedAt !== null, EXIT_WAIT).catch(() => {});
      }
      sessionApps.delete(proc.pid);
    },
    proc,
  };
}

export async function bridge(app) {
  const port = await freePort();
  const token = randomBytes(24).toString("hex");
  const base = `http://127.0.0.1:${port}`;
  // A .app is started by its binary, so the test holds the process.
  const exe = app.endsWith(".app") ? path.join(app, "Contents", "MacOS", path.basename(app, ".app")) : app;
  const child = spawn(exe, [], {
    env: { ...process.env, VODSCRIBE_E2E_PORT: String(port), VODSCRIBE_E2E_TOKEN: token },
    stdio: ["ignore", "inherit", "inherit"],
  });
  let exited = null;
  const proc = { exe, pid: child.pid, closedAt: null, exitedAt: null, exit: null };
  child.on("exit", (code, signal) => {
    exited = signal ?? code;
    proc.exitedAt = Date.now();
    proc.exit = signal ? `signal ${signal}` : `code ${code}`;
  });
  const post = (url, body) =>
    fetch(base + url, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: body && JSON.stringify(body),
    });
  await waitFor(
    "the app's test hook",
    async () => {
      if (exited !== null) throw new Error(`the app exited (${exited})`);
      // Any answer, even "no window yet", means it's listening.
      await post("/run", { script: "return 1", wait_ms: 1 });
      return true;
    },
    MINUTE,
  ).catch((e) => {
    throw new Error(exited !== null ? `the app exited (${exited}) before it listened` : e.message);
  });
  const driver = {
    run: async (body, args = [], waitMs = 30_000) => {
      if (exited !== null) throw new Error(`the app exited (${exited})`);
      // A script that doesn't parse never answers from the page: say so here.
      try {
        new Script(`(async function () { ${body} })`);
      } catch (e) {
        throw new Error(`the script doesn't parse (${e.message}): ${body}`);
      }
      const res = await post("/run", { script: body, args, wait_ms: waitMs });
      const text = await res.text();
      if (!res.ok) throw new Error(`the test hook: ${res.status} ${text}`);
      const answer = JSON.parse(text);
      if (!answer.ok) throw new Error(answer.value);
      return answer.value;
    },
    screenshot: async () => screen(),
    // Nothing of its own: the hook runs scripts in the page, and the OS
    // brings the app to the front (App.focused).
    focus: async () => {},
    // The e2e build's own command, which sizes the page itself.
    resize: (width, height) =>
      driver.run(
        `return window.__TAURI_INTERNALS__.invoke("e2e_set_inner_size", arguments[0]).catch((e) => {
          throw new Error(String(e));
        })`,
        [{ width, height }],
      ),
    close: async () => {
      proc.closedAt = Date.now();
      await post("/quit").catch(() => {});
      await waitFor("the app to quit", async () => exited !== null, 20_000).catch(() => child.kill("SIGKILL"));
    },
    proc,
  };
  return driver;
}

/** The whole screen, where the platform can take it without a driver. */
function screen() {
  if (process.platform !== "darwin") return null;
  const file = path.join(tmpdir(), `vodscribe-e2e-${process.pid}.png`);
  execFileSync("screencapture", ["-x", file]);
  const png = readFileSync(file);
  rmSync(file, { force: true });
  return png;
}
