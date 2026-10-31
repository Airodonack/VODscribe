import assert from "node:assert/strict";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, before, describe, test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

import { sessionGone, stopWebDriver, webdriver } from "./drivers.mjs";

test("a session WebKitWebDriver no longer has is gone", () => {
  // As CI's run got it, every poll after the app's web process died.
  assert.match(sessionGone({ error: "invalid session id", message: "", stacktrace: "" }), /session is gone/);
  assert.match(
    sessionGone({ error: "invalid session id", message: "session deleted because of page crash or hang." }),
    /session is gone.*: session deleted because of page crash or hang\.$/,
  );
});

test("tauri-driver not listening is a session gone", () => {
  const refused = new TypeError("fetch failed", {
    cause: Object.assign(new Error("connect"), { code: "ECONNREFUSED" }),
  });
  assert.match(sessionGone(refused), /tauri-driver/);
});

test("a command that failed in a live session is only that command", () => {
  // A page promise rejected with a string, the first answer once the web
  // process had died, and a script's own error: the session may live on.
  assert.equal(sessionGone({ error: "unknown error", message: "Could not parse script result" }), null);
  assert.equal(sessionGone({ error: "javascript error", message: "ReferenceError: x is not defined" }), null);
  assert.equal(sessionGone({ error: "no such element", message: "" }), null);
  // fetch's own limit, or a connection dropped mid-answer: not known to be gone.
  const timedOut = new TypeError("fetch failed", {
    cause: Object.assign(new Error("headers"), { code: "UND_ERR_HEADERS_TIMEOUT" }),
  });
  assert.equal(sessionGone(timedOut), null);
  assert.equal(sessionGone(undefined), null);
});

// The session's start and end against a stand-in tauri-driver (Linux: the
// app is found through /proc). The stand-in app is single-instance as the
// real one is: a second copy, while the first holds its lock (a port), hands
// over and exits (at once, or after FAKE_LINGER_FILE's milliseconds), and the stand-in driver then never answers, as
// WebKitWebDriver didn't (CI run 40231703153). Ended, the app lingers a
// moment, as the real one may after WebKitWebDriver's DELETE returns.
const FAKE_DRIVER = `
import { spawn } from "node:child_process";
import { createServer } from "node:http";
const port = Number(process.argv[process.argv.indexOf("--port") + 1]);
const apps = new Map();
let next = 1;
const send = (res, code, value) => { res.writeHead(code, { "Content-Type": "application/json" }); res.end(JSON.stringify({ value })); };
createServer(async (req, res) => {
  let body = "";
  for await (const chunk of req) body += chunk;
  if (req.url === "/status") return send(res, 200, { ready: true });
  if (req.method === "POST" && req.url === "/session") {
    const exe = JSON.parse(body).capabilities.alwaysMatch["tauri:options"].application;
    const script = exe.endsWith("python") ? process.env.FAKE_APP_THREADS_SCRIPT : process.env.FAKE_APP_SCRIPT;
    const app = spawn(exe, [script], { stdio: ["ignore", "pipe", "inherit"] });
    // An app that exits before its page answers leaves the request waiting.
    app.stdout.once("data", () => { const id = String(next++); apps.set(id, app); send(res, 200, { sessionId: id }); });
    return;
  }
  const m = req.url.match(/^\\/session\\/([^/]+)$/);
  if (req.method === "DELETE" && m) { apps.get(m[1])?.kill("SIGTERM"); apps.delete(m[1]); return send(res, 200, null); }
  send(res, 404, { error: "unknown command", message: req.url });
}).listen(port, "127.0.0.1");
`;
const FAKE_APP = `
import { readFileSync } from "node:fs";
import { createServer } from "node:net";
// How long it runs before handing over (the real one starts GTK first): the test's to set.
const linger = Number(readFileSync(process.env.FAKE_LINGER_FILE, "utf8"));
const lock = createServer();
lock.on("error", () => setTimeout(() => process.exit(0), linger));
lock.listen(Number(process.env.FAKE_LOCK), "127.0.0.1", () => process.stdout.write("ready\\n"));
process.on("SIGTERM", () => setTimeout(() => process.exit(0), 1500));
setInterval(() => {}, 1000);
`;

// A stand-in app whose quit ends its first thread before the others, as a
// process's exit can: its first thread then reads as a zombie while another
// still runs and holds its lock (python's, for a thread of its own to end).
const FAKE_APP_THREADS = `
import ctypes, os, signal, socket, sys, threading, time
lock = socket.socket()
lock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
lock.bind(("127.0.0.1", int(os.environ["FAKE_LOCK"])))
lock.listen()
print("ready", flush=True)
def last():
    time.sleep(1.5)
    os._exit(0)
def quit(*_):
    threading.Thread(target=last).start()
    # The first thread ends alone (the exit system call, not exit_group).
    ctypes.CDLL(None).syscall(60, 0)
signal.signal(signal.SIGTERM, quit)
while True:
    time.sleep(1)
`;

