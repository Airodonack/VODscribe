// A file manager to drop on, for the scenarios that drag a clip out of the
// window (drag-file-time, drag-promise): Dolphin in a KDE session
// (XDG_CURRENT_DESKTOP saying so) or where E2E_FILE_MANAGER=dolphin, else
// Nautilus, opened on a folder of the test's, with a home of its own (never
// the user's), beside the app's window on the test's display. And an app
// that takes finished files only (`openFinishedOnly`), in the same place.
// Linux (X11) only, so far. And what those scenarios share: the clip format
// picked, the length of a file dropped, the app's windows (the system's drag
// shows its picture in one of its own).

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "./app.mjs";
import { ffmpeg } from "./fixtures.mjs";
import * as os from "./os.mjs";

/** The file manager to drop on: E2E_FILE_MANAGER's, else Dolphin in a KDE session, else Nautilus. */
export const fileManager = () =>
  process.env.E2E_FILE_MANAGER ||
  (/(^|:)kde(:|$)/i.test(process.env.XDG_CURRENT_DESKTOP ?? "") ? "dolphin" : "nautilus");

/**
 * A file kept in the folder dropped on, so Nautilus shows it as a folder
 * with files: Nautilus 42 (Ubuntu 22.04's) shows an empty one as "Folder is
 * Empty", which takes no drop (one onto a folder with a file in it lands;
 * Nautilus 48 takes both).
 */
const KEPT = "kept-for-the-drop.txt";

/** The files dropped in folder `dest` so far (not hidden ones, nor the one kept there). */
export function droppedIn(dest) {
  return readdirSync(dest).filter((f) => !f.startsWith(".") && f !== KEPT);
}

/** Empties folder `dest` of what was dropped there, for the next drop. */
export function clearDropped(dest) {
  for (const f of readdirSync(dest)) if (f !== KEPT) rmSync(path.join(dest, f), { recursive: true, force: true });
}

/** Why there's no file manager to drop on here, or null if there is. */
export function noFileManager() {
  const why = os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux only, so far" : null);
  if (why) return why;
  try {
    execFileSync("sh", ["-c", `command -v ${fileManager()}`], { stdio: "ignore" });
  } catch {
    return `no ${fileManager()} to drop on`;
  }
  const major = fileManager() === "nautilus" ? nautilusVersion() : null;
  if (major !== null && major < 43)
    return `Nautilus ${major} asks for the drag's data during the motion; the drag scenarios need 43 or newer`;
  return null;
}

