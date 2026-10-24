// Processes still running that the app started: those whose command line
// names its data folder (the worker's model, the downloaded ffmpeg and
// yt-dlp, files in its work folder), and, given its binary, the app itself:
// a quit that hangs looks otherwise the same as a worker left behind. Other
// copies of the app on the same computer use another data folder, so
// aren't counted, but one of the same binary is (none runs beside a test).
//
// And when an app process ended (watchExit), for the page dump (lib/dump.mjs).

import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, readlinkSync, realpathSync } from "node:fs";

import { dataDirs } from "./data.mjs";

/**
 * "pid command line" of each process whose command line has the app's local
 * data folder in it, or, with `exe` (the app's binary), whose program is it.
 * Matched by the program, not the command line: the shells and the test
 * runner above the app name the binary in theirs.
 */
export function leftovers(exe = null) {
  const dir = dataDirs().at(-1);
  if (process.platform === "win32") {
    const script = `Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and (($_.CommandLine -and $_.CommandLine.Contains($env:E2E_DIR)) -or ($env:E2E_EXE -and $_.ExecutablePath -eq $env:E2E_EXE)) } | ForEach-Object { "$($_.ProcessId) $($_.CommandLine)" }`;
    const out = execFileSync("powershell.exe", ["-NoProfile", "-Command", script], {
      env: { ...process.env, E2E_DIR: dir, E2E_EXE: exe ? realpathSync(exe) : "" },
      encoding: "utf8",
    });
    return out.split(/\r?\n/).filter(Boolean);
  }
  const app = exe ? programs(exe) : new Set();
  const out = execFileSync("ps", ["-axww", "-o", "pid=,command="], { encoding: "utf8" });
  return out
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => (l.includes(dir) || app.has(l.split(" ")[0])) && !l.startsWith(`${process.pid} `));
}

/** The pids (strings) of the processes running the program `exe`: on
 *  Linux, only those started with this run's data folder (XDG_DATA_HOME in
 *  their environment), since parallel workers run the same binary. */
function programs(exe) {
  const binary = realpathSync(exe);
  if (process.platform === "linux") {
    const home = process.env.XDG_DATA_HOME ? `XDG_DATA_HOME=${process.env.XDG_DATA_HOME}\0` : null;
    const pids = readdirSync("/proc").filter((d) => /^\d+$/.test(d));
    return new Set(
      pids.filter((pid) => {
        try {
          if (readlinkSync(`/proc/${pid}/exe`) !== binary) return false;
          return !home || readFileSync(`/proc/${pid}/environ`, "latin1").includes(home);
        } catch {
          return false;
        }
      }),
    );
  }
  // macOS: `comm` is the program's path as it was started (lib/drivers.mjs's
  // bridge starts it by its full path).
  const out = execFileSync("ps", ["-axww", "-o", "pid=,comm="], { encoding: "utf8" });
  return new Set(
    out
      .split("\n")
      .map((l) => l.trim().match(/^(\d+) (.*)$/))
      .filter((m) => m && (m[2] === exe || m[2] === binary))
      .map((m) => m[1]),
  );
}

/** Whether process `pid` runs (a zombie, ended but not yet reaped by its parent, doesn't). */
function alive(pid) {
  if (process.platform === "linux") {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
      return stat.slice(stat.lastIndexOf(")") + 2)[0] !== "Z";
    } catch {
      return false;
    }
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === "EPERM";
  }
}

/**
 * Notes in `proc.exitedAt` when process `proc.pid` ends, looking ten times
 * a second: for a process this one didn't start (the app under
 * tauri-driver), whose end it isn't told of.
 */
export function watchExit(proc) {
  const timer = setInterval(() => {
    if (alive(proc.pid)) return;
    proc.exitedAt = Date.now();
    clearInterval(timer);
  }, 100);
  timer.unref();
}
