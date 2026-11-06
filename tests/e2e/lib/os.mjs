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
// - macOS: screencapture, and CoreGraphics events for clicks and gestures
//   (the process running the tests needs the Screen Recording and
//   Accessibility permissions).
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

/**
 * Why the OS layer can't run here, or null if it can. With `linuxOnly`, what
 * a scenario drives through it that is driven on Linux only so far ("drags",
 * "the system's keys"): elsewhere, that is why.
 */
export function unavailable({ linuxOnly = null } = {}) {
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
  if (linuxOnly && process.platform !== "linux") return `${linuxOnly} are driven on Linux only, so far`;
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
    // answer a script run over SSH (its AppleEvent times out). Pointed there,
    // pressed and released a moment later, as a hand clicks: after a click on
    // the title bar with its release right on the press's heels, the window
    // went on following the pointer, as if the release was missed, and the
    // next gesture dragged it off the screen (drag-note, the hosted Mac,
    // 2026-11-05; macGesture's releases wait for the same reason).
    const [px, py] = [Math.round(x / scale), Math.round(y / scale)];
    execFileSync("osascript", [
      "-l",
      "JavaScript",
      "-e",
      `${MAC_EVENTS}
       mouse(5, ${px}, ${py}); $.usleep(50000); mouse(1, ${px}, ${py}); $.usleep(100000); mouse(2, ${px}, ${py});`,
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
 * Linux (X11), Windows, and on macOS the left button's steps, keys, clicks
 * and waits. On Windows the steps go as one PowerShell script (user32's
 * mouse_event and keybd_event): starting one takes a moment, and a drag out
 * of the window holds the app's UI thread, so the page can't be asked
 * anything between the steps of one.
 */
export async function gesture(steps) {
  if (process.platform === "darwin") return macGesture(steps);
  if (process.platform === "win32") return windowsGesture(steps);
  if (process.platform !== "linux") throw new Error("gestures are driven on Linux, macOS and Windows only, so far");
  const WHEEL = { up: "4", down: "5", left: "6", right: "7" };
  for (const [what, a, b, c] of steps) {
    if (what === "down") {
      execFileSync("xdotool", ["mousemove", String(a), String(b), "mousedown", String(c ?? 1)]);
      held.buttons.add(String(c ?? 1));
    } else if (what === "move") execFileSync("xdotool", ["mousemove", String(a), String(b)]);
    else if (what === "up") {
      execFileSync("xdotool", ["mouseup", String(a ?? 1)]);
      held.buttons.delete(String(a ?? 1));
    } else if (what === "click") execFileSync("xdotool", ["click", "--repeat", String(a ?? 1), "--delay", "80", "1"]);
    else if (what === "wheel")
      execFileSync("xdotool", ["click", "--repeat", String(b ?? 1), "--delay", "30", WHEEL[a]]);
    else if (what === "key") execFileSync("xdotool", ["key", a]);
    else if (what === "hold") {
      execFileSync("xdotool", ["keydown", a]);
      held.keys.add(a);
    } else if (what === "release") {
      execFileSync("xdotool", ["keyup", a]);
      held.keys.delete(a);
    } else if (what === "letgo") {
      execFileSync("xdotool", ["mousemove", String(a), String(b), "mouseup", "1"]);
      held.buttons.delete("1");
    } else if (what === "wait") await new Promise((r) => setTimeout(r, a));
    else throw new Error(`no gesture step ${what}`);
  }
}

/** The buttons and keys gestures pressed and haven't let go of yet. */
const held = { buttons: new Set(), keys: new Set() };

/** macOS's virtual key codes for the keys gestures press. */
const MAC_KEYS = { Escape: 53, Return: 36, space: 49 };

/**
 * `gesture`'s steps on macOS, as one JXA script of CoreGraphics events (one
 * osascript per step would take a tenth of a second each): a move with the
 * button down is a drag event (AppKit's drag follows those only). Screen
 * points, which are the pixels on a screen of scale 1.
 */
async function macGesture(steps) {
  const lines = [];
  let down = held.buttons.has("1");
  const mouse = (type, x, y) => lines.push(`mouse(${type}, ${x}, ${y});`);
  for (const [what, a, b] of steps) {
    if (what === "down") {
      mouse(5, a, b);
      lines.push("$.usleep(200000);");
      mouse(1, a, b);
      down = true;
    } else if (what === "move") mouse(down ? 6 : 5, a, b);
    else if (what === "up" || what === "letgo") {
      // (A moment between the last move and the release, as a hand leaves:
      // a release right on a move's heels was sometimes missed by the
      // system's drag, which then never ended.)
      if (what === "letgo") mouse(6, a, b);
      lines.push("$.usleep(100000);", "mouse(2, ...here());");
      down = false;
    } else if (what === "click") {
      for (let i = 1; i <= (a ?? 1); i++) lines.push(`click(${i});`);
    } else if (what === "key") {
      if (!(a in MAC_KEYS)) throw new Error(`no macOS key code for ${a}`);
      lines.push(`key(${MAC_KEYS[a]});`);
    } else if (what === "wait") lines.push(`$.usleep(${Math.round(a * 1000)});`);
    else throw new Error(`no gesture step ${what} on macOS, so far`);
  }
  if (down) held.buttons.add("1");
  else held.buttons.delete("1");
  execFileSync("osascript", [
    "-l",
    "JavaScript",
    "-e",
    `${MAC_EVENTS}
${lines.join("\n")}`,
  ]);
}

/** The JXA helpers `macGesture`'s script uses. */
const MAC_EVENTS = `ObjC.import("CoreGraphics"); ObjC.import("unistd");
const here = () => { const p = $.CGEventGetLocation($.CGEventCreate(null)); return [p.x, p.y]; };
// (No modifiers: a key event posted with some leaves them held for the next.)
const post = (e) => { $.CGEventSetFlags(e, 0); $.CGEventPost(0, e); };
// From the HID system's state, with a click count on a press and its release,
// as a real mouse's are: the system's drag tracks those.
const source = $.CGEventSourceCreate(1);
const mouse = (type, x, y) => { const e = $.CGEventCreateMouseEvent(source, type, $.CGPointMake(x, y), 0); if (type === 1 || type === 2) $.CGEventSetIntegerValueField(e, 1, 1); post(e); };
const click = (n) => { const [x, y] = here(); for (const type of [1, 2]) { const e = $.CGEventCreateMouseEvent(null, type, $.CGPointMake(x, y), 0); $.CGEventSetIntegerValueField(e, 1, n); post(e); } $.usleep(80000); };
const key = (code) => { for (const down of [true, false]) { post($.CGEventCreateKeyboardEvent(null, code, down)); $.usleep(30000); } };`;

/** user32's mouse_event flags for each button (X11's numbering), down and up. */
const WINDOWS_BUTTONS = { 1: [0x2, 0x4], 2: [0x20, 0x40], 3: [0x8, 0x10] };
/** Virtual-key codes for the keys gestures press, by xdotool's names. */
const WINDOWS_KEYS = { Escape: 0x1b, Return: 0x0d, Delete: 0x2e, alt: 0x12, ctrl: 0x11, shift: 0x10 };
const WINDOWS_INPUT = `${WINDOWS_DPI} Add-Type 'using System; using System.Runtime.InteropServices; public class Input { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, IntPtr e); [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, IntPtr e); }';`;

/**
 * `steps` (as `gesture` takes them) as one PowerShell script, and run. One
 * more step here: ["shot", file], a PNG of the screen then.
 */
function windowsGesture(steps) {
  const button = (n) => WINDOWS_BUTTONS[String(n ?? 1)] ?? WINDOWS_BUTTONS[1];
  const key = (name) => {
    const code = WINDOWS_KEYS[name];
    if (code === undefined) throw new Error(`no Windows key for ${name}`);
    return code;
  };
  const mouse = (flags, data = 0) => `[Input]::mouse_event(${flags}, 0, 0, ${data}, [IntPtr]::Zero);`;
  // A move is the pointer put there and a move heard (OLE's drag loop
  // follows the moves it hears).
  const to = (x, y) => `[Input]::SetCursorPos(${Math.round(x)}, ${Math.round(y)}) | Out-Null; ${mouse(1)}`;
  const lines = [];
  for (const [what, a, b, c] of steps) {
    if (what === "down") {
      lines.push(to(a, b), "Start-Sleep -Milliseconds 30;", mouse(button(c)[0]));
      held.buttons.add(String(c ?? 1));
    } else if (what === "move") lines.push(to(a, b));
    else if (what === "up") {
      lines.push(mouse(button(a)[1]));
      held.buttons.delete(String(a ?? 1));
    } else if (what === "letgo") {
      lines.push(to(a, b), mouse(button(1)[1]));
      held.buttons.delete("1");
    } else if (what === "click") {
      for (let i = 0; i < (a ?? 1); i++) lines.push(mouse(0x2), mouse(0x4), "Start-Sleep -Milliseconds 80;");
    } else if (what === "wheel") {
      const [flag, sign] = { up: [0x800, 1], down: [0x800, -1], left: [0x1000, -1], right: [0x1000, 1] }[a];
      for (let i = 0; i < (b ?? 1); i++) lines.push(mouse(flag, sign * 120), "Start-Sleep -Milliseconds 30;");
    } else if (what === "key")
      lines.push(
        // (Held a moment, as a person's press is.)
        `[Input]::keybd_event(${key(a)}, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 100; [Input]::keybd_event(${key(a)}, 0, 2, [IntPtr]::Zero);`,
      );
    else if (what === "hold") {
      lines.push(`[Input]::keybd_event(${key(a)}, 0, 0, [IntPtr]::Zero);`);
      held.keys.add(a);
    } else if (what === "release") {
      lines.push(`[Input]::keybd_event(${key(a)}, 0, 2, [IntPtr]::Zero);`);
      held.keys.delete(a);
    } else if (what === "wait") lines.push(`Start-Sleep -Milliseconds ${Math.round(a)};`);
    else if (what === "shot")
      lines.push(
        `Add-Type -AssemblyName System.Drawing, System.Windows.Forms; $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height; [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save('${a}', [System.Drawing.Imaging.ImageFormat]::Png);`,
      );
    else throw new Error(`no gesture step ${what}`);
  }
  powershell(`${WINDOWS_INPUT}\n${lines.join("\n")}`);
}

/**
 * Lets go of whatever a gesture left pressed: a scenario that fails in the
 * middle of a drag leaves the X server's button down, and the next
 * scenario's real click is then only a release, which clicks nothing (CI,
 * 2026-10-29: player-picture's Play after drag-out failed mid-drag). The
 * test runner calls it after each scenario.
 */
export function letGoOfAll() {
  if (process.platform === "darwin") {
    if (held.buttons.size) execFileSync("osascript", ["-l", "JavaScript", "-e", `${MAC_EVENTS}\nmouse(2, ...here());`]);
  } else if (process.platform === "win32" && (held.buttons.size || held.keys.size)) {
    windowsGesture([...[...held.buttons].map((b) => ["up", Number(b)]), ...[...held.keys].map((k) => ["release", k])]);
  } else if (process.platform === "linux") {
    for (const button of held.buttons) execFileSync("xdotool", ["mouseup", button]);
    for (const key of held.keys) execFileSync("xdotool", ["keyup", key]);
  }
  held.buttons.clear();
  held.keys.clear();
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

/**
 * Moves the system's own pointer off the app's page, where this layer can
 * move it (X11): to the screen's far corner, or else onto what's above the
 * page (the title bar) or below it; then waits (up to 5 s) until the page has
 * nothing under it. Xvfb leaves the pointer mid-screen, inside the window,
 * and the page hears it as well as the pointer the steps make up: resting
 * where the timeline's tip pops up, it was under the tip as it appeared and
 * out from under it as it moved, and the timeline heard that as the pointer
 * leaving it, so the tip went (annotations, 2 runs in 10 on a 613 px page,
 * 2026-10-30). The test runner calls it after each start of the app.
 */
export async function parkPointer(app) {
  if (unavailable({ linuxOnly: "the system's pointer" })) return;
  const page = await windowRect(app);
  const [w, h] = execFileSync("xdotool", ["getdisplaygeometry"], { encoding: "utf8" }).trim().split(" ").map(Number);
  const off = ([x, y]) => x < page.x || y < page.y || x >= page.x + page.w || y >= page.y + page.h;
  const spot = [
    [w - 1, h - 1],
    [page.x + 1, page.y - 1],
    [page.x + 1, page.y + page.h],
  ].find((p) => p[0] >= 0 && p[1] >= 0 && p[1] < h && off(p));
  if (!spot) return;
  await gesture([["move", ...spot]]);
  for (let waited = 0; waited < 5000; waited += 50) {
    if (await app.js(`return !document.querySelector(":hover")`)) return;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error(`the page still has something under the system's pointer, moved off it to ${spot.join(",")}`);
}

/**
 * The app's window in front, as a click on its title bar puts it (an app
 * can't bring another to the front from a script on a Mac any more, and the
 * app a drop went to comes to the front, with the keys), once the page says
 * it has the focus. On a Mac, a moment instead: the page can say it has the
 * focus while its window isn't in front, and a press right after the click
 * that brought it there goes nowhere.
 */
export async function toFront(app) {
  const page = await windowRect(app);
  click(page.x + Math.round(page.w / 2), page.y - 14, page.scale);
  if (process.platform === "darwin") return new Promise((r) => setTimeout(r, 1500));
  for (let waited = 0; !(await app.js(`return document.hasFocus()`)); waited += 50) {
    if (waited >= 5000) throw new Error("the app's page doesn't have the focus after a click on its title bar");
    await new Promise((r) => setTimeout(r, 50));
  }
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
