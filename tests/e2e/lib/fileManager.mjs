// A file manager to drop on, for the scenarios that drag a clip out of the
// window (drag-file-time, drag-promise): the session's (Dolphin in a KDE
// one, XDG_CURRENT_DESKTOP saying so, as the app reads it; else Nautilus),
// opened on a folder of the test's, with a home of its own (never the
// user's), beside the app's window on the test's display. Linux (X11) only,
// so far. And what those scenarios share: the clip format picked, the length
// of a file dropped, the app's windows (the system's drag shows its picture
// in one of its own).

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "./app.mjs";
import { ffmpeg } from "./fixtures.mjs";
import * as os from "./os.mjs";

/** The session's file manager: Dolphin in KDE's, else Nautilus. */
export const fileManager = () =>
  /(^|:)kde(:|$)/i.test(process.env.XDG_CURRENT_DESKTOP ?? "") ? "dolphin" : "nautilus";

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
  const nautilus = spawn(fileManager(), ["--new-window", dest], {
    env: {
      ...process.env,
      HOME: home,
      XDG_CONFIG_HOME: path.join(home, ".config"),
      XDG_DATA_HOME: path.join(home, ".local", "share"),
      XDG_CACHE_HOME: path.join(home, ".cache"),
    },
    stdio: "ignore",
  });
  try {
    const name = path.basename(dest);
    const win = await until(
      "the file manager's window",
      () => {
        try {
          return execFileSync("xdotool", ["search", "--onlyvisible", "--name", `^${name}`], { encoding: "utf8" })
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
    nautilus.kill();
    throw e;
  }
  return {
    drop: { x: fm.x + Math.round(fm.w / 2), y: fm.y + Math.round(fm.h * 0.6) },
    close: () => nautilus.kill(),
  };
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
