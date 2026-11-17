// Every thread of an app that won't quit, as text, taken before it's killed:
// a hang then carries its evidence (which thread waits, on what) instead of
// only "still ran 5 s after its session closed".
//
// Linux only. Each thread's name, state and the kernel function it sleeps in
// (/proc, no tools needed), the processes the app started, and each thread's
// backtrace from gdb (`thread apply all bt`), which needs gdb and leave to
// attach (kernel.yama.ptrace_scope 0, or this process as root): where it
// can't, the dump says why and keeps the rest. A release build is stripped,
// so its frames are the system libraries' (a futex wait, a GLib loop); a
// debug build names the app's own.

import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync, readlinkSync } from "node:fs";

import { processesUnder } from "./memory.mjs";

/** How long gdb has: attaching to a debug build reads its symbols first. */
const GDB_WAIT_MS = 90_000;

const read = (file) => {
  try {
    return readFileSync(file, "utf8");
  } catch {
    return null;
  }
};

/** Now on this computer's clock, as the run's own log lines read. */
function localTime() {
  const now = new Date();
  return new Date(now.getTime() - now.getTimezoneOffset() * 60_000).toISOString().slice(0, 23).replace("T", " ");
}

/** A /proc stat line's state letter (the field after the parenthesised name). */
function stateOf(stat) {
  return stat?.slice(stat.lastIndexOf(")") + 2).split(" ")[0] ?? "?";
}

/** The process's threads: { tid, name, state, wchan }, from /proc. */
export function threadsOf(pid) {
  let tids;
  try {
    tids = readdirSync(`/proc/${pid}/task`);
  } catch {
    return [];
  }
  return tids
    .map((tid) => {
      const task = `/proc/${pid}/task/${tid}`;
      const state = stateOf(read(`${task}/stat`));
      return {
        tid: Number(tid),
        name: read(`${task}/comm`)?.trim() ?? "?",
        state,
        wchan: read(`${task}/wchan`)?.trim() || "-",
        // In the kernel and not to be woken (D): gdb can't stop it, and the
        // process can't end, even killed, till it's out; what it asked for.
        ...(state === "D" && { call: callOf(task) }),
      };
    })
    .sort((a, b) => a.tid - b.tid);
}

/** The system calls a thread may be waiting in, by number (x86-64), for the dump's reader. */
const CALLS = { 0: "read", 1: "write", 3: "close", 17: "pread64", 18: "pwrite64", 74: "fsync", 75: "fdatasync" };
Object.assign(CALLS, {
  82: "rename",
  87: "unlink",
  202: "futex",
  257: "openat",
  263: "unlinkat",
  277: "sync_file_range",
});

/**
 * The system call thread `task` (its /proc folder) is in, and the file its
 * first argument names if that's a file it has open: "fsync(fd 12:
 * /…/library.sqlite-wal)". Null if it can't be read.
 */
function callOf(task) {
  const fields = read(`${task}/syscall`)?.trim().split(" ");
  if (!fields || !/^\d+$/.test(fields[0])) return fields?.[0] ?? null;
  const number = Number(fields[0]);
  const name = (process.arch === "x64" && CALLS[number]) || `syscall ${number}`;
  const fd = Number(fields[1]);
  let file = null;
  try {
    // The thread's own table: the process's (its first thread's) is gone once that thread has exited.
    if (fd < 1 << 16) file = readlinkSync(`${task}/fd/${fd}`);
  } catch {}
  return file ? `${name}(fd ${fd}: ${file})` : `${name}(${fields[1]}, …)`;
}

/** The threads' table, one line each. Pure, for its tests. */
export function formatThreads(threads) {
  return threads
    .map(
      (t) => `  ${String(t.tid).padStart(7)} ${t.state} ${t.name.padEnd(16)} ${t.wchan}${t.call ? `  ${t.call}` : ""}`,
    )
    .join("\n");
}

/** gdb's backtrace of every thread, or why there's none. */
function backtraces(pid) {
  try {
    const { stdout, stderr } = spawnGdb(pid);
    // Nothing to show (a process already ending can't be attached to): what gdb said instead.
    return stdout.includes("Thread ") ? stdout : `(no backtraces)\n${`${stdout}${stderr}`.trim()}`;
  } catch (e) {
    if (e.code === "ENOENT") return "(no gdb here: install it for each thread's backtrace)";
    const said = `${e.stdout ?? ""}${e.stderr ?? ""}`.trim();
    const why = e.signal ? `gdb stopped after ${GDB_WAIT_MS / 1000} s` : `gdb: ${e.message.split("\n")[0]}`;
    return `(${why})${said ? `\n${said}` : ""}`;
  }
}

/** gdb run on `pid`: what it printed on each stream; throws as execFileSync does. */
function spawnGdb(pid) {
  const result = spawnSync(
    "gdb",
    // No symbols fetched over the network (debuginfod): the dump is taken before a kill, not after a download.
    [
      "-p",
      String(pid),
      "-batch",
      "-nx",
      "-ex",
      "set debuginfod enabled off",
      "-ex",
      "set pagination off",
      "-ex",
      "thread apply all bt",
    ],
    { encoding: "utf8", timeout: GDB_WAIT_MS, stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 2 ** 20 },
  );
  const { stdout, stderr, signal } = result;
  if (result.error) throw Object.assign(result.error, { stdout, stderr, signal });
  if (result.signal) throw Object.assign(new Error(`gdb: ${result.signal}`), result);
  return result;
}

/** The dump of process `pid`, as text, or null where there's no /proc (not Linux). */
export function threadDump(pid) {
  if (process.platform !== "linux") return null;
  const threads = threadsOf(pid);
  if (!threads.length) return `pid ${pid}: gone before its threads could be read`;
  const command = read(`/proc/${pid}/cmdline`)?.split("\0").filter(Boolean).join(" ") ?? "?";
  const children = processesUnder(pid).map((child) => {
    const line = read(`/proc/${child}/cmdline`)?.split("\0").filter(Boolean).join(" ") || "?";
    return `  ${child} ${stateOf(read(`/proc/${child}/stat`))} ${line}`;
  });
  return [
    `pid ${pid}: ${command}`,
    `taken ${localTime()}`,
    "",
    `threads (tid, state, name, where the kernel has it waiting):`,
    formatThreads(threads),
    "",
    `processes it started:`,
    children.length ? children.join("\n") : "  none",
    "",
    "backtraces:",
    backtraces(pid),
  ].join("\n");
}
