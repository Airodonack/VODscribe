// The app's data folders, emptied before each scenario so each starts as a
// new install would, but for the downloaded tools and speech model (bin,
// models, gpu), which setup would only fetch again.
//
// This deletes a library. It refuses unless E2E_RESET_DATA=1, or the folder
// has no library yet: on a computer that's used, point XDG_DATA_HOME (Linux)
// somewhere else, or run on a test machine.

import { existsSync, readdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

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
      // Retried for a while: on Windows the last app's WebView2 processes
      // can hold its files for some seconds after it's gone.
      if (!KEPT.has(name))
        rmSync(path.join(dir, name), { recursive: true, force: true, maxRetries: 50, retryDelay: 200 });
    }
  }
}
