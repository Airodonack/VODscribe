// Processes still running that the app started: those whose command line
// names its data folder (the worker's model, the downloaded ffmpeg and
// yt-dlp, files in its work folder). Other copies of the app on the same
// computer use another data folder, so aren't counted.

import { execFileSync } from "node:child_process";

import { dataDirs } from "./data.mjs";

/** "pid command line" of each process whose command line has the app's local data folder in it. */
export function leftovers() {
  const dir = dataDirs().at(-1);
  if (process.platform === "win32") {
    const script = `Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and $_.CommandLine.Contains($env:E2E_DIR) -and $_.ProcessId -ne $PID } | ForEach-Object { "$($_.ProcessId) $($_.CommandLine)" }`;
    const out = execFileSync("powershell.exe", ["-NoProfile", "-Command", script], {
      env: { ...process.env, E2E_DIR: dir },
      encoding: "utf8",
    });
    return out.split(/\r?\n/).filter(Boolean);
  }
  const out = execFileSync("ps", ["-axww", "-o", "pid=,command="], { encoding: "utf8" });
  return out
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.includes(dir) && !l.startsWith(`${process.pid} `));
}
