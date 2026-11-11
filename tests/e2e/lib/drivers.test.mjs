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

import { bridge, inspectorRefusal, sessionGone, stopWebDriver, webdriver } from "./drivers.mjs";

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

test("WebKitGTK's warning that the inspector server didn't start is read for its app and port", () => {
  // As CI's run printed it (43070303802).
  const line =
    "** (vodscribe:23000): WARNING **: 17:58:32.295: Failed to start remote inspector server on 127.0.0.1:41295: Error binding to address 127.0.0.1:41295: Address already in use";
  assert.deepEqual(inspectorRefusal(line), {
    pid: 23000,
    address: "127.0.0.1:41295",
    why: "Error binding to address 127.0.0.1:41295: Address already in use",
  });
  assert.equal(inspectorRefusal("(vodscribe:23000): dbind-WARNING **: 17:58:32.195: AT-SPI: Error retrieving"), null);
});

// The session's start and end against a stand-in tauri-driver (Linux: the
// app is found through /proc). The stand-in app is single-instance as the
// real one is: a second copy, while the first holds its lock (a port), hands
// over and exits (at once, or after FAKE_LINGER_FILE's milliseconds), and the stand-in driver then never answers, as
// WebKitWebDriver didn't (CI run 40231703153). Ended, the app lingers a
// moment, as the real one may after WebKitWebDriver's DELETE returns.
const FAKE_DRIVER = `
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
const port = Number(process.argv[process.argv.indexOf("--port") + 1]);
// The app's inspector port, as WebKitWebDriver picks it: bound to port 0 and
// closed; or the next of FAKE_INSPECTOR_FILE's ports, the test's to hold.
const inspectorPort = () => {
  const queued = readFileSync(process.env.FAKE_INSPECTOR_FILE, "utf8").split(" ").filter(Boolean);
  if (queued.length) {
    writeFileSync(process.env.FAKE_INSPECTOR_FILE, queued.slice(1).join(" "));
    return Promise.resolve(Number(queued[0]));
  }
  return new Promise((resolve) => {
    const s = createNetServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
};
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
    const env = { ...process.env, WEBKIT_INSPECTOR_SERVER: "127.0.0.1:" + (await inspectorPort()) };
    const app = spawn(exe, [script], { env, stdio: ["ignore", "pipe", "inherit"] });
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
const start = () => lock.listen(Number(process.env.FAKE_LOCK), "127.0.0.1", () => process.stdout.write("ready\\n"));
// WebKit's inspector server, where the driver said: its port taken, the app
// runs on with none, as WebKitGTK's does, saying so as GLib does.
const inspector = process.env.WEBKIT_INSPECTOR_SERVER;
const [host, at] = inspector.split(":");
const server = createServer();
server.on("error", () =>
  process.stderr.write(
    "** (vodscribe:" + process.pid + "): WARNING **: 17:58:32.295: Failed to start remote inspector server on " +
      inspector + ": Error binding to address " + inspector + ": Address already in use\\n",
  ),
);
server.listen(Number(at), host, start);
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
    writeFileSync(path.join(dir, "inspector"), "");
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
      FAKE_INSPECTOR_FILE: path.join(dir, "inspector"),
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

  /** A port held here, as something else held the one WebKitWebDriver chose; handed to the next `times` apps. */
  async function holdInspectorPort(times) {
    const held = createServer().listen(0, "127.0.0.1");
    await new Promise((ok) => held.once("listening", ok));
    const { port } = held.address();
    writeFileSync(path.join(dir, "inspector"), Array(times).fill(port).join(" "));
    return { port, close: () => held.close() };
  }

  test("an app whose WebDriver port was taken before it listened gets a session on another", async (t) => {
    // WebKitWebDriver chose the port and let it go, and something bound it
    // before the app's WebKit could (CI run 43070303802, upgrade-0.5.1:
    // "Failed to start remote inspector server on 127.0.0.1:41295 ...
    // Address already in use", then no session in 120 s).
    const held = await holdInspectorPort(1);
    const logged = t.mock.method(console, "log");
    try {
      const started = Date.now();
      const one = await webdriver(app);
      const took = Date.now() - started;
      assert.ok(took < 15_000, `it took ${took} ms`);
      await one.close();
    } finally {
      held.close();
    }
    const lines = logged.mock.calls.map((c) => c.arguments.join(" "));
    const said = new RegExp(
      `^tauri-driver: the app \\(pid \\d+\\) couldn't listen for WebDriver on 127\\.0\\.0\\.1:${held.port} ` +
        `\\(Error binding to address .*: Address already in use; the port has: pid ${process.pid} \\([^)]*\\), LISTEN\\): the session once more$`,
    );
    assert.ok(
      lines.some((l) => said.test(l)),
      `said:\n${lines.join("\n")}`,
    );
  });

  test("an app whose WebDriver port is taken twice running fails its start at once, saying so", async () => {
    const held = await holdInspectorPort(2);
    try {
      const said = await failedStart();
      assert.match(
        said,
        /^the app \(pid \d+\) couldn't listen for WebDriver on 127\.0\.0\.1:\d+ \(.*; the port has: pid \d+/,
      );
    } finally {
      held.close();
    }
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

test("an app that never listens on its test hook is stopped, not left running", async () => {
  // A stand-in that starts and never listens, as the Mac's app did once when
  // the host was thrashing (round072b): left up, it took every later start.
  const dir = mkdtempSync(path.join(tmpdir(), "vodscribe-bridge-"));
  const app = path.join(dir, "app");
  const pidFile = path.join(dir, "pid");
  writeFileSync(app, `#!/bin/sh\necho $$ > '${pidFile}'\nexec sleep 600\n`);
  chmodSync(app, 0o755);
  try {
    await assert.rejects(bridge(app, { listenMs: 1500 }), /the app's test hook/);
    const pid = Number(readFileSync(pidFile, "utf8"));
    assert.throws(() => process.kill(pid, 0), { code: "ESRCH" });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
