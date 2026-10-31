// A file manager to drop on, for the scenarios that drag a clip out of the
// window (drag-file-time, drag-promise): the session's (Dolphin in a KDE
// one, XDG_CURRENT_DESKTOP saying so, as the app reads it; else Nautilus),
// opened on a folder of the test's, with a home of its own (never the
// user's), beside the app's window on the test's display. Linux (X11) only,
// so far. And what those scenarios share: the clip format picked, the length
// of a file dropped, the app's windows (the system's drag shows its picture
// in one of its own).

import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, readdirSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "./app.mjs";
import * as os from "./os.mjs";

/** The session's file manager: Dolphin in KDE's, else Nautilus. */
export const fileManager = () =>
  /(^|:)kde(:|$)/i.test(process.env.XDG_CURRENT_DESKTOP ?? "") ? "dolphin" : "nautilus";

/** Why there's no file manager to drop on here, or null if there is. */
export function noFileManager() {
  const why = os.unavailable() ?? (process.platform !== "linux" ? "drags are driven on Linux only, so far" : null);
  if (why) return why;
  try {
    execFileSync("sh", ["-c", `command -v ${fileManager()}`], { stdio: "ignore" });
  } catch {
    return `no ${fileManager()} to drop on`;
  }
  return null;
}

/**
 * The file manager (`fileManager`) on folder `dest`, its home in `home`,
 * beside the app's window (skipped, through `ctx`, where the screen has no
 * room for it): where to drop on it, and `close`.
 */
export async function openFileManager(app, ctx, dest, home) {
  mkdirSync(dest, { recursive: true });
  mkdirSync(home, { recursive: true });
  const page = await os.windowRect(app);
  const [screenW] = execFileSync("xdotool", ["getdisplaygeometry"], { encoding: "utf8" }).trim().split(" ").map(Number);
  const fm = { x: page.x + page.w + 20, y: Math.max(0, page.y), w: 600, h: 500 };
  if (fm.x + fm.w > screenW) fm.x = page.x - 20 - fm.w;
  if (fm.x < 0)
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

/** A video file's length in seconds, as ffprobe reads it. */
export function probeDuration(file) {
  const out = execFileSync(
    "ffprobe",
    ["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", file],
    { encoding: "utf8" },
  );
  return Number(out.trim());
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
