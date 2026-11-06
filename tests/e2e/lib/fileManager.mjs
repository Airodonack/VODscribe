// A file manager to drop on, for the scenarios that drag a clip out of the
// window (drag-file-time, drag-promise): Dolphin in a KDE session
// (XDG_CURRENT_DESKTOP saying so) or where E2E_FILE_MANAGER=dolphin, else
// Nautilus, opened on a folder of the test's, with a home of its own (never
// the user's), beside the app's window on the test's display. And an app
// that takes finished files only (`openFinishedOnly`), in the same place.
// On macOS, Finder (in icon view, so a drop lands in the folder shown, not
// in the one selected in column view), and the finished-files-only app is
// one built with swiftc, as is one that gives a file as a promise only
// (`openPromiseGiver`, for drop-virtual). Linux (X11) and macOS, so far. And what those
// scenarios share: the clip format
// picked, the length of a file dropped, the app's windows (the system's drag
// shows its picture in one of its own).

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "./app.mjs";
import { ffmpeg } from "./fixtures.mjs";
import * as os from "./os.mjs";

/** The file manager to drop on: Finder on macOS; E2E_FILE_MANAGER's, else Dolphin in a KDE session, else Nautilus. */
export const fileManager = () =>
  (process.platform === "darwin" ? "finder" : null) ||
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
  if (process.platform === "darwin") return null;
  const why =
    os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux and macOS only, so far" : null);
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
  if (process.platform === "darwin") return openFinder(app, ctx, dest);
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
 * through `ctx`, where the screen has none): where to drop on it, its
 * window (`win`, X11's id), and `close`.
 */
export async function openBeside(app, ctx, [program, ...args], env, title, stdio) {
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
  let win;
  try {
    win = await until(
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
    win,
    close: () => proc.kill(),
  };
}

/** The python with GTK 3 that the app that takes finished files only runs in. */
const PYTHON = "/usr/bin/python3";

/** Why there's no app that takes finished files only to drop on here, or null if there is. */
export function noFinishedOnly() {
  if (process.platform === "darwin") {
    try {
      execFileSync("sh", ["-c", "command -v swiftc"], { stdio: "ignore" });
      return null;
    } catch {
      return "no swiftc to build the app that takes finished files only";
    }
  }
  const why =
    os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux and macOS only, so far" : null);
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
 * it, what it has said so far (`said`, its lines), and `close`. On macOS,
 * with `refuses`, it refuses every drag instead.
 */
export async function openFinishedOnly(app, ctx, work, { refuses = false } = {}) {
  mkdirSync(work, { recursive: true });
  if (process.platform === "darwin") return openFinishedOnlyMac(app, ctx, work, refuses);
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

/** The X windows of process `pid` that show (macOS: its windows on screen, by number). */
export function windowsOf(pid) {
  if (process.platform === "darwin")
    return jxa(
      `ObjC.import("CoreGraphics"); JSON.stringify(ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(1 | 16, 0))).filter((w) => w.kCGWindowOwnerPID === ${Number(pid)}).map((w) => String(w.kCGWindowNumber)))`,
    );
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

/** The codec each choice of Settings' clip format makes (crates/core `ClipCodec`). */
const CODECS = { "MP4 H.264": "h264", "MP4 H.265": "h265", "MOV DNxHR": "dnxhr", "MOV ProRes": "prores" };

/**
 * Clips made as `format`, chosen in Settings: a choice as it reads there
 * ("MOV ProRes"), or "MP4" or "MOV" alone for that file's first codec,
 * H.264 or DNxHR.
 */
export async function setFormat(app, format) {
  const label = Object.keys(CODECS).find((l) => l === format || l.startsWith(`${format} `));
  if (!label) throw new Error(`no clip format ${format}`);
  await choose(app, "Clip format", label, (s) => s.clip_codec === CODECS[label]);
}

/** Clips made at `quality` ("Proxy", "Standard" or "High"), chosen in Settings. */
export async function setQuality(app, quality) {
  await choose(app, "Clip quality", quality, (s) => s.clip_quality === quality.toLowerCase());
}

/** Picks `option` in Settings' radio group `group` (Storage), once `saved(settings)` says it's saved. */
async function choose(app, group, option, saved) {
  await app.openSettings("Storage");
  const buttons = `[role="radiogroup"][aria-label="${group}"] button`;
  await until(group, () => app.js(`return !!document.querySelector(arguments[0])`, buttons), MINUTE);
  await app.js(
    `const b = [...document.querySelectorAll(arguments[0])].find((b) => b.textContent.trim() === arguments[1]);
     if (!b) throw new Error("no " + arguments[1]);
     b.click();`,
    buttons,
    option,
  );
  await until(`${option} chosen`, async () => saved(await app.invoke("get_settings")), 10_000);
  await app.key("Escape");
}

/**
 * A video file's picture as ffmpeg reads it (the cells have no ffprobe):
 * its codec, profile, tag and pixel format, as in "hevc (Main) (hvc1 /
 * 0x31637668), yuv420p(tv, …)".
 */
export function probeVideo(file) {
  const said = spawnSync(ffmpeg(), ["-hide_banner", "-i", file], { encoding: "utf8" }).stderr ?? "";
  const [, codec, profile, tag, pixels] =
    said.match(/Video: (\w+) \(([^)]+)\) \((\w+) \/ 0x[0-9a-fA-F]+\), (\w+)/) ?? [];
  if (!codec) throw new Error(`no picture in ${file}: ${said.trim().split("\n").at(-1)}`);
  return { codec, profile, tag, pixels };
}

/** Runs JXA `script`, its last value read as JSON. */
function jxa(script) {
  return JSON.parse(execFileSync("osascript", ["-l", "JavaScript", "-e", script], { encoding: "utf8" }) || "null");
}

/**
 * JXA helpers: the windows on screen, front first (CoreGraphics' list: an
 * AppleEvent to Finder from an SSH session times out), the screen's width,
 * and the left button and keys as CoreGraphics events.
 */
const MAC_WINDOWS = `ObjC.import("CoreGraphics"); ObjC.import("unistd");
const windows = () => ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(1 | 16, 0))).filter((w) => w.kCGWindowLayer === 0);
const front = (owner) => windows().find((w) => w.kCGWindowOwnerName === owner && w.kCGWindowBounds.Height > 100);
const mouse = (type, x, y) => { const e = $.CGEventCreateMouseEvent(null, type, $.CGPointMake(x, y), 0); $.CGEventSetFlags(e, 0); $.CGEventPost(0, e); };
const drag = (from, to) => { mouse(5, ...from); $.usleep(100000); mouse(1, ...from); $.usleep(80000);
  for (let i = 1; i <= 10; i++) { mouse(6, from[0] + ((to[0] - from[0]) * i) / 10, from[1] + ((to[1] - from[1]) * i) / 10); $.usleep(30000); }
  mouse(2, ...to); $.usleep(300000); };
const command = (code, more = 0) => { for (const down of [true, false]) { const e = $.CGEventCreateKeyboardEvent(null, code, down); $.CGEventSetFlags(e, 0x100000 | more); $.CGEventPost(0, e); $.usleep(30000); }
  // The modifiers let go of: else the system keeps them for the next events (a click became a Control-click).
  for (const key of [55, 59]) { const e = $.CGEventCreateKeyboardEvent(null, key, false); $.CGEventSetFlags(e, 0); $.CGEventPost(0, e); } $.usleep(30000); };
const screenWidth = () => $.CGDisplayPixelsWide($.CGMainDisplayID());`;

/** Where a window to drop on goes beside the app's ({x, y, w, h}): right of it, else left; skipped, through `ctx`, where there's no room. */
async function besideMac(app, ctx) {
  const page = await os.windowRect(app);
  const screenW = jxa(`${MAC_WINDOWS} screenWidth()`);
  // As wide as there's room for, up to 600 (the runner's screen may be 1280 wide).
  const room = Math.max(screenW - (page.x + page.w) - 40, page.x - 40);
  const fm = { w: Math.min(600, room), h: 460, y: Math.max(60, page.y) };
  fm.x = fm.w < 400 ? null : page.x + page.w + 20 + fm.w <= screenW ? page.x + page.w + 20 : page.x - 20 - fm.w;
  if (fm.x === null)
    ctx.skip(
      `no room for a window beside the app's (screen ${screenW} px wide, the window ${page.x} to ${page.x + page.w})`,
    );
  return fm;
}

/**
 * A window of app `owner`, the frontmost, sized and moved to `fm` by its
 * corner and its title bar: first to the screen's left, where its corner is
 * on the screen to drag.
 */
function placeMac(owner, fm) {
  const at = JSON.stringify(owner);
  return jxa(`${MAC_WINDOWS}
    let b = front(${at}).kCGWindowBounds;
    drag([b.X + b.Width / 2, b.Y + 12], [40 + b.Width / 2, ${fm.y} + 12]);
    b = front(${at}).kCGWindowBounds;
    drag([b.X + b.Width - 3, b.Y + b.Height - 3], [b.X + ${fm.w} - 3, b.Y + ${fm.h} - 3]);
    b = front(${at}).kCGWindowBounds;
    drag([b.X + b.Width / 2, b.Y + 12], [${fm.x} + b.Width / 2, ${fm.y} + 12]);
    JSON.stringify(front(${at}).kCGWindowBounds)`);
}

/** On macOS, the app in front before a press that must reach it (`os.toFront`); elsewhere nothing. */
export async function frontmost(app) {
  if (process.platform === "darwin") await os.toFront(app);
}

/**
 * The top-left corner of the first icon (by place: top row, then left) in
 * the Finder window at `at` (its CoreGraphics bounds), read from Finder's
 * accessibility tree, not from a layout that differs between versions (the
 * sidebar is about 40 px narrower on macOS 12, and may be on others).
 */
function finderIcon(at) {
  return jxa(`ObjC.import("ApplicationServices"); ${MAC_WINDOWS}
    const attr = (el, name) => { const r = Ref(); return $.AXUIElementCopyAttributeValue(el, $(name), r) === 0 ? ObjC.castRefToObject(r[0]) : null; };
    const text = (el, name) => attr(el, name)?.description.js ?? "";
    const kids = (el) => attr(el, "AXChildren")?.js ?? [];
    const point = (el) => { const m = /x:(-?[\\d.]+) y:(-?[\\d.]+)/.exec(text(el, "AXPosition")); return m && { x: +m[1], y: +m[2] }; };
    const app = $.AXUIElementCreateApplication(front("Finder").kCGWindowOwnerPID);
    const win = kids(app).find((w) => { const p = point(w); return p && Math.abs(p.x - ${at.X}) < 2 && Math.abs(p.y - ${at.Y}) < 2; });
    const icons = [];
    const walk = (el, depth) => { if (depth > 8) return; for (const k of kids(el)) {
      const role = text(k, "AXRole");
      if (role === "AXImage") { const p = point(k); if (p) icons.push(p); }
      else if (role !== "AXToolbar" && role !== "AXOutline") walk(k, depth + 1); } };
    if (win) walk(win, 0);
    icons.sort((a, b) => a.y - b.y || a.x - b.x);
    JSON.stringify(icons[0] ?? null)`);
}

/** Finder on folder `dest`, in icon view, beside the app's window: where to drop on it, and `close`. */
async function openFinder(app, ctx, dest) {
  const fm = await besideMac(app, ctx);
  const had = jxa(
    `${MAC_WINDOWS} JSON.stringify(windows().filter((w) => w.kCGWindowOwnerName === "Finder").map((w) => w.kCGWindowNumber))`,
  );
  execFileSync("open", [dest]);
  await until(
    "Finder's window",
    () =>
      jxa(
        `${MAC_WINDOWS} const w = front("Finder"); JSON.stringify(!!w && !${JSON.stringify(had)}.includes(w.kCGWindowNumber))`,
      ),
    MINUTE,
  );
  await sleep(500);
  const at = placeMac("Finder", fm);
  // Icon view (Cmd+1): in column view a drop goes into the folder selected;
  // arranged by name (Ctrl+Cmd+1), so a file shows in the first place, not
  // where it was let go.
  jxa(`${MAC_WINDOWS} command(18); $.usleep(300000); command(18, 0x40000); "true"`);
  await sleep(500);
  await os.toFront(app);
  return {
    // (Right of its sidebar.)
    drop: { x: Math.round(at.X + at.Width * 0.65), y: Math.round(at.Y + at.Height * 0.6) },
    window: at,
    // The cancel button of the file in the first place (arranged by name):
    // at its icon's top-left corner, a little in, where the document's page begins.
    cancelAt: () => {
      const icon = finderIcon(at);
      if (!icon) throw new Error(`no icon in Finder's window at ${at.X},${at.Y}`);
      return { x: Math.round(icon.x + 3), y: Math.round(icon.y + 10) };
    },
    // Its title bar clicked (Finder in front), then Cmd+W.
    close: () => {
      jxa(`${MAC_WINDOWS} const b = front("Finder").kCGWindowBounds; mouse(5, b.X + b.Width / 2, b.Y + 12); $.usleep(100000);
        mouse(1, b.X + b.Width / 2, b.Y + 12); mouse(2, b.X + b.Width / 2, b.Y + 12); $.usleep(300000); command(13); "true"`);
      return os.toFront(app);
    },
  };
}

/** The app that takes finished files only, on macOS: a window that takes file addresses only, as most apps do, and says what it got.
 *  With REFUSE set it refuses every drag, as VLC does one with no address yet, and says so. */
const FINISHED_ONLY_MAC = `
import AppKit
func say(_ s: String) { print(String(format: "%.3f ", Date().timeIntervalSince1970) + s); fflush(stdout) }
final class Drop: NSView {
    override init(frame: NSRect) { super.init(frame: frame); registerForDraggedTypes([.fileURL]) }
    required init?(coder: NSCoder) { fatalError() }
    override func draw(_ r: NSRect) { NSColor.systemPurple.setFill(); r.fill() }
    let refuses = ProcessInfo.processInfo.environment["REFUSE"] != nil
    override func draggingEntered(_ s: NSDraggingInfo) -> NSDragOperation {
        if refuses { say("refused") }
        return refuses ? [] : .copy
    }
    override func performDragOperation(_ s: NSDraggingInfo) -> Bool {
        say("dropped")
        let urls = s.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        let there = urls.filter { FileManager.default.fileExists(atPath: $0.path) }
        say("took \\(there.count) of \\(urls.count): \\(there.map { $0.path }.joined(separator: " "))")
        return true
    }
}
let pos = (ProcessInfo.processInfo.environment["POS"] ?? "700 100 600 500").split(separator: " ").map { Double($0)! }
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let screen = NSScreen.main!.frame
let win = NSWindow(contentRect: NSRect(x: pos[0], y: screen.height - pos[1] - pos[3], width: pos[2], height: pos[3]), styleMask: [.titled], backing: .buffered, defer: false)
win.title = "finished-only"
win.contentView = Drop(frame: NSRect(x: 0, y: 0, width: pos[2], height: pos[3]))
win.makeKeyAndOrderFront(nil)
say("ready")
app.run()
`;

/** The name the app that takes finished files only goes by on macOS (its program's). */
export const FINISHED_ONLY_MAC_NAME = "Clip Taker";

async function openFinishedOnlyMac(app, ctx, work, refuses) {
  const fm = await besideMac(app, ctx);
  const source = path.join(work, "finished-only.swift");
  writeFileSync(source, FINISHED_ONLY_MAC);
  const program = path.join(work, FINISHED_ONLY_MAC_NAME);
  execFileSync("swiftc", ["-O", source, "-o", program], { stdio: "ignore" });
  const log = path.join(work, "finished-only.log");
  writeFileSync(log, "");
  const out = openSync(log, "a");
  const proc = spawn(program, [], {
    env: { ...process.env, POS: `${fm.x} ${fm.y} ${fm.w} ${fm.h}`, ...(refuses ? { REFUSE: "1" } : {}) },
    stdio: ["ignore", out, out],
  });
  closeSync(out);
  const said = () => readFileSync(log, "utf8").split("\n").filter(Boolean);
  try {
    await until("its window", () => said().some((l) => l.endsWith(" ready")), MINUTE);
    await sleep(1000);
    await os.toFront(app);
  } catch (e) {
    proc.kill();
    throw e;
  }
  return {
    drop: { x: fm.x + Math.round(fm.w / 2), y: fm.y + Math.round(fm.h * 0.6) },
    said,
    close: () => proc.kill(),
  };
}

/** An app that gives a file as a promise only, on macOS: a window that, pressed and dragged from, drags file FILE as an
 *  NSFilePromiseProvider named NAME, written (copied) where the receiver asks once it's let go, after DELAY seconds. */
const PROMISE_GIVER_MAC = `
import AppKit
func say(_ s: String) { print(String(format: "%.3f ", Date().timeIntervalSince1970) + s); fflush(stdout) }
let env = ProcessInfo.processInfo.environment
let file = URL(fileURLWithPath: env["FILE"]!)
let name = env["NAME"] ?? file.lastPathComponent
let delay = Double(env["DELAY"] ?? "0")!
final class Giver: NSView, NSFilePromiseProviderDelegate, NSDraggingSource {
    let queue = OperationQueue()
    var dragging = false
    override func draw(_ r: NSRect) { NSColor.systemTeal.setFill(); r.fill() }
    override func acceptsFirstMouse(for e: NSEvent?) -> Bool { true }
    override func mouseDown(with e: NSEvent) { dragging = false }
    override func mouseDragged(with e: NSEvent) {
        if dragging { return }
        dragging = true
        let p = convert(e.locationInWindow, from: nil)
        let item = NSDraggingItem(pasteboardWriter: NSFilePromiseProvider(fileType: "public.mpeg-4", delegate: self))
        item.setDraggingFrame(NSRect(x: p.x - 32, y: p.y - 32, width: 64, height: 64), contents: NSWorkspace.shared.icon(forFile: file.path))
        say("drag begins")
        beginDraggingSession(with: [item], event: e, source: self)
    }
    func draggingSession(_ s: NSDraggingSession, sourceOperationMaskFor c: NSDraggingContext) -> NSDragOperation { .copy }
    func draggingSession(_ s: NSDraggingSession, endedAt p: NSPoint, operation: NSDragOperation) { say("ended with operation \\(operation.rawValue)") }
    func filePromiseProvider(_ p: NSFilePromiseProvider, fileNameForType t: String) -> String { say("named \\(name)"); return name }
    func filePromiseProvider(_ p: NSFilePromiseProvider, writePromiseTo url: URL, completionHandler: @escaping (Error?) -> Void) {
        say("asked for \\(url.path)")
        Thread.sleep(forTimeInterval: delay)
        do { try FileManager.default.copyItem(at: file, to: url); say("written"); completionHandler(nil) }
        catch { say("failed: \\(error)"); completionHandler(error) }
    }
    func operationQueue(for p: NSFilePromiseProvider) -> OperationQueue { queue }
}
let pos = (env["POS"] ?? "700 100 400 300").split(separator: " ").map { Double($0)! }
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let screen = NSScreen.main!.frame
let win = NSWindow(contentRect: NSRect(x: pos[0], y: screen.height - pos[1] - pos[3], width: pos[2], height: pos[3]), styleMask: [.titled], backing: .buffered, defer: false)
win.title = "promise-giver"
win.contentView = Giver(frame: NSRect(x: 0, y: 0, width: pos[2], height: pos[3]))
win.makeKeyAndOrderFront(nil)
say("ready")
app.run()
`;

/**
 * On macOS, the app that gives `file` as a file promise only, named `name`
 * (`PROMISE_GIVER_MAC`), built and run in folder `work`, beside the app's
 * window: where to press to drag from it (`from`), what it has said so far
 * (`said`, its lines), and `close`. It writes the file `delay` seconds after
 * it's asked for.
 */
export async function openPromiseGiver(app, ctx, work, file, name, { delay = 1 } = {}) {
  if (process.platform !== "darwin") throw new Error("the promise giver is macOS's");
  mkdirSync(work, { recursive: true });
  const fm = await besideMac(app, ctx);
  const source = path.join(work, "promise-giver.swift");
  writeFileSync(source, PROMISE_GIVER_MAC);
  const program = path.join(work, "Promise Giver");
  execFileSync("swiftc", ["-O", source, "-o", program], { stdio: "ignore" });
  const log = path.join(work, "promise-giver.log");
  writeFileSync(log, "");
  const out = openSync(log, "a");
  const proc = spawn(program, [], {
    env: { ...process.env, FILE: file, NAME: name, DELAY: String(delay), POS: `${fm.x} ${fm.y} ${fm.w} ${fm.h}` },
    stdio: ["ignore", out, out],
  });
  closeSync(out);
  const said = () => readFileSync(log, "utf8").split("\n").filter(Boolean);
  try {
    await until("its window", () => said().some((l) => l.endsWith(" ready")), MINUTE);
    await sleep(1000);
  } catch (e) {
    proc.kill();
    throw e;
  }
  return {
    from: { x: fm.x + Math.round(fm.w / 2), y: fm.y + Math.round(fm.h * 0.6) },
    said,
    close: () => proc.kill(),
  };
}
