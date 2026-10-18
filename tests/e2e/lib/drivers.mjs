// The two ways into the app's page, behind one interface: each starts the app
// and returns { run(body, args, waitMs), screenshot(), close() }, where `run`
// does what WebDriver's "execute script" does (a function body, its
// `arguments`, the awaited result).
//
// - webdriver: tauri-driver and the platform's driver (WebKitWebDriver on
//   Linux, msedgedriver on Windows: NATIVE_DRIVER), for the installed app.
// - bridge: the app's own test hook (src-tauri/src/e2e.rs), in a build made
//   with the `e2e` feature, for macOS, whose webview has no WebDriver.

import { spawn, execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { Script } from "node:vm";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

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
  const port = 4444;
  const base = `http://127.0.0.1:${port}`;
  if (!tauriDriver) {
    const native = process.env.NATIVE_DRIVER ? ["--native-driver", process.env.NATIVE_DRIVER] : [];
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
  return {
    run: (body, args = []) =>
      wd("POST", `/session/${session}/execute/sync`, { script: `return (async () => { ${body} })()`, args }),
    screenshot: async () => Buffer.from(await wd("GET", `/session/${session}/screenshot`), "base64"),
    close: () => wd("DELETE", `/session/${session}`).catch(() => {}),
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
  child.on("exit", (code, signal) => (exited = signal ?? code));
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
  return {
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
    close: async () => {
      await post("/quit").catch(() => {});
      await waitFor("the app to quit", async () => exited !== null, 20_000).catch(() => child.kill("SIGKILL"));
    },
  };
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
