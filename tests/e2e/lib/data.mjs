// The app's data folders, emptied before each scenario so each starts as a
// new install would, but for the downloaded tools and speech model (bin,
// models, gpu), which setup would only fetch again.
//
// This deletes a library. It refuses unless E2E_RESET_DATA=1, or the folder
// has no library yet: on a computer that's used, point XDG_DATA_HOME (Linux)
// somewhere else, or run on a test machine.

import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const ID = "app.vodscribe";
const KEPT = new Set(["bin", "models", "gpu"]);

/** The app's data folders: [roaming, local], the same folder but on Windows. */
export function dataDirs() {
  switch (process.platform) {
    case "win32":
      return [path.join(process.env.APPDATA, ID), path.join(process.env.LOCALAPPDATA, ID)];
    case "darwin":
      return [path.join(homedir(), "Library", "Application Support", ID)];
    default:
      return [path.join(process.env.XDG_DATA_HOME || path.join(homedir(), ".local", "share"), ID)];
  }
}

export function resetData() {
  const dirs = [...new Set(dataDirs())];
  const library = dirs.map((d) => path.join(d, "library.sqlite")).find(existsSync);
  if (library && process.env.E2E_RESET_DATA !== "1") {
    throw new Error(`${library} exists: set E2E_RESET_DATA=1 to let the test delete it (and the rest of the library)`);
  }
  for (const dir of dirs) {
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) {
      if (!KEPT.has(name)) removeForSure(path.join(dir, name));
    }
  }
}

/**
 * Deletes `target` (a file or a folder), trying for up to 20 s: on Windows
 * the last app's WebView2 processes can hold its folder for some seconds
 * after it's gone ("EPERM, Permission denied: ...\EBWebView"), and node's
 * own retries don't always cover that (eleven scenarios failed in a second
 * once, 2026-10-23).
 */
function removeForSure(target) {
  const until = Date.now() + 20_000;
  for (;;) {
    try {
      rmSync(target, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      return;
    } catch (e) {
      if (Date.now() >= until) throw e;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 250);
    }
  }
}

/**
 * Puts the library and settings release `version` left (`fixtures`'
 * library-<version>.sqlite and settings-<version>.json, made by
 * make-old-library.mjs) in the data folder, each of `places`' placeholders
 * in its files' paths made a folder here ({ "{FIXTURES}": fixtures }).
 */
export function oldLibrary(fixtures, version, places) {
  const dir = dataDirs()[0];
  mkdirSync(dir, { recursive: true });
  const library = path.join(dir, "library.sqlite");
  copyFileSync(path.join(fixtures, `library-${version}.sqlite`), library);
  copyFileSync(path.join(fixtures, `settings-${version}.json`), path.join(dir, "settings.json"));
  const db = new DatabaseSync(library);
  for (const [from, to] of Object.entries(places))
    db.prepare("UPDATE vods SET source_path = replace(source_path, ?, ?)").run(from, to);
  db.close();
}
