// The app's memory (Linux), taken at the end of a scenario: the app's
// process and those it started (the web view's, the worker, ffmpeg), each
// by its proportional set size (Pss: its own pages, and its share of those
// it shares, such as the web engine's libraries between the web view's
// processes), so the total doesn't count a shared page twice.

import { readdirSync, readFileSync, readlinkSync, realpathSync } from "node:fs";

const MIB = 1024;

/** A /proc file, or null where it's gone or not ours to read. */
function proc(pid, file) {
  try {
    return readFileSync(`/proc/${pid}/${file}`, "utf8");
  } catch {
    return null;
  }
}

/**
 * The memory of the app this run started, whose binary is `exe`, and of
 * every process under it: { processes: [{ pid, name, pss, rss }], pss } in
 * MiB, or null where it can't be read (not Linux, or the app isn't running).
 */
export function memoryOf(exe) {
  const app = appPid(exe);
  if (!app) return null;
  const under = tree(parents(), app);
  const processes = [];
  for (const pid of under) {
    const rollup = proc(pid, "smaps_rollup");
    if (!rollup) continue;
    const kb = (field) => Number(rollup.match(new RegExp(`^${field}:\\s+(\\d+) kB`, "m"))?.[1] ?? 0);
    processes.push({
      pid: Number(pid),
      name: (proc(pid, "comm") ?? "?").trim(),
      pss: Math.round(kb("Pss") / MIB),
      rss: Math.round(kb("Rss") / MIB),
    });
  }
  processes.sort((a, b) => b.pss - a.pss);
  return { processes, pss: processes.reduce((sum, p) => sum + p.pss, 0) };
}

/** Each process's parent, by pid (as strings). */
function parents() {
  const parent = new Map();
  for (const pid of readdirSync("/proc").filter((d) => /^\d+$/.test(d))) {
    // The parent is the 4th field, after the name in parentheses (which may hold spaces).
    const stat = proc(pid, "stat");
    if (stat) parent.set(pid, stat.slice(stat.lastIndexOf(")") + 2).split(" ")[1]);
  }
  return parent;
}

/** `top` and every process under it, by `parent` (parents()). */
function tree(parent, top) {
  const under = new Set([top]);
  for (let grew = true; grew;) {
    grew = false;
    for (const [pid, ppid] of parent)
      if (under.has(ppid) && !under.has(pid)) {
        under.add(pid);
        grew = true;
      }
  }
  return under;
}

/** The pids (strings) of every process under `pid` (Linux), not `pid` itself. */
export function processesUnder(pid) {
  const under = tree(parents(), String(pid));
  under.delete(String(pid));
  return [...under];
}

/**
 * The pid (a string) of the app this run started, whose binary is `exe`
 * (Linux), or null: the one under this process (under its driver), as
 * another of the same binary, such as a developer's own, isn't this run's.
 */
export function appPid(exe) {
  return appPids(exe)[0] ?? null;
}

/** The pids (strings) of every app of binary `exe` under this process (Linux; none elsewhere). */
export function appPids(exe) {
  if (process.platform !== "linux") return [];
  let binary;
  try {
    binary = realpathSync(exe);
  } catch {
    return [];
  }
  const exeOf = (pid) => {
    try {
      return readlinkSync(`/proc/${pid}/exe`);
    } catch {
      return null;
    }
  };
  return [...tree(parents(), String(process.pid))].filter((pid) => exeOf(pid) === binary);
}