/** Whether process `pid` runs (a zombie doesn't). */
function running(pid) {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    return stat.slice(stat.lastIndexOf(")") + 2)[0] !== "Z";
  } catch {
    return false;
  }
}

const freePort = () =>
  new Promise((resolve) => {
    const s = createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

describe("sessions against a stand-in tauri-driver", { skip: process.platform !== "linux" }, () => {
  /** @type {string} */ let dir;
  /** @type {string} */ let app;
  before(async () => {
    dir = mkdtempSync(path.join(tmpdir(), "vodscribe-drivers-test-"));
    writeFileSync(path.join(dir, "driver.mjs"), FAKE_DRIVER);
    writeFileSync(path.join(dir, "app.mjs"), FAKE_APP);
    writeFileSync(path.join(dir, "linger"), "0");
    writeFileSync(path.join(dir, "app.py"), FAKE_APP_THREADS);
    // The app is found by its binary: a copy of node's of its own.
    app = path.join(dir, "vodscribe");
    copyFileSync(process.execPath, app);
    chmodSync(app, 0o755);
    const driver = path.join(dir, "tauri-driver");
    writeFileSync(driver, `#!/bin/sh\nexec "${process.execPath}" "${path.join(dir, "driver.mjs")}" "$@"\n`);
    chmodSync(driver, 0o755);
    Object.assign(process.env, {
      TAURI_DRIVER: driver,
      FAKE_APP_SCRIPT: path.join(dir, "app.mjs"),
      FAKE_APP_THREADS_SCRIPT: path.join(dir, "app.py"),
      FAKE_LINGER_FILE: path.join(dir, "linger"),
      FAKE_LOCK: String(await freePort()),
      E2E_DRIVER_PORT: String(await freePort()),
    });
  });
  after(async () => {
    await stopWebDriver();
    rmSync(dir, { recursive: true, force: true });
  });

  test("a session closed has its app ended before the next starts", async () => {
    const one = await webdriver(app);
    const pid = one.proc.pid;
    assert.ok(pid && running(pid), "the app is found");
    await one.close();
    assert.equal(running(pid), false, "the app still runs after close()");
  });

  test("a session closed has its app ended, every thread of it, before the next starts", async (t) => {
    // Found by its binary as the app is: python's, under a name of its own.
    const python = ["/usr/bin/python3", "/usr/local/bin/python3"].find((p) => existsSync(p));
    if (!python) return t.skip("no python3");
    const threaded = path.join(dir, "python");
    symlinkSync(python, threaded);
    try {
      const one = await webdriver(threaded);
      await one.close();
      // Its lock free: a copy started now doesn't find it held and hand over.
      const free = await new Promise((resolve) => {
        const probe = createServer();
        probe.once("error", () => resolve(false));
        probe.listen(Number(process.env.FAKE_LOCK), "127.0.0.1", () => probe.close(() => resolve(true)));
      });
      assert.ok(free, "close() returned with a thread of the app still holding its lock");
    } finally {
      rmSync(threaded, { force: true });
    }
  });

  /** A session's start that fails: what it said, and how long it took. */
  async function failedStart() {
    const started = Date.now();
    const err = await webdriver(app).then(
      () => new Error("the session started"),
      (e) => e,
    );
    const took = Date.now() - started;
    console.log(`  said: ${err.message} (${took} ms)`);
    assert.ok(took < 15_000, `it took ${took} ms to say so`);
    return err.message;
  }

  for (const linger of [0, 300])
    test(`an app that hands over to one still running (after ${linger} ms) fails its start at once, saying so`, async () => {
      writeFileSync(path.join(dir, "linger"), String(linger));
      const one = await webdriver(app);
      const said = await failedStart();
      await one.close();
      assert.match(said, /before its session started: another instance still running\? pid \d+ of this run's is$/);
      // Gone at once, it's never seen; running a moment, it is.
      if (linger) assert.match(said, /^the app \(pid \d+\) exited [\d.]+ s after its start/);
    });

  test("an app that exits on its own, before its session, fails its start at once", async () => {
    // The app's lock held by something else: it quits, with no other copy of it running.
    writeFileSync(path.join(dir, "linger"), "300");
    const holder = createServer().listen(Number(process.env.FAKE_LOCK), "127.0.0.1");
    await new Promise((ok) => holder.once("listening", ok));
    const said = await failedStart();
    holder.close();
    assert.match(said, /^the app \(pid \d+\) exited .* no other copy of it is running: see its output above$/);
  });

  test("a tauri-driver that died has its app ended with it", async () => {
    const one = await webdriver(app);
    const pid = one.proc.pid;
    // tauri-driver dies: its app, reparented, runs on.
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    process.kill(Number(stat.slice(stat.lastIndexOf(")") + 2).split(" ")[1]), "SIGKILL");
    await sleep(300);
    assert.ok(running(pid), "the app outlives its driver");
    const two = await webdriver(app);
    assert.equal(running(pid), false, "the dead driver's app still runs");
    await two.close();
  });
});
