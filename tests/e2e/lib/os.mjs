// The OS-level layer: what the page can't see or do. A screenshot of the
// whole screen (the app's own player draws outside the page), colours read
// from it, and real clicks from the OS (trusted, hit-tested, focusing).
// Each works where the test runs on the app's own computer and session:
//
// - Linux (X11, as under Xvfb): ffmpeg's x11grab and xdotool. Under a
//   headless Wayland compositor the screen only (E2E_DISPLAY: "weston", its
//   screenshooter; "gnome", GNOME Shell's Screenshot over D-Bus, which its
//   test extension allows): no clicks, and the window's place on the screen
//   found from a mark the page shows a moment (Wayland doesn't tell it).
// - Windows (run in the signed-in session: runners.mjs): PowerShell, with
//   System.Drawing for the screen and user32 for the mouse.
// - macOS: screencapture, and CoreGraphics events for clicks (the process
//   running the tests needs the Screen Recording and Accessibility permissions).
//
// Screen positions come from the page: an element's box, scaled by the
// window's scale factor, from the window's position (Tauri's read-only
// window calls, allowed to every window).

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { dataDirs } from "./data.mjs";

function ffmpeg() {
  const exe = process.platform === "win32" ? "ffmpeg.exe" : "ffmpeg";
  const downloaded = path.join(dataDirs().at(-1), "bin", exe);
  return existsSync(downloaded) ? downloaded : exe;
}

const temp = (name) => path.join(tmpdir(), `vodscribe-e2e-${process.pid}-${name}`);

const WINDOWS_DPI = `Add-Type 'using System; using System.Runtime.InteropServices; public class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'; [Dpi]::SetProcessDPIAware() | Out-Null;`;

function powershell(script, env = {}) {
  return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
}

/** Why the OS layer can't run here, or null if it can. */
export function unavailable() {
  // Under Wayland the app's window is Wayland's: x11grab and xdotool would
  // see and click Xwayland's screen instead (DISPLAY is set there too).
  if (process.platform === "linux" && process.env.WAYLAND_DISPLAY && process.env.GDK_BACKEND !== "x11")
    return "a Wayland session (this layer drives X11 only, so far)";
  if (process.platform === "linux" && !process.env.DISPLAY) return "no X display (DISPLAY)";
  if (process.platform === "linux") {
    try {
      execFileSync("xdotool", ["version"], { stdio: "ignore" });
    } catch {
      return "no xdotool";
    }
  }
  return null;
}

/** Under a Wayland compositor, which (E2E_DISPLAY), else null. */
function wayland() {
  return process.platform === "linux" && process.env.WAYLAND_DISPLAY && process.env.GDK_BACKEND !== "x11"
    ? (process.env.E2E_DISPLAY ?? "")
    : null;
}

/** Why the screen can't be seen here (no screenshot), or null if it can. */
export function cannotSee() {
  const compositor = wayland();
  if (compositor === null)
    return process.platform === "linux" && !process.env.DISPLAY ? "no X display (DISPLAY)" : null;
  return ["weston", "gnome"].includes(compositor) ? null : "a Wayland compositor the tests can't see into";
}