/** Nautilus's major version (`nautilus --version`), read once; null if it doesn't say. */
let nautilusMajor;
function nautilusVersion() {
  if (nautilusMajor === undefined) {
    try {
      const said = execFileSync("nautilus", ["--version"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
      nautilusMajor = Number(said.match(/(\d+)\.\d+/)?.[1] ?? NaN);
      if (Number.isNaN(nautilusMajor)) nautilusMajor = null;
    } catch {
      nautilusMajor = null;
    }
  }
  return nautilusMajor;
}

/**
 * The file manager (`fileManager`) on folder `dest`, its home in `home`,
 * beside the app's window (moved to the screen's left edge if that makes
 * room; skipped, through `ctx`, where the screen has none): where to drop
 * on it, and `close`.
 */
export async function openFileManager(app, ctx, dest, home) {
  mkdirSync(dest, { recursive: true });
  mkdirSync(home, { recursive: true });
  if (fileManager() === "nautilus") writeFileSync(path.join(dest, KEPT), "Kept so the folder isn't empty.\n");
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_DATA_HOME: path.join(home, ".local", "share"),
    XDG_CACHE_HOME: path.join(home, ".cache"),
  };
  return openBeside(app, ctx, [fileManager(), "--new-window", dest], env, `^${path.basename(dest)}`, "ignore");
}

/**
 * `program` run with its `args`, `env` over the test's environment and its
 * output to `stdio`, its window (titled to match `title`) put beside the
 * app's (moved to the screen's left edge if that makes room; skipped,
 * through `ctx`, where the screen has none): where to drop on it, and
 * `close`.
 */
async function openBeside(app, ctx, [program, ...args], env, title, stdio) {
  let page = await os.windowRect(app);
  const [screenW] = execFileSync("xdotool", ["getdisplaygeometry"], { encoding: "utf8" }).trim().split(" ").map(Number);
  const fm = { w: 600, h: 500 };
  // Right of the window, else left of it: the manager's left edge, or null.
  const beside = (p) =>
    p.x + p.w + 20 + fm.w <= screenW ? p.x + p.w + 20 : p.x - 20 - fm.w >= 0 ? p.x - 20 - fm.w : null;
  // A window manager that centres new windows (openbox) leaves neither side
  // wide enough on a screen that has room for both: the app's window goes
  // to the screen's left edge.
  if (beside(page) === null && page.w + 20 + fm.w <= screenW) {
    for (const win of windowsOf(app.driver.proc.pid)) execFileSync("xdotool", ["windowmove", win, "0", "y"]);
    const from = page.x;
    page = await until(
      "the app's window at the screen's left edge",
      async () => {
        const now = await os.windowRect(app);
        return now.x < from && now;
      },
      10_000,
    );
  }
  fm.x = beside(page);
  fm.y = Math.max(0, page.y);
  if (fm.x === null)
    ctx.skip(
      `no room for a file manager beside the window (screen ${screenW} px wide, the window ${page.x} to ${page.x + page.w})`,
    );
  const proc = spawn(program, args, { env: { ...process.env, ...env }, stdio });
  try {
    const win = await until(
      `${program}'s window`,
      () => {
        try {
          return execFileSync("xdotool", ["search", "--onlyvisible", "--name", title], { encoding: "utf8" })
            .trim()
            .split("\n")[0];
        } catch {
          return false;
        }
      },
      MINUTE,
    );
    execFileSync("xdotool", ["windowsize", win, String(fm.w), String(fm.h)]);
    execFileSync("xdotool", ["windowmove", win, String(fm.x), String(fm.y)]);
    await sleep(1000);
    os.activate(app.driver.proc);
  } catch (e) {
    proc.kill();
    throw e;
  }
  return {
    drop: { x: fm.x + Math.round(fm.w / 2), y: fm.y + Math.round(fm.h * 0.6) },
    close: () => proc.kill(),
  };
}

/** The python with GTK 3 that the app that takes finished files only runs in. */
const PYTHON = "/usr/bin/python3";

/** Why there's no app that takes finished files only to drop on here, or null if there is. */
export function noFinishedOnly() {
  const why = os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux only, so far" : null);
  if (why) return why;
  try {
    execFileSync(PYTHON, ["-c", "import gi; gi.require_version('Gtk', '3.0'); from gi.repository import Gtk"], {
      stdio: "ignore",
    });
  } catch {
    return `no GTK 3 for ${PYTHON}`;
  }
  return null;
}

/** An app that takes finished files only, as Qt's apps do (Kdenlive, say): let
 *  go on, it asks for the drop's files, takes those already there, and gives
 *  up after 5 s. Its windows' class is "Clip-taker". */
const FINISHED_ONLY = `
import os, sys, time
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import GLib
GLib.set_prgname("clip-taker")  # its windows' class, set before GTK starts
from gi.repository import Gdk, Gtk

def say(what):
    print(f"{time.time():.3f} {what}", flush=True)

win = Gtk.Window(title="finished-only")
win.add(Gtk.Label(label="Drop here"))
win.drag_dest_set(0, [Gtk.TargetEntry.new("text/uri-list", 0, 0)], Gdk.DragAction.COPY)
waiting = {}

def motion(w, ctx, x, y, t):
    Gdk.drag_status(ctx, Gdk.DragAction.COPY, t)
    return True

def drop(w, ctx, x, y, t):
    say("dropped")
    w.drag_get_data(ctx, Gdk.Atom.intern("text/uri-list", False), t)
    def give_up():
        if waiting.pop(ctx, None) is not None:
            say("gave up")
            Gtk.drag_finish(ctx, False, False, t)
        return False
    waiting[ctx] = GLib.timeout_add(5000, give_up)
    return True

def received(w, ctx, x, y, data, info, t):
    if waiting.get(ctx) is None:
        return
    GLib.source_remove(waiting.pop(ctx))
    files = [GLib.filename_from_uri(u)[0] for u in data.get_uris() or []]
    there = [f for f in files if os.path.isfile(f)]
    say(f"took {len(there)} of {len(files)}: {' '.join(there)}")
    Gtk.drag_finish(ctx, bool(there), False, t)

win.connect("drag-motion", motion)
win.connect("drag-drop", drop)
win.connect("drag-data-received", received)
win.show_all()
Gtk.main()
`;

/**
 * The app that takes finished files only (`FINISHED_ONLY`), its script and
 * what it says in folder `work`, beside the app's window: where to drop on
 * it, what it has said so far (`said`, its lines), and `close`.
 */
export async function openFinishedOnly(app, ctx, work) {
  mkdirSync(work, { recursive: true });
  const script = path.join(work, "finished-only.py");
  writeFileSync(script, FINISHED_ONLY);
  const log = path.join(work, "finished-only.log");
  writeFileSync(log, "");
  const out = openSync(log, "a");
  try {
    const opened = await openBeside(app, ctx, [PYTHON, script], {}, "^finished-only$", ["ignore", out, out]);
    return { ...opened, said: () => readFileSync(log, "utf8").split("\n").filter(Boolean) };
  } finally {
    closeSync(out);
  }
}

/** The X windows of process `pid` that show. */
export function windowsOf(pid) {
  try {
    return execFileSync("xdotool", ["search", "--onlyvisible", "--pid", String(pid)], { encoding: "utf8" })
      .split("\n")
      .filter(Boolean);
  } catch {
    return [];
  }
}

/** The file named `name` somewhere under `dir`, or null. */
export function findIn(dir, name) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const at = path.join(dir, entry.name);
    if (entry.isFile() && entry.name === name) return at;
    if (entry.isDirectory()) {
      const found = findIn(at, name);
      if (found) return found;
    }
  }
  return null;
}

/**
 * A video file's length in seconds, as ffmpeg reads it (the one setup
 * downloaded: the cells have no ffprobe), to the hundredth.
 */
export function probeDuration(file) {
  const said = spawnSync(ffmpeg(), ["-hide_banner", "-i", file], { encoding: "utf8" }).stderr ?? "";
  const [, h, m, s] = said.match(/Duration: (\d+):(\d+):([\d.]+)/) ?? [];
  if (h === undefined) throw new Error(`no length for ${file}: ${said.trim().split("\n").at(-1)}`);
  return Number(h) * 3600 + Number(m) * 60 + Number(s);
}

/** Clips made as `format` ("MP4" or "MOV"), chosen in Settings. */
export async function setFormat(app, format) {
  await app.openSettings("Storage");
  await until("Settings", () => app.js(`return !!document.querySelector('[aria-label="Clip format"]')`), MINUTE);
  await app.js(
    `[...document.querySelectorAll('[role="radiogroup"][aria-label="Clip format"] button')].find((b) => b.textContent.trim() === arguments[0]).click()`,
    format,
  );
  await until(
    `${format} chosen`,
    async () => (await app.invoke("get_settings")).clip_format === format.toLowerCase(),
    10_000,
  );
  await app.key("Escape");
}
