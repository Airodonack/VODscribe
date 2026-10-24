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
import { readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { Script } from "node:vm";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { appPid } from "./memory.mjs";
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

/** The tauri-driver process, shared by every scenario's session. */
let tauriDriver;

export async function webdriver(app) {
  // Another port for runs side by side on one computer: E2E_DRIVER_PORT.
  const port = Number(process.env.E2E_DRIVER_PORT ?? 4444);
  const base = `http://127.0.0.1:${port}`;
  if (!tauriDriver) {
    const native = process.env.NATIVE_DRIVER ? ["--native-driver", process.env.NATIVE_DRIVER] : [];
    // The native driver's port too (tauri-driver's default is 4445 for
    // every run): two runs side by side would otherwise share one
    // WebKitWebDriver, each sending its sessions to the other's.
    native.push("--native-port", String(await freePort()));
    tauriDriver = spawn(process.env.TAURI_DRIVER ?? "tauri-driver", ["--port", String(port), ...native], {
      stdio: ["ignore", "inherit", "inherit"],
    });
    process.on("exit", () => tauriDriver.kill());
    await waitFor("tauri-driver", () => fetch(`${base}/status`).then((r) => r.ok), MINUTE);
  }
  const wd = async (method, url, body) => {
    const res = await fetch(base + url, {
      method,
      headers: { "Content-Type": "application/json" },
      body: body && JSON.stringify(body),
    });
    const json = await res.json();
    if (!res.ok) throw new Error(`${method} ${url}: ${JSON.stringify(json.value ?? json)}`);
    return json.value;
  };
  const session = (
    await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } })
  ).sessionId;
  // The app runs under the native driver, which doesn't say when it ends:
  // found by its binary under this process (Linux), and watched.
  const pid = appPid(app);
  const proc = { exe: app, pid: pid && Number(pid), closedAt: null, exitedAt: null, exit: null };
  if (proc.pid) watchExit(proc);
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
    close: () => {
      proc.closedAt = Date.now();
      return wd("DELETE", `/session/${session}`).catch(() => {});
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
