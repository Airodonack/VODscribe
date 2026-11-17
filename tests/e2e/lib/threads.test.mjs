import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";

import { formatThreads, threadDump, threadsOf } from "./threads.mjs";

test("each thread is a line: its id, state, name and where it waits", () => {
  const lines = formatThreads([
    { tid: 41, state: "S", name: "vodscribe", wchan: "do_poll" },
    { tid: 1203, state: "D", name: "tokio-runtime-w", wchan: "futex_wait_queue" },
  ]).split("\n");
  assert.deepEqual(lines, ["       41 S vodscribe        do_poll", "     1203 D tokio-runtime-w  futex_wait_queue"]);
});

test("a thread in the kernel past waking says what it asked for", () => {
  const line = formatThreads([
    {
      tid: 7,
      state: "D",
      name: "tokio-rt-worker",
      wchan: "btrfs_sync_log",
      call: "fsync(fd 12: /d/library.sqlite-wal)",
    },
  ]);
  assert.equal(line, "        7 D tokio-rt-worker  btrfs_sync_log  fsync(fd 12: /d/library.sqlite-wal)");
});

test(
  "a process's dump has its threads and a backtrace or why there's none",
  { skip: process.platform !== "linux" },
  async () => {
    const child = spawn("sleep", ["30"], { stdio: "ignore" });
    await once(child, "spawn");
    try {
      const [main] = threadsOf(child.pid);
      assert.equal(main.tid, child.pid);
      assert.equal(main.name, "sleep");
      const dump = threadDump(child.pid);
      assert.match(dump, new RegExp(`^pid ${child.pid}: sleep 30$`, "m"));
      assert.match(dump, new RegExp(`^ +${child.pid} [RS] sleep `, "m"));
      // gdb's frames where it may attach, else what stopped it (no gdb, no leave to attach).
      assert.match(dump, /backtraces:\n(.*#0 |\()/s);
    } finally {
      child.kill("SIGKILL");
    }
  },
);

test("a process already gone says so", { skip: process.platform !== "linux" }, async () => {
  const child = spawn("true", [], { stdio: "ignore" });
  await once(child, "exit");
  assert.match(threadDump(child.pid), /gone before its threads could be read/);
});