/** A PNG of the whole screen, in its pixels. */
export function screenshot() {
  const file = temp("screen.png");
  try {
    if (wayland() === "gnome") {
      execFileSync("gdbus", [
        ...["call", "--session", "--dest", "org.gnome.Shell.Screenshot"],
        ...["--object-path", "/org/gnome/Shell/Screenshot", "--method", "org.gnome.Shell.Screenshot.Screenshot"],
        ...["false", "false", file],
      ]);
    } else if (wayland() === "weston") {
      // It writes wayland-screenshot-<time>.png where it's run.
      const dir = temp("weston");
      mkdirSync(dir, { recursive: true });
      execFileSync("weston-screenshooter", [], { cwd: dir });
      const made = readdirSync(dir).find((f) => f.endsWith(".png"));
      if (!made) throw new Error("weston-screenshooter made no screenshot (is weston run with --debug?)");
      renameSync(path.join(dir, made), file);
      rmSync(dir, { recursive: true, force: true });
    } else if (process.platform === "linux") {
      execFileSync(ffmpeg(), [
        "-loglevel",
        "error",
        "-y",
        "-f",
        "x11grab",
        "-i",
        process.env.DISPLAY,
        "-frames:v",
        "1",
        file,
      ]);
    } else if (process.platform === "win32") {
      powershell(
        `${WINDOWS_DPI} Add-Type -AssemblyName System.Drawing, System.Windows.Forms;
         $b = [System.Windows.Forms.SystemInformation]::VirtualScreen;
         $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height;
         [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size);
         $bmp.Save($env:E2E_SHOT, [System.Drawing.Imaging.ImageFormat]::Png)`,
        { E2E_SHOT: file },
      );
    } else {
      execFileSync("screencapture", ["-x", file]);
    }
    return readFileSync(file);
  } finally {
    rmSync(file, { force: true });
  }
}

/** The average colour [r, g, b] of `rect` ({x, y, w, h}, screen pixels) in the PNG `png`. */
export function colourIn(png, rect) {
  const file = temp("sample.png");
  try {
    requireInts(rect);
    writeFileSync(file, png);
    const raw = execFileSync(
      ffmpeg(),
      [
        "-loglevel",
        "error",
        "-i",
        file,
        "-vf",
        `crop=${rect.w}:${rect.h}:${rect.x}:${rect.y},scale=1:1:flags=area`,
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
      ],
      { maxBuffer: 1 << 20 },
    );
    return [raw[0], raw[1], raw[2]];
  } finally {
    rmSync(file, { force: true });
  }
}

/**
 * Brings the app to the front, as a person's first click would: on macOS a
 * click on an app that isn't in front only brings it there. On X11, given
 * the app's process (a driver's `proc`), its windows are activated, which
 * gives the page the keyboard back (App.focused; the page's window.focus()
 * and WebDriver's Switch To Window don't, under openbox, 2026-10-23).
 */
export function activate(proc = null) {
  if (process.platform === "linux") {
    if (wayland() !== null || !proc?.pid) return;
    const windows = execFileSync("xdotool", ["search", "--pid", String(proc.pid)], { encoding: "utf8" });
    // Each window of its process (there's more than the one shown); one that can't be activated is passed over.
    for (const id of windows.split("\n").filter(Boolean)) {
      try {
        execFileSync("xdotool", ["windowactivate", id], { stdio: "ignore" });
      } catch {}
    }
    return;
  }
  if (process.platform !== "darwin") return;
  execFileSync("osascript", [
    "-l",
    "JavaScript",
    "-e",
    `ObjC.import("AppKit");
     const apps = $.NSRunningApplication.runningApplicationsWithBundleIdentifier("app.vodscribe");
     if (apps.count === 0) throw new Error("VODscribe isn't running");
     apps.objectAtIndex(0).activateWithOptions(1);`,
  ]);
}

/** Clicks at screen pixel (x, y) with the OS's mouse. */
export function click(x, y, scale = 1) {
  x = Math.round(x);
  y = Math.round(y);
  if (process.platform === "linux") {
    execFileSync("xdotool", ["mousemove", String(x), String(y), "click", "1"]);
  } else if (process.platform === "win32") {
    powershell(
      `${WINDOWS_DPI} Add-Type 'using System; using System.Runtime.InteropServices; public class Mouse { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e); }';
       [Mouse]::SetCursorPos(${x}, ${y}) | Out-Null; Start-Sleep -Milliseconds 50;
       [Mouse]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); [Mouse]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)`,
    );
  } else {
    // Straight to CoreGraphics, in points, not pixels: System Events doesn't
    // answer a script run over SSH (its AppleEvent times out).
    const [px, py] = [Math.round(x / scale), Math.round(y / scale)];
    execFileSync("osascript", [
      "-l",
      "JavaScript",
      "-e",
      `ObjC.import("CoreGraphics"); const p = $.CGPointMake(${px}, ${py});
       for (const t of [1, 2]) $.CGEventPost(0, $.CGEventCreateMouseEvent(null, t, p, 0));`,
    ]);
  }
}

