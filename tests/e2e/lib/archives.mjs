// Apps that drag files with no address yet onto the window, on Linux (X11).
// An archive manager open on a zip, its file dragged out of its list: File
// Roller before 43 (later ones can't drag out), Engrampa or Ark, the first
// installed, or E2E_ARCHIVE_MANAGER's. Where there's none, a stand-in that
// saves its file the same way (XdndDirectSave), as a GTK 3 app run by the
// system's python. And a stand-in for a sandboxed app, sending a file
// through the document portal, where the portal can run. Each opens beside
// the app's window (fileManager.mjs).

import { execFileSync } from "node:child_process";
import { closeSync, openSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import path from "node:path";
import { crc32 } from "node:zlib";

import { openBeside } from "./fileManager.mjs";
import * as os from "./os.mjs";

/** The python with GTK 3 that the stand-ins run in. */
const PYTHON = "/usr/bin/python3";

/** The document portal's own program, as Debian, Ubuntu, Fedora and Arch install it. */
const DOCUMENT_PORTAL = ["/usr/libexec/xdg-document-portal", "/usr/lib/xdg-document-portal"];

/** Where window `win` (X11's id) shows its inside on the screen. */
function placeOf(win) {
  const shell = execFileSync("xdotool", ["getwindowgeometry", "--shell", win], { encoding: "utf8" });
  const said = Object.fromEntries(
    shell
      .trim()
      .split("\n")
      .map((l) => l.split("=")),
  );
  return { x: Number(said.X), y: Number(said.Y), w: Number(said.WIDTH), h: Number(said.HEIGHT) };
}

/** `program --version`'s first number, or null. */
function major(program) {
  try {
    const said = execFileSync(program, ["--version"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    const n = Number(said.match(/(\d+)\.\d+/)?.[1] ?? NaN);
    return Number.isNaN(n) ? null : n;
  } catch {
    return null;
  }
}

const installed = (program) => {
  try {
    execFileSync("sh", ["-c", `command -v ${program}`], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
};

/** The archive manager to drag out of, or null: File Roller before 43, Engrampa, Ark. */
export function archiveManager() {
  if (process.env.E2E_ARCHIVE_MANAGER) return process.env.E2E_ARCHIVE_MANAGER;
  if (installed("file-roller") && (major("file-roller") ?? 99) < 43) return "file-roller";
  return ["engrampa", "ark"].find(installed) ?? null;
}

/** Why there's no python with GTK 3 for a stand-in here, or null. */
export function noStandIn() {
  try {
    execFileSync(PYTHON, ["-c", "import gi; gi.require_version('Gtk', '3.0'); from gi.repository import Gtk"], {
      stdio: "ignore",
    });
    return null;
  } catch {
    return `no GTK 3 for ${PYTHON}`;
  }
}

/** Why a file can't be sent through the document portal here, or null. */
export function noPortal() {
  if (!DOCUMENT_PORTAL.some((p) => existsSync(p))) return "no document portal (xdg-desktop-portal)";
  if (!existsSync("/dev/fuse")) return "no /dev/fuse for the document portal";
  return noStandIn();
}

/** A zip at `to` holding each of `files` ({name, file}) as it is (stored). */
export function zipStored(to, files) {
  const local = [];
  const central = [];
  let at = 0;
  for (const { name, file } of files) {
    const data = readFileSync(file);
    const named = Buffer.from(name, "utf8");
    const crc = crc32(data);
    // Version 2.0, names in UTF-8 (bit 11), stored, 1980-01-01 00:00.
    const head = Buffer.alloc(30);
    head.writeUInt32LE(0x04034b50, 0);
    head.writeUInt16LE(20, 4);
    head.writeUInt16LE(0x0800, 6);
    head.writeUInt16LE(0, 8);
    head.writeUInt16LE(0, 10);
    head.writeUInt16LE(0x21, 12);
    head.writeUInt32LE(crc, 14);
    head.writeUInt32LE(data.length, 18);
    head.writeUInt32LE(data.length, 22);
    head.writeUInt16LE(named.length, 26);
    head.writeUInt16LE(0, 28);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(0x0314, 4); // made on Unix
    entry.writeUInt16LE(20, 6);
    entry.writeUInt16LE(0x0800, 8);
    entry.writeUInt16LE(0, 10);
    entry.writeUInt16LE(0, 12);
    entry.writeUInt16LE(0x21, 14);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(data.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(named.length, 28);
    entry.writeUInt32LE((0o100644 << 16) >>> 0, 38);
    entry.writeUInt32LE(at, 42);
    local.push(head, named, data);
    central.push(entry, named);
    at += head.length + named.length + data.length;
  }
  const dir = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(dir.length, 12);
  end.writeUInt32LE(at, 16);
  writeFileSync(to, Buffer.concat([...local, dir, end]));
}

/**
 * Archive manager `program` open on `zip`, beside the app's window, with
 * everything in it picked (Ctrl+A): where to press to drag its file (the
 * picked row, found on the screen), and `close`.
 */
export async function openArchive(app, ctx, program, zip, home) {
  const env = {
    HOME: home,
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_DATA_HOME: path.join(home, ".local", "share"),
    XDG_CACHE_HOME: path.join(home, ".cache"),
  };
  const opened = await openBeside(app, ctx, [program, zip], env, path.basename(zip), "ignore");
  try {
    // In front, then Ctrl+A as a real key (GTK ignores keys sent to a window).
    execFileSync("xdotool", ["windowactivate", "--sync", opened.win]);
    await new Promise((r) => setTimeout(r, 300));
    execFileSync("xdotool", ["key", "ctrl+a"]);
    await new Promise((r) => setTimeout(r, 700));
    const shot = os.screenshot();
    const row = pickedRow(shot, placeOf(opened.win));
    if (!row) {
      writeFileSync(path.join(ctx.out, `${program}-no-row.png`), shot);
      throw new Error(`${program} shows no picked row (Ctrl+A) to drag (the screen: ${program}-no-row.png)`);
    }
    os.activate(app.driver.proc);
    return { ...opened, from: row };
  } catch (e) {
    opened.close();
    throw e;
  }
}

/**
 * Where the picked row is in window `rect` on screenshot `png`: the middle of the longest band
 * of a selection's blue down one of two columns of the screen, or null.
 */
function pickedRow(png, rect) {
  let best = null;
  for (const along of [0.15, 0.3]) {
    const x = Math.round(rect.x + rect.w * along);
    const column = os.pixels(png, { x, y: rect.y, w: 1, h: rect.h }, 1, rect.h);
    const blue = (y) => {
      const [r, b] = [column[y * 3], column[y * 3 + 2]];
      return b > 140 && b - r > 30;
    };
    // Bands of the blue, a few pixels of text in them forgiven.
    let start = null;
    let gap = 0;
    for (let y = 0; y <= rect.h; y++) {
      if (y < rect.h && blue(y)) {
        start ??= y;
        gap = 0;
      } else if (start !== null && (y === rect.h || ++gap > 4)) {
        const end = y - gap;
        if (end - start >= 10 && end - start <= 80 && (!best || end - start > best.h))
          best = { x, y: rect.y + Math.round((start + end) / 2), h: end - start };
        start = null;
        gap = 0;
      }
    }
  }
  return best && { x: best.x, y: best.y };
}

/** A GTK 3 app with one button, `file`'s name on it, that drags it out as `kind` offers it ("xds", "portal"). */
const STAND_IN = `
import ctypes, os, sys, time
from urllib.parse import unquote, urlparse
import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, Gio, GLib, Gtk

kind, path = sys.argv[1], sys.argv[2]
name = os.path.basename(path)

def say(what):
    print(f"{time.time():.3f} {what}", flush=True)

# GDK's window properties aren't in its introspection data: through ctypes.
gdk = ctypes.CDLL("libgdk-3.so.0")
gdk.gdk_atom_intern.restype = ctypes.c_void_p
gdk.gdk_atom_intern.argtypes = [ctypes.c_char_p, ctypes.c_int]
gdk.gdk_property_change.argtypes = [ctypes.c_void_p] * 3 + [ctypes.c_int, ctypes.c_int, ctypes.c_char_p, ctypes.c_int]
gdk.gdk_property_get.restype = ctypes.c_int
gdk.gdk_property_get.argtypes = [ctypes.c_void_p] * 3 + [ctypes.c_ulong, ctypes.c_ulong, ctypes.c_int] + [
    ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_void_p)]
ctypes.pythonapi.PyCapsule_GetPointer.restype = ctypes.c_void_p
ctypes.pythonapi.PyCapsule_GetPointer.argtypes = [ctypes.py_object, ctypes.c_char_p]
XDS = gdk.gdk_atom_intern(b"XdndDirectSave0", 0)
TEXT = gdk.gdk_atom_intern(b"text/plain", 0)

def window_of(ctx):
    return ctypes.pythonapi.PyCapsule_GetPointer(ctx.get_source_window().__gpointer__, None)

win = Gtk.Window(title=f"stand-in-{kind}")
button = Gtk.Button(label=name)
win.add(button)
target = {"xds": "XdndDirectSave0", "portal": "application/vnd.portal.filetransfer"}[kind]
button.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [Gtk.TargetEntry.new(target, 0, 0)], Gdk.DragAction.COPY)

def begin(w, ctx):
    say("drag begins")
    if kind == "xds":
        gdk.gdk_property_change(window_of(ctx), XDS, TEXT, 8, 0, name.encode(), len(name.encode()))

def get(w, ctx, data, info, t):
    say(f"asked for {data.get_target().name()}")
    if kind == "xds":
        kind_, fmt, length, at = ctypes.c_void_p(), ctypes.c_int(), ctypes.c_int(), ctypes.c_void_p()
        ok = gdk.gdk_property_get(window_of(ctx), XDS, TEXT, 0, 4096, 0, ctypes.byref(kind_), ctypes.byref(fmt), ctypes.byref(length), ctypes.byref(at))
        uri = ctypes.string_at(at, length.value).decode() if ok else ""
        say(f"saving at {uri}")
        try:
            with open(path, "rb") as f, open(unquote(urlparse(uri).path), "wb") as out:
                out.write(f.read())
            data.set(data.get_target(), 8, b"S")
        except OSError as e:
            say(f"couldn't: {e}")
            data.set(data.get_target(), 8, b"E")
        return
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    def call(method, args, fds=None):
        return bus.call_with_unix_fd_list_sync(
            "org.freedesktop.portal.Documents", "/org/freedesktop/portal/documents",
            "org.freedesktop.portal.FileTransfer", method, args, None, Gio.DBusCallFlags.NONE, -1, fds, None)
    options = {"writable": GLib.Variant("b", False), "autostop": GLib.Variant("b", True)}
    key = call("StartTransfer", GLib.Variant("(a{sv})", (options,)))[0].unpack()[0]
    fds = Gio.UnixFDList()
    fds.append(os.open(path, os.O_PATH | os.O_CLOEXEC))
    call("AddFiles", GLib.Variant("(saha{sv})", (key, [0], {})), fds)
    say(f"sent as transfer {key}")
    data.set(data.get_target(), 8, key.encode())

button.connect("drag-begin", begin)
button.connect("drag-data-get", get)
button.connect("drag-end", lambda w, ctx: say("drag ends"))
win.connect("destroy", Gtk.main_quit)
win.show_all()
Gtk.main()
`;

/**
 * The stand-in that drags `file` out as `kind` offers it ("xds", "portal"),
 * its script and what it says in folder `work`, beside the app's window:
 * where to press to drag (its button), what it has said (`said`), `close`.
 */
export async function openStandIn(app, ctx, kind, file, work) {
  const script = path.join(work, "stand-in.py");
  writeFileSync(script, STAND_IN);
  const log = path.join(work, `stand-in-${kind}.log`);
  writeFileSync(log, "");
  const out = openSync(log, "a");
  try {
    const opened = await openBeside(app, ctx, [PYTHON, script, kind, file], {}, `^stand-in-${kind}$`, [
      "ignore",
      out,
      out,
    ]);
    const { x, y, w, h } = placeOf(opened.win);
    return {
      ...opened,
      from: { x: x + Math.round(w / 2), y: y + Math.round(h / 2) },
      said: () => readFileSync(log, "utf8").split("\n").filter(Boolean),
    };
  } finally {
    closeSync(out);
  }
}