/**
 * The OS's mouse and keys, a step at a time, for drags: `steps` like
 * ["down", x, y], ["move", x, y], ["key", "Escape"], ["up"], ["wait", ms];
 * ["down", x, y, 2] and ["up", 2] for the middle button (X11's numbering:
 * 1 left, 2 middle, 3 right); ["wheel", "down", n] turns the wheel n notches
 * ("up", "left", "right" too) where the pointer is; ["click", n] clicks n
 * times where the pointer is (2: a double-click);
 * ["hold", "alt"] and ["release", "alt"] for a key held through a drag;
 * ["letgo", x, y] moves and lets go at once.
 * Linux (X11) only, so far.
 */
export async function gesture(steps) {
  if (process.platform !== "linux") throw new Error("gestures are Linux-only, so far");
  const WHEEL = { up: "4", down: "5", left: "6", right: "7" };
  for (const [what, a, b, c] of steps) {
    if (what === "down") execFileSync("xdotool", ["mousemove", String(a), String(b), "mousedown", String(c ?? 1)]);
    else if (what === "move") execFileSync("xdotool", ["mousemove", String(a), String(b)]);
    else if (what === "up") execFileSync("xdotool", ["mouseup", String(a ?? 1)]);
    else if (what === "click") execFileSync("xdotool", ["click", "--repeat", String(a ?? 1), "--delay", "80", "1"]);
    else if (what === "wheel")
      execFileSync("xdotool", ["click", "--repeat", String(b ?? 1), "--delay", "30", WHEEL[a]]);
    else if (what === "key") execFileSync("xdotool", ["key", a]);
    else if (what === "hold") execFileSync("xdotool", ["keydown", a]);
    else if (what === "release") execFileSync("xdotool", ["keyup", a]);
    else if (what === "letgo") execFileSync("xdotool", ["mousemove", String(a), String(b), "mouseup", "1"]);
    else if (what === "wait") await new Promise((r) => setTimeout(r, a));
    else throw new Error(`no gesture step ${what}`);
  }
}

/**
 * The screen rectangle (pixels) of the element `selector` in the app's page,
 * and the window's scale factor.
 */
export async function rectOf(app, selector) {
  if (wayland() !== null) {
    const origin = await windowOrigin(app);
    const box = await app.js(
      `const el = document.querySelector(arguments[0]);
       if (!el) throw new Error("no " + arguments[0]);
       const r = el.getBoundingClientRect();
       return { x: r.left, y: r.top, w: r.width, h: r.height };`,
      selector,
    );
    return {
      x: Math.round(origin.x + box.x),
      y: Math.round(origin.y + box.y),
      w: Math.round(box.w),
      h: Math.round(box.h),
      scale: 1,
    };
  }
  const r = await app.js(
    `const el = document.querySelector(arguments[0]);
     if (!el) throw new Error("no " + arguments[0]);
     const box = el.getBoundingClientRect();
     const call = window.__TAURI_INTERNALS__.invoke;
     const at = await call("plugin:window|inner_position", { label: "main" });
     const size = await call("plugin:window|inner_size", { label: "main" });
     const scale = await call("plugin:window|scale_factor", { label: "main" });
     // On macOS the "inner" box still has the title bar on top: the page
     // starts below it. Elsewhere it's the page's own (Windows' would count twice).
     const mac = navigator.platform.startsWith("Mac");
     const top = mac ? Math.max(0, size.height - innerHeight * scale) : 0;
     const left = mac ? Math.max(0, (size.width - innerWidth * scale) / 2) : 0;
     return { x: at.x + left + box.left * scale, y: at.y + top + box.top * scale, w: box.width * scale, h: box.height * scale, scale };`,
    selector,
  );
  return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h), scale: r.scale };
}

/** The middle `fraction` of `rect`, for sampling away from edges. */
export function middle(rect, fraction = 0.2) {
  const w = Math.max(2, Math.round(rect.w * fraction));
  const h = Math.max(2, Math.round(rect.h * fraction));
  return { x: Math.round(rect.x + (rect.w - w) / 2), y: Math.round(rect.y + (rect.h - h) / 2), w, h };
}

/** How far apart two colours are (0 to 441). */
export function distance(a, b) {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

function requireInts(rect) {
  for (const k of ["x", "y", "w", "h"])
    if (!Number.isInteger(rect[k]) || rect[k] < 0) throw new Error(`a bad rectangle: ${JSON.stringify(rect)}`);
}

/** The app window's page area on the screen (pixels), as rectOf gives an element's. */
export async function windowRect(app) {
  return rectOf(app, "body");
}

/** A PNG of `rect` (screen pixels) cut from the PNG `png`. */
export function crop(png, rect) {
  requireInts(rect);
  const file = temp("crop-in.png");
  try {
    writeFileSync(file, png);
    return execFileSync(
      ffmpeg(),
      [
        "-loglevel",
        "error",
        "-i",
        file,
        "-vf",
        `crop=${rect.w}:${rect.h}:${rect.x}:${rect.y}`,
        "-frames:v",
        "1",
        "-f",
        "image2pipe",
        "-c:v",
        "png",
        "-",
      ],
      { maxBuffer: 1 << 28 },
    );
  } finally {
    rmSync(file, { force: true });
  }
}

/** `rect` of the PNG `png` scaled to w x h pixels: their RGB bytes, for comparing regions. */
export function pixels(png, rect, w = 64, h = 16) {
  requireInts(rect);
  const file = temp("pixels.png");
  try {
    writeFileSync(file, png);
    return execFileSync(
      ffmpeg(),
      [
        "-loglevel",
        "error",
        "-i",
        file,
        "-vf",
        `crop=${rect.w}:${rect.h}:${rect.x}:${rect.y},scale=${w}:${h}:flags=area`,
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-",
      ],
      { maxBuffer: 1 << 26 },
    );
  } finally {
    rmSync(file, { force: true });
  }
}

/** How different two pixels() buffers are: the mean difference of their bytes (0 to 255). */
export function difference(a, b) {
  let sum = 0;
  for (let i = 0; i < a.length; i++) sum += Math.abs(a[i] - b[i]);
  return sum / a.length;
}

/** A PNG's size in pixels, from its header. */
export function sizeOf(png) {
  return { w: png.readUInt32BE(16), h: png.readUInt32BE(20) };
}

/** Where the page's top-left corner is on the screen, under Wayland (which
 *  doesn't tell a window where it is): the page shows a magenta mark there a
 *  moment, and the screenshot says where it is. Found once (the window
 *  doesn't move in a test's compositor); at a scale of 1. */
let origin = null;
async function windowOrigin(app) {
  if (origin) return origin;
  await app.js(
    `const mark = document.createElement("div");
     mark.id = "e2e-mark";
     mark.style.cssText = "position: fixed; left: 0; top: 0; width: 8px; height: 8px; background: #ff00ff; z-index: 2147483647";
     document.body.append(mark);
     await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
     await new Promise((r) => setTimeout(r, 300));`,
  );
  try {
    const png = screenshot();
    const { w, h } = sizeOf(png);
    const rgb = pixels(png, { x: 0, y: 0, w, h }, w, h);
    const magenta = (i) => rgb[i] > 240 && rgb[i + 1] < 16 && rgb[i + 2] > 240;
    for (let y = 0; y + 7 < h && !origin; y++)
      for (let x = 0; x + 7 < w; x++) {
        const i = (y * w + x) * 3;
        // The mark's corner: its 8x8 all magenta.
        if (magenta(i) && magenta(i + 21) && magenta(((y + 7) * w + x) * 3) && magenta(((y + 7) * w + x + 7) * 3)) {
          origin = { x, y };
          break;
        }
      }
  } finally {
    await app.js(`document.getElementById("e2e-mark")?.remove()`);
  }
  if (!origin) throw new Error("the page's mark isn't on the screen (the window is hidden or covered?)");
  return origin;
}
